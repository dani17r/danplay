"""Las herramientas del asistente: lo que se le declara al modelo y lo que
hace cada una.

Hacen lo mismo que la persona con el raton (buscar, armar repertorios,
puntuar, reproducir, descargar) y llaman a las mismas funciones del nucleo
que los botones: no hay un camino paralelo. Lo que no tiene vuelta atras no
se ejecuta aqui dentro de la conversacion (ver NEEDS_CONFIRMATION).

Lo que hace cada herramienta lo decide Rust (library/src/assistant/tools.rs):
las declaraciones, cuales van en cada turno, lo que hace cada una con lo que
devuelven la biblioteca, las listas, YouTube o la web, y como se le cuenta al
modelo. Aqui quedan los modelos de los argumentos (pydantic los valida), el
reparto a cada manejador con su red de seguridad, y los modulos cuyas
funciones llama Rust en el momento (las pruebas las cambian).
"""

import logging
from collections.abc import Callable
from typing import Annotated, Any

import danplay_core as _rust
from pydantic import BaseModel, BeforeValidator, ConfigDict, ValidationError

from .. import chord_sheets, config, enrich, library, playlists, theory, web, youtube
from . import resolve, setlist
from .plan import MAX_DOWNLOADS  # noqa: F401  (lo reexporta chat)

log = logging.getLogger(__name__)

# Los modulos cuyas funciones llama Rust (las busca en el momento de cada
# llamada: lo que una prueba cambie en ellos, vale).
_MODULES = {
    "library": library,
    "playlists": playlists,
    "youtube": youtube,
    "enrich": enrich,
    "web": web,
    "theory": theory,
    "chord_sheets": chord_sheets,
    "config": config,
    "resolve": resolve,
}

_CONSTANTS = _rust.tools_constants()

# Lo que no tiene vuelta atras NO lo hace el modelo por su cuenta.
#
# El texto del prompt le pide que consulte antes, pero un prompt no es una
# barrera: por `search_web`, por los titulos de YouTube y por las letras entra
# texto que escribe cualquiera, y ahi puede venir algo con forma de orden. En
# vez de fiarlo a que el modelo se porte bien, estas herramientas devuelven
# lo que IBAN a hacer y quien decide es la persona, en un dialogo de la app
# (docs/INTERNAL-CONTRACT.md §3).
NEEDS_CONFIRMATION: tuple[str, ...] = _CONSTANTS["needs_confirmation"]


# Cuantas herramientas puede encadenar en un turno.
MAX_TOOL_CALLS: int = _CONSTANTS["max_tool_calls"]


# Las que HACEN algo (o lo piden, como las de confirmacion). Las demas solo
# consultan: si un turno solo consulto y el texto dice «añadida a la lista»,
# tampoco ha pasado nada.
ACTING_TOOLS: frozenset[str] = frozenset(_CONSTANTS["acting_tools"])


def _describe(name: str, args: dict) -> str:
    """Que se va a hacer, en una frase que se pueda leer en un dialogo.

    Va fuera del `try` de las herramientas: si algo de aqui fallara con unos
    argumentos raros («id»: "abc"), se diria en generico antes que tumbar la
    respuesta entera con un 500.
    """
    try:
        return _rust.tools_describe(name, args if isinstance(args, dict) else {}, _MODULES)
    except Exception:
        log.warning("no pude describir %s %r", name, args, exc_info=True)
        return f"Ejecutar {name}."


def _find_playlist(args: dict) -> tuple[dict | None, str]:
    """El repertorio al que se refiere la herramienta, o por que no se sabe.

    Se admite `playlist_id`/`id` o `name`. El nombre manda si viene: el
    modelo conoce las listas por como se llaman, y cuando adivinaba el id
    acababa pidiendo borrar «domingo» queriendo borrar «Herlin».
    """
    return _rust.tools_find_playlist(args, _MODULES)


def _checked_song_ids(raw) -> tuple[list[int], str]:
    """Los ids de cancion que existen, o el error si alguno no existe.

    Todo o nada: si el modelo trae un id que no es ninguna cancion es que se
    lo ha inventado, y entonces los demas tampoco son de fiar.
    """
    return _rust.tools_checked_song_ids(raw, _MODULES)


