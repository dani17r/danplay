"""La conversacion: el texto del sistema y el bucle que habla con el modelo,
ejecuta sus herramientas y para los pies a la narracion.

El turno lo lleva Rust (library/src/assistant/conversation.rs): el historial
que se le manda, cada vuelta con el modelo, que se hace con lo que contesta
(una llamada escrita como texto, una narracion sin hechos, el empujon, la
respuesta vacia), cada herramienta con su ficha y el informe que redacta la
app. Aqui queda lo que toca fuera y lo que las pruebas cambian: la IA, leer
la lista pegada del turno (`_turn_for`), lo que se apunta para el visto bueno
(`_ask`, con el registro de `plan`) y el uso y el tope del mes (`_finish`).
Rust llama a las funciones de este modulo por su nombre (`_judge_claims`,
`_brief`, `_summarize`, `tools_for`, `_for_model`, `_ask`).
"""

import logging
import sys

import danplay_core as _rust

from .. import ai, library, toon
from . import intake, safe, setlist
from . import tools as _tools
from .context import _brief, _context_note, _window  # noqa: F401  (_brief: lo llama Rust)
from .narration import _judge_claims  # noqa: F401  (lo llama Rust)
from .tools import _describe, _summarize, tools_for  # noqa: F401  (_summarize, tools_for: Rust)

log = logging.getLogger(__name__)

_CONSTANTS = _rust.conversation_constants()

# Cada regla de aqui viene de un fallo real (ids inventados, «ya la cree» sin
# crear nada, doble confirmacion, ordenes coladas por una letra, @menciones
# tomadas por artistas, «verifica» que reescribia la lista). Va en cada
# llamada: se escribe una vez cada cosa y sin adornos, que cada palabra se
# paga. Los resultados de las herramientas llegan en TOON (ver toon.py).
SYSTEM_PROMPT: str = _CONSTANTS["system_prompt"]


# Cuantas veces se le pregunta al modelo en un turno como mucho: vueltas con
# herramientas, reintentos, el empujon contra la narracion y el juez. Una
# lista de seis canciones llego a gastar seis vueltas (y 40 s) y acabo en «Me
# he enredado». Al agotarse, contesta la app con lo que se llevaba hecho.
MAX_MODEL_CALLS: int = _CONSTANTS["max_model_calls"]

# Las que no necesitan otra vuelta del modelo si van bien: la app redacta el
# informe (`setlist.report`), en pasado y con nombres.
SHORTCUT_TOOLS: frozenset[str] = frozenset(_CONSTANTS["shortcut_tools"])

# Lo que se ejecuta aunque el modelo lo haya ESCRITO en vez de llamarlo: solo
# consultas y meter/comprobar una lista. Nada que borre, baje, cree o deje una
# lista exacta: eso, solo con una llamada de verdad (y su confirmacion).
PSEUDO_SAFE: frozenset[str] = frozenset(_CONSTANTS["pseudo_safe"])

# Lo que se le enseña al modelo de cada mensaje del historial, como mucho:
# una lista pegada viaja leida en la nota de la app, no cruda en cada vuelta.
MESSAGE_CHARS: int = _CONSTANTS["message_chars"]

# Los modulos cuyas funciones llama Rust en el momento de cada llamada.
_MODULES = {"ai": ai, "tools": _tools, "toon": toon, "loop": sys.modules[__name__]}


def _clip(text: str, limit: int = MESSAGE_CHARS) -> str:
    return _rust.conversation_clip(str(text or ""), limit)


def _visible_user(messages: list[dict]) -> list[dict]:
    """Los mensajes que escribio la persona (sin avisos ocultos de la app)."""
    return [
        m
        for m in messages
        if m.get("role") != "ai" and not m.get("hidden") and not m.get("event") and not m.get("app")
    ]


def _turn_for(recent: list[dict], chat_id=None) -> setlist.Turn:
    """Lo que la app lee del turno por su cuenta, sin el modelo: la lista
    pegada (la de este mensaje o la ultima de la conversacion) y lo que la
    persona pidio con sus palabras."""
    turn = setlist.Turn(chat_id=chat_id)
    mine = _visible_user(recent)
    if not mine:
        return turn
    last = str(mine[-1].get("text") or "")
    try:
        now = intake.parse_setlist(last)
        if now.is_setlist:
            turn.setlist, turn.fresh = now, True
        else:
            turn.setlist = intake.last_setlist(recent)
        said = last
        # «agrégame esto a la lista del domingo:» y la lista en el siguiente
        # mensaje (el campo de antes los separaba): lo pedido va con la lista
        if turn.fresh and len(mine) > 1:
            before = str(mine[-2].get("text") or "").rstrip()
            if before.endswith(":"):
                said = before + "\n" + last
        turn.intent = intake.read_intent(said)
    except Exception:
        log.warning("no se pudo leer la lista pegada", exc_info=True)
    if turn.setlist is not None:
        turn.name_hint = turn.setlist.name_hint
    return turn


def _setlist_note(turn: setlist.Turn) -> str:
    """La lista pegada, ya leida, para el modelo: como DATOS."""
    if turn.setlist is None:
        return ""
    return _rust.conversation_setlist_note(turn.setlist, turn.fresh, turn.intent)


def _for_model(res: dict) -> dict:
    """El resultado sin la parte de la interfaz (`_ui`): la tabla entera y la
    tarjeta no le hacen falta al modelo y le costaban miles de tokens."""
    return {k: v for k, v in res.items() if k != "_ui"} if isinstance(res, dict) else res


def _pseudo_ok(name: str, args: dict) -> bool:
    """Si una llamada ESCRITA como texto se puede ejecutar igual."""
    return _rust.conversation_pseudo_ok(name, args)


