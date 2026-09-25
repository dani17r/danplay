"""Descargas de YouTube: el estado de la que va, yt-dlp y su motor de
JavaScript, y el historial.

El estado vive en `youtube.STATE` porque lo comparten la pagina de Descargas
y el asistente: una descarga a la vez y un solo sitio donde mirar como va.
"""

import logging
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Query

from ... import config, library, youtube, ytdlp
from .. import jobs
from ..common import _in_background, _started
from ..models import YoutubeIn

log = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/youtube")
def youtube_status():
    """El estado de la descarga en curso, y que yt-dlp hay: la version en uso
    (la bajada al actualizar o la de la app), la que viaja con la app, y el
    motor de JavaScript con el que resuelve los retos de YouTube (null, con
    `js_runtime_hint` diciendo que instalar, si no hay ninguno)."""
    rt = ytdlp.js_runtime()
    return {
        "available": youtube.available(),
        "reason": youtube.unavailable_reason(),
        "quality": config.MP3_QUALITY,
        **youtube.STATE,
        "version": ytdlp.version(),
        "bundled_version": ytdlp.bundled_version(),
        "js_runtime": rt["name"] if rt else None,
        "js_runtime_hint": ytdlp.js_runtime_hint(),
    }


@router.post("/api/youtube/update", status_code=202)
def youtube_update():
    """Pone yt-dlp al dia desde PyPI (trabajo «yt-dlp»); `result` es
    {previous, version, updated}."""

    def work(progress):
        try:
            return ytdlp.update(progress)
        except ytdlp.UpdateError as e:
            raise jobs.JobError(str(e)) from e
        finally:
            youtube.recheck()

    return _started(*jobs.start("yt-dlp", work, failure="no se pudo actualizar yt-dlp"))


@router.post("/api/youtube/info")
def youtube_info(body: Annotated[YoutubeIn | None, Body()] = None):
    """Que se bajaria, sin bajar nada todavia."""
    body = body or YoutubeIn()
    query = body.query.strip()
    if not query:
        raise HTTPException(400, "hace falta una URL o algo que buscar")
    return youtube.info(query, body.results)


@router.post("/api/youtube/download")
def youtube_download(body: Annotated[YoutubeIn | None, Body()] = None):
    """Arranca la descarga y vuelve enseguida. El avance se consulta en /api/youtube.

    Una descarga a la vez. Quedarse el turno es cosa de `youtube.claim()`,
    que comprueba y reserva bajo un mismo cerrojo; luego `run_job` corre con
    `claimed=True`. Antes se marcaba `active` aqui a mano y `run_job`, al
    verlo puesto, se negaba a descargar: el boton decia «Bajando…» para
    siempre y no bajaba nada.
    """
    body = body or YoutubeIn()
    query = body.query.strip()
    if not query:
        raise HTTPException(400, "hace falta una URL o algo que buscar")
    if not youtube.available():
        raise HTTPException(503, youtube.unavailable_reason())
    if not youtube.claim():
        raise HTTPException(409, "ya hay una descarga en marcha")
    _in_background(
        "danplay-download",
        lambda: youtube.run_job(
            query,
            quality=body.quality or config.MP3_QUALITY,
            file_it=body.file_it,
            results=body.results,
            force=body.force,
            claimed=True,
        ),
    )
    return {"ok": True, "active": True}


@router.get("/api/downloads/history")
def downloads_history(
    limit: Annotated[int, Query(ge=1, le=500)] = 60, offset: Annotated[int, Query(ge=0)] = 0
):
    """Todo lo descargado, del boton o del asistente, lo mas reciente arriba."""
    return {"items": library.download_history(limit, offset), "total": library.download_count()}


@router.delete("/api/downloads/history")
def clear_downloads_history():
    return {"removed": library.clear_download_history()}


@router.post("/api/youtube/cancel")
def youtube_cancel():
    youtube.cancel()
    return {"ok": True}
