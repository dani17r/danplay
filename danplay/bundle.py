"""Llevar una cancion separada al celular: un solo archivo `.danpista`.

La app del celular (`mobile/`) no separa, ni convierte, ni ordena nada: solo
abre lo que se separo aqui y deja estudiarlo (cada parte con su volumen, el
clic, la velocidad, los tramos, la letra y los acordes). Para eso necesita
todo lo de la cancion en un archivo que se pueda mandar por WhatsApp, Drive o
un cable, y que un musico de la banda abra sin tener DanPlay en la PC.

La carpeta de `Separadas/` no basta: ahi solo estan las partes. El pulso del
clic, lo guardado del modo estudio, la letra y los acordes viven en la base
y en la cache de esta PC. Por eso se arma un ZIP aparte:

    Celular/Adoracion La Ibi - Dios Poderoso.danpista
        pista.json          que cancion es y todo lo suyo (ver `manifest`): el
                            pulso, el estudio, la letra, el cifrado, la onda
        partes/Bateria.ogg  cada parte, en Ogg Vorbis
        partes/Voces.ogg    ...
        portada.jpg         si la cancion tiene caratula

Las partes van en Vorbis (calidad 6, unos 190 kbps) y no en el FLAC de
`Separadas/`: una cancion de cinco minutos baja de 86 MB a 23. Medido con el
decodificador del celular (symphonia 0.6), Vorbis y MP3 empiezan en la
muestra exacta y AAC llega 1024 muestras tarde (23 ms: el clic se iria). Y
Vorbis pesa la mitad que MP3 a 256 kbps y se decodifica antes. Opus no: el
celular no tiene con que leerlo sin traer libopus. Si este ffmpeg no sabe
hacer Vorbis, van en MP3, que tambien empieza en su sitio.

Va en `Celular/`, junto a `Separadas/`, dentro de la biblioteca de la que
cuelga la cancion: como todo lo que se escribe desde la API, nunca fuera de
ella. Volver a llevarla pisa la de antes (con lo que se haya cambiado desde
entonces). El escaneo no la mira: no es audio.

El manifiesto, las partes (en paralelo) y el zip los hace Rust
(library/src/bundle.rs); aqui se piden la rejilla, la caratula y la onda.
"""

import contextlib
import logging
import subprocess
import time
from collections.abc import Mapping
from pathlib import Path
from typing import Any

import danplay_core as _rust

from . import beatgrid, convert, stems
from .api import jobs

log = logging.getLogger(__name__)

FOLDER = "Celular"
EXTENSION = ".danpista"
MANIFEST = "pista.json"
PARTS = "partes"
JOB = "bundle"
# Lo que entiende la app del celular. Si cambia de forma que una app vieja no
# lo pueda leer, sube; si solo se añade algo, no.
VERSION = 1

# Varias partes a la vez: ffmpeg codifica con un nucleo. Mas de tres a la vez
# y la separacion (o lo que suene) lo notan.
WORKERS = 3


class BundleError(jobs.JobError):
    """No se pudo preparar la cancion para el celular (por que, en castellano)."""


def _encoders(ffmpeg: str) -> str:
    try:
        r = subprocess.run(
            [ffmpeg, "-hide_banner", "-encoders"], capture_output=True, text=True, check=False
        )
    except OSError:
        return ""
    return r.stdout


def codec(ffmpeg: str) -> tuple[str, list[str]]:
    """La extension y los argumentos de ffmpeg con que van las partes."""
    return _codec_of(_encoders(ffmpeg))


def _codec_of(encoders: str) -> tuple[str, list[str]]:
    try:
        ext, args = _rust.bundle_codec(encoders)
    except ValueError as e:
        raise BundleError(str(e)) from None
    return ext, list(args)


def _grid(path: str) -> tuple[dict | None, str]:
    """La rejilla del clic (la guardada, o calculada ahora) y, si no hay, por que."""
    try:
        return beatgrid.grid_of(path), ""
    except beatgrid.BeatsError as e:
        return None, str(e)
    except (OSError, ValueError, subprocess.SubprocessError) as e:
        # el detector es otro proceso: que no tumbe el resto
        log.warning("sin rejilla para %s: %s", path, e)
        return None, "no se pudo sacar el pulso"


def _song_wave(path: str) -> dict | None:
    """La onda de la cancion entera, la que pinta el estudio (la de la cache
    si ya estaba): en el celular se ve la misma."""
    from . import waveform

    with contextlib.suppress(Exception):
        made = waveform.compute(path, waveform.DEFAULT_BUCKETS)
        if made and made.get("peaks"):
            return {"peaks": made["peaks"], "rms": made.get("rms") or []}
    return None


