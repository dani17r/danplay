# -*- coding: utf-8 -*-
"""Listas de reproduccion, favoritos y valoraciones.

Todo lo que se puede guardar dentro del archivo, se guarda ahi tambien
(estrellas en POPM, favorito y listas en TXXX), para que la biblioteca
siga siendo portatil aunque se pierda la base de datos.
"""
import html, json, logging, os, time
from pathlib import Path
from . import external, library, names, tags, theory

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
    library._touch()
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
    library._touch()
    # los archivos dejan de nombrar la lista: si no, un reescaneo la resucitaria
    _stamp_playlists_into_files(members)


def rename_folder(playlist_id, name) -> None:
    conn = _connect()
    conn.execute("UPDATE playlists SET name=? WHERE id=?", (name, playlist_id))
    conn.commit(); conn.close()
    library._touch()


def by_id(playlist_id) -> dict | None:
    """La lista con ese id, con cuantos temas tiene, o None."""
    try:
        playlist_id = int(playlist_id)
    except (TypeError, ValueError):
        return None
    return next((l for l in list_all() if l["id"] == playlist_id), None)


def by_name(name: str) -> dict | None:
    """La lista que se llama asi, sin distinguir mayusculas ni tildes.

    El asistente conoce las listas por su nombre («Herlin»), no por su id; y
    cuando adivinaba el id acababa borrando la lista equivocada.
    """
    key = names._flat(str(name or ""))
    if not key:
        return None
    return next((l for l in list_all() if names._flat(l["name"]) == key), None)


def edit(playlist_id, name=None, note=None) -> dict | None:
    """Cambia el nombre o la nota. Devuelve la lista ya cambiada, o None."""
    current = by_id(playlist_id)
    if not current:
        return None
    fields, values = [], []
    if name is not None and str(name).strip():
        fields.append("name=?"); values.append(str(name).strip())
    if note is not None:
        fields.append("note=?"); values.append(str(note))
    if fields:
        conn = _connect()
        conn.execute(f"UPDATE playlists SET {','.join(fields)} WHERE id=?",
                     values + [current["id"]])
        conn.commit(); conn.close()
        library._touch()
        if name is not None:
            # las canciones llevan dentro los nombres de sus listas
            _stamp_playlists_into_files([s["id"] for s in songs(current["id"])])
    return by_id(current["id"])


def set_songs(playlist_id, song_ids) -> dict:
    """Deja la lista EXACTAMENTE con esas canciones, en ese orden.

    Es «corrige la lista»: lo que sobra se quita, lo que falta se añade, y lo
    que ya estaba se queda. Devuelve cuantas se quitaron y cuantas entraron.
    """
    wanted = existing_ids(song_ids)
    current = [s["id"] for s in songs(playlist_id)]
    gone = [i for i in current if i not in wanted]
    if gone:
        remove_song(playlist_id, gone)
    added = add(playlist_id, [i for i in wanted if i not in current])
    reorder(playlist_id, wanted)
    return {"removed": len(gone), "added": added, "total": len(wanted)}


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


def existing_ids(song_ids) -> list[int]:
    """De esos ids, los que son una cancion de verdad, en el mismo orden.

    Positivos: la biblioteca. Negativos: archivos abiertos desde fuera
    (`external.py`), si esa tabla existe. Lo que no este, fuera.
    """
    wanted = []
    for i in song_ids:
        try:
            wanted.append(int(i))
        except (TypeError, ValueError):
            continue
    if not wanted:
        return []
    conn = _connect()
    found: set = set()
    inside = [i for i in wanted if i > 0]
    if inside:
        found |= {r["id"] for r in conn.execute(
            f"SELECT id FROM songs WHERE id IN ({','.join('?' * len(inside))})", inside)}
    outside = [-i for i in wanted if i < 0]
    if outside and conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='external_songs'").fetchone():
        found |= {-r["id"] for r in conn.execute(
            f"SELECT id FROM external_songs WHERE id IN ({','.join('?' * len(outside))})",
            outside)}
    conn.close()
    seen: set = set()
    out = []
    for i in wanted:
        if i in found and i not in seen:
            seen.add(i)
            out.append(i)
    return out


def add(playlist_id, song_ids) -> int:
    if isinstance(song_ids, int):
        song_ids = [song_ids]
    # Solo lo que existe. Un id que no es ninguna cancion (el asistente se los
    # inventaba) dejaba una fila huerfana: la lista decia «6 temas» y
    # enseñaba cuatro.
    song_ids = existing_ids(song_ids)
    if not song_ids:
        return 0
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
    if n:
        library._touch()
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
    library._touch()
    _stamp_playlists_into_files(song_ids)


def reorder(playlist_id, ordered_song_ids) -> None:
    conn = _connect()
    for i, cid in enumerate(ordered_song_ids):
        conn.execute("UPDATE playlist_songs SET position=? WHERE playlist_id=? AND song_id=?",
                    (i, playlist_id, cid))
    conn.commit(); conn.close()
    library._touch()


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
    library._touch()
    return ok