def download_plan(args: dict | None) -> dict:
    """Lo que se va a bajar, saneado, a partir de los argumentos de la herramienta.

    Un solo sitio para leerlos: lo usan el dialogo de confirmacion, la
    ejecucion desde el chat y el endpoint que arranca la descarga aprobada.
    Cuando cada uno los leia por su cuenta, la API buscaba `query`, que la
    herramienta no tiene, y la descarga confirmada nunca arrancaba.

    Se admite tambien `query` (texto suelto) por si el modelo lo manda asi.
    """
    return _rust.tools_download_plan(args, config.MP3_QUALITY)


def check_downloads(args: dict | None) -> dict:
    """Biblioteca primero: lo que de verdad hay que pedir permiso para bajar.

    Antes de enseñar la tarjeta de una descarga pedida («baja esto»), cada
    texto se busca en la biblioteca y cada enlace en el historial de descargas
    (si sigue siendo esa cancion). Lo que ya esta no se propone: se dice donde
    esta. Con `force` (la quiere como otra version) no se mira nada.

    Devuelve el plan de `download_plan` con `items` = lo que falta, `already`
    = [{item, song}] y `rows` = las filas de la tarjeta.
    """
    return _rust.tools_check_downloads(args, _MODULES)


def wrap_external(text: str) -> str:
    """Envuelve el texto de terceros para que se vea que es un dato.

    Lo que devuelven la busqueda web, los titulos de YouTube y las letras lo
    escribe cualquiera. Marcarlo no es una garantia, pero es una señal mas
    para el modelo, ademas de lo que ya dice el prompt.
    """
    return _rust.tools_wrap_external(text)


# Lo que se le declara al modelo de cada herramienta.
TOOLS: list[dict] = _rust.tools_declared()


# Herramientas que se fusionaron para ahorrar tokens en cada llamada (cada
# declaracion cuesta lo suyo): los nombres viejos siguen valiendo por si un
# modelo los recuerda o una conversacion guardada los trae (ver `_alias`).
TOOL_ALIASES: dict[str, str] = _CONSTANTS["tool_aliases"]


TOOL_NAMES = frozenset(h["function"]["name"] for h in TOOLS) | frozenset(TOOL_ALIASES)


# Las herramientas se declaran en cada llamada y cada una cuesta tokens.
# Las de descargar, las de musico y la de letra+caratula solo hacen falta
# cuando la conversacion va de eso: se añaden si la ventana de historial
# (lo que dijo cualquiera de los dos) menciona el tema, si la persona
# responde a una oferta o si es el remate de una descarga. El resto va siempre.
# Las expresiones (con re.IGNORECASE), las mira Rust.
OPTIONAL_TOOLS: dict[str, str] = _CONSTANTS["optional_tools"]


def tools_for(recent: list[dict], everything: bool = False, setlist: bool = False) -> list[dict]:
    """Las herramientas que se declaran en este turno. Con una lista pegada
    (`setlist`), las de listas de canciones van aunque no se nombren."""
    if everything:
        return TOOLS
    text = "\n".join(str(m.get("text") or "") for m in recent)
    return [TOOLS[i] for i in _rust.tools_for(text, bool(setlist))]


# Lo que edit_song puede cambiar con library.edit: lo que declara la
# herramienta. Lo demas que admite `library.update` (acordes, estudio, la
# letra con tiempos) no es cosa del modelo.
EDITABLE_BY_ASSISTANT: tuple[str, ...] = _CONSTANTS["editable"]


def _song_brief(c):
    return _rust.tools_song_brief(c)


# ------------------------------------------------------ las herramientas
# Cada herramienta es un manejador con el modelo de sus argumentos: el
# modelo de IA manda lo que quiere (claves de mas, numeros como texto,
# `null`), pydantic lo convierte o dice que falla, y el manejador solo ve
# argumentos con su tipo. Antes era una cadena de doscientas lineas de `if`
# que leia el diccionario a mano.


class Args(BaseModel):
    """Los argumentos de una herramienta. Lo que sobra se ignora: los modelos
    se inventan claves, y eso no es motivo para no hacer lo que se pidio."""

    model_config = ConfigDict(extra="ignore")


def _int_or_none(value):
    """Un entero si se entiende como tal; si no, None (y decide el tope)."""
    if value is None or isinstance(value, bool):
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _text_or_none(value):
    """El año o el tono pueden llegar como numero (2018, 7): valen como texto."""
    if value is None or isinstance(value, str):
        return value
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return str(value)
    return value


