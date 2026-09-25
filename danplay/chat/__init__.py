"""Chat con la biblioteca.

El asistente consulta el catalogo, arma listas, busca letras y acordes,
busca en YouTube y en la web, y descarga musica pasandola por la misma
tuberia de siempre (identificar, renombrar, archivar por artista).

Habla SOLO de musica. Lo que no tenga que ver con musica lo dice y ya.

Por partes:

  tools      lo que se le declara al modelo y lo que hace cada herramienta
  loop       el texto del sistema y la conversacion (`reply`)
  context    la ventana de historial, las notas de cada turno y el estado real
  narration  detectar que dice haber hecho algo sin hacerlo
"""

from .. import ai, library, playlists, youtube
from .context import (
    CONTEXT_ROWS,
    HISTORY_CHARS,
    HISTORY_MESSAGES,
    _brief,
    _context_note,
    _screen_note,
    _tool_note,
    _window,
)
from .loop import SYSTEM_PROMPT, reply
from .narration import (
    NUDGE,
    PSEUDO_CALL,
    _answers_an_offer,
    _judge_claims,
    claims_action,
    has_markers,
    parse_pseudo_call,
    strip_markers,
    wants_action,
)
from .tools import (
    ACTING_TOOLS,
    EDITABLE_BY_ASSISTANT,
    HANDLERS,
    MAX_DOWNLOADS,
    MAX_TOOL_CALLS,
    NEEDS_CONFIRMATION,
    OPTIONAL_TOOLS,
    TOOL_ALIASES,
    TOOL_NAMES,
    TOOLS,
    _checked_song_ids,
    _describe,
    _find_playlist,
    _first_names,
    _song_brief,
    _summarize,
    confirm,
    download_plan,
    run_tool,
    tools_for,
    wrap_external,
)

_strip_thoughts = ai.strip_thoughts

__all__ = [
    "ACTING_TOOLS",
    "CONTEXT_ROWS",
    "EDITABLE_BY_ASSISTANT",
    "HANDLERS",
    "HISTORY_CHARS",
    "HISTORY_MESSAGES",
    "MAX_DOWNLOADS",
    "MAX_TOOL_CALLS",
    "NEEDS_CONFIRMATION",
    "NUDGE",
    "OPTIONAL_TOOLS",
    "PSEUDO_CALL",
    "SYSTEM_PROMPT",
    "TOOLS",
    "TOOL_ALIASES",
    "TOOL_NAMES",
    "_answers_an_offer",
    "_brief",
    "_checked_song_ids",
    "_context_note",
    "_describe",
    "_find_playlist",
    "_first_names",
    "_judge_claims",
    "_screen_note",
    "_song_brief",
    "_strip_thoughts",
    "_summarize",
    "_tool_note",
    "_window",
    "ai",
    "claims_action",
    "confirm",
    "download_plan",
    "has_markers",
    "library",
    "parse_pseudo_call",
    "playlists",
    "reply",
    "run_tool",
    "strip_markers",
    "tools_for",
    "wants_action",
    "wrap_external",
    "youtube",
]