def favorite(song_id, value=True) -> bool:
    c = library.by_id(song_id)
    if not c:
        return False
    ok = tags.set_favorite(c["path"], value)
    conn = _connect()
    conn.execute("UPDATE songs SET favorite=? WHERE id=?", (1 if value else 0, song_id))
    conn.commit(); conn.close()
    library._touch()
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


def _sheet_target(playlist_id, suffix) -> tuple[Path, str]:
    """Donde va un archivo exportado de la lista, dentro de Listas/, y su nombre."""
    conn = _connect()
    row = conn.execute("SELECT name FROM playlists WHERE id=?", (playlist_id,)).fetchone()
    conn.close()
    if not row:
        raise ValueError("no existe esa lista")
    folder = export_folder()
    safe = names.sanitize(str(row["name"]).replace("/", " ").replace("\\", " "))
    if not safe or safe in (".", "..") or ".." in safe.split():
        raise ValueError("el nombre de la lista no sirve como nombre de archivo")
    target = folder / f"{safe}{suffix}"
    folder.mkdir(parents=True, exist_ok=True)
    if not library._inside(target.parent, folder):
        raise ValueError("la lista solo se exporta dentro de la carpeta Listas")
    return target, str(row["name"])


def _sheet_chords(c: dict) -> dict:
    """Los acordes guardados de una cancion (los de la IA), si los hay."""
    raw = c.get("chords") or ""
    try:
        d = json.loads(raw) if raw else {}
    except Exception:                                        # noqa: BLE001
        d = {}
    return d if isinstance(d, dict) else {}


def _mmss(seconds) -> str:
    m, s = divmod(int(seconds or 0), 60)
    return f"{m}:{s:02d}"


def export_sheet(playlist_id, with_lyrics=False) -> str:
    """Escribe la hoja para el atril: un HTML en Listas/ con las canciones
    del repertorio en orden, tono (americano y latino), bpm, cejilla
    sugerida, acordes por secciones si la IA los dio, y la letra si se pide.
    Se abre con el navegador y se imprime (o se guarda como PDF) desde ahi:
    no hace falta ninguna libreria, y queda al lado del .m3u de siempre.
    """
    target, title = _sheet_target(playlist_id, ".html")
    rows = songs(playlist_id)
    e = html.escape
    parts = [f"<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\">"
             f"<title>{e(title)}</title><style>"
             "body{font-family:system-ui,sans-serif;max-width:800px;margin:24px auto;padding:0 16px;color:#111}"
             "h1{font-size:22px;margin:0 0 4px}.meta{color:#666;font-size:13px;margin-bottom:18px}"
             "ol{padding-left:22px}li{margin:0 0 14px;page-break-inside:avoid}"
             ".t{font-weight:600}.k{display:inline-block;margin-left:8px;padding:1px 7px;border:1px solid #999;border-radius:4px;font-size:12px}"
             ".sub{color:#555;font-size:12.5px;margin-top:2px}.chords{font-family:ui-monospace,monospace;font-size:12.5px;"
             "white-space:pre-wrap;margin:4px 0 0;padding:6px 8px;background:#f4f4f4;border-radius:4px}"
             ".lyrics{white-space:pre-wrap;font-size:12.5px;margin:6px 0 0;column-width:300px;column-gap:24px}"
             "@media print{body{margin:0}.lyrics{column-width:auto}}"
             "</style></head><body>",
             f"<h1>{e(title)}</h1><div class=\"meta\">{len(rows)} canciones · "
             f"{_mmss(sum(float(c.get('duration') or 0) for c in rows))} · "
             f"{time.strftime('%d/%m/%Y')}</div><ol>"]
    for c in rows:
        key = str(c.get("key") or "").strip()
        head = f"<span class=\"t\">{e(c.get('artist') or '')} - {e(c.get('title') or '')}</span>"
        if key:
            head += f"<span class=\"k\">{e(key)} · {e(theory.to_latin(key))}</span>"
        sub = []
        if c.get("bpm"):
            sub.append(f"{int(round(float(c['bpm'])))} bpm")
        sub.append(_mmss(c.get("duration")))
        capo = theory.suggested_capo(key) if key else []
        if capo:
            sub.append("cejilla " + ", ".join(f"{f} ({sh})" for f, sh in capo[:3]))
        item = f"<li>{head}<div class=\"sub\">{e(' · '.join(sub))}</div>"
        d = _sheet_chords(c)
        sections = d.get("section_chords") or {}
        lines = [f"{k}: {v}" for k, v in sections.items() if v] if isinstance(sections, dict) else []
        if not lines and d.get("progression"):
            lines = [str(d["progression"])]
        if lines:
            item += f"<div class=\"chords\">{e(chr(10).join(lines))}</div>"
        if with_lyrics and c.get("lyrics"):
            item += f"<div class=\"lyrics\">{e(str(c['lyrics']))}</div>"
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
