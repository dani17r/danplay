"""La lista del reproductor: lo que se abre desde fuera de DanPlay (ver
`danplay/external.py`). NO se importa a la biblioteca: solo se recuerda que
sono."""

import logging
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException

from ... import external
from ..models import PathIn, PlaylistName

log = logging.getLogger(__name__)

router = APIRouter()


@router.post("/api/external/play")
def external_play(body: Annotated[PathIn, Body()]):
    """Esa ruta acaba de sonar. Devuelve la cancion, venga de donde venga.

    Es POST y no GET porque la clave es una ruta: con acentos, espacios, `&` y
    `#` dentro, meterla en la parte de consulta de una URL es pedir un fallo
    de codificacion.

    Si el archivo ya esta indexado, devuelve la cancion de la biblioteca tal
    cual, con su caratula y sus estrellas. Si no, se le leen las etiquetas una
    vez y se le da un id negativo. En los dos casos sube al principio de la
    lista, y volver a ponerla no la duplica.
    """
    try:
        song = external.played(body.path)
    except external.NotAudio as e:
        raise HTTPException(400, str(e)) from e
    if not song:
        raise HTTPException(404, "ese archivo no esta")
    return {"song": song}


@router.get("/api/external")
def external_list():
    """La lista, de lo ultimo que sono a lo mas antiguo (filas ligeras)."""
    return {"songs": external.listing(light=True)}


@router.delete("/api/external")
def external_clear():
    """Descarta la lista. Ni toca los archivos ni deshace lo ya guardado."""
    return {"removed": external.clear()}


@router.delete("/api/external/{cid}")
def external_forget(cid: int):
    """Quita una sola cancion de la lista."""
    return {"removed": external.forget(cid)}


@router.post("/api/external/save")
def external_save(body: Annotated[PlaylistName, Body()]):
    """Guarda la lista de ahora como una lista de reproduccion de DanPlay."""
    try:
        return external.save_as_playlist(body.name)
    except ValueError as e:
        raise HTTPException(400, str(e)) from e
