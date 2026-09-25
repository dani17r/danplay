"""Una cancion: su ficha, su audio y su caratula, estrellas y favorito, la IA
sobre ella y el modo estudio."""

import json
import logging
import os
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Query
from fastapi.responses import FileResponse, Response

from ... import (
    convert,
    enrich,
    external,
    library,
    playlists,
    tags,
    theory,
    thumbnails,
    waveform,
)
from ..common import audio_type
from ..models import Blur, EnrichIn, Favorite, PathIn, SongEdit, Stars, StudyIn, TransposeIn

log = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/song/{cid}")
def song(cid: int):
    # `resolve`: esto solo LEE, y la ficha de una cancion abierta desde fuera
    # tiene que poder verse igual que la de una de la biblioteca. Editarla,
    # borrarla o ponerle estrellas son otros endpoints, y esos siguen usando
    # `library.by_id`, que no las encuentra: de ahi que no se les pueda tocar.
    c = external.resolve(cid)
    if not c:
        raise HTTPException(404, "no existe")
    c["playlists"] = playlists.playlists_of(cid)
    c["existe"] = os.path.exists(c["path"])
    return c


@router.patch("/api/song/{cid}")
def edit(cid: int, body: Annotated[SongEdit, Body()]):
    """Solo los campos que el usuario puede corregir a mano.

    Antes se pasaba el cuerpo entero, asi que se podian poner estrellas o el
    favorito directamente en la base, sin escribirlos en el archivo: el
    indice decia una cosa y el mp3 otra.
    """
    c = library.edit(cid, **body.model_dump(exclude_none=True))
    if not c:
        raise HTTPException(404, "no existe")
    return c


@router.delete("/api/song/{cid}")
def delete_song(cid: int):
    """Manda el archivo a la papelera del sistema y lo saca del indice.

    A la papelera y no `unlink`: borrar musica del usuario sin vuelta atras
    por un clic en un menu es demasiado definitivo. Si el escritorio no
    tiene papelera se avisa y no se borra nada.
    """
    r = library.trash(cid)
    if not r["ok"]:
        raise HTTPException(404 if "no existe" in r["error"] else 501, r["error"])
    return r


@router.get("/api/song/{cid}/audio")
def audio(cid: int):
    c = library.by_id(cid)
    if not c or not os.path.exists(c["path"]):
        raise HTTPException(404, "archivo no encontrado")
    return FileResponse(c["path"], media_type=audio_type(c["path"]), filename=c["file"])


# Tamaños de miniatura que se generan y se guardan (ver `thumbnails`).
THUMBNAIL_SIZES = thumbnails.SIZES


@router.get("/api/song/{cid}/waveform")
def song_waveform(
    cid: int, buckets: Annotated[int, Query(ge=1, le=4000)] = waveform.DEFAULT_BUCKETS
):
    """La forma de onda para el modo estudio: `buckets` columnas con pico y
    RMS entre 0 y 1. `resolve`: una cancion abierta desde fuera tambien se
    estudia. 404 si el archivo no esta; 501 si no hay con que decodificarla
    (ni el nucleo en Rust ni ffmpeg)."""
    c = external.resolve(cid)
    if not c or not os.path.exists(c["path"]):
        raise HTTPException(404, "archivo no encontrado")
    made = waveform.compute(c["path"], buckets)
    if made is None:
        if not waveform.available():
            raise HTTPException(501, "hace falta ffmpeg para dibujar la forma de onda")
        raise HTTPException(422, "no se pudo decodificar el archivo")
    return Response(
        content=json.dumps({**made, "buckets": len(made["peaks"])}),
        media_type="application/json",
        headers={"cache-control": "private, max-age=3600"},
    )


@router.get("/api/song/{cid}/cover")
def cover(cid: int, size: Annotated[int | None, Query()] = None):
    # `resolve`: la portada sale del propio archivo, asi que una cancion de
    # fuera de la biblioteca tambien tiene la suya.
    c = external.resolve(cid)
    if not c:
        raise HTTPException(404, "no existe")
    if not os.path.exists(c["path"]):
        raise HTTPException(404, "sin portada")
    r = thumbnails.get(c["path"], size) if size in THUMBNAIL_SIZES else tags.cached_cover(c["path"])
    if not r:
        raise HTTPException(404, "sin portada")
    return Response(
        content=r[0], media_type=r[1], headers={"cache-control": "private, max-age=300"}
    )


# Estrellas, favorito y difuminado se guardan DENTRO del archivo, asi que
# solo valen para la biblioteca: a una cancion abierta desde fuera no se le
# escribe nada. Antes esto contestaba 200 con un `null` y quien llamaba se
# quedaba creyendo que habia funcionado.


