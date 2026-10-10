"""El asistente: la conversacion (entera o en vivo), sus confirmaciones y las
conversaciones guardadas (docs/CONTRATO-INTERNO.md §3)."""

import logging
import secrets
import threading
import time
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Path, Query

from ... import ai, chat, chats, youtube
from ...chat import plan
from ..common import _in_background
from ..models import ChatAppendIn, ChatConfirmIn, ChatCreateIn, ChatIn, ChatRenameIn
from .downloads import NO_FOLDER

log = logging.getLogger(__name__)

router = APIRouter()


@router.post("/api/chat")
def converse(body: Annotated[ChatIn, Body()]):
    """Chat con acceso a la biblioteca. `messages`: [{role: 'user'|'ai', text}]"""
    if not body.messages:
        raise HTTPException(400, "no hay mensajes")
    return chat.reply([m.plain() for m in body.messages], context=body.context)


# ------------------------------------------------- el chat, en vivo
# La respuesta se pide en un hilo y la interfaz la va leyendo: el texto
# segun sale del modelo, las herramientas segun terminan, y al final el
# mismo resultado que da /api/chat. Se hace preguntando (cada pocos
# cientos de milisegundos por el socket) y no con un flujo abierto: asi no
# hay que enseñar al puente de Rust a leer respuestas a trozos, y vale
# igual en el navegador.
CHAT_JOBS: dict[str, dict] = {}


_CHAT_JOBS_LOCK = threading.Lock()


CHAT_JOB_TTL = 600


def _chat_job_new(messages, context) -> dict:
    with _CHAT_JOBS_LOCK:
        now = time.time()
        for key, job in list(CHAT_JOBS.items()):
            if job["done"] and now - job["at"] > CHAT_JOB_TTL:
                del CHAT_JOBS[key]
        job = {
            "id": secrets.token_hex(8),
            "at": now,
            "text": "",
            "tools": [],
            "done": False,
            "result": None,
            "cancel": threading.Event(),
        }
        CHAT_JOBS[job["id"]] = job

    def work():
        try:
            r = chat.reply(
                messages,
                context=context,
                on_text=lambda t: job.__setitem__("text", t),
                on_tool=job["tools"].append,
                cancel=job["cancel"],
            )
        except Exception as e:
            log.warning("el chat en vivo fallo", exc_info=True)
            r = {"error": f"no pude responder: {e}"}
        job["result"] = r
        job["done"] = True
        job["at"] = time.time()

    threading.Thread(target=work, name="danplay-chat", daemon=True).start()
    return job


@router.post("/api/chat/start")
def chat_start(body: Annotated[ChatIn, Body()]):
    if not body.messages:
        raise HTTPException(400, "no hay mensajes")
    job = _chat_job_new([m.plain() for m in body.messages], body.context)
    return {"id": job["id"]}


@router.get("/api/chat/poll/{job_id}")
def chat_poll(job_id: str):
    job = CHAT_JOBS.get(job_id)
    if not job:
        raise HTTPException(404, "esa respuesta ya no esta")
    return {
        "id": job_id,
        "text": job["text"],
        "tools": list(job["tools"]),
        "done": job["done"],
        "result": job["result"],
    }


@router.post("/api/chat/cancel/{job_id}")
def chat_cancel(job_id: str):
    job = CHAT_JOBS.get(job_id)
    if not job:
        raise HTTPException(404, "esa respuesta ya no esta")
    job["cancel"].set()
    return {"ok": True}


# ------------------------------------------------- conversaciones guardadas


@router.get("/api/chats")
def chats_list():
    return {"chats": chats.list_all()}


@router.post("/api/chats")
def chats_create(body: Annotated[ChatCreateIn | None, Body()] = None):
    body = body or ChatCreateIn()
    return chats.create(body.title)


@router.get("/api/chats/search")
def chats_search(q: Annotated[str, Query(max_length=200)] = ""):
    return {"hits": chats.search(q)}


@router.get("/api/chats/{chat_id}")
def chats_get(chat_id: int):
    c = chats.get(chat_id)
    if not c:
        raise HTTPException(404, "no existe esa conversacion")
    return c


