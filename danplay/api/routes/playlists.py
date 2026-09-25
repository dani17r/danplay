"""Los repertorios: crearlos, llenarlos, ordenarlos y exportarlos (m3u y la
hoja para el atril)."""

import logging
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException

from ... import playlists
from ..models import OrderIn, PlaylistEdit, PlaylistIn, SheetIn, SongsIn

log = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/playlists")
def list_playlists():
    return {"playlists": playlists.list_all(), "favorites": playlists.favorites_count()}


@router.post("/api/playlists")
def create_playlist(body: Annotated[PlaylistIn, Body()]):
    # `created` importa: con un nombre repetido se devuelve la que ya habia,
    # y la interfaz tiene que poder decirlo en vez de fingir que creo una.
    made = playlists.create(body.name, body.note, body.color)
    return {**made, "playlists": playlists.list_all()}


@router.patch("/api/playlists/{lid}")
def edit_playlist(lid: int, body: Annotated[PlaylistEdit, Body()]):
    """Renombra una lista o cambia su nota. `409` si el nombre ya es de otra."""
    if body.name is None and body.note is None:
        raise HTTPException(400, "no hay nada que cambiar")
    if body.name is not None:
        other = playlists.by_name(body.name)
        if other and other["id"] != lid:
            raise HTTPException(409, f"ya hay una lista que se llama «{other['name']}»")
    out = playlists.edit(lid, name=body.name, note=body.note)
    if not out:
        raise HTTPException(404, "esa lista no existe")
    return {"playlist": out, "playlists": playlists.list_all()}


@router.delete("/api/playlists/{lid}")
def delete_playlist(lid: int):
    playlists.remove(lid)
    return {"playlists": playlists.list_all()}


@router.get("/api/playlists/{lid}/songs")
def playlist_songs_of(lid: int):
    return {"songs": playlists.songs(lid, light=True)}


@router.post("/api/playlists/{lid}/songs")
def add_to_playlist(lid: int, body: Annotated[SongsIn, Body()]):
    ids = body.ids or ([body.id] if body.id else [])
    added = playlists.add(lid, [int(i) for i in ids if i])
    return {"added": added, "songs": playlists.songs(lid, light=True)}


@router.delete("/api/playlists/{lid}/songs/{cid}")
def remove_from_playlist(lid: int, cid: int):
    playlists.remove_song(lid, cid)
    return {"songs": playlists.songs(lid, light=True)}


@router.post("/api/playlists/{lid}/order")
def reorder(lid: int, body: Annotated[OrderIn, Body()]):
    playlists.reorder(lid, body.ids)
    return {"songs": playlists.songs(lid, light=True)}


@router.post("/api/playlists/{lid}/export")
def export(lid: int):
    try:
        return {"file": playlists.export_m3u(lid)}
    except ValueError as e:
        raise HTTPException(400, str(e)) from e


@router.post("/api/playlists/{pid}/sheet")
def playlist_sheet(pid: int, body: Annotated[SheetIn | None, Body()] = None):
    """La hoja para el atril del repertorio, en HTML dentro de Listas/."""
    body = body or SheetIn()
    try:
        path = playlists.export_sheet(pid, with_lyrics=body.with_lyrics)
    except ValueError as e:
        raise HTTPException(400, str(e)) from e
    return {"file": path}