@router.post("/api/song/{cid}/stars")
def stars(cid: int, body: Annotated[Stars, Body()]):
    # Se mira que EXISTA, no si la escritura funciono: en un archivo de solo
    # lectura la etiqueta no se puede poner y aun asi la cancion esta.
    if not library.by_id(cid):
        raise HTTPException(404, "no esta en la biblioteca")
    playlists.rate(cid, body.stars)
    return library.by_id(cid)


@router.post("/api/song/{cid}/favorite")
def favorite(cid: int, body: Annotated[Favorite, Body()]):
    if not library.by_id(cid):
        raise HTTPException(404, "no esta en la biblioteca")
    playlists.favorite(cid, body.favorite)
    return library.by_id(cid)


@router.post("/api/song/{cid}/blur")
def blur_cover(cid: int, body: Annotated[Blur | None, Body()] = None):
    """Difumina la portada al pintarla. La imagen no se toca.

    Para portadas que uno no quiere tener delante. Se guarda dentro del mp3,
    asi que la decision no se pierde ni al rehacer el indice.
    """
    body = body or Blur()
    c = library.set_blur(cid, body.blur)
    if not c:
        raise HTTPException(404, "no existe")
    return c


# ---------------------------------------------------------------- IA


@router.post("/api/song/{cid}/enrich")
def enrich_song(cid: int, body: Annotated[EnrichIn | None, Body()] = None):
    body = body or EnrichIn()
    r = enrich.enrich(cid, body.lyrics, body.cover, body.details)
    return {"result": r, "song": library.by_id(cid)}


@router.post("/api/song/{cid}/cover")
def set_cover(cid: int, body: Annotated[PathIn, Body()]):
    """Incrusta una imagen del disco como caratula de la cancion.

    Se comprueba que sea una imagen de verdad ANTES de pasarsela a ffmpeg y
    de meterla en el mp3: por aqui se podia leer cualquier archivo del disco
    y recuperarlo despues pidiendo la caratula.
    """
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    src = body.path.strip()
    if not src or not os.path.isfile(src):
        raise HTTPException(400, "esa imagen no existe")
    if os.path.splitext(src)[1].lower() not in (".jpg", ".jpeg", ".png", ".webp"):
        raise HTTPException(400, "solo valen imagenes jpg, png o webp")
    try:
        with open(src, "rb") as f:
            head = f.read(16)
    except OSError as e:
        raise HTTPException(400, "no se pudo leer esa imagen") from e
    if not enrich.image_type(head):
        raise HTTPException(400, "ese archivo no es una imagen")
    r = convert.shrink_image(src)
    if not r:
        raise HTTPException(400, "no se pudo leer esa imagen")
    data, mime = r
    if not tags.write_cover(c["path"], data, mime):
        raise HTTPException(500, "no se pudo incrustar la imagen")
    tags.forget_cover(c["path"])
    library.update(cid, cover="embedded")
    return {"ok": True, "kb": len(data) // 1024, "song": library.by_id(cid)}


@router.post("/api/song/{cid}/autofill")
def autofill_song(cid: int):
    """Completa la ficha con IA. Dice que relleno y que no pudo, con el motivo."""
    r = enrich.autofill(cid)
    return {**r, "song": library.by_id(cid)}


@router.get("/api/song/{cid}/details")
def details(cid: int):
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    d, cached = enrich.details_for(c)
    return {"details": d, "cached": cached}


@router.put("/api/song/{cid}/study")
def song_study(cid: int, body: Annotated[StudyIn, Body()]):
    c = library.set_study(cid, body.model_dump(exclude_none=True))
    if not c:
        raise HTTPException(404, "no existe")
    return c


@router.get("/api/song/{cid}/path")
def audio_path(cid: int):
    """Devuelve la ruta en disco. La usa Rust para servir el audio sin pasar por Python.

    `resolve` y no `by_id`: aqui solo se REPRODUCE, y una cancion abierta
    desde fuera de la biblioteca tiene que sonar igual que las demas.
    """
    c = external.resolve(cid)
    if not c or not os.path.exists(c["path"]):
        raise HTTPException(404, "no existe")
    return {"path": c["path"], "kind": audio_type(c["path"]), "bytes": os.path.getsize(c["path"])}


@router.post("/api/transpose")
def transpose(body: Annotated[TransposeIn, Body()]):
    if body.from_key and body.to_key:
        out = theory.transpose_to(body.text, body.from_key, body.to_key)
    else:
        out = theory.transpose(body.text, body.semitones)
    return {
        "text": out,
        "latin": theory.to_latin(out),
        "capo": theory.suggested_capo(body.to_key),
        "keys": theory.available_keys(),
    }
