"""El pulso y el «1» de una cancion con Beat This! (CPJKU, MIT), en ONNX Runtime.

Beat This! es una red que mira el espectrograma de la cancion y dice, cada
50 avos de segundo, lo probable que es que ahi haya un pulso y que ese pulso
sea el «1». Medido en GTZAN (que la red no vio al entrenar), acierta mucho
mas que el analisis de siempre (`beats.rs` en la app de escritorio), sobre
todo el «1»: ver scripts/evaluar-pulso.py y docs/ARQUITECTURA.md.

Como el separador, la red va en un grafo ONNX (`danplay/data/pulso/`) y lo
de alrededor se hace aqui con numpy, igual que en su codigo:

- el espectrograma: mel de 128 bandas (30 Hz a 11 kHz, escala de Slaney) a
  22.050 Hz, ventanas de 1024 cada 441 muestras (50 por segundo), en
  magnitud y con log(1 + 1000·x);
- la cancion en trozos de 30 s (1500 tramas) que se solapan 6 tramas por
  cada lado; en lo que se pisan manda el primero;
- los pulsos: los maximos de la probabilidad en ±70 ms que pasan del 50 %,
  y cada «1» llevado al pulso mas cercano.

Y lo que la app necesita encima (`grid`): una rejilla sin huecos (donde la
red no oye pulso, en una parte sin ritmo, se rellena al tempo de alrededor:
el clic no se puede callar ahi) y el «1» de cada compas tal como lo oye,
compases irregulares incluidos (el 2/4 antes del coro).

Corre en un proceso aparte (`main`), con prioridad baja, como el separador:
son unos segundos de todos los nucleos.
"""

import hashlib
import json
import math
import os
import subprocess
import sys

import numpy as np

RATE = 22050
NFFT = 1024
HOP = 441
FPS = RATE / HOP  # 50 tramas por segundo
MELS = 128
F_MIN = 30.0
F_MAX = 11000.0
CHUNK = 1500
BORDER = 6

WINDOW = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(NFFT) / NFFT)).astype(np.float64)


# ------------------------------------------------------------ espectrograma


def _hz_to_mel(f):
    """Escala de Slaney: lineal hasta 1 kHz y logaritmica por encima (librosa,
    torchaudio con mel_scale="slaney")."""
    f = np.asarray(f, dtype=np.float64)
    f_sp = 200.0 / 3
    mels = f / f_sp
    min_log_hz = 1000.0
    min_log_mel = min_log_hz / f_sp
    logstep = math.log(6.4) / 27.0
    return np.where(
        f >= min_log_hz, min_log_mel + np.log(np.maximum(f, 1e-10) / min_log_hz) / logstep, mels
    )


def _mel_to_hz(m):
    m = np.asarray(m, dtype=np.float64)
    f_sp = 200.0 / 3
    freqs = f_sp * m
    min_log_hz = 1000.0
    min_log_mel = min_log_hz / f_sp
    logstep = math.log(6.4) / 27.0
    return np.where(m >= min_log_mel, min_log_hz * np.exp(logstep * (m - min_log_mel)), freqs)


