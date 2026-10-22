"""La forma de onda de una cancion, para pintarla en el modo estudio.

Son `buckets` columnas a lo largo de la cancion, cada una con su pico y su
RMS entre 0 y 1. El pico da la silueta; el RMS es lo que se oye como volumen
y deja ver donde empieza el estribillo (en un mp3 moderno los picos van casi
todos a tope).

La calcula el crate de Rust (library/src/waveform.rs: un tercio de segundo
para seis minutos); lo que no sabe leer (opus, wma) lo decodifica ffmpeg a
PCM crudo. Se guarda en disco: la segunda vez es gratis, y la forma de onda
no cambia mientras no cambie el archivo. Aqui queda la puerta de Python, con
los mismos nombres, y lo que depende de `config` (la carpeta, ffmpeg).
"""

import logging
from pathlib import Path

import danplay_core as _rust

from . import config, convert

log = logging.getLogger(__name__)

# El crate es obligatorio: siempre hay con que calcularla.
RUST = True
DEFAULT_BUCKETS, FALLBACK_RATE, RANGE_RATE = _rust.waveform_constants()


def _folder_path() -> Path:
    return config.DATA_DIR / "waveforms"


def _folder():
    folder = _folder_path()
    folder.mkdir(parents=True, exist_ok=True)
    return folder


def _cache_file(path: str):
    """Un archivo por cancion, con el nombre sacado SOLO de la ruta: asi se
    puede borrar sabiendo la ruta, sin tener que adivinar mtime ni columnas.
    Dentro van la ruta, el mtime y las columnas con que se calculo, para
    saber si sigue valiendo."""
    return _folder() / f"{_rust.waveform_cache_name(path)}.json"


def retag(path: str, before, after) -> bool:
    """El archivo cambio solo de etiqueta (lo escribio DanPlay): la onda
    guardada sigue valiendo con la fecha nueva. True si se apunto."""
    return moved(path, path, before, after)


def moved(old_path: str, new_path: str, before, after) -> bool:
    """El archivo se movio o se renombro (lo hizo DanPlay), con su etiqueta
    reescrita o no: la onda guardada pasa a la ruta y la fecha nuevas, que
    el sonido es el mismo. True si se apunto."""
    return _rust.waveform_moved(
        _folder(), str(old_path), str(new_path), before.st_mtime, after.st_mtime
    )


def forget(*paths: str) -> int:
    """Borra la forma de onda guardada de esas canciones. Devuelve cuantas."""
    return _rust.waveform_forget(_folder(), [str(p) for p in paths])


def prune(known_paths) -> int:
    """Borra las formas de onda de canciones que ya no estan en el indice.

    Las de `known_paths` se quedan; el resto sobra: un archivo borrado por
    fuera, movido, o de una carpeta que se quito. Devuelve cuantas se fueron.
    """
    try:
        folder = _folder()
    except OSError:
        return 0
    return _rust.waveform_prune(folder, [str(p) for p in known_paths])


def columns(samples, buckets: int) -> tuple[list[float], list[float]]:
    """Resume muestras (enteros de 16 bits o flotantes) en `buckets` columnas.

    El pico es el maximo de la columna y el RMS la raiz de la energia media,
    los dos partidos por el pico mas alto de la cancion.
    """
    return _rust.waveform_columns(samples, int(buckets))


def _with_ffmpeg(path: str, buckets: int):
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        return None
    return _rust.waveform_with_ffmpeg(ffmpeg, path, int(buckets))


def compute(path: str, buckets: int = DEFAULT_BUCKETS) -> dict | None:
    """{'peaks': [...], 'rms': [...]} o None si no hay con que decodificar."""
    buckets = max(1, min(int(buckets or DEFAULT_BUCKETS), 4000))
    if not path:
        return None
    return _rust.waveform_compute(_folder_path(), path, buckets, convert.ffmpeg())


def compute_range(path: str, start: float, end: float, buckets: int) -> dict | None:
    """{'peaks', 'rms'} del trozo de `start` a `end` (segundos) en `buckets`
    columnas, a la escala de la salida (1 = el tope digital: lo que la onda de
    toda la cancion tiene como pico, en un mp3 de hoy). Sin cache: es un
    trozo que se pide al acercar y se tira al alejar. None si no se puede."""
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        return None
    return _rust.waveform_range(ffmpeg, path, float(start), float(end), int(buckets))


def available() -> bool:
    """Si hay con que calcularla: siempre, con el crate de Rust."""
    return True
