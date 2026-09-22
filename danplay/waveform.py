# -*- coding: utf-8 -*-
"""La forma de onda de una cancion, para pintarla en el modo estudio.

Son `buckets` columnas a lo largo de la cancion, cada una con su pico y su
RMS entre 0 y 1. El pico da la silueta; el RMS es lo que se oye como volumen
y deja ver donde empieza el estribillo (en un mp3 moderno los picos van casi
todos a tope).

La calcula el nucleo en Rust si esta compilado (un tercio de segundo para
seis minutos). Si no, ffmpeg decodifica a PCM crudo y se resume aqui, mas
despacio pero sin depender de nada mas. Se guarda en disco: la segunda vez es
gratis, y la forma de onda no cambia mientras no cambie el archivo.
"""
import hashlib, json, logging, math, os, subprocess
from array import array
from . import config, convert

log = logging.getLogger("danplay")

try:
    import danplay_core as _rust
    RUST = True
except ImportError:                      # pragma: no cover - depende de la compilacion
    _rust, RUST = None, False

DEFAULT_BUCKETS = 800
# El respaldo por ffmpeg baja la frecuencia: para pintar sobra, y son ocho
# veces menos muestras que recorrer en Python.
FALLBACK_RATE = 8000


def _cache_file(path: str, buckets: int):
    try:
        stamp = os.path.getmtime(path)
    except OSError:
        stamp = 0
    key = hashlib.sha1(f"{path}:{stamp}:{buckets}".encode()).hexdigest()
    folder = config.DATA_DIR / "waveforms"
    folder.mkdir(parents=True, exist_ok=True)
    return folder / f"{key}.json"


def columns(samples, buckets: int) -> tuple[list[float], list[float]]:
    """Resume muestras (enteros de 16 bits o flotantes) en `buckets` columnas.

    Es el mismo reparto que hace Rust (`audio::columns`): el pico es el maximo
    de la columna y el RMS la raiz de la energia media, los dos partidos por
    el pico mas alto de la cancion.
    """
    n = len(samples)
    if not n or buckets <= 0:
        return [], []
    peaks, rms = [], []
    for i in range(buckets):
        a = i * n // buckets
        b = max(a + 1, (i + 1) * n // buckets)
        chunk = samples[a:b]
        peaks.append(max(abs(x) for x in chunk))
        rms.append(math.sqrt(sum(x * x for x in chunk) / len(chunk)))
    top = max(peaks)
    if top > 0:
        peaks = [min(1.0, p / top) for p in peaks]
        rms = [min(1.0, r / top) for r in rms]
    return peaks, rms


def _with_ffmpeg(path: str, buckets: int):
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        return None
    cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-i", path,
           "-f", "s16le", "-ac", "1", "-ar", str(FALLBACK_RATE), "-"]
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        log.warning("ffmpeg no pudo decodificar %s: %s", path, e)
        return None
    if r.returncode != 0 or len(r.stdout) < 2:
        return None
    pcm = array("h")
    pcm.frombytes(r.stdout[: len(r.stdout) - len(r.stdout) % 2])
    return columns(pcm, buckets)


def compute(path: str, buckets: int = DEFAULT_BUCKETS) -> dict | None:
    """{'peaks': [...], 'rms': [...]} o None si no hay con que decodificar."""
    buckets = max(1, min(int(buckets or DEFAULT_BUCKETS), 4000))
    if not path or not os.path.exists(path):
        return None
    cached = _cache_file(path, buckets)
    if cached.is_file():
        try:
            return json.loads(cached.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            pass
    made = None
    if RUST:
        made = _rust.waveform(path, buckets)
        if made is None:
            log.info("el nucleo no pudo con la forma de onda: %s", _rust.last_error())
    if made is None:
        made = _with_ffmpeg(path, buckets)
    if made is None:
        return None
    # tres decimales: de sobra para pintar, y el json pesa la mitad
    out = {"peaks": [round(float(v), 3) for v in made[0]],
           "rms": [round(float(v), 3) for v in made[1]]}
    try:
        cached.write_text(json.dumps(out, separators=(",", ":")), encoding="utf-8")
    except OSError:
        log.warning("no pude guardar la forma de onda en %s", cached, exc_info=True)
    return out


def available() -> bool:
    """Si hay con que calcularla: el nucleo en Rust o ffmpeg."""
    return RUST or convert.tool("ffmpeg") is not None
