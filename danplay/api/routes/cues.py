"""Las guias habladas del modo estudio: las voces (y bajarlas), decir una
frase, traer una grabacion y escucharlas (ver danplay/cues.py)."""

import base64
import os
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Query
from fastapi.responses import JSONResponse

from ... import cues, external
from ..models import CueEditIn, CueFileIn, CueImportIn, CueIn

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
def cue_import(cid: int, body: Annotated[CueImportIn, Body()]):
    """Una guia grabada (o un audio del equipo) a la carpeta de guias,
    recortada y a su volumen. Con `limit`, hasta lo que dura la cancion."""
    song = _song(cid)
    longest = body.limit
    if longest:
        longest = min(longest, float(song.get("duration") or longest) + 0.5)
    try:
        return cues.import_recording(body.path, longest)
    except cues.CueError as e:
        raise HTTPException(422, str(e)) from e


@router.get("/api/cues/wave")
def cue_wave(file: Annotated[str, Query(min_length=1, max_length=1024)], buckets: int = 96):
    """La onda de una guia: {seconds, peaks}."""
    try:
        return cues.wave_of(file, buckets)
    except cues.CueError as e:
        raise HTTPException(404, str(e)) from e


@router.post("/api/cues/edit")
def cue_edit(body: Annotated[CueEditIn, Body()]):
    """Una guia nueva recortada y limpia a partir de otra: {file, seconds}."""
    try:
        return cues.edit_clip(**body.model_dump())
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
