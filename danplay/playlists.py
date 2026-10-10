"""Listas de reproduccion, favoritos y valoraciones.

Todo lo que se puede guardar dentro del archivo, se guarda ahi tambien
(estrellas en POPM, favorito y listas en TXXX), para que la biblioteca
siga siendo portatil aunque se pierda la base de datos.
"""

import html
import json
import logging
import os
import threading
import time
from pathlib import Path

from . import cifrados, external, library, names, tags, theory

log = logging.getLogger(__name__)

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
-- «¿en que listas esta esta cancion?» (la ficha, las etiquetas LISTAS, el
-- escaneo): sin esto era recorrer la tabla entera por cada cancion
CREATE INDEX IF NOT EXISTS i_playlist_songs_song ON playlist_songs(song_id);
"""


# Las tablas de las listas viven en la misma base. Se declaran una vez y las
# crea `library.connect()` junto a las suyas. Antes cada operacion de lista
# reejecutaba este esquema Y la migracion completa, encima de lo que ya hacia
# connect(): dos pasadas de esquema por cada clic en un repertorio.
library.register_schema(SCHEMA)


def _connect():
    return library.connect()


# Un solo escritor de listas a la vez DENTRO del proceso. Casi todo lo que se
# hace con una lista es leer, calcular y escribir (insertar donde toca,
# renumerar, dejarla exacta), y la tocan a la vez el turno de descarga, el
# turno del chat y arrastrar una cancion en la interfaz: leian la misma foto
# de la lista y se pisaban, con posiciones repetidas y canciones perdidas. Es
# un RLock porque unas operaciones se apoyan en otras (`set_songs`) y porque
# quien necesite leer y escribir sin que nadie se cuele puede envolverlo todo
# en `with playlists.LOCK:`. Frente a OTROS escritores de la base (el
# escaneo, otro proceso) vale la transaccion de `_begin`.
LOCK = threading.RLock()

# Variables por consulta: las SQLite viejas admiten 999.
_CHUNK = 500


def _begin():
    """Una conexion con la transaccion de escritura ya abierta.

    `BEGIN IMMEDIATE` pide el permiso de escribir ANTES de leer: lo leido no
    cambia hasta confirmar. Con `with`, confirma al salir (o deshace si algo
    fallo) y cierra.
    """
    conn = _connect()
    try:
        conn.execute("BEGIN IMMEDIATE")
    except BaseException:
        conn.close()
        raise
    return conn


def _ints(values) -> list[int]:
    """Los valores que son un entero, en su orden; lo demas se descarta."""
    out = []
    for v in values or ():
        try:
            out.append(int(v))
        except (TypeError, ValueError):
            continue
    return out


def _unique(values) -> list:
    """Sin repetidos, dejando cada valor donde aparece por primera vez."""
    return list(dict.fromkeys(values))


def _alive(conn, ids) -> set[int]:
    """De esos ids, los que son una cancion de verdad.

    Positivos: la biblioteca. Negativos: archivos abiertos desde fuera
    (`external.py`), si esa tabla existe.
    """
    found: set[int] = set()
    inside = [i for i in ids if i > 0]
    outside = [-i for i in ids if i < 0]
    for k in range(0, len(inside), _CHUNK):
        part = inside[k : k + _CHUNK]
        marks = ",".join("?" * len(part))
        sql = f"SELECT id FROM songs WHERE id IN ({marks})"  # noqa: S608
        found |= {r["id"] for r in conn.execute(sql, part)}
    if (
        outside
        and conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='external_songs'"
        ).fetchone()
    ):
        for k in range(0, len(outside), _CHUNK):
            part = outside[k : k + _CHUNK]
            marks = ",".join("?" * len(part))
            sql = f"SELECT id FROM external_songs WHERE id IN ({marks})"  # noqa: S608
            found |= {-r["id"] for r in conn.execute(sql, part)}
    return found


def _rows(conn, playlist_id) -> list[tuple[int, int]]:
    """(cancion, posicion) de TODAS las filas de la lista, en su orden.

    Tambien las de canciones cuyo archivo se fue: siguen aqui para volver a su
    sitio (ver library._to_missing). Si dos empatan en posicion, va antes la
    fila mas vieja, que es el orden en que las enseña `songs`.
    """
    return [
        (r["song_id"], r["position"])
        for r in conn.execute(
            "SELECT song_id, position FROM playlist_songs WHERE playlist_id=? "
            "ORDER BY position, rowid",
            (playlist_id,),
        )
    ]


def _settle(conn, playlist_id, head, rows) -> tuple[list[int], list[int], int]:
    """Deja las filas de la lista en las posiciones 0..n-1, sin repetir ninguna.

    Primero `head`, en ese orden (las que no estaban se insertan); detras, las
    que ya habia y `head` no nombra, en el orden que tenian. `rows` es lo que
    devolvio `_rows`. Solo se escribe lo que cambia. Devuelve (el orden final,
    las insertadas, cuantas filas se movieron). No confirma.
    """
    have = dict(rows)
    named = set(head)
    final = [*head, *(song for song, _ in rows if song not in named)]
    now = time.time()
    moves: list[tuple[int, int, int]] = []
    new: list[tuple[int, int, int, float]] = []
    for pos, song in enumerate(final):
        if song not in have:
            new.append((playlist_id, song, pos, now))
        elif have[song] != pos:
            moves.append((pos, playlist_id, song))
    if moves:
        conn.executemany(
            "UPDATE playlist_songs SET position=? WHERE playlist_id=? AND song_id=?", moves
        )
    if new:
        conn.executemany(
            "INSERT INTO playlist_songs (playlist_id, song_id, position, added) VALUES (?,?,?,?)",
            new,
        )
    return final, [song for _, song, _, _ in new], len(moves)


# ---------------------------------------------------------------- listas


def create(name, note="", color="") -> dict:
    """Crea la lista, o devuelve la que ya habia con ese nombre.

    Devuelve {"id", "name", "created"}. `created` es False si la lista ya
    existia: antes se devolvia solo el id y quien llamaba no podia saberlo,
    asi que el asistente «creaba» una lista y en realidad añadia a otra.
    """
    name = str(name or "").strip()
    with LOCK:
        # sin pedir el permiso de escribir de antemano: si la lista ya esta no
        # se escribe nada, y no tiene que esperar a un escaneo en marcha
        with _connect() as conn:
            row = conn.execute("SELECT id FROM playlists WHERE name=?", (name,)).fetchone()
            if row:
                return {"id": row["id"], "name": name, "created": False}
            cur = conn.execute(
                "INSERT INTO playlists (name,note,color,created) VALUES (?,?,?,?)",
                (name, note, color, time.time()),
            )
            lid = cur.lastrowid
        library._touch()
    return {"id": lid, "name": name, "created": True}


def remove(playlist_id) -> None:
    with LOCK:
        with _begin() as conn:
            members = [
                r["song_id"]
                for r in conn.execute(
                    "SELECT song_id FROM playlist_songs WHERE playlist_id=?", (playlist_id,)
                )
            ]
            conn.execute("DELETE FROM playlist_songs WHERE playlist_id=?", (playlist_id,))
            conn.execute("DELETE FROM playlists WHERE id=?", (playlist_id,))
        library._touch()
        # los archivos dejan de nombrar la lista: si no, un reescaneo la resucitaria
        _stamp_playlists_into_files(members)


def rename_folder(playlist_id, name) -> None:
    with LOCK:
        with _connect() as conn:
            conn.execute("UPDATE playlists SET name=? WHERE id=?", (name, playlist_id))
        library._touch()


def by_id(playlist_id) -> dict | None:
    """La lista con ese id, con cuantos temas tiene, o None.

    Solo esa: antes se calculaba `list_all()` entero (con la suma de
    duraciones de todas las listas) para quedarse con una."""
    try:
        playlist_id = int(playlist_id)
    except (TypeError, ValueError):
        return None
    found = _summaries("WHERE l.id=?", (playlist_id,))
    return found[0] if found else None


def by_name(name: str) -> dict | None:
    """La lista que se llama asi, sin distinguir mayusculas ni tildes.

    El asistente conoce las listas por su nombre («Herlin»), no por su id; y
    cuando adivinaba el id acababa borrando la lista equivocada.
    """
    key = names._flat(str(name or ""))
    if not key:
        return None
    with _connect() as conn:
        rows = conn.execute("SELECT id, name FROM playlists ORDER BY name").fetchall()
    match = next((r["id"] for r in rows if names._flat(r["name"]) == key), None)
    return by_id(match) if match is not None else None


def edit(playlist_id, name=None, note=None) -> dict | None:
    """Cambia el nombre o la nota. Devuelve la lista ya cambiada, o None."""
    with LOCK:
        current = by_id(playlist_id)
        if not current:
            return None
        fields, values = [], []
        if name is not None and str(name).strip():
            fields.append("name=?")
            values.append(str(name).strip())
        if note is not None:
            fields.append("note=?")
            values.append(str(note))
        if fields:
            with _connect() as conn:
                conn.execute(
                    f"UPDATE playlists SET {','.join(fields)} WHERE id=?",  # noqa: S608
                    [*values, current["id"]],
                )
            library._touch()
            if name is not None:
                # las canciones llevan dentro los nombres de sus listas
                _stamp_playlists_into_files([s["id"] for s in songs(current["id"])])
        return by_id(current["id"])


def set_songs(playlist_id, song_ids) -> dict:
    """Deja la lista EXACTAMENTE con esas canciones, en ese orden.

    Es «corrige la lista»: lo que sobra se quita, lo que falta se añade, y lo
    que ya estaba se queda. Devuelve cuantas se quitaron y cuantas entraron.

    Todo en UNA transaccion y bajo `LOCK` (antes eran tres, `remove_song`,
    `add` y `reorder`, con la lista a medias a la vista de quien mirase en
    medio) y con UN solo sellado de etiquetas al final. Las filas de canciones
    cuyo archivo se fue no se tocan: se quedan detras, para volver a su sitio.
    """
    with LOCK:
        with _begin() as conn:
            ints = _ints(song_ids)
            alive = _alive(conn, ints)
            wanted = _unique(i for i in ints if i in alive)
            rows = _rows(conn, playlist_id)
            shown = _alive(conn, [song for song, _ in rows])
            keep = set(wanted)
            gone = [song for song, _ in rows if song in shown and song not in keep]
            for song in gone:
                conn.execute(
                    "DELETE FROM playlist_songs WHERE playlist_id=? AND song_id=?",
                    (playlist_id, song),
                )
            removed = set(gone)
            left = [(song, pos) for song, pos in rows if song not in removed]
            _, added, moved = _settle(conn, playlist_id, wanted, left)
        if gone or added or moved:
            library._touch()
        if gone or added:
            _stamp_playlists_into_files([*gone, *added])
    return {"removed": len(gone), "added": len(added), "total": len(wanted)}


def list_all() -> list[dict]:
    return _summaries()


def _summaries(where: str = "", params: tuple = ()) -> list[dict]:
    """Las listas con cuantos temas tienen y cuanto duran (`where` filtra;
    lo escribe este modulo, nunca viene de fuera)."""
    # La duracion suma las dos procedencias: las de la biblioteca y las de
    # fuera. Con un solo JOIN a `songs`, una lista guardada desde el
    # reproductor salia con «0 min» aunque tuviera veinte canciones.
    #
    # Y se cuenta solo lo que se enseña: una cancion cuyo archivo se fue
    # sigue en `playlist_songs` (para volver a su sitio si el archivo vuelve,
    # ver library._to_missing), pero la lista no la muestra y el numero decia
    # «4» con tres.
    with _connect() as conn:
        rows = conn.execute(
            "SELECT l.*, (SELECT COUNT(*) FROM playlist_songs lc WHERE lc.playlist_id=l.id "  # noqa: S608
            " AND (EXISTS (SELECT 1 FROM songs c WHERE c.id=lc.song_id) "
            "      OR EXISTS (SELECT 1 FROM external_songs e WHERE e.id=-lc.song_id))) n, "
            "(SELECT COALESCE(SUM(c.duration),0) FROM playlist_songs lc "
            " JOIN songs c ON c.id=lc.song_id WHERE lc.playlist_id=l.id) "
            "+ (SELECT COALESCE(SUM(e.duration),0) FROM playlist_songs lc "
            "   JOIN external_songs e ON e.id=-lc.song_id WHERE lc.playlist_id=l.id) seconds "
            f"FROM playlists l {where} ORDER BY l.name",
            params,
        ).fetchall()
    return [dict(f) for f in rows]


def existing_ids(song_ids) -> list[int]:
    """De esos ids, los que son una cancion de verdad, en el mismo orden.

    Positivos: la biblioteca. Negativos: archivos abiertos desde fuera
    (`external.py`), si esa tabla existe. Lo que no este, fuera.
    """
    wanted = _ints(song_ids)
    if not wanted:
        return []
    with _connect() as conn:
        found = _alive(conn, wanted)
    return _unique(i for i in wanted if i in found)


def add(playlist_id, song_ids) -> int:
    if isinstance(song_ids, int):
        song_ids = [song_ids]
    if not _ints(song_ids):
        return 0
    with LOCK:
        with _begin() as conn:
            # Solo lo que existe. Un id que no es ninguna cancion (el asistente
            # se los inventaba) dejaba una fila huerfana: la lista decia «6
            # temas» y enseñaba cuatro. Se mira DENTRO de la transaccion, para
            # que no se borre la cancion entre mirarlo e insertarla.
            ints = _ints(song_ids)
            alive = _alive(conn, ints)
            song_ids = _unique(i for i in ints if i in alive)
            if not song_ids:
                return 0
            row = conn.execute(
                "SELECT COALESCE(MAX(position),-1) m FROM playlist_songs WHERE playlist_id=?",
                (playlist_id,),
            ).fetchone()
            sort = row["m"] + 1
            n = 0
            for cid in song_ids:
                cur = conn.execute(
                    "INSERT OR IGNORE INTO playlist_songs VALUES (?,?,?,?)",
                    (playlist_id, cid, sort, time.time()),
                )
                # las que ya estaban no cuentan: si no, la app decia «Añadida a
                # la lista» aunque no hubiera añadido nada
                if cur.rowcount:
                    sort += 1
                    n += 1
        if n:
            library._touch()
        _stamp_playlists_into_files(song_ids)
        return n


def remove_song(playlist_id, song_ids) -> None:
    if isinstance(song_ids, int):
        song_ids = [song_ids]
    with LOCK:
        with _begin() as conn:
            for cid in song_ids:
                conn.execute(
                    "DELETE FROM playlist_songs WHERE playlist_id=? AND song_id=?",
                    (playlist_id, cid),
                )
        library._touch()
        _stamp_playlists_into_files(song_ids)


def reorder(playlist_id, ordered_song_ids) -> None:
    """Pone las canciones en ese orden.

    Lo que la lista no tiene se ignora (no se inserta: eso es `place`). Lo que
    la lista tiene y no se nombra se queda detras, en su orden: antes
    conservaba su posicion de antes y repetia la de otra. Siempre queda en
    0..n-1.
    """
    wanted = _ints(ordered_song_ids)
    with LOCK:
        with _begin() as conn:
            rows = _rows(conn, playlist_id)
            have = {song for song, _ in rows}
            head = _unique(i for i in wanted if i in have)
            _settle(conn, playlist_id, head, rows)
        library._touch()


def place(playlist_id, ordered_ids) -> dict:
    """Pone esas canciones en la lista, en ese orden, de una sola vez.

    Es lo que necesita quien completa una lista con lo que acaba de bajar
    («esta nueva va justo despues de aquella»): `add` solo sabe poner al
    final, y `add` + `reorder` eran dos pasos con la lista a la vista de
    cualquiera entre medias. Aqui, en UNA transaccion y bajo `LOCK`:

    - se insertan las que falten; lo que no es una cancion (`existing_ids`:
      ni de la biblioteca ni de fuera) y lo repetido se ignora;
    - se renumeran 0..n-1 TODAS las filas de la lista: primero `ordered_ids`,
      en ese orden, y detras, en el orden que tenian, las que no se nombran
      (tambien las de fuera de la biblioteca, ids negativos: no se pierden);
    - las etiquetas de los archivos (LISTAS) se escriben UNA vez al final, solo
      de las que acaban de entrar (lo demas no cambia de lista).

    Idempotente: repetirla con lo mismo no cambia nada ni toca ningun archivo.
    Devuelve {"added": cuantas entraron, "total": cuantas canciones enseña la
    lista, "order": sus ids en el orden final, igual que daria `songs`}.
    `ValueError` si la lista no existe (nada se escribe).
    """
    try:
        playlist_id = int(playlist_id)
    except (TypeError, ValueError):
        raise ValueError("no existe esa lista") from None
    if isinstance(ordered_ids, int):
        ordered_ids = [ordered_ids]
    wanted = _ints(ordered_ids)
    with LOCK:
        with _begin() as conn:
            if not conn.execute("SELECT 1 FROM playlists WHERE id=?", (playlist_id,)).fetchone():
                raise ValueError("no existe esa lista")
            alive = _alive(conn, wanted)
            head = _unique(i for i in wanted if i in alive)
            rows = _rows(conn, playlist_id)
            final, added, moved = _settle(conn, playlist_id, head, rows)
            # solo cuenta lo que enseña la lista: las filas de canciones cuyo
            # archivo se fue siguen ahi, detras, pero no salen
            shown = _alive(conn, final)
            order = [song for song in final if song in shown]
        if added or moved:
            library._touch()
        if added:
            _stamp_playlists_into_files(added)
    return {"added": len(added), "total": len(order), "order": order}


def songs(playlist_id, light: bool = False) -> list[dict]:
    """Las canciones de la lista, en su orden.

    Una lista puede llevar canciones de la biblioteca (id positivo) y
    canciones de fuera de ella (id negativo, ver `external.py`), asi que no
    vale el JOIN con `songs` de toda la vida: las de fuera se caian por el
    camino sin decir nada. Con `light`, filas ligeras (lo que da la API).
    """
    with _connect() as conn:
        # si dos empatan en posicion (listas de antes del cerrojo), la fila mas
        # vieja primero: el mismo orden que usa `place` al renumerar
        order = conn.execute(
            "SELECT song_id, position FROM playlist_songs WHERE playlist_id=? "
            "ORDER BY position, rowid",
            (playlist_id,),
        ).fetchall()
        cols = library.light_columns(conn) if light else "c.*"
        inside = {
            r["id"]: dict(r)
            for r in conn.execute(
                f"SELECT {cols} FROM playlist_songs lc JOIN songs c ON c.id=lc.song_id "  # noqa: S608
                "WHERE lc.playlist_id=?",
                (playlist_id,),
            ).fetchall()
        }
        outside = external.by_ids(conn, [r["song_id"] for r in order])

    out = []
    for r in order:
        cid = r["song_id"]
        song = inside.get(cid) if cid > 0 else outside.get(cid)
        if song:  # si el archivo ya no esta, no se enseña
            if light:
                song = library.light(song)
            out.append({**song, "position": r["position"]})
    return out


def playlists_of(song_id) -> list[str]:
    with _connect() as conn:
        rows = conn.execute(
            "SELECT l.name FROM playlist_songs lc JOIN playlists l "
            "ON l.id=lc.playlist_id WHERE lc.song_id=? ORDER BY l.name",
            (song_id,),
        ).fetchall()
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
    with _connect() as conn:
        sql = f"SELECT id, path FROM songs WHERE id IN ({placeholders})"  # noqa: S608
        paths = {r["id"]: r["path"] for r in conn.execute(sql, song_ids)}
        lists: dict = {}
        for r in conn.execute(
            f"SELECT lc.song_id id, l.name name FROM playlist_songs lc "  # noqa: S608
            f"JOIN playlists l ON l.id=lc.playlist_id "
            f"WHERE lc.song_id IN ({placeholders}) ORDER BY l.name",
            song_ids,
        ):
            lists.setdefault(r["id"], []).append(r["name"])
    for cid in song_ids:
        path = paths.get(cid)
        if path and os.path.exists(path) and not tags.set_playlists(path, lists.get(cid, [])):
            log.warning("no se pudo apuntar las listas dentro de %s", path)


# ------------------------------------------------------- estrellas y favoritos


def rate(song_id, stars: int) -> bool:
    c = library.by_id(song_id)
    if not c:
        return False
    ok = tags.rate(c["path"], stars)
    with _connect() as conn:
        conn.execute("UPDATE songs SET stars=? WHERE id=?", (max(0, min(5, int(stars))), song_id))
    library._touch()
    return ok


def favorite(song_id, value=True) -> bool:
    c = library.by_id(song_id)
    if not c:
        return False
    ok = tags.set_favorite(c["path"], value)
    with _connect() as conn:
        conn.execute("UPDATE songs SET favorite=? WHERE id=?", (1 if value else 0, song_id))
    library._touch()
    return ok


def favorites() -> list[dict]:
    with _connect() as conn:
        rows = conn.execute(
            "SELECT * FROM songs WHERE favorite=1 ORDER BY artist, title"
        ).fetchall()
    return [dict(f) for f in rows]


def favorites_count() -> int:
    """Cuantas favoritas hay. Para el numero del menu no hace falta traerse
    las filas enteras, con su letra y sus acordes."""
    with _connect() as conn:
        return conn.execute("SELECT COUNT(*) FROM songs WHERE favorite=1").fetchone()[0] or 0


# ---------------------------------------------------------------- m3u


def export_folder() -> Path:
    return library.config.LIBRARY / "Listas"


def _listas(folder: Path) -> None:
    """Crea Listas/ si hace falta; si la carpeta de musica ya no esta (se
    movio), no la hace reaparecer vacia: se dice (ver library.ensure_folder)."""
    try:
        library.ensure_folder(folder)
    except library.FolderGone as e:
        raise ValueError(str(e)) from e


def export_m3u(playlist_id, target=None) -> str:
    """Escribe la lista como .m3u8 en LIBRARY/Listas/.

    El nombre de archivo sale del nombre de la lista pasado por
    `names.sanitize`, y el resultado tiene que quedar DENTRO de Listas/: el
    nombre lo puede fijar el asistente, y «../../x» escribia donde quisiera.
    Un nombre que no de un archivo valido, o un `target` fuera de la
    carpeta, levantan ValueError.
    """
    with _connect() as conn:
        row = conn.execute("SELECT name FROM playlists WHERE id=?", (playlist_id,)).fetchone()
    if not row:
        raise ValueError("no existe esa lista")
    folder = export_folder()
    if target is None:
        safe = names.sanitize(str(row["name"]).replace("/", " ").replace("\\", " "))
        if not safe or safe in (".", "..") or ".." in safe.split():
            raise ValueError("el nombre de la lista no sirve como nombre de archivo")
        target = folder / f"{safe}.m3u8"
    target = Path(target)
    _listas(folder)
    if not library._inside(target.parent, folder) or target.name in ("", ".", ".."):
        raise ValueError("la lista solo se exporta dentro de la carpeta Listas")
    lines = ["#EXTM3U"]
    for c in songs(playlist_id):
        lines.append(f"#EXTINF:{int(c['duration'])},{c['artist']} - {c['title']}")
        lines.append(os.path.relpath(c["path"], target.parent))
    target.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return str(target)


def _sheet_target(playlist_id, suffix) -> tuple[Path, str]:
    """Donde va un archivo exportado de la lista, dentro de Listas/, y su nombre."""
    with _connect() as conn:
        row = conn.execute("SELECT name FROM playlists WHERE id=?", (playlist_id,)).fetchone()
    if not row:
        raise ValueError("no existe esa lista")
    folder = export_folder()
    safe = names.sanitize(str(row["name"]).replace("/", " ").replace("\\", " "))
    if not safe or safe in (".", "..") or ".." in safe.split():
        raise ValueError("el nombre de la lista no sirve como nombre de archivo")
    target = folder / f"{safe}{suffix}"
    _listas(folder)
    if not library._inside(target.parent, folder):
        raise ValueError("la lista solo se exporta dentro de la carpeta Listas")
    return target, str(row["name"])


def _sheet_chords(c: dict) -> dict | None:
    """El cifrado guardado de una cancion (de una pagina de acordes), si lo hay."""
    raw = c.get("chords") or ""
    try:
        d = json.loads(raw) if raw else {}
    except Exception:  # noqa: BLE001
        d = {}
    sheet = d.get("sheet") if isinstance(d, dict) else None
    return sheet if isinstance(sheet, dict) else None


def _mmss(seconds) -> str:
    m, s = divmod(int(seconds or 0), 60)
    return f"{m}:{s:02d}"


# El aspecto de la hoja: legible en pantalla e imprimible (la letra a dos
# columnas en pantalla, a una en papel).
_SHEET_CSS = (
    "body{font-family:system-ui,sans-serif;max-width:800px;margin:24px auto;padding:0 16px;"
    "color:#111}"
    "h1{font-size:22px;margin:0 0 4px}.meta{color:#666;font-size:13px;margin-bottom:18px}"
    "ol{padding-left:22px}li{margin:0 0 14px;page-break-inside:avoid}"
    ".t{font-weight:600}.k{display:inline-block;margin-left:8px;padding:1px 7px;"
    "border:1px solid #999;border-radius:4px;font-size:12px}"
    ".sub{color:#555;font-size:12.5px;margin-top:2px}.chords{font-family:ui-monospace,monospace;"
    "font-size:12.5px;white-space:pre-wrap;margin:4px 0 0;padding:6px 8px;background:#f4f4f4;"
    "border-radius:4px}"
    ".lyrics{white-space:pre-wrap;font-size:12.5px;margin:6px 0 0;column-width:300px;"
    "column-gap:24px}"
    "@media print{body{margin:0}.lyrics{column-width:auto}}"
)


def export_sheet(playlist_id, with_lyrics=False) -> str:
    """Escribe la hoja para el atril: un HTML en Listas/ con las canciones
    del repertorio en orden, tono (americano y latino), bpm, cejilla
    sugerida, acordes por secciones si se encontro su cifrado (con la
    fuente), y la letra si se pide.
    Se abre con el navegador y se imprime (o se guarda como PDF) desde ahi:
    no hace falta ninguna libreria, y queda al lado del .m3u de siempre.
    """
    target, title = _sheet_target(playlist_id, ".html")
    rows = songs(playlist_id)
    e = html.escape
    total = _mmss(sum(float(c.get("duration") or 0) for c in rows))
    parts = [
        (
            f'<!doctype html><html lang="es"><head><meta charset="utf-8">'
            f"<title>{e(title)}</title><style>{_SHEET_CSS}</style></head><body>"
        ),
        (
            f'<h1>{e(title)}</h1><div class="meta">{len(rows)} canciones · '
            f"{total} · {time.strftime('%d/%m/%Y')}</div><ol>"
        ),
    ]
    for c in rows:
        sheet = _sheet_chords(c)
        key = str(c.get("key") or "").strip() or cifrados.sheet_key(sheet)
        head = f'<span class="t">{e(c.get("artist") or "")} - {e(c.get("title") or "")}</span>'
        if key:
            head += f'<span class="k">{e(key)} · {e(theory.to_latin(key))}</span>'
        sub = []
        if c.get("bpm"):
            sub.append(f"{round(float(c['bpm']))} bpm")
        sub.append(_mmss(c.get("duration")))
        capo = theory.suggested_capo(key) if key else []
        if capo:
            sub.append("cejilla " + ", ".join(f"{f} ({sh})" for f, sh in capo[:3]))
        item = f'<li>{head}<div class="sub">{e(" · ".join(sub))}</div>'
        lines = [
            (f"{s['name']}: " if s.get("name") else "") + " ".join(s["chords"])
            for s in (sheet or {}).get("sections") or []
            if s.get("chords")
        ]
        if lines:
            lines.append(f"(de {sheet['source']})")  # type: ignore[index]
            item += f'<div class="chords">{e(chr(10).join(lines))}</div>'
        if with_lyrics and c.get("lyrics"):
            # sin las marcas de tiempo, si el archivo trae una LRC en el USLT
            item += f'<div class="lyrics">{e(tags.lrc_to_plain(str(c["lyrics"])))}</div>'
        parts.append(item + "</li>")
    parts.append("</ol></body></html>")
    target.write_text("".join(parts), encoding="utf-8")
    return str(target)


def import_m3u(path) -> int:
    """Crea una lista a partir de un .m3u/.m3u8 existente."""
    path = Path(path)
    lid = create(path.stem)["id"]
    base = path.parent
    ids = []
    with _connect() as conn:
        for line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            p = str((base / line).resolve()) if not os.path.isabs(line) else line
            f = conn.execute("SELECT id FROM songs WHERE path=?", (p,)).fetchone()
            if f:
                ids.append(f["id"])
    return add(lid, ids) if ids else 0