def mel_filters() -> np.ndarray:
    """(513, 128): el banco de filtros de torchaudio.functional.melscale_fbanks
    con esos parametros, sin normalizar."""
    freqs = np.linspace(0, RATE // 2, NFFT // 2 + 1)
    m_pts = np.linspace(_hz_to_mel(F_MIN), _hz_to_mel(F_MAX), MELS + 2)
    f_pts = _mel_to_hz(m_pts)
    f_diff = f_pts[1:] - f_pts[:-1]
    slopes = f_pts[None, :] - freqs[:, None]
    down = -slopes[:, :-2] / f_diff[:-1]
    up = slopes[:, 2:] / f_diff[1:]
    return np.maximum(0.0, np.minimum(down, up)).astype(np.float32)


_FILTERS: np.ndarray | None = None


def spectrogram(mono: np.ndarray) -> np.ndarray:
    """(tramas, 128): el log-mel de Beat This! (`LogMelSpect`) para `mono` a 22.050 Hz."""
    global _FILTERS
    if _FILTERS is None:
        _FILTERS = mel_filters()
    x = np.asarray(mono, dtype=np.float32)
    pad = NFFT // 2
    if x.size <= pad:
        x = np.pad(x, (0, pad + 1 - x.size))
    x = np.pad(x, (pad, pad), mode="reflect")
    frames = 1 + (x.size - NFFT) // HOP
    out = np.empty((frames, MELS), dtype=np.float32)
    # normalized="frame_length" de torchaudio: torch.stft(normalized=True),
    # que divide por la raiz del tamaño de la ventana
    norm = math.sqrt(NFFT)
    # por bloques: una cancion de diez minutos son 30.000 tramas
    step = 2048
    for a in range(0, frames, step):
        b = min(frames, a + step)
        idx = np.arange(NFFT)[None, :] + HOP * np.arange(a, b)[:, None]
        spec = np.abs(np.fft.rfft(x[idx] * WINDOW, axis=-1)) / norm
        out[a:b] = spec.astype(np.float32) @ _FILTERS
    return np.log1p(1000.0 * out)


# ------------------------------------------------------------ la red


def _starts(frames: int) -> list[int]:
    """Donde empieza cada trozo (`split_piece` con avoid_short_end)."""
    starts = list(range(-BORDER, frames - BORDER, CHUNK - 2 * BORDER))
    if frames > CHUNK - 2 * BORDER:
        starts[-1] = frames - (CHUNK - BORDER)
    return starts


def logits(session, spect: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """La probabilidad (en logits) de pulso y de «1» en cada trama."""
    frames = spect.shape[0]
    beat = np.full(frames, -1000.0, dtype=np.float32)
    down = np.full(frames, -1000.0, dtype=np.float32)
    starts = _starts(frames)
    # en lo que se pisan manda el primero: se escriben del ultimo al primero
    for start in reversed(starts):
        lo, hi = max(start, 0), min(start + CHUNK, frames)
        chunk = spect[lo:hi]
        chunk = np.pad(chunk, [(lo - start, max(0, min(BORDER, start + CHUNK - frames))), (0, 0)])
        b, d = session.run(None, {"spect": chunk[None].astype(np.float32)})
        b, d = b[0][BORDER:-BORDER], d[0][BORDER:-BORDER]
        at, n = start + BORDER, len(b)
        keep = slice(max(0, -at), min(n, frames - at))
        beat[at + keep.start : at + keep.stop] = b[keep]
        down[at + keep.start : at + keep.stop] = d[keep]
    return beat, down


def _peaks(values: np.ndarray) -> np.ndarray:
    """Las tramas con un maximo en ±3 tramas (±60 ms) por encima del 50 %,
    con las pegadas juntadas en su media (el `postp_minimal` de Beat This!)."""
    padded = np.pad(values, 3, constant_values=-np.inf)
    window = np.lib.stride_tricks.sliding_window_view(padded, 7).max(axis=1)
    frames = np.flatnonzero((values == window) & (values > 0))
    out: list[float] = []
    group: list[int] = []
    for f in frames:
        if group and f - group[-1] > 1:
            out.append(float(np.mean(group)))
            group = []
        group.append(int(f))
    if group:
        out.append(float(np.mean(group)))
    return np.array(out) / FPS


def beats_of(beat_logits: np.ndarray, down_logits: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Los pulsos y los «1» en segundos; cada «1», en el pulso mas cercano."""
    beats = _peaks(beat_logits)
    downs = _peaks(down_logits)
    if beats.size and downs.size:
        idx = np.abs(beats[None, :] - downs[:, None]).argmin(axis=1)
        downs = np.unique(beats[idx])
    return beats, downs


# ------------------------------------------------------------ la rejilla


def grid(beats: np.ndarray, downs: np.ndarray, duration: float, down_logits=None) -> dict:
    """La rejilla que usa la app (`BeatGrid` de beats.rs) a partir de los pulsos
    y los «1» de la red.

    - Sin huecos: donde entre dos pulsos cabe mas de uno y medio del tempo de
      alrededor, se ponen los que faltan, repartidos. Y antes del primero y
      despues del ultimo, hasta el principio y el final de la cancion.
    - El compas, el mas comun entre «1» y «1»; y cada «1» donde lo oye la red
      (`bars`), aunque algun compas sea mas corto o mas largo.
    """
    beats = np.asarray(beats, dtype=np.float64)
    if beats.size < 4:
        raise ValueError("la cancion no tiene pulso claro")
    period = float(np.median(np.diff(beats)))
    filled = [float(beats[0])]
    for b in beats[1:]:
        gap = b - filled[-1]
        local = period
        n = round(gap / local)
        if gap > 1.5 * local and n >= 2:
            filled += [filled[-1] + gap * k / n for k in range(1, n)]
        filled.append(float(b))
    while filled[0] - period >= 0:
        filled.insert(0, filled[0] - period)
    while filled[-1] + period <= duration:
        filled.append(filled[-1] + period)
    full = np.array(filled)
    # cada «1» en su pulso de la rejilla ya rellena
    bars = sorted({int(np.abs(full - d).argmin()) for d in np.asarray(downs, dtype=np.float64)})
    counts = np.diff(bars)
    counts = counts[(counts >= 2) & (counts <= 12)]
    meter = int(np.bincount(counts).argmax()) if counts.size else 4
    first = bars[0] if bars else 0
    # donde caeria el «1» si fuera un 3/4 o un 4/4 regular (para cambiarlo a mano)
    phases = {}
    for m in (3, 4):
        votes = (
            np.bincount(np.array(bars, dtype=np.int64) % m, minlength=m) if bars else np.zeros(m)
        )
        phases[m] = int(np.argmax(votes))
    confidence = 0.0
    if down_logits is not None and bars:
        frames = np.clip((full[bars] * FPS).round().astype(int), 0, len(down_logits) - 1)
        confidence = float(np.mean(1 / (1 + np.exp(-np.asarray(down_logits)[frames]))))
    intervals = np.diff(full)
    return {
        "bpm": round(float(60.0 / np.median(intervals)), 2),
        "meter": meter,
        "beats": [round(float(t), 4) for t in full],
        "first_downbeat": int(first),
        "phase3": phases[3],
        "phase4": phases[4],
        "confidence": round(confidence, 3),
        "bars": [int(b) for b in bars],
    }


# ------------------------------------------------------------ la cancion


def _decode(ffmpeg: str, path: str) -> np.ndarray:
    """La cancion a mono y 22.050 Hz."""
    r = subprocess.run(
        [
            *(ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-i", path, "-vn"),
            *("-f", "f32le", "-ac", "1", "-ar", str(RATE), "-"),
        ],
        capture_output=True,
        check=False,
    )
    if r.returncode != 0 or len(r.stdout) < 4:
        detail = r.stderr.decode("utf-8", "replace").strip().splitlines()
        raise RuntimeError("ffmpeg no pudo leer la cancion" + (f": {detail[-1]}" if detail else ""))
    return np.frombuffer(r.stdout[: len(r.stdout) // 4 * 4], dtype=np.float32)


def open_session(graph):
    import onnxruntime as ort

    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    return ort.InferenceSession(str(graph), options, providers=["CPUExecutionProvider"])


def analyze(ffmpeg: str, path: str, graph) -> dict:
    """La rejilla de un archivo, de principio a fin."""
    mono = _decode(ffmpeg, path)
    session = open_session(graph)
    beat, down = logits(session, spectrogram(mono))
    beats, downs = beats_of(beat, down)
    return grid(beats, downs, mono.size / RATE, down)


def key_of(path) -> str:
    """Como se reconoce un archivo en la cache: ruta, tamaño y fecha."""
    st = os.stat(path)
    raw = f"{os.path.abspath(path)}|{st.st_size}|{int(st.st_mtime)}"
    return hashlib.sha1(raw.encode("utf-8")).hexdigest()  # noqa: S324 - no es seguridad


def main(argv=None) -> int:
    """El proceso hijo: `python -m danplay.pulse GRAFO FFMPEG ARCHIVO` -> JSON."""
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 3:
        sys.stderr.write("uso: danplay.pulse GRAFO FFMPEG ARCHIVO\n")
        return 2
    if hasattr(os, "nice"):
        try:
            os.nice(10)
        except OSError:
            pass
    graph, ffmpeg, path = args
    try:
        result = analyze(ffmpeg, path, graph)
    except Exception as e:  # noqa: BLE001 - se cuenta al proceso padre
        sys.stdout.write(json.dumps({"error": str(e) or e.__class__.__name__}) + "\n")
        return 1
    sys.stdout.write(json.dumps({"ok": True, **result}) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
