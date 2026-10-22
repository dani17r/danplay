"""Una cancion del indice: leerla, cambiarla (en el indice y en su archivo),
mandarla a la papelera o sacarla del indice."""

import contextlib
import logging
import os
import shutil
import subprocess
import sys
from typing import cast

import danplay_core as _rust

from .. import config, tags
from . import db
from .db import _SCAN_LOCK, Song, _to_missing, _touch

log = logging.getLogger(__name__)


def by_id(cid: int) -> Song | None:
    """La cancion de la biblioteca con ese id (la fila entera), o None."""
    with db.connect() as conn:
        f = _rust.songs_by_id(conn, int(cid))
    return cast(Song, f) if f else None


def update(cid: int, **fields) -> int:
    """Cambia columnas del indice (solo el indice). Devuelve filas tocadas.
    Solo las que se pueden cambiar a mano (`biblioteca::indice::canciones::
    EDITABLE`); lo demas sale del archivo."""
    with db.connect() as conn:
        n = _rust.songs_update(conn, int(cid), fields)
    if n is None:
        return 0
    _touch()
    return n


_METADATA_FIELDS = ("artist", "title", "album", "year", "genre")


def edit(cid: int, **fields) -> Song | None:
    """Cambia los datos de una cancion en el indice y en las etiquetas.

    Lo usan la API y el asistente: un solo camino para que no se separen.
    La ficha que devuelve lleva `tags_written`: si el archivo se actualizo
    de verdad. Antes se ignoraba el resultado y en un flac o un m4a la
    edicion cambiaba el indice y el archivo se quedaba igual, sin aviso.
    """
    c = by_id(cid)
    if not c:
        return None
    update(cid, **fields)
    written = False
    if config.WRITE_TAGS and os.path.exists(c["path"]):
        written = True
        if any(k in fields for k in _METADATA_FIELDS):
            written = (
                tags.write(
                    c["path"],
                    artist=fields.get("artist", c["artist"]),
                    title=fields.get("title", c["title"]),
                    album=fields.get("album", c["album"]),
                    year=str(fields.get("year", c["year"])),
                    genre=fields.get("genre", c["genre"]),
                )
                and written
            )
        if "key" in fields or "bpm" in fields:
            written = (
                tags.write_analysis(
                    c["path"], fields.get("key", c["key"]), fields.get("bpm", c["bpm"])
                )
                and written
            )
        # la letra vive en el USLT del propio archivo, no solo en el indice
        if "lyrics" in fields:
            written = tags.write_lyrics(c["path"], fields["lyrics"] or "") and written
    out = by_id(cid)
    if out is not None:
        out["tags_written"] = bool(written)
    return out


def set_blur(cid: int, value=True) -> Song | None:
    """Difumina (o deja de difuminar) la portada de una cancion.

    La imagen no se toca: se guarda una marca dentro del propio mp3 y la
    interfaz la pinta borrosa. Quitarla devuelve la portada intacta.
    """
    c = by_id(cid)
    if not c:
        return None
    value = bool(value)
    written = False
    if config.WRITE_TAGS and os.path.exists(c["path"]):
        written = tags.set_blurred_cover(c["path"], value)
    update(cid, blur=1 if value else 0)
    out = by_id(cid)
    if out is not None:
        out["tags_written"] = bool(written)
    return out


def trash_path(path) -> dict:
    """Manda un archivo a la papelera del sistema. No toca el indice.

    `send2trash` sabe de la papelera de Linux, Windows y macOS. En Linux, si
    falla (archivos en otro disco montado, por ejemplo), se prueba con
    `gio trash`, que conoce las papeleras de cada volumen. Si nada funciona
    se avisa: borrar sin vuelta atras no es una opcion.
    """
    path = str(path)
    if not os.path.lexists(path):
        return {"ok": True, "missing": True}
    reason = ""
    try:
        from send2trash import send2trash

        send2trash(path)
        return {"ok": True}
    except Exception as e:
        log.warning("send2trash no pudo con %s", path, exc_info=True)
        reason = str(e)[:120]
    if sys.platform.startswith("linux") and shutil.which("gio"):
        r = subprocess.run(["gio", "trash", path], capture_output=True, text=True, check=False)
        if r.returncode == 0:
            return {"ok": True}
        reason = (r.stderr or "").strip()[:120] or reason
    return {"ok": False, "error": "no se pudo mandar a la papelera: " + reason}


