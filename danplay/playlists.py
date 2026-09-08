# -*- coding: utf-8 -*-
"""Listas de reproduccion, favoritos y valoraciones.

Todo lo que se puede guardar dentro del archivo, se guarda ahi tambien
(estrellas en POPM, favorito y listas en TXXX), para que la biblioteca
siga siendo portatil aunque se pierda la base de datos.
"""
import logging, os, time
from pathlib import Path
from . import external, library, names, tags

log = logging.getLogger("danplay")

SCHEMA = """
CREATE TABLE IF NOT EXISTS playlists (
    id      INTEGER PRIMARY KEY,
    name  TEXT UNIQUE,
    note    TEXT DEFAULT '',
    color   TEXT DEFAULT '',
    created  REAL
);
CREATE TABLE IF NOT EXISTS playlist_songs (
    playlist_id   INTEGER,
    song_id INTEGER,
    position      INTEGER,
    added   REAL,
    PRIMARY KEY (playlist_id, song_id)
);
CREATE INDEX IF NOT EXISTS i_lc ON playlist_songs(playlist_id, position);
"""


# Las tablas de las listas viven en la misma base. Se declaran una vez y las
# crea `library.connect()` junto a las suyas. Antes cada operacion de lista
# reejecutaba este esquema Y la migracion completa, encima de lo que ya hacia
# connect(): dos pasadas de esquema por cada clic en un repertorio.
library.register_schema(SCHEMA)


def _connect():
    return library.connect()


# ---------------------------------------------------------------- listas

def create(name, note="", color="") -> dict:
    """Crea la lista, o devuelve la que ya habia con ese nombre.

    Devuelve {"id", "name", "created"}. `created` es False si la lista ya
    existia: antes se devolvia solo el id y quien llamaba no podia saberlo,
    asi que el asistente «creaba» una lista y en realidad añadia a otra.
    """
    name = str(name or "").strip()
    conn = _connect()
    row = conn.execute("SELECT id FROM playlists WHERE name=?", (name,)).fetchone()
    if row:
        conn.close()
        return {"id": row["id"], "name": name, "created": False}
    cur = conn.execute("INSERT INTO playlists (name,note,color,created) "
                       "VALUES (?,?,?,?)", (name, note, color, time.time()))
    lid = cur.lastrowid
    conn.commit(); conn.close()
    return {"id": lid, "name": name, "created": True}


def remove(playlist_id) -> None:
    conn = _connect()
    members = [r["song_id"] for r in conn.execute(
        "SELECT song_id FROM playlist_songs WHERE playlist_id=?", (playlist_id,))]
    conn.execute("DELETE FROM playlist_songs WHERE playlist_id=?", (playlist_id,))
    conn.execute("DELETE FROM playlists WHERE id=?", (playlist_id,))
    conn.commit(); conn.close()
    # los archivos dejan de nombrar la lista: si no, un reescaneo la resucitaria
    _stamp_playlists_into_files(members)


def rename_folder(playlist_id, name) -> None:
    conn = _connect()
    conn.execute("UPDATE playlists SET name=? WHERE id=?", (name, playlist_id))
    conn.commit(); conn.close()


def list_all() -> list[dict]:
    conn = _connect()
    # La duracion suma las dos procedencias: las de la biblioteca y las de
    # fuera. Con un solo JOIN a `songs`, una lista guardada desde el
    # reproductor salia con «0 min» aunque tuviera veinte canciones.
    rows = conn.execute(
        "SELECT l.*, (SELECT COUNT(*) FROM playlist_songs lc WHERE lc.playlist_id=l.id) n, "
        "(SELECT COALESCE(SUM(c.duration),0) FROM playlist_songs lc "
        " JOIN songs c ON c.id=lc.song_id WHERE lc.playlist_id=l.id) "
        "+ (SELECT COALESCE(SUM(e.duration),0) FROM playlist_songs lc "
        "   JOIN external_songs e ON e.id=-lc.song_id WHERE lc.playlist_id=l.id) seconds "
        "FROM playlists l ORDER BY l.name").fetchall()
    conn.close()
    return [dict(f) for f in rows]


