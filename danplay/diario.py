"""El diario: lo tuyo, fuera de la base y junto a tu musica.

Las listas (en su orden), las estrellas, las favoritas, lo del modo estudio,
los cifrados y las letras con tiempos viven en la base de DanPlay, que es
desechable (se reconstruye escaneando). Parte viaja tambien dentro de cada
archivo (las etiquetas), pero no todo: el orden de las listas, sus notas y
colores, los cifrados... Y una lista guarda canciones por su id: si la
cancion se mueve por fuera, se vuelve a bajar o la base se pierde, en
cualquier reproductor la lista se rompe.

Aqui se guarda todo eso en un archivo, `.danplay/diario.json`, dentro de
cada carpeta de musica (viaja con ella si la mueves a otro disco) y en los
datos de DanPlay, con la ficha de cada cancion (ruta, nombre, tamaño,
artista, titulo, duracion). Se escribe solo, unos segundos despues de cada
cambio. Y al abrir DanPlay o al cambiar algo en las carpetas se compara con
lo que hay (`reconcile`):

- una cancion que se movio, se renombro o se volvio a bajar se reconoce por
  su ficha, y vuelve a sus listas, en su sitio, con sus estrellas y su
  estudio;
- si la base es nueva (se perdio, otro equipo), se rehacen las listas
  enteras: lo que no se encuentra queda como «No está», para buscarlo o
  bajarlo;
- las listas exportadas (`Listas/*.m3u8`) se reescriben con las rutas
  nuevas: tambien siguen valiendo en otros reproductores.

Para no deshacer lo que se acaba de cambiar a proposito (quitar una cancion
de una lista), con la misma base solo se recupera lo de canciones que
cambiaron de sitio; lo demas, ya esta en la base.
"""

import contextlib
import json
import logging
import os
import threading
import time
import uuid
from pathlib import Path

from . import config, library, names

log = logging.getLogger(__name__)

FOLDER = ".danplay"
FILE = "diario.json"
FORMAT = "danplay-diario"
VERSION = 1
# Lo que la persona pone de una cancion, como esta en la base.
FIELDS = ("stars", "favorite", "blur", "study", "chords", "lyrics_synced", "key", "bpm")
# Cada cuanto se mira si hay algo nuevo que escribir.
EVERY = 3.0


def _uid() -> str:
    """El id de esta base: cambia si la base es otra (se perdio, otro equipo)."""
    with library.connect() as conn:
        row = conn.execute("SELECT value FROM meta WHERE key='library_uid'").fetchone()
        if row and row[0]:
            return str(row[0])
        uid = uuid.uuid4().hex
        conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('library_uid', ?)", (uid,))
        conn.commit()
        return uid


def _roots() -> list[str]:
    from .library.folders import _roots as roots

    return [r for r in roots() if os.path.isdir(r)]


def _root_of(path: str, roots: list[str]) -> str:
    return next((r for r in roots if path == r or path.startswith(r.rstrip(os.sep) + os.sep)), "")


# ------------------------------------------------------------ escribir


def snapshot() -> dict:
    """Lo que va en el diario: las canciones con algo tuyo (o en una lista)
    con su ficha, y las listas en su orden."""
    from . import __version__, playlists

    roots = _roots()
    with library.connect() as conn:
        listed = {r[0] for r in conn.execute("SELECT DISTINCT song_id FROM playlist_songs")}
        rows = conn.execute(
            "SELECT id, path, root, file, size, artist, title, duration, "
            "stars, favorite, blur, study, chords, lyrics_synced, key, bpm FROM songs"
        ).fetchall()
        lists = conn.execute("SELECT * FROM playlists ORDER BY name").fetchall()
        members: dict[int, list[int]] = {}
        for r in conn.execute(
            "SELECT playlist_id, song_id FROM playlist_songs ORDER BY playlist_id, position, rowid"
        ):
            members.setdefault(r[0], []).append(r[1])
        present = {r["id"] for r in rows}
        ghosts = playlists.cards(conn, [i for i in listed if i > 0 and i not in present])
    songs = []
    for r in rows:
        mine = {f: r[f] for f in FIELDS if r[f] not in (None, "", 0, 0.0)}
        if r["id"] not in listed and not mine:
            continue
        root = r["root"] or _root_of(r["path"], roots)
        songs.append(
            {
                "id": r["id"],
                "path": r["path"],
                "root": os.path.basename(root.rstrip(os.sep)) if root else "",
                "rel": os.path.relpath(r["path"], root) if root else "",
                "file": r["file"],
                "size": r["size"],
                "artist": r["artist"],
                "title": r["title"],
                "duration": round(float(r["duration"] or 0), 2),
                **mine,
            }
        )
    for gid, card in ghosts.items():
        songs.append({"id": gid, "gone": True, **card})
    return {
        "format": FORMAT,
        "version": VERSION,
        "app": f"DanPlay {__version__}",
        "library": _uid(),
        "updated": round(time.time(), 1),
        "songs": songs,
        "playlists": [
            {
                "name": p["name"],
                "note": p["note"] or "",
                "color": p["color"] or "",
                "created": p["created"],
                "songs": members.get(p["id"], []),
            }
            for p in lists
        ],
    }


