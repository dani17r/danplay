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

Y lo que la app necesita encima (`grid`), medido canción a canción sobre
una biblioteca de alabanza (`scripts/revisar-metronomo.py`) y contra GTZAN:

- un solo nivel de pulso de principio a fin. La red, por tramos, oye el pulso
  al doble (el «y» de cada tiempo) o a la mitad, y el clic se aceleraba y
  frenaba dentro de la canción. Entre los candidatos de la red se elige la
  cadena de tempo estable (programacion dinamica) y lo que falta se rellena;
- cada pulso afinado entre tramas (una parabola sobre la red: la red mira
  cada 20 ms y el clic bailaba ±10 ms) y suavizado con el tempo de alrededor;
- el «1» con un modelo de compas: un compas irregular solo si los «1» de la
  red lo piden de verdad (antes salian compases de 1, 2, 5 y 8 tiempos);
- sin clic donde no hay pulso: antes del primero y despues del ultimo (una
  intro libre, un final que se apaga) y en un hueco que no es un numero
  entero de tiempos (una parte libre). Un hueco a tempo se rellena.

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

# Cuanto cuesta, al elegir la cadena de pulsos: apartarse del tempo de
# alrededor (por el cuadrado del log2 del cociente), un pulso que falta y
# empezar otra cadena despues de un hueco. Medidos sobre GTZAN y sobre una
# biblioteca de alabanza: con menos, vuelven los tramos al doble.
TEMPO_COST = 240.0
MISSING_COST = 0.6
RESTART_COST = 3.0
# A partir de que probabilidad un maximo de la red es candidato a pulso: por
# debajo del 50 % (el umbral de `beats_of`) para no perder los flojos de una
# parte suave; la cadena ya descarta los que no cuadran.
CANDIDATE = 0.2
# Un hueco se rellena si cabe un numero entero de pulsos (± esto de pulso).
WHOLE = 0.25
# Cuantos vecinos a cada lado alisan cada pulso.
SMOOTH = 4
# Lo que cuesta un compas irregular (cortado o con un tiempo de mas).
IRREGULAR_BAR = 0.004


def _sigmoid(x) -> np.ndarray:
    return 1.0 / (1.0 + np.exp(-np.asarray(x, dtype=np.float64)))


