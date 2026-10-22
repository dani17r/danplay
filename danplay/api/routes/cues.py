"""Las guias habladas del modo estudio: las voces (y bajarlas), decir una
frase, traer una grabacion, escucharlas y la guia automatica (las partes de
la cancion, dichas antes de cada una). Ver danplay/cues.py y
danplay/structure.py."""

import base64
import os
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Query
from fastapi.responses import JSONResponse

from ... import cues, external
from .. import jobs
from ..common import _started
from ..models import CueAutoIn, CueEditIn, CueFileIn, CueImportIn, CueIn

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


def _voice_ready(voice: str) -> None:
    """Si la voz aun no esta bajada, empieza a bajarla y contesta 409."""
    if not cues.installed(voice):
        try:
            cues.install(voice)
        except cues.CueError as e:
            raise HTTPException(422, str(e)) from e
        raise HTTPException(409, "esa voz se está bajando: en cuanto acabe se puede usar")


@router.post("/api/song/{cid}/cues")
def cue_make(cid: int, body: Annotated[CueIn, Body()]):
    """La guia escrita, dicha con esa voz: su archivo (y lo que dura). 409
    si la voz aun no esta bajada."""
    _song(cid)
    _voice_ready(body.voice)
    try:
        return cues.make(body.text, body.voice, preview=body.preview)
    except cues.CueError as e:
        raise HTTPException(422, str(e)) from e


@router.post("/api/song/{cid}/cues/auto", status_code=202)
def cue_auto(cid: int, body: Annotated[CueAutoIn, Body()]):
    """La guia automatica (trabajo «guia-automatica»): las partes de la
    cancion (o las de `sections`, tal cual) y una guia hablada con esa voz
    justo antes de cada una. Su resultado: {sections: [{start, end, label}],
    cues: [{t, text, voice, file, seconds}]}. 409 si la voz aun no esta
    bajada."""
    from ... import structure

    song = _song(cid)
    _voice_ready(body.voice)
    sections = [s.model_dump() for s in body.sections] if body.sections else None

    def work(progress):
        r = structure.auto_guide(
            song,
            body.voice,
            bars=body.bars,
            beat=body.beat,
            sections=sections,
            progress=progress,
        )
        n = len(r["sections"])
        progress(message=f"Guía lista: {n} parte{'s' if n != 1 else ''}")
        return r

    snap, already = jobs.start(
        structure.AUTO_JOB, work, failure="no se pudo hacer la guía automática"
    )
    return JSONResponse(_started(snap, already), status_code=202)


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
