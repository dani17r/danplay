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


def _folder():
    folder = config.DATA_DIR / "waveforms"
    folder.mkdir(parents=True, exist_ok=True)
    return folder


def _cache_file(path: str):
    """Un archivo por cancion, con el nombre sacado SOLO de la ruta: asi se
    puede borrar sabiendo la ruta, sin tener que adivinar mtime ni columnas.
    Dentro van la ruta, el mtime y las columnas con que se calculo, para
    saber si sigue valiendo."""
    return _folder() / f"{hashlib.sha1(path.encode('utf-8', 'surrogateescape')).hexdigest()}.json"


def _mtime(path: str) -> float:
    try:
        return os.path.getmtime(path)
    except OSError:
        return 0.0


def forget(*paths: str) -> int:
    """Borra la forma de onda guardada de esas canciones. Devuelve cuantas."""
    n = 0
    for path in paths:
        try:
            _cache_file(str(path)).unlink()
            n += 1
        except FileNotFoundError:
            pass
        except OSError:
            log.warning("no pude borrar la forma de onda de %s", path, exc_info=True)
    return n


def prune(known_paths) -> int:
    """Borra las formas de onda de canciones que ya no estan en el indice.

    Las de `known_paths` se quedan; el resto sobra: un archivo borrado por
    fuera, movido, o de una carpeta que se quito. Devuelve cuantas se fueron.
    """
    known = {str(p) for p in known_paths}
    n = 0
    try:
        files = list(_folder().glob("*.json"))
    except OSError:
        return 0
    for f in files:
        try:
            path = json.loads(f.read_text(encoding="utf-8")).get("path")
        except (OSError, ValueError, AttributeError):
            path = None                        # roto o de un formato viejo: fuera
        if path in known:
            continue
        try:
            f.unlink()
            n += 1
        except OSError:
            pass
    return n


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
    cached = _cache_file(path)
    stamp = _mtime(path)
    if cached.is_file():
        try:
            saved = json.loads(cached.read_text(encoding="utf-8"))
            if saved.get("path") == path and saved.get("mtime") == stamp \
                    and len(saved.get("peaks") or []) == buckets:
                return {"peaks": saved["peaks"], "rms": saved["rms"]}
        except (OSError, ValueError, AttributeError, KeyError):
            pass                               # se recalcula y se sobrescribe
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
        cached.write_text(json.dumps({"path": path, "mtime": stamp, **out},
                                     separators=(",", ":")), encoding="utf-8")
    except OSError:
        log.warning("no pude guardar la forma de onda en %s", cached, exc_info=True)
    return out


def available() -> bool:
    """Si hay con que calcularla: el nucleo en Rust o ffmpeg."""
    return RUST or convert.tool("ffmpeg") is not None