LooseInt = Annotated[int | None, BeforeValidator(_int_or_none)]
LooseText = Annotated[str | None, BeforeValidator(_text_or_none)]


class PlaylistRef(Args):
    """Un repertorio, por nombre (lo normal) o por id. Ver `_find_playlist`."""

    name: LooseText = None
    playlist: LooseText = None
    playlist_id: Any = None
    id: Any = None

    def find(self) -> tuple[dict | None, str]:
        return _find_playlist(self.model_dump())


class SongRef(Args):
    id: int


class SearchSongsArgs(Args):
    query: LooseText = ""
    limit: LooseInt = None
    sort: LooseText = "artist"
    desc: bool = False


class CreatePlaylistArgs(Args):
    name: LooseText = ""
    ids: Any = None
    note: LooseText = ""


class PlaylistSongsArgs(PlaylistRef):
    ids: Any = None


class RenamePlaylistArgs(PlaylistRef):
    new_name: LooseText = ""
    note: LooseText = None


class GetLyricsArgs(Args):
    id: LooseInt = None
    artist: LooseText = ""
    title: LooseText = ""


class TransposeArgs(Args):
    chords: str
    from_key: str
    to_key: str


class QueryArgs(Args):
    query: LooseText = ""
    limit: LooseInt = None


class DownloadArgs(Args):
    """Tal cual: `download_plan` los sanea (ahi se leen tambien desde la API)."""

    model_config = ConfigDict(extra="allow")


class PlayArgs(PlaylistRef):
    id: LooseInt = None


class PlayerArgs(Args):
    command: LooseText = ""


class EditSongArgs(Args):
    id: int
    title: LooseText = None
    artist: LooseText = None
    album: LooseText = None
    year: LooseText = None
    genre: LooseText = None
    key: LooseText = None
    bpm: float | None = None
    stars: LooseInt = None
    favorite: bool | None = None


class LyricsAndCoverArgs(Args):
    id: int
    lyrics: bool = True
    cover: bool = True


class RemoveFromPlaylistArgs(PlaylistRef):
    song_ids: Any = None
    song_id: Any = None


class KeyArgs(Args):
    key: LooseText = ""


class SheetArgs(PlaylistRef):
    with_lyrics: bool = False


class FillPlaylistArgs(Args):
    playlist: LooseText = ""
    name: LooseText = ""
    mode: LooseText = None
    items: Any = None
    only: Any = None
    overrides: Any = None
    create: bool = False
    download_missing: bool | None = None


class FindSongsArgs(Args):
    items: Any = None


Handler = Callable[..., dict]
HANDLERS: dict[str, tuple[type[Args], Handler]] = {}
# Las que necesitan saber del turno (la lista pegada, lo que pidio la persona
# con sus palabras): reciben el `setlist.Turn` como segundo argumento.
_WITH_TURN: set[str] = set()


def tool(name: str, model: type[Args] = Args, *, turn: bool = False):
    """Registra el manejador de una herramienta con el modelo de sus argumentos."""

    def register(fn: Handler) -> Handler:
        HANDLERS[name] = (model, fn)
        if turn:
            _WITH_TURN.add(name)
        return fn

    return register


@tool("fill_playlist", FillPlaylistArgs, turn=True)
def _fill_playlist(a: FillPlaylistArgs, turn: setlist.Turn) -> dict:
    return setlist.fill(a.model_dump(), turn)


@tool("find_songs", FindSongsArgs, turn=True)
def _find_songs(a: FindSongsArgs, turn: setlist.Turn) -> dict:
    return setlist.find(a.items, turn)


def _in_rust(name: str) -> Handler:
    """El manejador de una herramienta que hace Rust, con sus argumentos ya
    validados."""

    def handler(a: Args) -> dict:
        return _rust.tools_run(name, a.model_dump(), _MODULES)

    handler.__name__ = handler.__qualname__ = f"_{name}"
    return handler