def candidates(beat_logits: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Los maximos (±3 tramas) de la probabilidad de pulso que pasan de
    `CANDIDATE`, cada uno en su instante afinado entre tramas (el vertice de
    la parabola por la trama y sus dos vecinas), con su probabilidad."""
    lg = np.asarray(beat_logits, dtype=np.float64)
    p = _sigmoid(lg)
    padded = np.pad(lg, 3, constant_values=-np.inf)
    window = np.lib.stride_tricks.sliding_window_view(padded, 7).max(axis=1)
    frames: list[int] = []
    for f in np.flatnonzero((lg == window) & (p > CANDIDATE)):
        if not frames or f - frames[-1] > 1:  # una meseta: uno
            frames.append(int(f))
    idx = np.array(frames, dtype=np.int64)
    times = idx.astype(np.float64)
    inner = (idx > 0) & (idx < lg.size - 1)
    a, b, c = lg[idx[inner] - 1], lg[idx[inner]], lg[idx[inner] + 1]
    den = a - 2 * b + c
    frac = np.divide(0.5 * (a - c), den, out=np.zeros_like(den), where=den != 0)
    times[inner] += np.clip(frac, -0.5, 0.5)
    return times / FPS, p[idx]


def _dominant_period(times: np.ndarray, probs: np.ndarray) -> float:
    """El periodo que mas se repite entre los pulsos claros: cada intervalo,
    un voto, como la mediana de antes (ante la duda, el nivel rapido)."""
    strong = times[probs > 0.5]
    ibi = np.diff(strong if strong.size >= 4 else times)
    ibi = ibi[(ibi > 0.15) & (ibi < 3.0)]
    if ibi.size == 0:
        raise ValueError("la cancion no tiene pulso claro")
    hist, edges = np.histogram(np.log2(ibi), bins=np.linspace(np.log2(0.15), np.log2(3.0), 160))
    k = int(np.argmax(np.convolve(hist, [1, 2, 1], mode="same")))
    guess = 2 ** ((edges[k] + edges[k + 1]) / 2)
    near = ibi[np.abs(np.log2(ibi / guess)) < 0.1]
    return float(np.median(near)) if near.size else float(guess)


def _local_period(beats: np.ndarray, period: float, half: int = 7):
    """El periodo de alrededor de cada instante, en el nivel del dominante:
    un intervalo al doble o a la mitad cuenta como el suyo. Asi una cancion
    que acelera o un popurri cambian de tempo, pero no de nivel. Hasta un 28 %
    del dominante (un popurri de 87 a 111): mas lejos, a un tercio de otro
    nivel, es la red confundiendo un tresillo, no la cancion."""
    ibi = np.diff(beats)
    mid = (beats[1:] + beats[:-1]) / 2
    folded = ibi * 2.0 ** np.clip(np.round(np.log2(period / ibi)), -2, 2)
    ok = np.abs(np.log2(folded / period)) < 0.36
    folded, mid = folded[ok], mid[ok]
    if folded.size < 3:
        return lambda t: np.full_like(np.asarray(t, dtype=np.float64), period)
    smooth = np.array(
        [np.median(folded[max(0, i - half) : i + half + 1]) for i in range(folded.size)]
    )
    return lambda t: np.interp(t, mid, smooth)


def _chain(times: np.ndarray, probs: np.ndarray, local) -> np.ndarray:
    """Los candidatos que forman la cadena de pulsos con mas probabilidad y
    menos tirones de tempo (programacion dinamica). Saltarse uno sale gratis
    (no suma); uno que falta en medio cuesta `MISSING_COST`; despues de un
    hueco largo se puede empezar otra cadena por `RESTART_COST`."""
    n = times.size
    score = np.full(n, -np.inf)
    back = np.full(n, -1, dtype=np.int64)
    best = np.full(n, -np.inf)  # la mejor cadena que acaba en <= i
    best_at = np.full(n, -1, dtype=np.int64)
    for j in range(n):
        tj, pl = times[j], float(local(times[j]))
        far = int(np.searchsorted(times, tj - 4.5 * pl))
        near = int(np.searchsorted(times, tj - 0.55 * pl))
        # empezar aqui: el primero de la cancion no paga; despues de un hueco, si
        s_j, b_j = probs[j], -1
        if far > 0 and best[far - 1] > -np.inf:
            s_j, b_j = best[far - 1] + probs[j] - RESTART_COST, int(best_at[far - 1])
            if s_j < probs[j] - RESTART_COST:
                s_j, b_j = probs[j], -1
        for i in range(far, near):
            d = tj - times[i]
            k = max(1, round(d / pl))
            dev = math.log2(d / (k * pl))
            s = score[i] + probs[j] - TEMPO_COST * dev * dev - MISSING_COST * (k - 1)
            if s > s_j:
                s_j, b_j = s, i
        score[j], back[j] = s_j, b_j
        if j > 0 and best[j - 1] >= s_j:
            best[j], best_at[j] = best[j - 1], best_at[j - 1]
        else:
            best[j], best_at[j] = s_j, j
    chain = []
    at = int(best_at[n - 1])
    while at != -1:
        chain.append(at)
        at = int(back[at])
    return np.array(chain[::-1], dtype=np.int64)


def _fill(beats: np.ndarray, local) -> np.ndarray:
    """Los pulsos que faltan dentro de la cadena: un hueco en el que cabe un
    numero entero de pulsos (±`WHOLE`) se reparte; uno que no (una parte
    libre, un ritardando) se queda sin clic."""
    out = [float(beats[0])]
    for b in beats[1:]:
        gap = b - out[-1]
        n = gap / float(local((b + out[-1]) / 2))
        k = round(n)
        if k >= 2 and abs(n - k) <= WHOLE * (1 if k <= 8 else 2):
            out += [out[-1] + gap * i / k for i in range(1, k)]
        out.append(float(b))
    return np.array(out)


def _smooth(beats: np.ndarray) -> np.ndarray:
    """Cada pulso en la recta de sus `SMOOTH` vecinos de cada lado, si el
    tramo va regular; donde el tempo cambia de golpe, como estaba."""
    out = beats.copy()
    for i in range(beats.size):
        lo, hi = max(0, i - SMOOTH), min(beats.size, i + SMOOTH + 1)
        seg = beats[lo:hi]
        ibi = np.diff(seg)
        if seg.size < 5:
            continue
        med = np.median(ibi)
        if ibi.max() > 1.25 * med or ibi.min() < 0.8 * med:
            continue
        idx = np.arange(lo, hi)
        slope, icept = np.polyfit(idx, seg, 1)
        out[i] = slope * i + icept
    for i in range(1, out.size):  # sin cruzarse nunca
        if out[i] <= out[i - 1]:
            out[i] = beats[i]
    return out


def _bars(down_p: np.ndarray, meter: int) -> tuple[np.ndarray, float]:
    """El «1» de cada compas (Viterbi): los estados son el tiempo del compas
    (0..meter-1) y uno mas para un compas con un tiempo de mas; lo normal es
    pasar al siguiente, y cortar o alargar un compas cuesta `IRREGULAR_BAR`.
    Devuelve los indices de los «1» y lo bien que cuadra."""
    m, n = meter, down_p.size
    states = m + 1
    lp = np.log(np.clip(down_p, 1e-4, 1 - 1e-4))
    lq = np.log(np.clip(1 - down_p, 1e-4, 1 - 1e-4))
    emit = np.empty((n, states))
    emit[:, 0] = lp
    emit[:, 1:] = lq[:, None]
    trans = np.full((states, states), -np.inf)
    odd, keep = math.log(IRREGULAR_BAR), math.log(1 - 2 * IRREGULAR_BAR)
    for s in range(m):
        trans[s, (s + 1) % m] = keep
        if 1 <= s < m - 1:
            trans[s, 0] = odd  # compas cortado
    trans[m - 1, m] = odd  # un tiempo de mas
    trans[m, 0] = 0.0
    delta = np.full(states, -np.inf)
    delta[:m] = emit[0, :m] - math.log(m)
    psi = np.zeros((n, states), dtype=np.int64)
    for t in range(1, n):
        cand = delta[:, None] + trans
        psi[t] = np.argmax(cand, axis=0)
        delta = cand[psi[t], np.arange(states)] + emit[t]
    state = int(np.argmax(delta))
    fit = float(delta[state])
    path = np.empty(n, dtype=np.int64)
    for t in range(n - 1, -1, -1):
        path[t] = state
        state = int(psi[t, state])
    return np.flatnonzero(path == 0), fit


def grid(beat_logits: np.ndarray, down_logits: np.ndarray) -> dict:
    """La rejilla que usa la app (`BeatGrid` de beats.rs) a partir de lo que
    dice la red trama a trama (ver el principio del archivo).

    `closed`: acaba en su ultimo pulso; despues no hay clic."""
    times, probs = candidates(beat_logits)
    if times.size < 4:
        raise ValueError("la cancion no tiene pulso claro")
    period = _dominant_period(times, probs)
    strong = times[probs > 0.5]
    local = _local_period(strong if strong.size >= 4 else times, period)
    # dos pasadas: la primera cadena ya va en un solo nivel, y el tempo de
    # alrededor que sale de ella (sin los golpes sueltos de la red) es fiable
    first = times[_chain(times, probs, local)]
    local = _local_period(first, period, half=4)
    beats = _smooth(_fill(times[_chain(times, probs, local)], local))
    if beats.size < 4:
        raise ValueError("la cancion no tiene pulso claro")
    # la probabilidad de «1» en cada pulso (la mejor de su trama y las de al lado)
    dp = _sigmoid(down_logits)
    f = np.clip(np.round(beats * FPS).astype(int), 0, dp.size - 1)
    down_p = np.maximum.reduce([dp[np.clip(f + o, 0, dp.size - 1)] for o in (-1, 0, 1)])
    fits = {m: _bars(down_p, m) for m in (3, 4)}
    # 4 salvo que el 3 cuadre claramente mejor
    meter = 3 if fits[3][1] > fits[4][1] + 0.05 * beats.size else 4
    bars = fits[meter][0]
    phases = {
        m: int(np.bincount(fits[m][0] % m, minlength=m).argmax()) if fits[m][0].size else 0
        for m in (3, 4)
    }
    return {
        "bpm": round(float(60.0 / np.median(np.diff(beats))), 2),
        "meter": meter,
        "beats": [round(float(t), 4) for t in beats],
        "first_downbeat": int(bars[0]) if bars.size else 0,
        "phase3": phases[3],
        "phase4": phases[4],
        "confidence": round(float(np.mean(down_p[bars])) if bars.size else 0.0, 3),
        "bars": [int(b) for b in bars],
        "closed": True,
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
    return grid(beat, down)


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
