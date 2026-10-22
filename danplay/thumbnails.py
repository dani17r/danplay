"""Miniaturas de las caratulas, guardadas en disco (DATA_DIR/covers).

Una lista de mil canciones pedia mil caratulas completas —a menudo de dos
megas cada una— para pintarlas a 40 pixeles; ahora se pide el tamaño que se
va a enseñar (96, 192 o 320) y se guarda para la proxima.

Cada miniatura se llama por su cancion: `<path>_<tamaño>_<version>.<ext>`,
con la ruta y la version (fecha y tamaño del archivo) resumidas en un hash.
Asi se pueden podar como las formas de onda: al escanear se van las de
canciones que ya no estan y las versiones viejas de una caratula que cambio.
Ademas hay un tope en bytes: pasado, se van las que hace mas que no se piden.

Lo hace el crate de Rust (library/src/thumbnails.rs); aqui queda la
puerta de Python, y lo que depende de `config` (la carpeta, solo para su
dueño) y de las etiquetas (la caratula original).
"""

from pathlib import Path

import danplay_core as _rust

from . import config, convert, tags

# Los tamaños que se generan (pedir otro devuelve la imagen tal cual), el
# tope de la carpeta y cada cuantas nuevas se mira.
_sizes, MAX_BYTES, CHECK_EVERY = _rust.thumbnails_constants()
SIZES = tuple(_sizes)

_ready: set[str] = set()


def folder() -> Path:
    path = config.DATA_DIR / "covers"
    if str(path) not in _ready:
        config.private_dir(path)
        _ready.add(str(path))
    return path


def get(path: str, size: int) -> tuple[bytes, str] | None:
    """(datos, mime) de la miniatura de `size` px, o de la caratula tal cual
    si no se pudo encoger. None si el archivo no tiene caratula."""
    original = tags.cached_cover(path)
    if not original:
        return None
    where = folder()
    data, mime, unsaved = _rust.thumbnails_get(
        where, path, int(size), original[0], original[1], convert.ffmpeg()
    )
    if unsaved:
        _ready.discard(str(where))  # por si se borro la carpeta
    return data, mime


def forget(*paths: str) -> int:
    """Borra las miniaturas de esas canciones. Devuelve cuantas."""
    return _rust.thumbnails_forget(folder(), [str(p) for p in paths])


def prune(known_paths) -> int:
    """Deja solo las miniaturas de `known_paths`, y de cada una la ultima
    version. Luego aplica el tope. Devuelve cuantas se borraron."""
    try:
        where = folder()
    except OSError:
        return 0
    return _rust.thumbnails_prune(where, [str(p) for p in known_paths])


def trim(max_bytes: int | None = None) -> int:
    """Si la carpeta pasa del tope, borra las que hace mas que no se piden
    hasta quedarse en el 90 %. Devuelve cuantas."""
    try:
        where = folder()
    except OSError:
        return 0
    return _rust.thumbnails_trim(where, MAX_BYTES if max_bytes is None else int(max_bytes))