for _name, _model in (
    ("search_songs", SearchSongsArgs),
    ("library_summary", Args),
    ("create_playlist", CreatePlaylistArgs),
    ("list_playlists", Args),
    ("playlist_songs", PlaylistRef),
    ("add_to_playlist", PlaylistSongsArgs),
    ("set_playlist_songs", PlaylistSongsArgs),
    ("rename_playlist", RenamePlaylistArgs),
    ("get_lyrics", GetLyricsArgs),
    ("get_chords", GetLyricsArgs),
    ("music_details", SongRef),
    ("transpose_chords", TransposeArgs),
    ("search_youtube", QueryArgs),
    ("download_music", DownloadArgs),
    ("download_status", Args),
    ("search_web", QueryArgs),
    ("play", PlayArgs),
    ("player_control", PlayerArgs),
    ("edit_song", EditSongArgs),
    ("find_lyrics_and_cover", LyricsAndCoverArgs),
    ("delete_song", SongRef),
    ("remove_from_playlist", RemoveFromPlaylistArgs),
    ("delete_playlist", PlaylistRef),
    ("related_keys", KeyArgs),
    ("setlist_sheet", SheetArgs),
):
    tool(_name, _model)(_in_rust(_name))


PLAYER_COMMANDS: tuple[str, ...] = _CONSTANTS["player_commands"]


def _alias(name: str, args: dict) -> tuple[str, dict]:
    """Los nombres viejos de las herramientas fusionadas, con sus argumentos
    de entonces pasados a los de ahora."""
    if name == "set_stars":
        return "edit_song", {"id": args.get("id"), "stars": args.get("stars", 0)}
    if name == "set_favorite":
        return "edit_song", {"id": args.get("id"), "favorite": args.get("favorite", True)}
    if name in ("play_song", "play_playlist"):
        return "play", args
    if name == "lyrics_by_name":
        return "get_lyrics", {k: v for k, v in args.items() if k != "id"}
    return name, args


def _explain(name: str, error: ValidationError) -> str:
    """Por que no valen los argumentos, en castellano (el modelo lo lee y
    corrige la llamada; en ingles tecnico no siempre lo entendia)."""
    return _rust.tools_explain(
        name, [([str(x) for x in err["loc"]], err["type"]) for err in error.errors()]
    )


def run_tool(name, args, turn: setlist.Turn | None = None) -> dict:
    """Ejecuta una herramienta y devuelve el resultado (un dict; al modelo le
    llega en TOON, ver `reply`). Nunca levanta: lo que falle vuelve como
    `{"error": ...}` y el modelo lo cuenta.

    `turn` es lo que el bucle sabe del turno (la lista pegada, lo que pidio la
    persona con sus palabras): lo usan `fill_playlist` y `find_songs`. Sin el,
    como si no hubiera lista ni pedido nada especial."""
    name, args = _alias(str(name or ""), args if isinstance(args, dict) else {})
    entry = HANDLERS.get(name)
    if entry is None:
        return {"error": f"herramienta desconocida: {name}"}
    model, handler = entry
    try:
        parsed = model.model_validate(args)
    except ValidationError as e:
        return {"error": _explain(name, e)}
    try:
        if name in _WITH_TURN:
            return handler(parsed, turn if turn is not None else setlist.Turn())
        return handler(parsed)
    except Exception as e:
        # lo que falle aqui se le cuenta al modelo como un error de la
        # herramienta: el turno sigue y el lo explica
        log.info("la herramienta %s fallo", name, exc_info=True)
        return {"error": str(e)}


def confirm(tool: str, args: dict) -> dict:
    """Ejecuta lo que la persona acaba de aprobar.

    Solo las tres que hacen falta aprobar: cualquier otra cosa no pasa por
    aqui, y asi esta puerta no se convierte en una forma de ejecutar
    herramientas sin pasar por la conversacion.
    """
    if tool not in NEEDS_CONFIRMATION:
        return {"ok": False, "error": "eso no necesita confirmacion"}
    result = run_tool(tool, args or {})
    ok = not result.get("error")
    if not ok:
        return {"ok": False, "result": result, "text": str(result.get("error"))}
    texts = {
        "delete_song": "Listo, esta en la papelera del sistema.",
        "delete_playlist": "Repertorio borrado. Las canciones siguen en tu biblioteca.",
        "download_music": _summarize("download_music", result),
    }
    return {"ok": True, "result": result, "text": texts.get(tool, "Hecho.")}


def _summarize(name, res) -> str:
    return _rust.tools_summarize(name, res)


def _first_names(res, limit=3) -> str:
    """Las primeras canciones de la lista, para que se vea QUE entro."""
    return _rust.tools_first_names(res, limit)
