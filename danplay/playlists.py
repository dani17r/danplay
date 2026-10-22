"""Listas de reproduccion, favoritos y valoraciones.

Todo lo que se puede guardar dentro del archivo, se guarda ahi tambien
(estrellas en POPM, favorito y listas en TXXX), para que la biblioteca
siga siendo portatil aunque se pierda la base de datos.

Lo de la base lo hace Rust (biblioteca/src/listas.rs); esto es la puerta de
Python, con los mismos nombres. Aqui siguen el cerrojo de las listas
(`LOCK`), el aviso a la interfaz, escribir dentro de los archivos y lo que se
exporta a Listas/.
"""

import html
import json
import logging
import os
import threading
import time
from pathlib import Path

import danplay_core as _rust

from . import (
    cifrados,
    external,  # noqa: F401  (registra su esquema: las listas leen external_songs)
    library,
    names,
    tags,
    theory,
)

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
-- La ficha de una cancion de una lista que ya no esta en la biblioteca (se
-- borro por fuera, se fue de las carpetas y paso un mes): para que la lista
-- la siga enseñando («No está»), y para reconocerla si vuelve.
CREATE TABLE IF NOT EXISTS song_cards (
    song_id   INTEGER PRIMARY KEY,
    artist    TEXT DEFAULT '',
    title     TEXT DEFAULT '',
    duration  REAL DEFAULT 0,
    file      TEXT DEFAULT '',
    path      TEXT DEFAULT '',
    gone_at   REAL DEFAULT 0
);
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


# ---------------------------------------------------------------- listas


def create(name, note="", color="") -> dict:
    """Crea la lista, o devuelve la que ya habia con ese nombre.

    Devuelve {"id", "name", "created"}. `created` es False si la lista ya
    existia: antes se devolvia solo el id y quien llamaba no podia saberlo,
    asi que el asistente «creaba» una lista y en realidad añadia a otra.
    """
    with LOCK:
        # sin pedir el permiso de escribir de antemano: si la lista ya esta no
        # se escribe nada, y no tiene que esperar a un escaneo en marcha
        with _connect() as conn:
            out = _rust.playlists_create(
                conn,
                str(name or ""),
                None if note is None else str(note),
                None if color is None else str(color),
            )
        if out["created"]:
            library._touch()
    return out


def remove(playlist_id) -> None:
    with LOCK:
        with _begin() as conn:
            members = _rust.playlists_remove(conn, playlist_id)
        library._touch()
        # los archivos dejan de nombrar la lista: si no, un reescaneo la resucitaria
        _stamp_playlists_into_files(members)


def rename_folder(playlist_id, name) -> None:
    with LOCK:
        with _connect() as conn:
            _rust.playlists_rename_folder(conn, playlist_id, name)
        library._touch()


def by_id(playlist_id) -> dict | None:
    """La lista con ese id, con cuantos temas tiene, o None.

    Solo esa: antes se calculaba `list_all()` entero (con la suma de
    duraciones de todas las listas) para quedarse con una."""
    try:
        playlist_id = int(playlist_id)
    except (TypeError, ValueError):
        return None
    with _connect() as conn:
        return _rust.playlists_by_id(conn, playlist_id)


def by_name(name: str) -> dict | None:
    """La lista que se llama asi, sin distinguir mayusculas ni tildes.

    El asistente conoce las listas por su nombre («Herlin»), no por su id; y
    cuando adivinaba el id acababa borrando la lista equivocada.
    """
    with _connect() as conn:
        return _rust.playlists_by_name(conn, names._txt(str(name or "")))


def edit(playlist_id, name=None, note=None) -> dict | None:
    """Cambia el nombre o la nota. Devuelve la lista ya cambiada, o None."""
    try:
        pid = int(playlist_id)
    except (TypeError, ValueError):
        return None
    with LOCK:
        with _connect() as conn:
            r = _rust.playlists_edit(
                conn,
                pid,
                None if name is None else str(name),
                None if note is None else str(note),
            )
        if r is None:
            return None
        current, changed = r
        if changed:
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
            gone, added, moved, total = _rust.playlists_set_songs(
                conn, playlist_id, _ints(song_ids)
            )
        if gone or added or moved:
            library._touch()
        if gone or added:
            _stamp_playlists_into_files([*gone, *added])
    return {"removed": len(gone), "added": len(added), "total": total}


def list_all() -> list[dict]:
    with _connect() as conn:
        return _rust.playlists_list_all(conn)


def existing_ids(song_ids) -> list[int]:
    """De esos ids, los que son una cancion de verdad, en el mismo orden.

    Positivos: la biblioteca. Negativos: archivos abiertos desde fuera
    (`external.py`), si esa tabla existe. Lo que no este, fuera.
    """
    wanted = _ints(song_ids)
    if not wanted:
        return []
    with _connect() as conn:
        return _rust.playlists_existing_ids(conn, wanted)


def add(playlist_id, song_ids) -> int:
    if isinstance(song_ids, int):
        song_ids = [song_ids]
    ints = _ints(song_ids)
    if not ints:
        return 0
    with LOCK:
        with _begin() as conn:
            # Solo lo que existe, mirado DENTRO de la transaccion: que no se
            # borre la cancion entre mirarlo e insertarla
            r = _rust.playlists_add(conn, playlist_id, ints)
        if r is None:
            return 0
        n, wanted = r
        if n:
            library._touch()
        _stamp_playlists_into_files(wanted)
        return n


