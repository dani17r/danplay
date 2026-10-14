"""Llevar una cancion separada al celular: un solo archivo `.danpista`.

La app del celular (`movil/`) no separa, ni convierte, ni ordena nada: solo
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
"""

import contextlib
import json
import logging
import os
import subprocess
import tempfile
import time
import zipfile
from collections.abc import Mapping
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any

from . import beatgrid, convert, stems
from .api import jobs

log = logging.getLogger(__name__)

FOLDER = "Celular"
EXTENSION = ".danpista"
MANIFEST = "pista.json"
PARTS = "partes"
JOB = "celular"
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
    found = _encoders(ffmpeg)
    if " libvorbis " in found:
        return ".ogg", ["-c:a", "libvorbis", "-q:a", "6"]
    if " libmp3lame " in found:
        return ".mp3", ["-c:a", "libmp3lame", "-b:a", "256k"]
    raise BundleError("este ffmpeg no sabe hacer ni Vorbis ni MP3")


def _encode(ffmpeg: str, source: Path, target: Path, args: list[str]) -> None:
    # sin las etiquetas de la parte (solo pesan) y siempre en 44,1 kHz
    # estereo: las de Opus son de 48 kHz, y el celular las suma muestra a
    # muestra
    cmd = [
        ffmpeg,
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-i",
        str(source),
        "-map",
        "0:a:0",
        "-map_metadata",
        "-1",
        "-ac",
        "2",
        "-ar",
        "44100",
        *args,
        str(target),
    ]
    r = subprocess.run(
        cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False
    )
    if r.returncode != 0 or not target.is_file():
        raise BundleError(f"ffmpeg no pudo preparar «{source.name}»: {r.stderr.strip()[-200:]}")


def _json(text: Any) -> Any:
    try:
        return json.loads(text or "null")
    except (TypeError, ValueError):
        return None


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

    chords = _json(song.get("chords"))
    sheet = chords.get("sheet") if isinstance(chords, dict) else None
    study = _json(song.get("study"))
    try:
        waves = json.loads((Path(info["folder"]) / stems.WAVES).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        waves = None
    return {
        "format": "danpista",
        "version": VERSION,
        "app": f"DanPlay {__version__}",
        "created": round(time.time(), 1),
        "song": {
            "title": song.get("title") or Path(song["file"]).stem,
            "artist": song.get("artist") or "",
            "album": song.get("album") or "",
            "year": song.get("year") or "",
            "duration": song.get("duration") or 0,
            "key": song.get("key") or "",
            "bpm": song.get("bpm") or 0,
            "file": song["file"],
        },
        "quality": info.get("quality") or "",
        "tracks": [
            {"source": t["source"], "name": t["name"], "file": files[t["source"]]}
            for t in info["tracks"]
            if t["source"] in files
        ],
        "grid": grid,
        "study": study if isinstance(study, dict) else None,
        "lyrics": song.get("lyrics") or "",
        "lyrics_synced": song.get("lyrics_synced") or "",
        "chords": sheet if isinstance(sheet, dict) else None,
        "waves": waves if isinstance(waves, dict) else None,
        "song_wave": song_wave,
        "cover": cover,
    }


def target_for(song: Mapping[str, Any]) -> Path:
    """Donde va: `Celular/<nombre del archivo>.danpista` en su biblioteca."""
    return Path(song["root"]) / FOLDER / (stems._safe_name(Path(song["file"]).stem) + EXTENSION)


def export(cid: int, progress=None) -> dict:
    """Prepara la cancion `cid` para el celular y devuelve donde quedo.

    `progress(done, total)` va contando las partes ya codificadas.
    """
    from . import library

    song = library.by_id(cid)
    if not song:
        raise BundleError("esa canción ya no está en la biblioteca")
    info = stems.info(song)
    if not info or not info["complete"]:
        raise BundleError("no tiene sus pistas separadas completas")
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise BundleError("hace falta ffmpeg")
    ext, args = codec(ffmpeg)
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
    with tempfile.TemporaryDirectory(prefix="danplay-celular-") as work:
        work_dir = Path(work)
        files = {t["source"]: f"{PARTS}/{Path(t['file']).stem}{ext}" for t in tracks}

        def one(track: Mapping[str, Any]) -> None:
            _encode(ffmpeg, Path(track["path"]), work_dir / files[track["source"]], args)
            step()

        (work_dir / PARTS).mkdir()
        with ThreadPoolExecutor(max_workers=min(WORKERS, len(tracks))) as pool:
            # list(): que salte el primer fallo de cualquiera
            list(pool.map(one, tracks))
        cover_name = f"portada{cover[1]}" if cover else None
        data = manifest(song, info, files, grid, cover_name, song_wave)
        tmp = target.with_name(f".{target.stem}.parte{EXTENSION}")
        try:
            with zipfile.ZipFile(tmp, "w") as z:
                # el manifiesto primero: el celular lo lee sin recorrer el resto
                z.writestr(
                    MANIFEST,
                    json.dumps(data, ensure_ascii=False, separators=(",", ":")),
                    compress_type=zipfile.ZIP_DEFLATED,
                )
                # el audio ya va comprimido: tal cual, que se lee mas rapido
                for t in tracks:
                    name = files[t["source"]]
                    z.write(work_dir / name, name, compress_type=zipfile.ZIP_STORED)
                if cover and cover_name:
                    z.writestr(cover_name, cover[0], compress_type=zipfile.ZIP_STORED)
            os.replace(tmp, target)
        except BaseException:
            tmp.unlink(missing_ok=True)
            raise
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
