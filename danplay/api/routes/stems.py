"""Separar canciones en pistas (bateria, voces, bajo...) y lo que se hace
con ellas: oirlas en el estudio, guardarlas mezcladas, quitarlas."""

import logging
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException
from fastapi.responses import JSONResponse

from ... import library, stems
from .. import jobs
from ..common import _started
from ..models import StemMixIn

log = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/separate")
def separate_status():
    """Si se puede separar aqui, lo que pesa el separador (y si ya esta
    bajado), la cola y lo ultimo que acabo."""
    return stems.status()


@router.post("/api/song/{cid}/separate", status_code=202)
def separate(cid: int):
    """A la cola de separacion: que la cancion tenga las mejores pistas (si ya
    tiene las rapidas, solo se mejoran). Tarda: se sigue en GET
    /api/separate. La primera vez baja el separador."""
    try:
        state = stems.request(cid)
    except stems.SeparateError as e:
        raise HTTPException(404 if "ya no está" in str(e) else 422, str(e)) from e
    return {**state, "job": jobs.snapshot(stems.JOB)}


@router.delete("/api/separate")
def cancel_separation():
    """Para lo que se este separando y vacia la cola."""
    return stems.cancel()


@router.delete("/api/separate/queue/{cid}")
def unqueue(cid: int):
    return stems.unqueue(cid)


@router.delete("/api/separate/weights")
def remove_weights():
    """Borra lo bajado del separador (se vuelve a bajar al separar)."""
    if jobs.active(stems.JOB):
        raise HTTPException(409, "hay una separación en marcha")
    return {"removed": stems.remove_weights(), **stems.status()}


@router.get("/api/song/{cid}/stems")
def song_stems(cid: int):
    """Las pistas separadas de la cancion: cada una con su ruta y su onda."""
    song = library.by_id(cid)
    if not song:
        raise HTTPException(404, "no existe")
    data = stems.info(song)
    if data is None:
        if song.get("stems"):
            # la carpeta ya no esta (se borro a mano): se olvida
            library.update(cid, stems="")
        raise HTTPException(404, "esta canción no tiene pistas separadas")
    return data


@router.delete("/api/song/{cid}/stems")
def delete_stems(cid: int):
    """Manda a la papelera las pistas separadas de la cancion."""
    song = library.by_id(cid)
    if not song:
        raise HTTPException(404, "no existe")
    r = stems.forget(song)
    if not r["ok"]:
        raise HTTPException(500, r["error"])
    return r


@router.post("/api/song/{cid}/stems/mix", status_code=202)
def export_mix(cid: int, body: Annotated[StemMixIn, Body()]):
    """Guarda la mezcla de las pistas en un archivo (trabajo «mezcla»)."""
    song = library.by_id(cid)
    if not song or stems.info(song) is None:
        raise HTTPException(404, "esta canción no tiene pistas separadas")
    tracks = [t.model_dump() for t in body.tracks]

    def work(progress):
        r = stems.export_mix(
            cid,
            tracks,
            body.path,
            body.format,
            body.speed,
            body.pitch,
            progress=lambda done, total: progress(round(done, 1), round(total, 1)),
        )
        progress(message=f"Guardada «{r['name']}»")
        return r

    snap, already = jobs.start(stems.MIX_JOB, work, failure="no se pudo guardar la mezcla")
    return JSONResponse(_started(snap, already), status_code=202)