def remove_song(playlist_id, song_ids) -> None:
    if isinstance(song_ids, int):
        song_ids = [song_ids]
    with LOCK:
        with _begin() as conn:
            _rust.playlists_remove_songs(conn, playlist_id, list(song_ids))
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
            _rust.playlists_reorder(conn, playlist_id, wanted)
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
            r = _rust.playlists_place(conn, playlist_id, wanted)
            if r is None:
                raise ValueError("no existe esa lista")
            added, moved, order = r
        if added or moved:
            library._touch()
        if added:
            _stamp_playlists_into_files(added)
    return {"added": len(added), "total": len(order), "order": order}


def cards(conn, ids) -> dict[int, dict]:
    """La ficha de las canciones de esos ids que ya no estan en la biblioteca:
    de las apartadas (su fila entera, guardada) o, pasado el mes, de
    `song_cards`."""
    return _rust.playlists_cards(conn, [int(i) for i in ids])


def ghost(cid: int, card: dict, position: int) -> dict:
    """Una cancion de la lista que ya no esta: con su ficha, para enseñarla
    («No está») y ofrecer buscarla o bajarla."""
    return _rust.playlists_ghost(int(cid), dict(card), position)


def songs(playlist_id, light: bool = False, ghosts: bool = True) -> list[dict]:
    """Las canciones de la lista, en su orden.

    Una lista puede llevar canciones de la biblioteca (id positivo) y
    canciones de fuera de ella (id negativo, ver `external.py`), asi que no
    vale el JOIN con `songs` de toda la vida: las de fuera se caian por el
    camino sin decir nada. Con `light`, filas ligeras (lo que da la API).

    Las que ya no estan (se borraron o se movieron por fuera) siguen en su
    sitio con su ficha y `ghost`: la lista no se rompe y se pueden buscar o
    volver a bajar (sin `ghosts`, solo las que suenan).
    """
    with _connect() as conn:
        return _rust.playlists_songs(conn, playlist_id, bool(light), bool(ghosts))


def take_out_everywhere(song_id: int) -> int:
    """Quita la cancion de todas las listas (se borro a proposito: no tiene
    que quedar como «No está»). Devuelve de cuantas."""
    with LOCK, _begin() as conn:
        n = _rust.playlists_take_out_everywhere(conn, int(song_id))
    if n:
        library._touch()
    return n


def keep_cards(conn, ids) -> None:
    """Guarda la ficha de esas canciones apartadas que estan en alguna lista
    (antes de olvidarlas del todo: la lista la sigue enseñando). No confirma."""
    _rust.playlists_keep_cards(conn, [int(i) for i in ids])


def relink_ghosts(new_ids) -> int:
    """Las canciones que vuelven en otro archivo (se bajaron otra vez, se
    trajeron de otro sitio): las listas que las tenian como «No está» las
    recuperan, en su sitio. Se reconocen por artista y titulo (o nombre) y
    duracion, como las gemelas de `reuse`. Devuelve cuantas filas."""
    new_ids = [int(i) for i in new_ids if int(i) > 0]
    if not new_ids:
        return 0
    with LOCK, _begin() as conn:
        n, moved = _rust.playlists_relink_ghosts(conn, new_ids)
    if n:
        library._touch()
        _stamp_playlists_into_files(moved)
    return n


def playlists_of(song_id) -> list[str]:
    with _connect() as conn:
        return _rust.playlists_of(conn, int(song_id))


def _stamp_playlists_into_files(song_ids) -> None:
    """Escribe TXXX:LISTAS dentro del mp3 para que sobreviva a la base de datos.

    Las rutas y los nombres de lista se piden de una vez (en Rust). Antes se
    abrian dos conexiones POR CANCION (una para la ruta y otra para sus
    listas): meter treinta temas en un repertorio eran sesenta aperturas de la
    base.
    """
    # Solo las de la biblioteca. Escribir dentro de un archivo de fuera seria
    # justo lo que DanPlay promete no hacer: si lo abriste desde el
    # explorador, es tuyo y se queda como esta.
    song_ids = [int(i) for i in song_ids if int(i) > 0]
    if not song_ids:
        return
    with _connect() as conn:
        paths, lists = _rust.playlists_stamp_targets(conn, song_ids)
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
        _rust.playlists_set_stars(conn, int(song_id), int(stars))
    library._touch()
    return ok


def favorite(song_id, value=True) -> bool:
    c = library.by_id(song_id)
    if not c:
        return False
    ok = tags.set_favorite(c["path"], value)
    with _connect() as conn:
        _rust.playlists_set_favorite(conn, int(song_id), bool(value))
    library._touch()
    return ok


def favorites() -> list[dict]:
    with _connect() as conn:
        return _rust.playlists_favorites(conn)


def favorites_count() -> int:
    """Cuantas favoritas hay. Para el numero del menu no hace falta traerse
    las filas enteras, con su letra y sus acordes."""
    with _connect() as conn:
        return _rust.playlists_favorites_count(conn)


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
        name = _rust.playlists_name_of(conn, playlist_id)
    if name is None:
        raise ValueError("no existe esa lista")
    folder = export_folder()
    if target is None:
        safe = names.sanitize(str(name).replace("/", " ").replace("\\", " "))
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
        name = _rust.playlists_name_of(conn, playlist_id)
    if name is None:
        raise ValueError("no existe esa lista")
    folder = export_folder()
    safe = names.sanitize(str(name).replace("/", " ").replace("\\", " "))
    if not safe or safe in (".", "..") or ".." in safe.split():
        raise ValueError("el nombre de la lista no sirve como nombre de archivo")
    target = folder / f"{safe}{suffix}"
    _listas(folder)
    if not library._inside(target.parent, folder):
        raise ValueError("la lista solo se exporta dentro de la carpeta Listas")
    return target, str(name)


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
    wanted = []
    for line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        wanted.append(str((base / line).resolve()) if not os.path.isabs(line) else line)
    with _connect() as conn:
        ids = _rust.playlists_ids_by_path(conn, wanted)
    return add(lid, ids) if ids else 0
