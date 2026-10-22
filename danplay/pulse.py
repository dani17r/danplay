"""El pulso y el «1» de una cancion con Beat This! (CPJKU, MIT), en ONNX Runtime.

Beat This! es una red que mira el espectrograma de la cancion y dice, cada
50 avos de segundo, lo probable que es que ahi haya un pulso y que ese pulso
sea el «1». Medido en GTZAN (que la red no vio al entrenar), acierta mucho
mas que el analisis de siempre (`beats.rs` en la app de escritorio), sobre
todo el «1»: ver scripts/evaluar-pulso.py y docs/ARQUITECTURA.md.

Todo lo hace el crate (analisis/src/pulso.rs y pulso/, cuenta a cuenta lo
que antes estaba aqui con numpy): el espectrograma, la red en ONNX Runtime
(el de la biblioteca que trae el paquete `onnxruntime`, cargada desde Rust),
la rejilla que usa la app y lo que se guarda de la red. Esto es la puerta de
Python, con los mismos nombres; las explicaciones de cada paso estan alli.

Lo que dice la red se guarda (`raw`): volver a analizar un tramo, o toda la
cancion de otra manera («Re-analizar el clic»), tarda milisegundos.

Corre en un proceso aparte (`main`), con prioridad baja, como el separador:
son unos segundos de todos los nucleos.
"""

import importlib.util
import json
import os
import sys
from pathlib import Path

import danplay_core as _rust
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

# Lo que cuesta cada cosa al sacar la rejilla (ver analisis/src/pulso/rejilla.rs,
# donde se usan): apartarse del tempo, un pulso que falta, empezar otra cadena.
TEMPO_COST = 240.0
MISSING_COST = 0.6
RESTART_COST = 3.0
CANDIDATE = 0.2
WHOLE = 0.25
SMOOTH = 4
IRREGULAR_BAR = 0.004
PHASE_JUMP = 0.25
STEADY_R = 0.9
STEADY_DRIFT = 0.015
STEADY_FLIPPED = 0.15

MODES = ("auto", "repair", "steady", "fill", "follow")

# Cuanto del total es leer la cancion; el resto, la red (medido: 2,5 s de
# ~18 en una de 4 min, leida de un disco duro).
READ_SHARE = 0.15


def _f64(x) -> np.ndarray:
    return np.ascontiguousarray(x, dtype=np.float64).ravel()


def _f32(x) -> np.ndarray:
    return np.ascontiguousarray(x, dtype=np.float32).ravel()


# ------------------------------------------------------------ espectrograma


def mel_filters() -> np.ndarray:
    """(513, 128): el banco de filtros de torchaudio.functional.melscale_fbanks
    con esos parametros, sin normalizar."""
    return _rust.pulse_mel_filters()


def spectrogram(mono: np.ndarray) -> np.ndarray:
    """(tramas, 128): el log-mel de Beat This! (`LogMelSpect`) para `mono` a 22.050 Hz."""
    return _rust.pulse_spectrogram(_f32(mono))


# ------------------------------------------------------------ la red


def _starts(frames: int) -> list[int]:
    """Donde empieza cada trozo (`split_piece` con avoid_short_end)."""
    return _rust.pulse_starts(int(frames))


def logits(session, spect: np.ndarray, progress=None) -> tuple[np.ndarray, np.ndarray]:
    """La probabilidad (en logits) de pulso y de «1» en cada trama.
    `progress(hechos, total)`, si se da, tras cada trozo. `session`, la de
    `open_session` (todo en Rust) o cualquiera con `run` como las de
    onnxruntime."""
    spect = np.ascontiguousarray(spect, dtype=np.float32)
    return _rust.pulse_logits(session, spect, progress)


