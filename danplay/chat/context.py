"""Lo que el modelo ve de cada turno ademas del mensaje: la ventana de
historial, las notas de lo que hizo de verdad en los anteriores y el estado
real de la app (lo que hay, lo que la persona tiene delante)."""

import datetime as _dt
import os
import time as _t

import danplay_core as _rust

from .. import library, playlists, youtube
from .narration import _txt

# Cuanto historial se le da al modelo, lo que se recuerda de la tabla de una
# lista y cuantas canciones de la vista se le enseñan: ver
# library/src/assistant/context.rs, que es quien escribe las notas.
HISTORY_MESSAGES = 48
HISTORY_CHARS = 16_000
LIST_DETAIL = 1800
CONTEXT_ROWS = 20


def _window(messages: list[dict]) -> list[dict]:
    """Los ultimos mensajes que caben, empezando por uno del usuario."""
    return messages[_rust.context_window_start(messages) :]


def _brief(name: str, res: dict, limit=10) -> str:
    """Lo que devolvio una herramienta, en una linea con ids y nombres (la
    memoria entre turnos: «la segunda», «esa»)."""
    return _rust.context_brief(name, res, limit)


def _tool_note(text: str, tools, app=False) -> str:
    """La nota de sistema que acompaña a un mensaje anterior del asistente:
    que herramientas uso y que devolvieron, o que lo que dice haber hecho no
    ocurrio. Vacia si no hay nada que decir."""
    return _rust.context_tool_note(_txt(text or ""), tools, app)


def _screen_note(context: dict | None) -> list[str]:
    """Lo que la persona tiene delante, en dos o tres lineas («esta», «la
    segunda», «las seleccionadas», «la que suena»)."""
    return _rust.context_screen_note(context)


def _still_there(h: dict) -> dict | None:
    """La cancion de una descarga del historial, si sigue siendo ESA.

    Los ids de cancion se reutilizan cuando se borra la ultima: el id de una
    descarga de hace una semana puede ser hoy otra cancion. Vale solo si su
    archivo es el que dejo la descarga (o se llama igual: se pudo mover)."""
    try:
        song = library.by_id(int(h["song_id"])) if h.get("song_id") else None
    except (TypeError, ValueError):
        return None
    if not song:
        return None
    target = str(h.get("target") or "")
    path = str(song.get("path") or "")
    if not target or path == target or os.path.basename(path) == os.path.basename(target):
        return dict(song)
    return None


def _context_note(messages: list[dict], context: dict | None = None) -> str:
    """El estado real de la app, para el turno que empieza.

    Lo que hay de verdad manda sobre lo que el modelo dijo antes. Aqui se
    reune (lo que no se pueda leer, no se dice); el texto lo escribe Rust.
    """
    facts: dict = {"today": _dt.date.today().strftime("%d/%m/%Y")}
    try:
        facts["stats"] = library.stats_of()
        facts["top"] = library.top_artists(12)
    except Exception:  # noqa: BLE001
        facts.pop("stats", None)
    try:
        facts["lists"] = playlists.list_all()
    except Exception:  # noqa: BLE001
        pass
    try:
        facts["download"] = dict(youtube.STATE)
    except Exception:  # noqa: BLE001
        pass
    try:
        recent = [h for h in library.download_history(6) if h.get("ok")][:3]
        facts["recent"] = [(h, bool(_still_there(h))) for h in recent]
        facts["now"] = _t.time()
    except Exception:  # noqa: BLE001
        facts.pop("recent", None)
    return _rust.context_note(messages, context, facts)
