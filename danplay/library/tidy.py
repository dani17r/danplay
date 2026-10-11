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

from .. import config, names
from . import db
from .songs import by_id, edit, relocate

# Lo que «Transformar» mira: lo suelto en la raiz de la biblioteca y lo que
# cuelga de Artistas/. El resto son carpetas de quien escucha.
_ARTISTS = "Artistas"


def _split(stem: str) -> tuple[str | None, str]:
    """«Artista - Titulo» -> (artista, titulo); sin guion, (None, todo)."""
    if " - " in stem:
        artist, title = stem.split(" - ", 1)
        if artist.strip() and title.strip():
            return artist.strip(), title.strip()
    return None, stem.strip()


def _category_folder(title: str) -> str | None:
    """La carpeta de su tipo si el nombre lo dice sin duda (una Drum Cam, un
    tutorial, una secuencia): `names.video_kind`, la misma regla que las
    descargas."""
    from ..ingest import CATEGORY_FOLDER

    kind = names.video_kind(title)
    return CATEGORY_FOLDER.get(kind["category"]) if kind else None


def propose(song: dict, vocab: dict | None = None) -> dict | None:
    """Como quedaria `song` (una fila del indice), o None si esta bien o no
    es de las que se tocan. {"id", "from", "to" (rutas dentro de la
    biblioteca), "artist", "title", "why": [...], "sure"}."""
    library = os.path.abspath(config.LIBRARY)
    if os.path.normcase(os.path.abspath(song.get("root") or "")) != os.path.normcase(library):
        return None
    path = song.get("path") or ""
    rel = os.path.relpath(path, library)
    parts = rel.split(os.sep)
    loose = len(parts) == 1
    in_artists = len(parts) >= 3 and parts[0] == _ARTISTS
    if not (loose or in_artists):
        return None
    vocab = vocab if vocab is not None else names.vocabulary(config.ARTISTS_DIR)
    file = song.get("file") or os.path.basename(path)
    stem, ext = os.path.splitext(file)
    cleaned = names.clean(stem) or stem
    # solo espacios o mayusculas («#EternoLive» -> «#Eterno Live»): no es
    # ruido, y no merece mover nada
    if cleaned.replace(" ", "").casefold() == stem.replace(" ", "").casefold():
        cleaned = stem
    name_artist, title = _split(cleaned)
    why: list[str] = []
    sure = True
    if loose:
        why.append("suelta en la raíz de tu música")
    elif len(parts) > 3:
        why.append("dentro de una subcarpeta")
    folder_artist = parts[1] if in_artists else None
    if folder_artist:
        artist_dir = folder_artist
    elif name_artist:
        artist_dir = vocab.get(names._flat(name_artist), names.sanitize(name_artist))
    elif song.get("artist"):
        # tal cual (solo lo que no vale en un nombre de archivo): `clean`
        # esta hecho para nombres de archivo y partiria «YouTube»
        tagged = song["artist"].strip()
        artist_dir = vocab.get(names._flat(tagged), names.sanitize(tagged))
        why.append("el artista sale de sus etiquetas: revísalo")
        sure = False
    else:
        return None
    if not title:
        return None
    file_artist = name_artist or artist_dir
    if not name_artist:
        why.append("al nombre le falta el artista")
    elif cleaned != stem:
        why.append("ruido en el nombre")
    new_name = names.sanitize(f"{file_artist} - {title}") + ext.lower()
    target_dir = os.path.join(_ARTISTS, artist_dir)
    # una Drum Cam, un tutorial, una secuencia: a su carpeta, con su nombre
    kind_folder = _category_folder(stem)
    if kind_folder:
        target_dir, new_name = kind_folder, file
        why = [f"es de {kind_folder}"]
        sure = False
    new_rel = os.path.join(target_dir, new_name)
    if new_rel == rel:
        return None
    same_dir = os.path.dirname(new_rel) == os.path.dirname(rel)
    if same_dir and names.match_key(new_name) == names.match_key(file) and not kind_folder:
        return None  # solo mayusculas o espacios: no merece mover nada
    return {
        "id": song["id"],
        "from": rel,
        "to": new_rel,
        "artist": file_artist if not kind_folder else (song.get("artist") or ""),
        "title": title,
        "why": why,
        "sure": sure,
    }


def proposals() -> dict:
    """Lo que «Transformar» propondria ahora, las seguras primero.
    {"items": [...], "looked": cuantas se miraron, "total": cuantas hay}."""
    vocab = names.vocabulary(config.ARTISTS_DIR)
    with db.connect() as conn:
        rows = [dict(r) for r in conn.execute("SELECT id, path, root, file, artist FROM songs")]
    items = [p for p in (propose(r, vocab) for r in rows) if p]
    items.sort(key=lambda p: (not p["sure"], p["from"].lower()))
    library = os.path.normcase(os.path.abspath(config.LIBRARY))
    looked = sum(1 for r in rows if os.path.normcase(os.path.abspath(r["root"] or "")) == library)
    return {"items": items, "looked": looked, "total": len(rows)}


def target_of(artist: str, title: str, ext: str) -> str:
    """Donde va (dentro de la biblioteca) una cancion de `artist` llamada
    `title`: Artistas/<Artista>/<Artista - Titulo>.ext. La carpeta del
    artista, la que ya haya si es el mismo nombre con otras mayusculas o
    acentos."""
    artist, title = names.sanitize(artist.strip()), names.sanitize(title.strip())
    if not artist or not title:
        raise ValueError("hace falta el artista y el título")
    vocab = names.vocabulary(config.ARTISTS_DIR)
    folder = vocab.get(names._flat(artist), artist)
    return os.path.join(_ARTISTS, folder, f"{artist} - {title}{ext.lower()}")


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
