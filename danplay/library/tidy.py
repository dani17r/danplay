"""«Transformar»: cada cancion en su sitio y con su nombre, una a una.

La regla de la casa es la de las descargas (`ingest`):
Artistas/<Artista>/<Artista - Titulo>.ext. Aqui se mira cada cancion de la
biblioteca y se propone como quedaria; nada se toca hasta que quien escucha
dice que si, cancion por cancion (o todas las seguras de una vez).

Medido en una biblioteca de verdad (281 canciones, octubre de 2026): con
los nombres sacados de las etiquetas, 118 «no seguian la regla», pero
renombrarlas asi las dejaba PEOR muchas veces: canales de YouTube como
artista («La Mejor Musica Cristiana»), webs pegadas al titulo
(«CeeNaija.com»), el «(ft. X)» perdido. Por eso:

- solo se tocan Artistas/ y lo suelto en la raiz de la biblioteca: las
  carpetas propias (Pistas, Tutoriales y Play Along...) y las demas
  carpetas de musica, no;
- el nombre que ya tiene la forma «Artista - Titulo» manda: solo se le
  quita ruido (video oficial, «letra», el numero de pista);
- una cancion de Artistas/<X>/ es de X: el artista sale de la carpeta;
- lo que solo sale de las etiquetas se propone para revisar, no como
  seguro.

Con esas reglas, de esas 281 salen 19 propuestas, todas con sentido (un
disco metido en su subcarpeta, con los numeros de pista en el nombre).
"""

import os

import danplay_core as _rust

from .. import config, names
from .songs import by_id, edit, relocate


def _vocab(vocab: dict | None = None) -> list:
    vocab = names.vocabulary(config.ARTISTS_DIR) if vocab is None else vocab
    return list(vocab.items())


def propose(song: dict, vocab: dict | None = None) -> dict | None:
    """Como quedaria `song` (una fila del indice), o None si esta bien o no
    es de las que se tocan. {"id", "from", "to" (rutas dentro de la
    biblioteca), "artist", "title", "why": [...], "sure"}. Lo decide Rust
    (biblioteca/src/indice/ordenar.rs), con las reglas de arriba."""
    return _rust.tidy_propose(dict(song), _vocab(vocab), str(config.LIBRARY))


def proposals() -> dict:
    """Lo que «Transformar» propondria ahora, las seguras primero.
    {"items": [...], "looked": cuantas se miraron, "total": cuantas hay}."""
    from . import db

    vocab = _vocab()
    with db.connect() as conn:
        return _rust.tidy_proposals(conn, vocab, str(config.LIBRARY))


def target_of(artist: str, title: str, ext: str) -> str:
    """Donde va (dentro de la biblioteca) una cancion de `artist` llamada
    `title`: Artistas/<Artista>/<Artista - Titulo>.ext. La carpeta del
    artista, la que ya haya si es el mismo nombre con otras mayusculas o
    acentos. `ValueError` si falta el artista o el titulo."""
    return _rust.tidy_target_of(artist, title, ext, _vocab())


def apply(
    cid: int,
    *,
    artist: str | None = None,
    title: str | None = None,
    to: str | None = None,
    write_tags: bool = False,
) -> dict:
    """La pone en su sitio. Con `artist` y `title` (los de la propuesta, o
    corregidos a mano) va a Artistas/<Artista>/<Artista - Titulo>; con `to`,
    a esa ruta de la biblioteca (para deshacer). Con `write_tags` (quien
    escucha los corrigio), tambien a sus etiquetas: si no, se quedan como
    estan, que el titulo de la propuesta sale del nombre del archivo y la
    etiqueta puede decir mas («(Live)»).
    Devuelve {"song", "from", "to"} (rutas absolutas). `ValueError` si algo
    no vale; `OSError` si el disco no deja (en Windows, una cancion abierta
    por otro programa no se puede mover)."""
    song = by_id(cid)
    if not song:
        raise ValueError("no existe esa cancion")
    library = os.path.abspath(config.LIBRARY)
    ext = os.path.splitext(song["file"])[1]
    if to:
        rel = os.path.normpath(to)
        if os.path.isabs(rel) or rel.startswith(".."):
            raise ValueError("esa ruta esta fuera de tu musica")
    elif artist and title:
        rel = target_of(artist, title, ext)
    else:
        raise ValueError("hace falta el artista y el título, o la ruta")
    src = song["path"]
    dest = os.path.join(library, rel)
    moved = relocate(cid, os.path.dirname(dest), os.path.basename(dest))
    fields = {}
    if write_tags and artist and artist.strip() != (song.get("artist") or ""):
        fields["artist"] = artist.strip()
    if write_tags and title and title.strip() != (song.get("title") or ""):
        fields["title"] = title.strip()
    if fields:
        moved = edit(cid, **fields) or moved
    return {"song": moved, "from": src, "to": moved["path"]}
