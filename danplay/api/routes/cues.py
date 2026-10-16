"""Las guias habladas del modo estudio: las voces (y bajarlas), decir una
frase, traer una grabacion y escucharlas (ver danplay/cues.py)."""

import base64
import os
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException
from fastapi.responses import JSONResponse

from ... import cues, external
from ..models import CueFileIn, CueIn, PathIn

router = APIRouter()


def _song(cid: int) -> dict:
    c = external.resolve(cid)
    if not c or not os.path.exists(c["path"]):
        raise HTTPException(404, "archivo no encontrado")
    return c


@router.get("/api/cues/voices")
def cue_voices():
    """Las voces con que se dicen las guias: las bajadas y las que se pueden
    bajar (lo que pesan y, si se estan bajando, por donde van)."""
    return {"voices": cues.voices(), "available": cues.available()}


@router.post("/api/cues/voices/{voice_id}/install")
def cue_voice_install(voice_id: str):
    """Baja esa voz en segundo plano (se sigue en GET /api/cues/voices)."""
    try:
        state = cues.install(voice_id)
    except cues.CueError as e:
        raise HTTPException(404 if "no conozco" in str(e) else 502, str(e)) from e
    return JSONResponse(state, status_code=200 if state.get("installed") else 202)


@router.post("/api/song/{cid}/cues")
def cue_make(cid: int, body: Annotated[CueIn, Body()]):
    """La guia escrita, dicha con esa voz: su archivo (y lo que dura). 409
    si la voz aun no esta bajada."""
    _song(cid)
    if not cues.installed(body.voice):
        try:
            cues.install(body.voice)
        except cues.CueError as e:
            raise HTTPException(422, str(e)) from e
        raise HTTPException(409, "esa voz se está bajando: en cuanto acabe se puede usar")
    try:
        return cues.make(body.text, body.voice, preview=body.preview)
    except cues.CueError as e:
        raise HTTPException(422, str(e)) from e


@router.post("/api/song/{cid}/cues/import")
def cue_import(cid: int, body: Annotated[PathIn, Body()]):
    """Una guia grabada (o un audio del equipo) a la carpeta de guias,
    recortada y a su volumen."""
    _song(cid)
    try:
        return cues.import_recording(body.path)
    except cues.CueError as e:
        raise HTTPException(422, str(e)) from e


@router.post("/api/cues/audio")
def cue_audio(body: Annotated[CueFileIn, Body()]):
    """Una guia, para escucharla en la interfaz: {mime, data} en base64."""
    try:
        data = cues.audio(body.file)
    except cues.CueError as e:
        raise HTTPException(404, str(e)) from e
    return {"mime": "audio/wav", "data": base64.b64encode(data).decode("ascii")}