def _targets() -> list[Path]:
    out = [config.DATA_DIR / FILE]
    out += [Path(r) / FOLDER / FILE for r in _roots()]
    return out


def _write(path: Path, text: str) -> bool:
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(path.name + ".part")
        tmp.write_text(text, encoding="utf-8")
        os.replace(tmp, path)
        return True
    except OSError:
        log.info("no se pudo escribir el diario en %s", path, exc_info=True)
        return False


_STATE: dict = {"written": "", "revision": -1, "restored": False, "report": None, "seq": 0}
_LOCK = threading.Lock()


def write() -> int:
    """Escribe el diario donde toca (si cambio). Devuelve en cuantos sitios."""
    data = snapshot()
    body = json.dumps(
        {k: v for k, v in data.items() if k != "updated"}, ensure_ascii=False, sort_keys=True
    )
    with _LOCK:
        if body == _STATE["written"]:
            return 0
    text = json.dumps(data, ensure_ascii=False, indent=1)
    n = sum(_write(p, text) for p in _targets())
    with _LOCK:
        _STATE["written"] = body
    return n


def _load(path: Path) -> dict | None:
    with contextlib.suppress(OSError, ValueError):
        data = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data, dict) and data.get("format") == FORMAT:
            return data
    return None


def latest() -> dict | None:
    """El diario mas reciente de los que haya (el de los datos de DanPlay o
    el de alguna carpeta de musica: puede venir de otro equipo)."""
    found = [d for d in (_load(p) for p in _targets()) if d]
    return max(found, key=lambda d: d.get("updated") or 0) if found else None


# ------------------------------------------------------------ recuperar


def _index(rows):
    from . import reuse

    by_path, by_rel, by_file, by_key = {}, {}, {}, {}
    roots = _roots()
    for r in rows:
        by_path[r["path"]] = r
        root = r["root"] or _root_of(r["path"], roots)
        if root:
            by_rel[(os.path.basename(root.rstrip(os.sep)), os.path.relpath(r["path"], root))] = r
        by_file.setdefault((r["file"], r["size"]), []).append(r)
        by_key.setdefault(reuse.ident(r), []).append(r)
    return by_path, by_rel, by_file, by_key


def _sign(card: dict) -> tuple:
    """La ficha de una cancion que no esta, para no ponerla dos veces."""
    return (card.get("file") or "", card.get("artist") or "", card.get("title") or "")


def _find(entry: dict, by_path, by_rel, by_file, by_key):
    """La cancion de ahora que es la del diario: en su ruta, en la misma ruta
    dentro de su carpeta (la carpeta se movio), con el mismo nombre y tamaño,
    o con el mismo artista y titulo y la misma duracion."""
    from . import reuse

    if entry.get("path") in by_path:
        return by_path[entry["path"]]
    hit = by_rel.get((entry.get("root"), entry.get("rel")))
    if hit is not None:
        return hit
    same = by_file.get((entry.get("file"), entry.get("size")), [])
    if len(same) == 1:
        return same[0]
    near = [
        r
        for r in by_key.get(reuse.ident(entry), [])
        if reuse._close(r["duration"], entry.get("duration"))
    ]
    if len(near) == 1:
        return near[0]
    return None