@router.post("/api/chats/{chat_id}/messages")
def chats_append(chat_id: int, body: Annotated[ChatAppendIn, Body()]):
    try:
        n = chats.append(chat_id, [m.plain() for m in body.messages])
    except ValueError as e:
        raise HTTPException(404, str(e)) from e
    return {"n": n}


@router.get("/api/chats/{chat_id}/export")
def chats_export(chat_id: int):
    md = chats.export_markdown(chat_id)
    if md is None:
        raise HTTPException(404, "no existe esa conversacion")
    return {"markdown": md}


@router.patch("/api/chats/{chat_id}")
def chats_rename(chat_id: int, body: Annotated[ChatRenameIn, Body()]):
    if not chats.rename(chat_id, body.title):
        raise HTTPException(404, "no existe esa conversacion")
    return {"ok": True}


@router.delete("/api/chats/{chat_id}")
def chats_delete(chat_id: int):
    if not chats.delete(chat_id):
        raise HTTPException(404, "no existe esa conversacion")
    return {"ok": True}


@router.post("/api/chat/confirm")
def confirm_tool(body: Annotated[ChatConfirmIn, Body()]):
    """Ejecuta lo que el asistente pidio y la persona acaba de aprobar.

    Lo que no tiene vuelta atras no lo hace el modelo por su cuenta: devuelve
    lo que iba a hacer y hasta que no pasa por aqui no ocurre nada.

    Forma nueva `{id, selection?}`: el id es el de una propuesta registrada
    (`chat.plan.register`) y lo que se ejecuta sale del REGISTRO, no del
    cliente (que no puede mandar `tool` ni `args`). 410 si ya no esta viva,
    409 si hay otra descarga en marcha (sin gastarla). Forma antigua
    `{tool, args}`: la de siempre, por compatibilidad.
    """
    if body.id is not None:
        return _confirm_registered(body.id, body.selection)
    tool = body.tool or ""
    if tool not in chat.NEEDS_CONFIRMATION:
        raise HTTPException(400, "eso no necesita confirmacion")
    args = body.args or {}
    # Descargar tarda minutos: se arranca y se contesta enseguida, igual que
    # el boton de Descargas. Antes bloqueaba la peticion del chat.
    if tool == "download_music":
        # Los argumentos son los de la herramienta tal y como los declara
        # `chat.TOOLS`: `items` (lista), `quality`, `file_it` y `force`.
        # Aqui se leia `query`, que la herramienta no tiene, asi que TODA
        # confirmacion de descarga acababa en «hace falta algo que
        # descargar» aunque la persona acabara de decir que si.
        wanted = chat.download_plan(args)
        if not wanted["items"]:
            raise HTTPException(400, "hace falta algo que descargar")
        if not youtube.available():
            raise HTTPException(503, youtube.unavailable_reason())
        if not youtube.folder()["ready"]:
            raise HTTPException(409, NO_FOLDER)
        if not youtube.claim():
            raise HTTPException(409, "ya hay una descarga en marcha")
        _in_background(
            "danplay-download",
            lambda: youtube.run_many(
                wanted["items"],
                quality=wanted["quality"],
                file_it=wanted["file_it"],
                results=1,
                force=wanted["force"],
                source="assistant",
                claimed=True,
            ),
        )
        n = len(wanted["items"])
        return {
            "ok": True,
            "result": {
                "active": True,
                "items": wanted["items"],
                "force": wanted["force"],
                "trimmed": wanted["trimmed"],
            },
            "text": (
                "Descargando" + (f" {n} temas" if n > 1 else "") + ". Te cuento cuando termine."
            ),
        }

    return chat.confirm(tool, args)