def _cover(path: str) -> tuple[bytes, str] | None:
    from . import thumbnails

    with contextlib.suppress(Exception):
        made = thumbnails.get(path, 320)
        if made:
            data, mime = made
            return data, ".png" if "png" in mime else ".jpg"
    return None


def manifest(
    song: Mapping[str, Any],
    info: Mapping[str, Any],
    files: dict[str, str],
    grid: dict | None,
    cover: str | None,
    song_wave: dict | None = None,
) -> dict:
    """Lo que va en `pista.json`. Las claves, en ingles como las del
    manifiesto de `Separadas/`; lo de dentro, tal cual lo guarda la base."""
    from . import __version__

    return _rust.bundle_manifest(
        dict(song), dict(info), dict(files), grid, cover, song_wave, f"DanPlay {__version__}"
    )


# Las guias habladas viajan con la pista, en esta carpeta del .danpista.
GUIDES = "guias"


def guides_of(study: dict | None) -> tuple[dict | None, dict[str, Path]]:
    """El estudio con cada guia apuntando a su archivo dentro del .danpista
    (`guias/<nombre>.wav`), y esos archivos: {nombre dentro: archivo}. Solo
    las de la carpeta de guias de DanPlay; las demas no viajan."""
    from . import cues

    if not isinstance(study, dict) or not study.get("cues"):
        return study, {}
    return _rust.bundle_guides_of(study, str(cues.folder()))


def target_for(song: Mapping[str, Any]) -> Path:
    """Donde va: `Celular/<nombre del archivo>.danpista` en su biblioteca."""
    return Path(_rust.bundle_target_for(dict(song)))


def export(cid: int, progress=None) -> dict:
    """Prepara la cancion `cid` para el celular y devuelve donde quedo.

    `progress(done, total)` va contando las partes ya codificadas.
    """
    from . import __version__, cues, library

    song = library.by_id(cid)
    if not song:
        raise BundleError("esa canción ya no está en la biblioteca")
    info = stems.info(song)
    if not info or not info["complete"]:
        raise BundleError("no tiene sus pistas separadas completas")
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise BundleError("hace falta ffmpeg")
    encoders = _encoders(ffmpeg)
    _codec_of(encoders)  # que diga ya si este ffmpeg no sabe
    target = target_for(song)
    if not library.within_roots(target):
        raise BundleError("su biblioteca ya no está entre tus carpetas")
    target.parent.mkdir(parents=True, exist_ok=True)
    tracks = info["tracks"]
    total = len(tracks) + 1
    done = [0]

    def step() -> None:
        done[0] += 1
        if progress:
            progress(done[0], total)

    grid, why = _grid(song["path"])
    step()
    cover = _cover(song["path"])
    song_wave = _song_wave(song["path"])
    try:
        data = _rust.bundle_write(
            dict(song),
            dict(info),
            ffmpeg,
            encoders,
            str(target),
            grid,
            cover,
            song_wave,
            str(cues.folder()),
            f"DanPlay {__version__}",
            time.localtime().tm_gmtoff,
            step,
        )
    except ValueError as e:
        raise BundleError(str(e)) from None
    return {
        "id": cid,
        "title": data["song"]["title"],
        "path": str(target),
        "name": target.name,
        "size": target.stat().st_size,
        "tracks": len(tracks),
        "click": grid is not None,
        "click_error": why,
    }


def export_many(ids: list[int], progress=None) -> dict:
    """Varias canciones, una detras de otra (un repertorio para la banda). Las
    que no se pueden no paran a las demas: van en `failed` con su motivo."""
    from . import library

    done: list[dict] = []
    failed: list[dict] = []
    ids = list(dict.fromkeys(ids))  # sin repetidas, en su orden
    for i, cid in enumerate(ids):
        song = library.by_id(cid) or {}
        title = song.get("title") or Path(song.get("file") or "").stem or f"#{cid}"
        if progress:
            progress(i, len(ids), f"«{title}»")

        def parts(d, t, i=i, title=title):
            if progress:
                progress(i + d / max(t, 1), len(ids), f"«{title}»")

        try:
            done.append(export(cid, parts))
        except BundleError as e:
            failed.append({"id": cid, "title": title, "error": str(e)})
        except Exception as e:
            log.exception("no se pudo llevar %s al celular", cid)
            failed.append({"id": cid, "title": title, "error": str(e) or "falló"})
    if progress:
        progress(len(ids), len(ids), "")
    return {"done": done, "failed": failed}
