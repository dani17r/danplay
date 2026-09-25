"""Miniaturas de las caratulas, guardadas en disco (DATA_DIR/covers).

Una lista de mil canciones pedia mil caratulas completas —a menudo de dos
megas cada una— para pintarlas a 40 pixeles; ahora se pide el tamaño que se
va a enseñar (96, 192 o 320) y se guarda para la proxima.

Cada miniatura se llama por su cancion: `<ruta>_<tamaño>_<version>.<ext>`,
con la ruta y la version (fecha y tamaño del archivo) resumidas en un hash.
Asi se pueden podar como las formas de onda: al escanear se van las de
canciones que ya no estan y las versiones viejas de una caratula que cambio.
Antes el nombre era un hash de todo junto, no habia forma de saber de quien
era cada una y la carpeta crecia para siempre. Ademas hay un tope en bytes:
pasado, se van las que hace mas que no se piden.
"""

import contextlib
import hashlib
import logging
import os
import threading
from pathlib import Path

from . import config, convert, tags

log = logging.getLogger(__name__)

# Los tamaños que se generan. Pedir otro devuelve la imagen tal cual: si no,
# cada pixel distinto seria un archivo nuevo en la cache.
SIZES = (96, 192, 320)
# Tope de la carpeta. Una miniatura ronda los 5-30 KB, asi que esto son
# decenas de miles: una biblioteca grande entera, en sus tres tamaños.
MAX_BYTES = 256 * 1024 * 1024
# Cuantas miniaturas nuevas entre dos comprobaciones del tope: mirar la
# carpeta entera en cada peticion seria mas caro que la propia miniatura.
CHECK_EVERY = 200

_EXT = {"image/jpeg": ".jpg", "image/png": ".png", "image/webp": ".webp"}
_MIME = {v: k for k, v in _EXT.items()}
_lock = threading.Lock()
_written = 0
_ready: set[str] = set()


def folder() -> Path:
    path = config.DATA_DIR / "covers"
    if str(path) not in _ready:
        config.private_dir(path)
        _ready.add(str(path))
    return path


# sha1 solo para dar nombre a los archivos, no para proteger nada
def _key(path: str) -> str:
    raw = str(path).encode("utf-8", "surrogateescape")
    return hashlib.sha1(raw, usedforsecurity=False).hexdigest()[:24]


def _version(st: os.stat_result) -> str:
    raw = f"{st.st_mtime_ns}:{st.st_size}".encode()
    return hashlib.sha1(raw, usedforsecurity=False).hexdigest()[:10]


def _parts(f: Path) -> tuple[str, str, str] | None:
    """(ruta, tamaño, version) del nombre de una miniatura, o None si no es
    una de este formato (las de antes, un temporal a medias)."""
    bits = f.stem.split("_")
    if len(bits) != 3 or f.suffix not in _MIME:
        return None
    return bits[0], bits[1], bits[2]


def get(path: str, size: int) -> tuple[bytes, str] | None:
    """(datos, mime) de la miniatura de `size` px, o de la caratula tal cual
    si no se pudo encoger. None si el archivo no tiene caratula."""
    original = tags.cached_cover(path)
    if not original:
        return None
    try:
        st = os.stat(path)
    except OSError:
        return original
    key, version = _key(path), _version(st)
    for ext, mime in _MIME.items():
        cached = folder() / f"{key}_{size}_{version}{ext}"
        try:
            data = cached.read_bytes()
        except OSError:
            continue
        # marca de uso: al pasar del tope se van las que hace mas que no se piden
        with contextlib.suppress(OSError):
            os.utime(cached)
        return data, mime
    made = convert.shrink_bytes(original[0], original[1], max_side=size)
    if not made:
        return original
    data, mime = made
    _store(key, size, version, data, mime)
    return made


def _store(key: str, size: int, version: str, data: bytes, mime: str) -> None:
    global _written
    target = folder() / f"{key}_{size}_{version}{_EXT.get(mime, '.jpg')}"
    try:
        # las versiones viejas de esta misma miniatura ya no sirven
        for old in folder().glob(f"{key}_{size}_*"):
            if old != target:
                old.unlink(missing_ok=True)
        tmp = target.with_name(target.name + ".tmp")
        tmp.write_bytes(data)
        os.replace(tmp, target)
    except OSError:
        log.warning("no pude guardar la miniatura %s", target, exc_info=True)
        _ready.discard(str(target.parent))  # por si se borro la carpeta
        return
    with _lock:
        _written += 1
        due = _written % CHECK_EVERY == 0
    if due:
        trim()


def forget(*paths: str) -> int:
    """Borra las miniaturas de esas canciones. Devuelve cuantas."""
    n = 0
    for path in paths:
        for f in folder().glob(f"{_key(str(path))}_*"):
            try:
                f.unlink()
                n += 1
            except OSError:
                log.warning("no pude borrar la miniatura %s", f, exc_info=True)
    return n


def prune(known_paths) -> int:
    """Deja solo las miniaturas de `known_paths`, y de cada una la ultima
    version. Luego aplica el tope. Devuelve cuantas se borraron."""
    known = {_key(str(p)) for p in known_paths}
    newest: dict[tuple[str, str], Path] = {}
    gone = []
    try:
        files = list(folder().iterdir())
    except OSError:
        return 0
    for f in files:
        parts = _parts(f)
        if parts is None or parts[0] not in known:
            gone.append(f)
            continue
        slot = (parts[0], parts[1])
        other = newest.get(slot)
        if other is None:
            newest[slot] = f
        elif _mtime(f) > _mtime(other):
            gone.append(other)
            newest[slot] = f
        else:
            gone.append(f)
    n = 0
    for f in gone:
        try:
            f.unlink()
            n += 1
        except OSError:
            pass
    return n + trim()


def _mtime(f: Path) -> float:
    try:
        return f.stat().st_mtime
    except OSError:
        return 0.0


def trim(max_bytes: int | None = None) -> int:
    """Si la carpeta pasa del tope, borra las que hace mas que no se piden
    hasta quedarse en el 90 %. Devuelve cuantas."""
    limit = MAX_BYTES if max_bytes is None else max_bytes
    entries = []
    total = 0
    try:
        for f in folder().iterdir():
            try:
                st = f.stat()
            except OSError:
                continue
            entries.append((st.st_mtime, st.st_size, f))
            total += st.st_size
    except OSError:
        return 0
    if total <= limit:
        return 0
    n = 0
    for _, size, f in sorted(entries, key=lambda e: e[0]):
        if total <= limit * 0.9:
            break
        try:
            f.unlink()
            total -= size
            n += 1
        except OSError:
            pass
    if n:
        log.info("cache de miniaturas al tope: %d borradas", n)
    return n
