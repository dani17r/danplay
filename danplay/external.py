"""La lista del reproductor: lo que has abierto desde fuera de DanPlay.

Abrir un mp3 desde el explorador de archivos **no es importarlo**. El archivo
puede estar en las descargas, en un pincho o en la carpeta de otra persona, y
DanPlay no tiene por que tocarlo, ni moverlo, ni renombrarlo, ni escribirle
etiquetas dentro. Esa es la separacion que guarda este modulo.

Lo que si se recuerda es que sono, para poder volver a ella desde el propio
reproductor en vez de buscar otra vez la carpeta. Es la lista que VLC llama
«lista de reproduccion»: se llena sola, no repite, y la que acabas de poner
sube al principio.

Son dos tablas porque son dos cosas distintas:

  `external_songs`  el archivo en si, con sus etiquetas leidas una sola vez.
                    Solo para lo que NO esta en la biblioteca; lo que ya esta
                    indexado no se copia aqui.
  `recent`          la lista. Guarda ids y la hora, y admite las dos
                    procedencias: positivo es de la biblioteca, negativo es de
                    `external_songs`.

Que los ids de fuera sean NEGATIVOS no es un truco: es lo que impide que una
operacion de biblioteca —borrar, renombrar, escribir etiquetas— caiga por
error sobre un archivo que no es suyo. `library.by_id` no los encuentra.

Lo hace Rust (biblioteca/src/fuera.rs); esto es la puerta de Python, con los
mismos nombres.
"""

import logging
import os
from typing import Any, cast

import danplay_core as _rust

from . import config, library

log = logging.getLogger(__name__)

SCHEMA = """
CREATE TABLE IF NOT EXISTS external_songs (
    id       INTEGER PRIMARY KEY,
    path     TEXT UNIQUE,
    file     TEXT DEFAULT '',
    title    TEXT DEFAULT '',
    artist   TEXT DEFAULT '',
    album    TEXT DEFAULT '',
    duration REAL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS recent (
    song_id  INTEGER PRIMARY KEY,   -- positivo: biblioteca. negativo: de fuera
    played   REAL
);
CREATE INDEX IF NOT EXISTS i_recent ON recent(played DESC);
"""
library.register_schema(SCHEMA)

COLUMNS = ("id", "path", "file", "title", "artist", "album", "duration")


def by_ids(conn, ids) -> dict[int, dict]:
    """{id negativo: cancion} de las de fuera con esos ids, en una consulta."""
    wanted = [int(i) for i in ids if int(i) < 0]
    if not wanted:
        return {}
    return _rust.external_by_ids(conn, wanted)


def by_id(cid: int) -> dict | None:
    """La cancion de fuera con ese id (negativo), o None."""
    if cid >= 0:
        return None
    with library.connect() as conn:
        return _rust.external_by_id(conn, int(cid))


def resolve(cid: int) -> dict[str, Any] | None:
    """La cancion con ese id, venga de donde venga.

    Es lo que hay que usar en todo sitio que solo necesite REPRODUCIR o
    ENSEÑAR una cancion: la ruta del audio, la portada, una lista. Para
    modificar algo, `library.by_id`, y asi lo de fuera queda fuera de alcance.
    """
    if cid > 0:
        # una fila entera de la biblioteca: quien la enseña le añade cosas
        # (sus listas, si el archivo sigue ahi)
        return cast(dict[str, Any] | None, library.by_id(cid))
    return by_id(cid)


def _register(path: str) -> int | None:
    """El id de esa ruta, creando la entrada de fuera si hace falta (las
    etiquetas se leen UNA vez, la primera)."""
    with library.connect() as conn:
        return _rust.external_register(conn, path)


class NotAudio(ValueError):
    """Lo que se quiso abrir no es un archivo de audio."""


def played(path) -> dict | None:
    """Apunta que esa ruta acaba de sonar y devuelve la cancion.

    Si ya estaba en la lista NO se añade otra vez: se le pone la hora nueva y
    con eso sube al principio. None si el archivo no esta; `NotAudio` si no es
    audio: antes se aceptaba cualquier archivo y se le pasaba tal cual a Rust
    para reproducirlo (y a mutagen para leerlo).
    """
    path = os.path.abspath(str(path))
    if os.path.splitext(path)[1].lower() not in config.EXTENSIONS:
        raise NotAudio("eso no es un archivo de audio")
    if not os.path.isfile(path):
        return None
    with library.connect() as conn:
        cid = _rust.external_played(conn, path)
    return resolve(cid)


def describe(paths, limit: int = 1000) -> list[dict | None]:
    """Las canciones de esas rutas (de la biblioteca, o de fuera con su id
    negativo), sin apuntarlas como escuchadas: al abrir un archivo, el resto
    de su carpeta va a la cola, pero solo suena (y sube a la lista) el que
    se abrio. None para lo que no es audio o ya no esta."""
    out: list[dict | None] = []
    for raw in list(paths)[:limit]:
        path = os.path.abspath(str(raw))
        if os.path.splitext(path)[1].lower() not in config.EXTENSIONS or not os.path.isfile(path):
            out.append(None)
            continue
        cid = _register(path)
        out.append(resolve(cid) if cid is not None else None)
    return out


def listing(limit: int = 500, light: bool = False) -> list[dict]:
    """La lista, de lo ultimo que sono a lo mas antiguo.

    Con una sola conexion y dos consultas: antes se resolvia cancion a
    cancion, y eran hasta quinientas aperturas de la base por peticion. Con
    `light`, filas ligeras (ver `library.HEAVY_COLUMNS`), que es lo que da la API.
    """
    with library.connect() as conn:
        return _rust.external_listing(conn, int(limit), bool(light))


def forget(cid: int) -> bool:
    """Quita una cancion de la lista. El archivo no se toca."""
    with library.connect() as conn:
        return _rust.external_forget(conn, int(cid))


def clear() -> int:
    """Descarta la lista entera. Devuelve cuantas habia.

    Los archivos siguen donde estaban, y lo que hayas guardado antes en una
    lista de reproduccion sigue ahi y se sigue oyendo: esto solo vacia la
    lista suelta del reproductor.
    """
    with library.connect() as conn:
        return _rust.external_clear(conn)


def _sweep(conn) -> None:
    """Borra las entradas de fuera que ya no cuelgan de ningun sitio (lo que
    este en una lista guardada se respeta)."""
    _rust.external_sweep(conn)


def save_as_playlist(name: str) -> dict:
    """Guarda la lista de ahora como una lista de reproduccion de DanPlay.

    En el mismo orden en que se ve, que es el de escucha. Las canciones de
    fuera **no se importan**: la lista las nombra por su id y siguen viviendo
    donde vivian.
    """
    from . import playlists  # aqui dentro, para no dar un ciclo

    songs = listing()
    if not songs:
        raise ValueError("la lista esta vacia")
    lista = playlists.create(name)
    playlists.add(lista["id"], [s["id"] for s in songs])
    return {**lista, "n": len(songs)}