def _confirm_registered(entry_id: str, selection: list[int] | None) -> dict:
    """Atiende una propuesta registrada por su id (`chat.plan`).

    Nada sale del cliente salvo las casillas: herramienta, argumentos y lo que
    se hace al terminar los pone el registro. La descarga primero se queda el
    turno (`youtube.claim`) y SOLO despues gasta la propuesta: si hay otra en
    marcha, 409 y la propuesta sigue viva; si la propuesta ya no vale, se
    suelta el turno y 410.
    """
    st = plan.status(entry_id)
    if st["state"] != "live":
        raise HTTPException(410, plan.GONE_TEXT.get(st["state"], plan.GONE_TEXT["unknown"]))
    if st["tool"] != "download_music":
        try:
            entry = plan.claim(entry_id)
        except plan.Gone as e:
            raise HTTPException(410, str(e)) from e
        return chat.confirm(entry.tool, entry.args)

    if not youtube.available():
        raise HTTPException(503, youtube.unavailable_reason())
    if not youtube.folder()["ready"]:
        raise HTTPException(409, NO_FOLDER)
    token = youtube.claim()
    if not token:
        raise HTTPException(409, "ya hay una descarga en marcha")
    try:
        entry = plan.claim(entry_id, selection)
    except plan.Gone as e:
        youtube.release(token)
        raise HTTPException(410, str(e)) from e
    except plan.NothingSelected as e:
        youtube.release(token)
        raise HTTPException(400, str(e)) from e
    except BaseException:
        youtube.release(token)
        raise
    args, after = entry.args, entry.after
    items = list(args["items"])

    def finish(results: list[dict]) -> dict:
        # lo de completar la lista sale del registro, nunca del cliente
        return plan.apply_after(after, results)

    _in_background(
        "danplay-download",
        lambda: youtube.run_many(
            items,
            quality=args.get("quality"),
            file_it=args.get("file_it", True),
            results=1,
            force=args.get("force", False),
            source="assistant",
            claimed=True,
            job=token,
            on_done=finish if after else None,
        ),
    )
    n = len(items)
    text = "Descargando" + (f" {n} temas" if n > 1 else "") + ". Te cuento cuando termine."
    target = after["playlist"] if after else None
    if target:
        text += f" Al terminar {'la' if n == 1 else 'las'} añado a «{target['name']}»."
    return {
        "ok": True,
        "result": {
            "active": True,
            "id": entry.id,
            "job": token,
            "items": items,
            "force": bool(args.get("force")),
            "trimmed": entry.trimmed > 0,
            "playlist": target,
            "skipped": len(after["skipped"]) if after else 0,
        },
        "text": text,
    }


@router.get("/api/chat/confirm/{entry_id}")
def chat_confirm_status(entry_id: Annotated[str, Path(max_length=40)]):
    """Si la propuesta se puede aceptar: {state: live|used|superseded|expired|unknown,
    tool, summary}. La tarjeta solo habilita sus botones con `live`."""
    return plan.status(entry_id)


@router.get("/api/chat/plan/{entry_id}")
def chat_plan(entry_id: Annotated[str, Path(max_length=40)]):
    """Las filas de la tarjeta de descarga segun se van resolviendo (ver
    `chat.plan.plan_view`). 404 si esa propuesta no tiene plan."""
    view = plan.plan_view(entry_id)
    if view is None:
        raise HTTPException(404, "esa propuesta no tiene plan")
    return view


@router.post("/api/chat/plan/{entry_id}/cancel")
def chat_plan_cancel(entry_id: Annotated[str, Path(max_length=40)]):
    """Deja de buscar en YouTube. La propuesta sigue viva: «Descargar» vale
    con lo ya resuelto. 404 si esa propuesta no tiene plan."""
    done = plan.cancel(entry_id)
    if done is None:
        raise HTTPException(404, "esa propuesta no tiene plan")
    return done


@router.get("/api/chat/tools")
def chat_tools():
    # El estado del proveedor para la franja del chat: por que el principal no
    # contesta (o None) y quien contesto la ultima vez (o None). Sin ningun
    # texto del proveedor: solo tipo, codigo HTTP y hora (ver `ai.status`).
    st = ai.status()
    return {
        "model": ai.chat_model(),
        "provider": ai.provider_name(),
        "available": ai.available(),
        "reason": ai.unavailable_reason(),
        "primary_problem": st["primary"]["problem"],
        "answering": st["answering"],
        "tools": [
            {"name": h["function"]["name"], "description": h["function"]["description"]}
            for h in chat.TOOLS
        ],
    }
