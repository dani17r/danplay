# -*- coding: utf-8 -*-
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
"""
import logging, os, time
from . import library, tags

log = logging.getLogger("danplay")

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


def _row(r) -> dict:
    """Una fila de `external_songs` con la forma que espera la interfaz."""
    d = dict(r)
    d["id"] = -int(d["id"])
    # Campos que toda cancion tiene en la interfaz. Una de fuera no los tiene
    # y no los va a tener —no se le escribe nada dentro—, pero es mejor un
    # cero que un hueco que rompe la pantalla.
    d.update(stars=0, favorite=0, blur=0, external=True,
             folder="", year="", genre="", key="", bpm=0, bitrate=0, size=0)
    return d


def by_id(cid: int) -> dict | None:
    """La cancion de fuera con ese id (negativo), o None."""
    if cid >= 0:
        return None
    conn = library.connect()
    r = conn.execute(f"SELECT {','.join(COLUMNS)} FROM external_songs WHERE id=?",
                     (-cid,)).fetchone()
    conn.close()
    return _row(r) if r else None


def resolve(cid: int) -> dict | None:
    """La cancion con ese id, venga de donde venga.

    Es lo que hay que usar en todo sitio que solo necesite REPRODUCIR o
    ENSEÑAR una cancion: la ruta del audio, la portada, una lista. Para
    modificar algo, `library.by_id`, y asi lo de fuera queda fuera de alcance.
    """
    return library.by_id(cid) if cid > 0 else by_id(cid)


def _register(path: str) -> int | None:
    """El id de esa ruta, creando la entrada de fuera si hace falta."""
    song = library.by_path(path)
    if song:
        return song["id"]                       # ya esta en la biblioteca
    conn = library.connect()
    row = conn.execute("SELECT id FROM external_songs WHERE path=?", (path,)).fetchone()
    if row:
        rowid = row["id"]
    else:
        # Las etiquetas se leen UNA vez, la primera. Esto se llama en cada
        # play, y abrir el archivo cada vez seria pagar disco por nada.
        t = tags.read(path)
        name = os.path.splitext(os.path.basename(path))[0]
        rowid = conn.execute(
            "INSERT INTO external_songs (path,file,title,artist,album,duration) "
            "VALUES (?,?,?,?,?,?)",
            (path, os.path.basename(path), t.get("title") or name,
             t.get("artist", ""), t.get("album", ""), tags.duration(path))).lastrowid
        conn.commit()
    conn.close()
    return -rowid


def played(path) -> dict | None:
    """Apunta que esa ruta acaba de sonar y devuelve la cancion.

    Si ya estaba en la lista NO se añade otra vez: se le pone la hora nueva y
    con eso sube al principio.
    """
    path = os.path.abspath(str(path))
    if not os.path.isfile(path):
        return None
    cid = _register(path)
    if cid is None:
        return None
    conn = library.connect()
    conn.execute("INSERT INTO recent (song_id, played) VALUES (?,?) "
                 "ON CONFLICT(song_id) DO UPDATE SET played=excluded.played",
                 (cid, time.time()))
    conn.commit()
    conn.close()
    return resolve(cid)


def listing(limit: int = 500) -> list[dict]:
    """La lista, de lo ultimo que sono a lo mas antiguo."""
    conn = library.connect()
    ids = [r["song_id"] for r in conn.execute(
        "SELECT song_id FROM recent ORDER BY played DESC LIMIT ?", (int(limit),))]
    conn.close()
    # Lo que ya no se puede resolver no se enseña, pero tampoco se borra: una
    # unidad desconectada no es motivo para olvidar nada.
    return [s for s in (resolve(cid) for cid in ids) if s]


def forget(cid: int) -> bool:
    """Quita una cancion de la lista. El archivo no se toca."""
    conn = library.connect()
    n = conn.execute("DELETE FROM recent WHERE song_id=?", (int(cid),)).rowcount
    _sweep(conn)
    conn.commit()
    conn.close()
    return n > 0


def clear() -> int:
    """Descarta la lista entera. Devuelve cuantas habia.

    Los archivos siguen donde estaban, y lo que hayas guardado antes en una
    lista de reproduccion sigue ahi y se sigue oyendo: esto solo vacia la
    lista suelta del reproductor.
    """
    conn = library.connect()
    n = conn.execute("DELETE FROM recent").rowcount
    _sweep(conn)
    conn.commit()
    conn.close()
    return n


def _sweep(conn) -> None:
    """Borra las entradas de fuera que ya no cuelgan de ningun sitio.

    Sin esto la tabla solo crece: cada archivo abierto alguna vez se quedaria
    ahi para siempre. Se respeta lo que este en una lista guardada, que la
    nombra por su id.
    """
    guardadas = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='playlist_songs'"
    ).fetchone()
    vivas = "SELECT -song_id FROM recent WHERE song_id<0"
    if guardadas:
        vivas += " UNION SELECT -song_id FROM playlist_songs WHERE song_id<0"
    conn.execute(f"DELETE FROM external_songs WHERE id NOT IN ({vivas})")


def save_as_playlist(name: str) -> dict:
    """Guarda la lista de ahora como una lista de reproduccion de DanPlay.

    En el mismo orden en que se ve, que es el de escucha. Las canciones de
    fuera **no se importan**: la lista las nombra por su id y siguen viviendo
    donde vivian.
    """
    from . import playlists                     # aqui dentro, para no dar un ciclo
    songs = listing()
    if not songs:
        raise ValueError("la lista esta vacia")
    lista = playlists.create(name)
    playlists.add(lista["id"], [s["id"] for s in songs])
    return {**lista, "n": len(songs)}
