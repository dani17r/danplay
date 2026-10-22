# Tipos del modulo en Rust (core/src/lib.rs), para el comprobador de tipos.
#
# maturin mete este archivo en la rueda junto al modulo compilado (va al lado
# de Cargo.toml con el nombre del modulo); en el proyecto lo encuentra
# basedpyright por `stubPath`. Si cambia una firma en lib.rs, hay que
# cambiarla aqui.
#
# Todas las funciones sueltan el GIL mientras trabajan. Las rutas se aceptan
# como `str`, `bytes` u `os.PathLike`; las que vuelven en un lote son el
# mismo `str` que llego (con `surrogateescape`). Una ruta que falla devuelve
# None y el motivo queda en `last_error()`, por hilo.
from os import PathLike
from typing import TypeAlias

_Path: TypeAlias = str | bytes | PathLike[str] | PathLike[bytes]

def last_error() -> str | None:
    """Motivo del ultimo fallo de la ultima llamada desde este hilo."""

def partial_hash(path: _Path, bytes: int = 1048576) -> str | None:
    """md5 de los primeros `bytes` del archivo."""

def hashes(paths: list[_Path], bytes: int = 1048576) -> list[tuple[str, str | None]]:
    """`partial_hash` de muchos archivos, en paralelo: [(ruta, hash | None)]."""

def full_hashes(paths: list[_Path]) -> list[tuple[str, str | None]]:
    """md5 del archivo entero, en paralelo."""

def waveform(path: _Path, buckets: int = 800) -> tuple[list[float], list[float]] | None:
    """La forma de onda: (picos, rms), `buckets` valores entre 0 y 1 cada uno."""

# ---- las etiquetas (biblioteca/src/etiquetas.rs, con lofty): ver danplay/tags.py

def tags_read(path: _Path) -> dict:
    """Todo lo de una cancion, con las claves de `tags.read_all`. No falla: lo
    ilegible sale vacio (un panic de lofty queda en `last_error()`)."""

def tags_read_many(paths: list[_Path]) -> list[tuple[str, dict]]:
    """`tags_read` de muchas canciones, en paralelo: [(ruta, etiquetas)]."""

def tags_properties(path: _Path) -> tuple[float, int]:
    """(segundos, bits por segundo), sin mirar las etiquetas."""

def tags_write(path: _Path, fields: dict) -> bool:
    """Escribe los campos de `fields` (los de `tags._write_fields`); False si
    no se pudo, con el motivo en `last_error()`."""

def tags_write_txxx(path: _Path, description: str, value: str) -> bool:
    """Un TXXX cualquiera (solo en los archivos con ID3)."""

def cover_read(path: _Path) -> tuple[bytes, str] | None:
    """La portada incrustada: (datos, tipo), o None."""

def cover_write(path: _Path, data: bytes, mime: str) -> bool:
    """Pone la portada (quita las que hubiera); False si no se pudo."""