def trash(cid: int, from_lists: bool = False) -> dict:
    """Manda el archivo a la papelera del sistema y lo saca del indice.

    A la papelera y no `unlink`: borrar musica del usuario sin vuelta atras,
    y menos desde un menu o desde el chat, es demasiado definitivo.

    Con `from_lists`, sale tambien de todas sus listas (se queria borrar de
    verdad). Sin el, las listas la siguen enseñando como «No está», para
    buscarla o volver a bajarla.
    """
    c = by_id(cid)
    if not c:
        return {"ok": False, "error": "no existe esa cancion"}
    path = c["path"]
    r = trash_path(path)
    if not r["ok"]:
        return r
    # Sus pistas separadas se van con ella, tambien a la papelera: son de
    # esa cancion y no tiene sentido guardarlas sin ella.
    stems_gone = False
    if c.get("stems"):
        from .. import stems as _stems

        stems_gone = _stems.forget(c).get("ok", False)
    forget(cid)
    lists = 0
    if from_lists:
        from .. import playlists

        lists = playlists.take_out_everywhere(cid)
    return {
        "ok": True,
        "trashed": True,
        "lists": lists,
        "path": path,
        "name": os.path.basename(path),
        "artist": c["artist"],
        "title": c["title"],
        "stems_trashed": stems_gone,
    }


def forget(cid: int) -> None:
    """Quita la cancion del indice. No toca el archivo.

    Se aparta, no se borra (`_to_missing`): si el archivo vuelve —se restaura
    de la papelera— recupera su id y sus listas. La forma de onda cacheada se
    borra aqui.
    """
    from .. import thumbnails as _thumbnails
    from .. import waveform as _waveform

    with db.connect() as conn:
        row = conn.execute("SELECT path FROM songs WHERE id=?", (cid,)).fetchone()
        if row:
            _to_missing(conn, [row["path"]])
    if row:
        _waveform.forget(row["path"])
        _thumbnails.forget(row["path"])
    _touch()


def forget_path(path: str) -> int:
    """Saca del indice el archivo que estaba en esa ruta. No toca el disco.

    Para cuando el archivo ya no esta ahi (se borro o se renombro) y solo hay
    que ponerse al dia. Se aparta, como en `forget`. Devuelve cuantas filas
    se quitaron.
    """
    from .. import thumbnails as _thumbnails
    from .. import waveform as _waveform

    with db.connect() as conn:
        n = _to_missing(conn, [os.path.abspath(path)])
    _waveform.forget(os.path.abspath(path))
    _thumbnails.forget(os.path.abspath(path))
    _touch()
    return n


def move_to(cid: int, folder) -> Song:
    """Lleva el archivo de una cancion a otra carpeta de la biblioteca.

    Sigue siendo la misma cancion: mismo id, y con el sus estrellas, sus
    listas, su estudio y sus pistas separadas. El nombre del archivo no
    cambia (salvo el sufijo « - r» si alli ya hay uno igual). Si la carpeta
    de la que sale se queda vacia, se quita. `ValueError` si la cancion o la
    carpeta no valen; `OSError` si el disco no deja moverlo.
    """
    return relocate(cid, folder)


