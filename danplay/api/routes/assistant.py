"""El asistente: la conversacion (entera o en vivo), sus confirmaciones y las
conversaciones guardadas (docs/CONTRATO-INTERNO.md §3)."""

import logging
import secrets
import threading
import time
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Query

from ... import ai, chat, chats, youtube
from ..common import _in_background
from ..models import ChatAppendIn, ChatConfirmIn, ChatCreateIn, ChatIn, ChatRenameIn

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
    """
    if body.tool not in chat.NEEDS_CONFIRMATION:
        raise HTTPException(400, "eso no necesita confirmacion")
    # Descargar tarda minutos: se arranca y se contesta enseguida, igual que
    # el boton de Descargas. Antes bloqueaba la peticion del chat.
    if body.tool == "download_music":
        # Los argumentos son los de la herramienta tal y como los declara
        # `chat.TOOLS`: `items` (lista), `quality`, `file_it` y `force`.
        # Aqui se leia `query`, que la herramienta no tiene, asi que TODA
        # confirmacion de descarga acababa en «hace falta algo que
        # descargar» aunque la persona acabara de decir que si.
        plan = chat.download_plan(body.args)
        if not plan["items"]:
            raise HTTPException(400, "hace falta algo que descargar")
        if not youtube.available():
            raise HTTPException(503, youtube.unavailable_reason())
        if not youtube.claim():
            raise HTTPException(409, "ya hay una descarga en marcha")
        _in_background(
            "danplay-download",
            lambda: youtube.run_many(
                plan["items"],
                quality=plan["quality"],
                file_it=plan["file_it"],
                results=1,
                force=plan["force"],
                source="assistant",
                claimed=True,
            ),
        )
        n = len(plan["items"])
        return {
            "ok": True,
            "result": {
                "active": True,
                "items": plan["items"],
                "force": plan["force"],
                "trimmed": plan["trimmed"],
            },
            "text": (
                "Descargando" + (f" {n} temas" if n > 1 else "") + ". Te cuento cuando termine."
            ),
        }

    return chat.confirm(body.tool, body.args)


@router.get("/api/chat/tools")
def chat_tools():
    return {
        "model": ai.chat_model(),
        "provider": ai.provider_name(),
        "available": ai.available(),
        "reason": ai.unavailable_reason(),
        "tools": [
            {"name": h["function"]["name"], "description": h["function"]["description"]}
            for h in chat.TOOLS
        ],
    }