def reply(
    messages: list[dict],
    max_rounds=6,
    context: dict | None = None,
    on_text=None,
    on_tool=None,
    cancel=None,
    chat_id=None,
) -> dict:
    """Conversa usando herramientas. `messages` son {role, text} del historial.

    `context` es lo que la persona tiene delante (vista, seleccion, lo que
    suena) y entra en el estado real. Con `on_text` la respuesta final se
    va entregando segun sale del modelo (texto acumulado; vacio para
    retirar lo enseñado); `on_tool` avisa de cada herramienta al terminar;
    `cancel` es un Event con el que la persona corta a medias.

    Las herramientas que no tienen vuelta atras no se ejecutan aqui: se
    registran (`plan.register`) y se devuelven en `confirm` para que las
    apruebe la persona (ver NEEDS_CONFIRMATION y docs/INTERNAL-CONTRACT.md §3).
    Lo que se hizo con una lista va en `setlist` (la tabla de la interfaz).
    """
    if not ai.available():
        return {"error": _rust.conversation_unavailable(ai.unavailable_reason())}
    ai.begin_turn()
    recent = _window(messages)
    turn = _turn_for(recent, chat_id)
    note = _setlist_note(turn) if turn.setlist is not None and turn.setlist.items else None
    return _rust.conversation_reply(
        recent,
        note,
        _context_note(messages, context),
        max_rounds,
        turn,
        on_text,
        on_tool,
        cancel,
        _finish,
        _MODULES,
    )


def _finish(out: dict, used: list, actions: list, pending, sheet) -> dict:
    """El resultado del turno, con lo que se llevaba hecho, lo que gasto y,
    si lo hay, el tope del mes."""
    out.setdefault("tools", used)
    out.setdefault("actions", actions)
    out.setdefault("confirm", pending)
    if sheet is not None:
        out.setdefault("setlist", sheet)
    usage, via = ai.turn_summary()
    if usage and usage["calls"]:
        out["usage"] = {
            "calls": usage["calls"],
            "prompt": usage["prompt"],
            "completion": usage["completion"],
            "cost": round(usage["cost"], 6) if usage["priced"] else None,
        }
    if via:
        out["via"] = via
    # el tope mensual, si lo hay: la interfaz avisa, no corta
    try:
        limit = ai.providers.budget()
        if limit:
            month = library.ai_usage_summary()["month"]["cost"]
            out["budget"] = {"limit": limit, "month": round(month, 4), "over": month > limit}
    except Exception:  # noqa: BLE001
        pass
    return out


def _ask(
    name: str, args: dict, pending: dict | None, turn: setlist.Turn
) -> tuple[dict, dict | None]:
    """Lo que no tiene vuelta atras: se registra para que la persona lo apruebe.

    Devuelve (lo que lee el modelo, la confirmacion puesta o None). La
    confirmacion la genera el servidor (`plan.register`): la interfaz solo
    puede decir que si a ese id, y lo que se ejecuta sale del registro, no de
    lo que mande el cliente ni de lo que escriba el modelo. Antes de pedir
    permiso para bajar, se mira la biblioteca: lo que ya esta no se ofrece.
    """
    from . import plan

    # Si ya hay una esperando, la segunda NO se pide: se le dice con un error
    # claro, para que no la de por pedida ni la ficha diga «espera tu visto
    # bueno».
    if pending:
        return {
            "error": "ya hay otra accion esperando el visto bueno del usuario; esta NO se "
            "ha pedido. Dilo, y pidela en el siguiente turno."
        }, None
    already: list[dict] = []
    try:
        if name == "download_music":
            checked = _tools.check_downloads(args)
            already = checked["already"]
            if not checked["items"]:
                if already:
                    return {
                        "already_there": [
                            {"item": a["item"][:80], "song": _tools._song_brief(a["song"])}
                            for a in already[:20]
                        ],
                        "note": "ya estaban todas en la biblioteca: no hace falta bajar nada. "
                        "Diselo con sus nombres.",
                    }, None
                return {"error": "no me has dicho que bajar"}, None
            clean = {
                "items": checked["items"],
                "quality": args.get("quality"),
                "file_it": args.get("file_it", True) is not False,
                "force": args.get("force") is True,
            }
            asked = plan.register(
                name,
                clean,
                plan={
                    "rows": checked["rows"],
                    "already": [
                        {"title": a["item"], "song": a["song"]} for a in already[: plan.MAX_ALREADY]
                    ],
                    "trimmed": checked["dropped"],
                },
                chat_id=turn.chat_id,
            )
            summary = asked["summary"]
            # compatibilidad: una interfaz de antes confirma con {tool, args}
            asked = {**asked, "args": clean}
        else:
            asked = plan.register(name, args, chat_id=turn.chat_id)
            summary = asked["summary"]
            asked = {**asked, "args": args}
    except ValueError as e:
        return {"error": f"no se pudo pedir: {safe.datum(str(e), 160)}"}, None
    except Exception:
        log.warning("no se pudo registrar la confirmacion de %s", name, exc_info=True)
        summary = _describe(name, args)
        asked = {"tool": name, "args": args, "summary": summary}
    res: dict = {
        "needs_confirmation": True,
        "summary": summary,
        "note": "se le ha preguntado al usuario en la app; no lo repitas",
    }
    if already:
        res["already_there"] = [
            {"item": a["item"][:80], "song": _tools._song_brief(a["song"])} for a in already[:20]
        ]
        res["note"] += "; las de already_there ya estaban y no se ofrecen: diselo"
    turn.can_ask = False
    return res, asked