def reconcile() -> dict:
    """Compara el diario con lo que hay y recupera lo que se perdio (ver
    arriba). Devuelve lo que hizo: {songs, lists, entries, ghosts}."""
    from . import playlists
    from .library.db import _next_song_id

    told = {"songs": 0, "lists": 0, "entries": 0, "ghosts": 0, "fresh": False}
    journal = latest()
    if not journal:
        with _LOCK:
            _STATE["restored"] = True
        return told
    fresh = journal.get("library") != _uid()
    told["fresh"] = fresh
    with library.connect() as conn:
        rows = [
            dict(r)
            for r in conn.execute(
                "SELECT id, path, root, file, size, artist, title, match_key, duration, "
                "stars, favorite, blur, study, chords, lyrics_synced, key, bpm FROM songs"
            )
        ]
    index = _index(rows)
    mapping: dict[int, int] = {}  # id del diario -> id de ahora
    moved: set[int] = set()
    entries = {e["id"]: e for e in journal.get("songs") or [] if isinstance(e, dict) and "id" in e}
    for old, e in entries.items():
        if e.get("gone"):
            continue
        cur = _find(e, *index)
        if cur is None:
            continue
        mapping[old] = cur["id"]
        if cur["path"] != e.get("path") or old != cur["id"]:
            moved.add(old)
    # lo de cada cancion: solo lo que le falta (lo que hay en la base manda)
    touched: set[int] = set()
    with library.connect() as conn:
        for old, new in mapping.items():
            if not fresh and old not in moved:
                continue
            e = entries[old]
            # FIELDS: nombres fijos de columnas, de aqui
            sql = f"SELECT {', '.join(FIELDS)} FROM songs WHERE id=?"  # noqa: S608
            cur = conn.execute(sql, (new,)).fetchone()
            if cur is None:
                continue
            sets = {f: e[f] for f in FIELDS if e.get(f) not in (None, "", 0) and not cur[f]}
            if sets:
                conn.execute(
                    f"UPDATE songs SET {', '.join(f + '=?' for f in sets)} WHERE id=?",  # noqa: S608
                    (*sets.values(), new),
                )
                touched.add(new)
        conn.commit()
    told["songs"] = len(touched)
    # las listas
    stamp: set[int] = set()
    present = {r["id"] for r in rows}
    with playlists.LOCK, library.connect() as conn:
        for pl in journal.get("playlists") or []:
            name = str(pl.get("name") or "").strip()
            if not name:
                continue
            row = conn.execute("SELECT id FROM playlists WHERE name=?", (name,)).fetchone()
            created = False
            if row is None:
                if not fresh:
                    continue  # con la misma base, una lista que no esta se borro a proposito
                pid = conn.execute(
                    "INSERT INTO playlists (name, note, color, created) VALUES (?,?,?,?)",
                    (
                        name,
                        pl.get("note") or "",
                        pl.get("color") or "",
                        pl.get("created") or time.time(),
                    ),
                ).lastrowid
                created = True
            else:
                pid = row[0]
                if fresh:
                    # rehecha por las etiquetas de los archivos: sin nota ni color
                    conn.execute(
                        "UPDATE playlists SET note=CASE WHEN note='' OR note IS NULL THEN ? "
                        "ELSE note END, color=CASE WHEN color='' OR color IS NULL THEN ? "
                        "ELSE color END WHERE id=?",
                        (pl.get("note") or "", pl.get("color") or "", pid),
                    )
            have = {
                r[0]: r[1]
                for r in conn.execute(
                    "SELECT song_id, position FROM playlist_songs WHERE playlist_id=?", (pid,)
                )
            }
            # las «No está» que ya tiene, por su ficha (no repetirlas)
            known = {
                _sign(card): gid
                for gid, card in playlists.cards(
                    conn, [i for i in have if i not in present]
                ).items()
            }
            before = (told["entries"], told["ghosts"])
            order: list[int] = []
            for old in pl.get("songs") or []:
                e = entries.get(old)
                new = mapping.get(old)
                if new is not None:
                    if new in have:
                        order.append(new)
                        continue
                    if not (fresh or old in moved or old in have):
                        continue  # se quito de la lista a proposito
                    if old in have and old not in mapping.values():
                        # la fila vieja («No está») pasa a ser la de ahora
                        conn.execute(
                            "UPDATE playlist_songs SET song_id=? WHERE playlist_id=? AND song_id=?",
                            (new, pid, old),
                        )
                        have[new] = have.pop(old)
                    else:
                        conn.execute(
                            "INSERT OR IGNORE INTO playlist_songs (playlist_id, song_id, position, added) "
                            "VALUES (?,?,?,?)",
                            (pid, new, len(have), time.time()),
                        )
                        have[new] = len(have)
                    order.append(new)
                    stamp.add(new)
                    told["entries"] += 1
                elif (created or fresh) and e and _sign(e) in known:
                    order.append(known.pop(_sign(e)))
                elif (created or fresh) and e:
                    # no esta: queda en la lista como «No está», con su ficha y
                    # un id que no es de nadie (la base nueva reparte otros)
                    gid = _next_song_id(conn)
                    conn.execute(
                        "INSERT OR REPLACE INTO meta (key, value) VALUES ('song_id_hwm', ?)", (gid,)
                    )
                    conn.execute(
                        "INSERT OR REPLACE INTO song_cards (song_id, artist, title, duration, file, path, gone_at) "
                        "VALUES (?,?,?,?,?,?,?)",
                        (
                            gid,
                            e.get("artist") or "",
                            e.get("title") or "",
                            e.get("duration") or 0,
                            e.get("file") or "",
                            e.get("path") or "",
                            time.time(),
                        ),
                    )
                    conn.execute(
                        "INSERT INTO playlist_songs (playlist_id, song_id, position, added) VALUES (?,?,?,?)",
                        (pid, gid, len(have), time.time()),
                    )
                    have[gid] = len(have)
                    order.append(gid)
                    told["ghosts"] += 1
            # el orden del diario, y detras lo que la lista tenga y el diario no
            final = [
                *order,
                *(s for s, _ in sorted(have.items(), key=lambda x: x[1]) if s not in order),
            ]
            moved_rows = 0
            for pos, song in enumerate(final):
                if have.get(song) != pos:
                    conn.execute(
                        "UPDATE playlist_songs SET position=? WHERE playlist_id=? AND song_id=?",
                        (pos, pid, song),
                    )
                    moved_rows += 1
            if created or moved_rows or before != (told["entries"], told["ghosts"]):
                told["lists"] += 1
        conn.commit()
    if touched or told["lists"] or told["entries"] or told["ghosts"]:
        library._touch()
        if stamp:
            with contextlib.suppress(Exception):
                playlists._stamp_playlists_into_files(sorted(stamp))
    refresh_exports()
    if fresh:
        # el diario pasa a ser el de esta base (si no, la proxima vez se
        # volveria a tomar por nueva)
        with _LOCK:
            _STATE["written"] = ""
        write()
    with _LOCK:
        _STATE["restored"] = True
        if touched or told["lists"] or told["entries"] or told["ghosts"]:
            _STATE["seq"] += 1
            _STATE["report"] = {**told, "seq": _STATE["seq"], "at": round(time.time())}
    if told["songs"] or told["entries"] or told["lists"]:
        log.info("diario: recuperado %s", told)
    return told