def add(playlist_id, song_ids) -> int:
    if isinstance(song_ids, int):
        song_ids = [song_ids]
    conn = _connect()
    row = conn.execute("SELECT COALESCE(MAX(position),-1) m FROM playlist_songs "
                       "WHERE playlist_id=?", (playlist_id,)).fetchone()
    sort = row["m"] + 1
    n = 0
    for cid in song_ids:
        cur = conn.execute("INSERT OR IGNORE INTO playlist_songs VALUES (?,?,?,?)",
                          (playlist_id, cid, sort, time.time()))
        # las que ya estaban no cuentan: si no, la app decia «Añadida a la
        # lista» aunque no hubiera añadido nada
        if cur.rowcount:
            sort += 1; n += 1
    conn.commit(); conn.close()
    _stamp_playlists_into_files(song_ids)
    return n


def remove_song(playlist_id, song_ids) -> None:
    if isinstance(song_ids, int):
        song_ids = [song_ids]
    conn = _connect()
    for cid in song_ids:
        conn.execute("DELETE FROM playlist_songs WHERE playlist_id=? AND song_id=?",
                    (playlist_id, cid))
    conn.commit(); conn.close()
    _stamp_playlists_into_files(song_ids)


def reorder(playlist_id, ordered_song_ids) -> None:
    conn = _connect()
    for i, cid in enumerate(ordered_song_ids):
        conn.execute("UPDATE playlist_songs SET position=? WHERE playlist_id=? AND song_id=?",
                    (i, playlist_id, cid))
    conn.commit(); conn.close()


def songs(playlist_id) -> list[dict]:
    """Las canciones de la lista, en su orden.

    Una lista puede llevar canciones de la biblioteca (id positivo) y
    canciones de fuera de ella (id negativo, ver `external.py`), asi que no
    vale el JOIN con `songs` de toda la vida: las de fuera se caian por el
    camino sin decir nada.
    """
    conn = _connect()
    order = conn.execute(
        "SELECT song_id, position FROM playlist_songs WHERE playlist_id=? "
        "ORDER BY position", (playlist_id,)).fetchall()
    inside = {r["id"]: dict(r) for r in conn.execute(
        "SELECT c.* FROM playlist_songs lc JOIN songs c ON c.id=lc.song_id "
        "WHERE lc.playlist_id=?", (playlist_id,)).fetchall()}
    conn.close()

    out = []
    for r in order:
        cid = r["song_id"]
        song = inside.get(cid) if cid > 0 else external.by_id(cid)
        if song:                       # si el archivo ya no esta, no se enseña
            out.append({**song, "position": r["position"]})
    return out


def playlists_of(song_id) -> list[str]:
    conn = _connect()
    rows = conn.execute("SELECT l.name FROM playlist_songs lc JOIN playlists l "
                        "ON l.id=lc.playlist_id WHERE lc.song_id=? ORDER BY l.name",
                        (song_id,)).fetchall()
    conn.close()
    return [f["name"] for f in rows]


def _stamp_playlists_into_files(song_ids) -> None:
    """Escribe TXXX:LISTAS dentro del mp3 para que sobreviva a la base de datos.

    Las rutas y los nombres de lista se piden de una vez. Antes se abrian dos
    conexiones POR CANCION (una para la ruta y otra para sus listas): meter
    treinta temas en un repertorio eran sesenta aperturas de la base.
    """
    # Solo las de la biblioteca. Escribir dentro de un archivo de fuera seria
    # justo lo que DanPlay promete no hacer: si lo abriste desde el
    # explorador, es tuyo y se queda como esta. El `IN (...)` de abajo ya no
    # los encontraria, pero mas vale decirlo que dejarlo al azar del SQL.
    song_ids = [int(i) for i in song_ids if int(i) > 0]
    if not song_ids:
        return
    placeholders = ",".join("?" * len(song_ids))
    conn = _connect()
    paths = {r["id"]: r["path"] for r in conn.execute(
        f"SELECT id, path FROM songs WHERE id IN ({placeholders})", song_ids)}
    lists: dict = {}
    for r in conn.execute(
            f"SELECT lc.song_id id, l.name name FROM playlist_songs lc "
            f"JOIN playlists l ON l.id=lc.playlist_id "
            f"WHERE lc.song_id IN ({placeholders}) ORDER BY l.name", song_ids):
        lists.setdefault(r["id"], []).append(r["name"])
    conn.close()
    for cid in song_ids:
        path = paths.get(cid)
        if path and os.path.exists(path):
            if not tags.set_playlists(path, lists.get(cid, [])):
                log.warning("no se pudo apuntar las listas dentro de %s", path)


