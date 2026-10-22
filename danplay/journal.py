"""El diario: lo tuyo, fuera de la base y junto a tu musica.

Las listas (en su orden), las estrellas, las favoritas, lo del modo estudio,
los cifrados y las letras con tiempos viven en la base de DanPlay, que es
desechable (se reconstruye escaneando). Parte viaja tambien dentro de cada
archivo (las etiquetas), pero no todo: el orden de las listas, sus notas y
colores, los cifrados... Y una lista guarda canciones por su id: si la
cancion se mueve por fuera, se vuelve a bajar o la base se pierde, en
cualquier reproductor la lista se rompe.

Aqui se guarda todo eso en un archivo, `.danplay/journal.json`, dentro de
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
import logging
import os
import threading
import time
from pathlib import Path

import danplay_core as _rust

from . import config, library, names

log = logging.getLogger(__name__)

FOLDER = ".danplay"
FILE = "journal.json"
FORMAT = "danplay-journal"
VERSION = 1
# Lo que la persona pone de una cancion, como esta en la base.
FIELDS = ("stars", "favorite", "blur", "study", "chords", "lyrics_synced", "key", "bpm")
# Cada cuanto se mira si hay algo nuevo que escribir.
EVERY = 3.0

# Lo de leer, escribir y comparar el diario lo hace Rust
# (library/src/journal.rs); aqui quedan el hilo que lo escribe, el cerrojo
# de las listas, el aviso a la interfaz y las listas exportadas.


def _uid() -> str:
    """El id de esta base: cambia si la base es otra (se perdio, otro equipo)."""
    with library.connect() as conn:
        return _rust.journal_uid(conn)


def _roots() -> list[str]:
    from .library.folders import _roots as roots

    return [r for r in roots() if os.path.isdir(r)]


# ------------------------------------------------------------ escribir


def snapshot() -> dict:
    """Lo que va en el diario: las canciones con algo tuyo (o en una lista)
    con su ficha, y las listas en su orden."""
    from . import __version__

    roots = _roots()
    uid = _uid()
    with library.connect() as conn:
        return _rust.journal_snapshot(conn, f"DanPlay {__version__}", uid, roots)


def _targets() -> list[Path]:
    return [Path(p) for p in _rust.journal_targets(config.DATA_DIR, _roots())]


def _write(path: Path, text: str) -> bool:
    if _rust.journal_write_to(path, text):
        return True
    log.info("no se pudo escribir el diario en %s", path)
    return False


_STATE: dict = {"written": "", "revision": -1, "restored": False, "report": None, "seq": 0}
_LOCK = threading.Lock()


def write() -> int:
    """Escribe el diario donde toca (si cambio). Devuelve en cuantos sitios."""
    data = snapshot()
    body = _rust.journal_body(data)
    with _LOCK:
        if body == _STATE["written"]:
            return 0
    text = _rust.journal_text(data)
    n = sum(_write(p, text) for p in _targets())
    with _LOCK:
        _STATE["written"] = body
    return n


def _load(path: Path) -> dict | None:
    return _rust.journal_load(path)


def latest() -> dict | None:
    """El diario mas reciente de los que haya (el de los datos de DanPlay o
    el de alguna carpeta de musica: puede venir de otro equipo)."""
    return _rust.journal_latest(_targets())


# ------------------------------------------------------------ recuperar


def _index(rows):
    """Las canciones de ahora, para buscar en ellas (`_find`)."""
    return (list(rows), _roots())


def _find(entry: dict, rows, roots):
    """La cancion de ahora que es la del diario: en su ruta, en la misma ruta
    dentro de su carpeta (la carpeta se movio), con el mismo nombre y tamaño,
    o con el mismo artista y titulo y la misma duracion."""
    at = _rust.journal_find(dict(entry), [dict(r) for r in rows], roots)
    return None if at is None else rows[at]


def reconcile() -> dict:
    """Compara el diario con lo que hay y recupera lo que se perdio (ver
    arriba). Devuelve lo que hizo: {songs, lists, entries, ghosts}."""
    from . import playlists

    told = {"songs": 0, "lists": 0, "entries": 0, "ghosts": 0, "fresh": False}
    journal = latest()
    if not journal:
        with _LOCK:
            _STATE["restored"] = True
        return told
    uid = _uid()
    with playlists.LOCK, library.connect() as conn:
        r = _rust.journal_reconcile(conn, journal, uid, _roots())
    stamp = r.pop("stamp")
    told.update(r)
    fresh = told["fresh"]
    if told["songs"] or told["lists"] or told["entries"] or told["ghosts"]:
        library._touch()
        if stamp:
            with contextlib.suppress(Exception):
                playlists._stamp_playlists_into_files(stamp)
    refresh_exports()
    if fresh:
        # el diario pasa a ser el de esta base (si no, la proxima vez se
        # volveria a tomar por nueva)
        with _LOCK:
            _STATE["written"] = ""
        write()
    with _LOCK:
        _STATE["restored"] = True
        if told["songs"] or told["lists"] or told["entries"] or told["ghosts"]:
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

    t = threading.Thread(target=run, name="danplay-journal", daemon=True)
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