def beats_of(beat_logits: np.ndarray, down_logits: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Los pulsos y los «1» en segundos; cada «1», en el pulso mas cercano."""
    return _rust.pulse_beats_of(_f64(beat_logits), _f64(down_logits))


# ------------------------------------------------------------ la rejilla


def candidates(beat_logits: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Los maximos (±3 tramas) de la probabilidad de pulso que pasan de
    `CANDIDATE`, cada uno en su instante afinado entre tramas, con su
    probabilidad."""
    return _rust.pulse_candidates(_f64(beat_logits))


def anchored(a: float, b: float, k: int, beat_logits, slack=0.15, tempo_w=40.0) -> np.ndarray:
    """Los `k - 1` pulsos que van entre `a` y `b` (que no se mueven): donde
    la red dice que hay pulso, sin tirones de tempo."""
    return _rust.pulse_anchored(float(a), float(b), int(k), _f64(beat_logits), slack, tempo_w)


def steady_fit(beats: np.ndarray, weights: np.ndarray, p0: float, span=0.03):
    """El tempo fijo que mejor explica los pulsos y su fase: (periodo, fase,
    R, lo que se aparta por medio minuto (s), la parte a contratiempo)."""
    return _rust.pulse_steady_fit(_f64(beats), _f64(weights), float(p0), float(span))


def steady_beats(period: float, phase: float, t0: float, t1: float) -> np.ndarray:
    """Los pulsos a tempo fijo de `t0` a `t1`."""
    return _rust.pulse_steady_beats(float(period), float(phase), float(t0), float(t1))


def combine(beat_logits: np.ndarray, *others: np.ndarray) -> np.ndarray:
    """La red sobre la cancion y sobre sus pistas (la bateria): en cada trama,
    la mas segura (el maximo de las probabilidades), otra vez como logits.
    El maximo y no la media: la media perdia la intro (la bateria calla)."""
    return _rust.pulse_combine(_f64(beat_logits), [_f64(o) for o in others])


def grid(
    beat_logits: np.ndarray,
    down_logits: np.ndarray,
    *,
    lowband=None,
    mode: str = "auto",
    edges: bool = False,
    duration: float | None = None,
    bpm: float | None = None,
) -> dict:
    """La rejilla que usa la app (`BeatGrid` de beats.rs) a partir de lo que
    dice la red trama a trama (ver analisis/src/pulso/rejilla.rs).

    `mode`: 'auto' (reparar los saltos de fase; si la cancion va a clic,
    tempo fijo), 'repair' (lo mismo, sin pasar a tempo fijo), 'steady'
    (tempo fijo siempre; `bpm`, si se da), 'fill' (como 'auto' y ademas con
    clic en la intro y el final: `edges`) o 'follow' (como antes de 1.28:
    sin reparar nada). `lowband`: la envolvente del bombo y el bajo, para
    decidir la fase. `closed`: acaba en su ultimo pulso; despues no hay clic.
    `report` dice lo que se arreglo."""
    env, fps = lowband if lowband is not None else (None, 0.0)
    return _rust.pulse_grid(
        _f64(beat_logits),
        _f64(down_logits),
        None if env is None else _f32(env),
        float(fps),
        lowband is not None,
        str(mode),
        bool(edges),
        float(duration) if duration else None,
        float(bpm) if bpm else None,
    )


def finish(
    beats: np.ndarray, down_logits: np.ndarray, report: dict, meter: int | None = None
) -> dict:
    """La rejilla con sus pulsos ya puestos: el «1» de cada compas (con el
    compas que se diga, o el que cuadre: 4 salvo que el 3 cuadre claramente
    mejor), el tempo y lo demas que espera la app."""
    out = _rust.pulse_finish(_f64(beats), _f64(down_logits), meter if meter in (3, 4) else None)
    out["report"] = report
    return out


def lowband(mono: np.ndarray, fmax: float = 150.0) -> tuple[np.ndarray, float]:
    """Donde golpean el bombo y el bajo: el flujo espectral por debajo de
    `fmax` Hz, cada 10 ms (para decidir la fase de un trozo: el flujo de toda
    la banda no distingue, los platos van a corcheas)."""
    return _rust.pulse_lowband(_f32(mono), float(fmax))


# ------------------------------------------------------------ la cancion


def _decode(ffmpeg: str, path: str) -> np.ndarray:
    """La cancion a mono y 22.050 Hz."""
    return _rust.pulse_decode(ffmpeg, path)


def onnxruntime_library() -> str:
    """La biblioteca de ONNX Runtime que trae el paquete `onnxruntime` (su
    `capi/`), sin importarlo: la carga Rust. Es la misma version que usaba
    Python (la 1.23.2 en los Mac Intel, la ultima que hay para ellos)."""
    spec = importlib.util.find_spec("onnxruntime")
    places = list(spec.submodule_search_locations or []) if spec else []
    for place in places:
        capi = Path(place) / "capi"
        found = sorted(
            p
            for p in (capi.iterdir() if capi.is_dir() else [])
            if p.name.startswith("libonnxruntime.") or p.name == "onnxruntime.dll"
        )
        if found:
            return str(found[0])
    raise RuntimeError("falta ONNX Runtime en esta instalación de DanPlay")


def open_session(graph):
    """La red, en ONNX Runtime (desde Rust). Cuatro hilos: con mas no va mas
    rapido (medido: 11,7 s con 4 y 13,3 con todos en una cancion de 4 min) y
    se deja sitio a la musica y a la app."""
    return _rust.pulse_open_session(onnxruntime_library(), str(graph))


def analyze(ffmpeg: str, path: str, graph, progress=None, raw=None) -> dict:
    """La rejilla de un archivo, de principio a fin. `progress(0..1)`, si se
    da, segun avanza: la app enseña una barra mientras espera. Con `raw` (un
    dict), deja ahi lo que dijo la red y la envolvente del bombo y el bajo,
    para guardarlo."""
    result, got = _rust.pulse_analyze(ffmpeg, path, onnxruntime_library(), str(graph), progress)
    if raw is not None:
        raw.update(got)
    return result


def save_raw(path, raw: dict) -> None:
    """Lo que dijo la red, en float16 (unos 100 KB para una de 4 min). A un
    temporal y a su sitio al final: cortado a medias, no queda roto."""
    _rust.pulse_save_raw(
        str(path),
        _f64(raw["beat"]),
        _f64(raw["down"]),
        _f64(raw["low"]),
        float(raw["low_fps"]),
        float(raw["seconds"]),
    )


def load_raw(path) -> dict | None:
    """Lo guardado por `save_raw`, o None. Uno dañado (de antes de guardarse
    asi) se borra: se vuelve a calcular."""
    return _rust.pulse_load_raw(str(path))


def key_of(path, st=None) -> str:
    """Como se reconoce un archivo en la cache: ruta, tamaño y fecha (los de
    `st`, si se dan: los de antes de reescribir su etiqueta)."""
    st = st or os.stat(path)
    return _rust.pulse_key(os.path.abspath(path), st.st_size, int(st.st_mtime))


def main(argv=None) -> int:
    """El proceso hijo: `python -m danplay.pulse GRAFO FFMPEG ARCHIVO [CRUDO]`
    -> JSON. Con CRUDO (un .npz), deja ahi lo que dijo la red."""
    args = sys.argv[1:] if argv is None else argv
    if len(args) not in (3, 4):
        sys.stderr.write("uso: danplay.pulse GRAFO FFMPEG ARCHIVO [CRUDO]\n")
        return 2
    if hasattr(os, "nice"):
        try:
            os.nice(10)
        except OSError:
            pass
    graph, ffmpeg, path = args[:3]
    raw_path = args[3] if len(args) == 4 else None

    def progress(part: float) -> None:
        # una linea por paso: el padre las lee segun llegan
        sys.stdout.write(json.dumps({"progress": round(part, 3)}) + "\n")
        sys.stdout.flush()

    try:
        raw: dict = {}
        result = analyze(ffmpeg, path, graph, progress, raw)
        if raw_path:
            save_raw(raw_path, raw)
    except Exception as e:  # noqa: BLE001 - se cuenta al proceso padre
        sys.stdout.write(json.dumps({"error": str(e) or e.__class__.__name__}) + "\n")
        return 1
    sys.stdout.write(json.dumps({"ok": True, **result}) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