def refresh_exports() -> int:
    """Las listas ya exportadas (`Listas/<nombre>.m3u8`), con las rutas de
    ahora: siguen valiendo en otros reproductores aunque se mueva la musica."""
    from . import playlists

    n = 0
    with contextlib.suppress(Exception):
        folder = playlists.export_folder()
        if not folder.is_dir():
            return 0
        for p in playlists.list_all():
            safe = names.sanitize(str(p["name"]).replace("/", " ").replace("\\", " "))
            target = folder / f"{safe}.m3u8"
            if not target.is_file():
                continue
            before = target.read_text(encoding="utf-8")
            playlists.export_m3u(p["id"], target)
            n += int(target.read_text(encoding="utf-8") != before)
    return n


def status() -> dict | None:
    """Lo ultimo que se recupero (para avisar en la interfaz)."""
    with _LOCK:
        return dict(_STATE["report"]) if _STATE["report"] else None


# ------------------------------------------------------------ en marcha

_STOP = threading.Event()
_THREAD: list[threading.Thread] = []


def after_scan(result: dict | None, first: bool = False) -> None:
    """Tras un escaneo: la primera vez, recuperar todo lo que haga falta; las
    siguientes, solo si algo cambio de sitio."""
    if first or (
        result and (result.get("added_count") or result.get("back") or result.get("removed"))
    ):
        try:
            reconcile()
        except Exception:
            log.warning("el diario no pudo recuperar", exc_info=True)
            with _LOCK:
                _STATE["restored"] = True


def start() -> None:
    """El hilo que escribe el diario unos segundos despues de cada cambio
    (y nunca antes de haber recuperado: una base nueva y vacia no puede
    pisar el diario que la va a rellenar)."""
    if _THREAD and _THREAD[0].is_alive():
        return
    _STOP.clear()

    def run() -> None:
        while not _STOP.wait(EVERY):
            with _LOCK:
                ready = _STATE["restored"]
            if not ready:
                continue
            rev = library.revision()
            if rev == _STATE["revision"]:
                continue
            _STATE["revision"] = rev
            with contextlib.suppress(Exception):
                write()

    t = threading.Thread(target=run, name="danplay-diario", daemon=True)
    _THREAD[:] = [t]
    t.start()


def stop() -> None:
    """Para el hilo y escribe lo ultimo (al cerrar DanPlay)."""
    _STOP.set()
    with _LOCK:
        ready = _STATE["restored"]
    if ready:
        with contextlib.suppress(Exception):
            write()


def _reset() -> None:
    """Para las pruebas."""
    with _LOCK:
        _STATE.update(written="", revision=-1, restored=False, report=None, seq=0)