# ------------------------------------------------------- estrellas y favoritos

def rate(song_id, stars: int) -> bool:
    c = library.by_id(song_id)
    if not c:
        return False
    ok = tags.rate(c["path"], stars)
    conn = _connect()
    conn.execute("UPDATE songs SET stars=? WHERE id=?",
                (max(0, min(5, int(stars))), song_id))
    conn.commit(); conn.close()
    return ok


def favorite(song_id, value=True) -> bool:
    c = library.by_id(song_id)
    if not c:
        return False
    ok = tags.set_favorite(c["path"], value)
    conn = _connect()
    conn.execute("UPDATE songs SET favorite=? WHERE id=?", (1 if value else 0, song_id))
    conn.commit(); conn.close()
    return ok


def favorites() -> list[dict]:
    conn = _connect()
    rows = conn.execute("SELECT * FROM songs WHERE favorite=1 "
                        "ORDER BY artist, title").fetchall()
    conn.close()
    return [dict(f) for f in rows]


# ---------------------------------------------------------------- m3u

def export_folder() -> Path:
    return library.config.LIBRARY / "Listas"


def export_m3u(playlist_id, target=None) -> str:
    """Escribe la lista como .m3u8 en LIBRARY/Listas/.

    El nombre de archivo sale del nombre de la lista pasado por
    `names.sanitize`, y el resultado tiene que quedar DENTRO de Listas/: el
    nombre lo puede fijar el asistente, y «../../x» escribia donde quisiera.
    Un nombre que no de un archivo valido, o un `target` fuera de la
    carpeta, levantan ValueError.
    """
    conn = _connect()
    row = conn.execute("SELECT name FROM playlists WHERE id=?", (playlist_id,)).fetchone()
    conn.close()
    if not row:
        raise ValueError("no existe esa lista")
    folder = export_folder()
    if target is None:
        safe = names.sanitize(str(row["name"]).replace("/", " ").replace("\\", " "))
        if not safe or safe in (".", "..") or ".." in safe.split():
            raise ValueError("el nombre de la lista no sirve como nombre de archivo")
        target = folder / f"{safe}.m3u8"
    target = Path(target)
    folder.mkdir(parents=True, exist_ok=True)
    if not library._inside(target.parent, folder) or target.name in ("", ".", ".."):
        raise ValueError("la lista solo se exporta dentro de la carpeta Listas")
    lines = ["#EXTM3U"]
    for c in songs(playlist_id):
        lines.append(f"#EXTINF:{int(c['duration'])},{c['artist']} - {c['title']}")
        lines.append(os.path.relpath(c["path"], target.parent))
    target.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return str(target)


def import_m3u(path) -> int:
    """Crea una lista a partir de un .m3u/.m3u8 existente."""
    path = Path(path)
    lid = create(path.stem)["id"]
    base = path.parent
    ids = []
    conn = _connect()
    for line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        p = str((base / line).resolve()) if not os.path.isabs(line) else line
        f = conn.execute("SELECT id FROM songs WHERE path=?", (p,)).fetchone()
        if f:
            ids.append(f["id"])
    conn.close()
    return add(lid, ids) if ids else 0