def relocate(cid: int, folder, name: str | None = None) -> Song:
    """`move_to`, y con otro nombre de archivo si se da `name` (con su
    extension): lo que hace «Transformar». La misma cancion (mismo id, con
    todo lo suyo) y lo calculado de su sonido (la rejilla del clic, la
    onda), que sigue valiendo en el sitio nuevo. Si alli ya hay otro con
    ese nombre, « - r». Cambiar solo mayusculas tambien vale."""
    from .. import beatgrid as _beatgrid
    from .. import names
    from .. import thumbnails as _thumbnails
    from .. import waveform as _waveform
    from .folders import _inside, _roots

    with _SCAN_LOCK:
        c = by_id(cid)
        if not c:
            raise ValueError("no existe esa cancion")
        src = c["path"]
        if not os.path.isfile(src):
            raise ValueError("su archivo ya no esta donde decia el indice")
        dest_dir = os.path.abspath(folder)
        root = c["root"]
        # dentro de su misma carpeta de musica: sus pistas separadas se
        # apuntan desde ella, y en otra no se encontrarian
        if not _inside(dest_dir, root) or root not in _roots():
            raise ValueError("esa carpeta esta fuera de su carpeta de musica")
        name = names.sanitize(name) if name else os.path.basename(src)
        if not name or name.startswith("."):
            raise ValueError("ese nombre de archivo no vale")
        wanted = os.path.join(dest_dir, name)
        if wanted == src:
            return c
        os.makedirs(dest_dir, exist_ok=True)
        # solo cambian mayusculas: en un disco que no las distingue «ya
        # existe» (es ella misma) y saldria « - r»
        same_file = os.path.normcase(wanted) == os.path.normcase(src) or (
            os.path.exists(wanted) and os.path.samefile(wanted, src)
        )
        dest = wanted if same_file else os.path.join(dest_dir, names.free_name(dest_dir, name))
        before = os.stat(src)
        shutil.move(src, dest)
        try:
            with db.connect() as conn:
                conn.execute(
                    "UPDATE songs SET path=?, folder=?, file=?, match_key=? WHERE id=?",
                    (
                        dest,
                        os.path.relpath(dest_dir, root),
                        os.path.basename(dest),
                        names.match_key(os.path.basename(dest)),
                        cid,
                    ),
                )
        except Exception:
            # sin el indice al dia, el archivo vuelve a donde estaba
            shutil.move(dest, src)
            raise
        _touch()
    with contextlib.suppress(OSError):
        after = os.stat(dest)
        for carry in (_beatgrid.moved, _waveform.moved):
            try:
                carry(src, dest, before, after)
            except Exception:
                log.warning("no pude llevar lo calculado de %s", src, exc_info=True)
    _thumbnails.forget(src)
    # la carpeta de la que sale, si se queda vacia (la de un artista que
    # creo la descarga, la de un disco): no se deja. Artistas/ misma, nunca
    parent = os.path.dirname(src)
    if _inside(parent, config.ARTISTS_DIR) and not _inside(config.ARTISTS_DIR, parent):
        with contextlib.suppress(OSError):
            os.rmdir(parent)
    return by_id(cid) or c


def paths_of(ids) -> dict[int, str | None]:
    """Donde esta ahora cada una de esas canciones de la biblioteca, o None si
    su archivo no esta (ya no es del indice, o se fue y aun no se escaneo).

    Lo usa la cola de Rust para ponerse al dia de una vez cuando cambia la
    biblioteca: una cancion movida sigue sonando desde su sitio nuevo, y la
    que ya no esta sale de la cola.
    """
    ids = [int(i) for i in ids if int(i) > 0]
    if not ids:
        return {}
    with db.connect() as conn:
        return _rust.songs_paths_of(conn, ids)


# Lo que se guarda del modo estudio y sus limites: un JSON pequeño y con
# forma conocida, no lo que mande cualquiera (lo limpia Rust:
# `biblioteca::indice::canciones::clean_study`).
STUDY_KEYS = ("loop", "loops", "speed", "pitch", "metronome", "markers", "notes", "mixer")


def set_study(cid: int, study: dict | None) -> Song | None:
    """Guarda el modo estudio de una cancion (indice y etiqueta del archivo).
    Con None o vacio, lo quita. Devuelve la ficha.

    Lo que se guarda, ya limpio: el tramo que se repite (o varios), la
    velocidad, el tono corrido, lo que se ajusto del metronomo (y el clic
    propio), los marcadores con sus notas, las notas, el mezclador de las
    pistas, las guias habladas (solo de la carpeta de guias de DanPlay) y sus
    carriles.
    """
    from .. import cues as guides

    c = by_id(cid)
    if not c:
        return None
    raw = _rust.songs_clean_study(study or {}, str(guides.folder()))
    update(cid, study=raw)
    if config.WRITE_TAGS and os.path.exists(c["path"]):
        tags.write_study(c["path"], raw)
    return by_id(cid)
