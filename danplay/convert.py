"""Conversion de formatos a mp3, conservando etiquetas y portada.

Es opcional y desactivable: en la app sera una casilla.
Las carpetas de trabajo (Secuencias, Pistas) quedan siempre fuera:
ahi los .wav son material de produccion y comprimirlos seria perder calidad.

Lo hace el crate de Rust (biblioteca/src/convertir.rs): la conversion, las
medidas de una imagen y encoger caratulas. Aqui queda la puerta de Python,
con los mismos nombres, y lo que depende de `config` (donde esta ffmpeg, que
carpetas no se tocan) o de la biblioteca (los candidatos).
"""

import logging
import time
from pathlib import Path

import danplay_core as _rust

from . import config

log = logging.getLogger(__name__)

SOURCE_FORMATS = set(_rust.convert_source_formats())
QUALITIES = {q: list(args) for q, args in _rust.convert_qualities()}


# `/api/status` se consulta constantemente desde la app, y cada consulta
# preguntaba al sistema de archivos por ffmpeg. Se recuerda la respuesta: si
# esta, para siempre (un binario no desaparece); si no esta, se vuelve a mirar
# de vez en cuando, para que instalarlo se note sin reiniciar la app.
#
# Se guarda la RUTA y no solo «si esta»: en la app empaquetada ffmpeg puede
# vivir junto al ejecutable y no en el PATH (ver `config.find_tool`), asi que
# los comandos tienen que usar la ruta encontrada.
_binaries: dict = {}
_RECHECK_SECONDS = 5.0


def tool(name: str) -> str | None:
    """Ruta del binario (`ffmpeg`, `ffprobe`...), o None si no esta."""
    found, checked = _binaries.get(name, (None, 0.0))
    if found:
        return found
    if time.monotonic() - checked < _RECHECK_SECONDS:
        return None
    found = config.find_tool(name)
    _binaries[name] = (found, time.monotonic())
    return found


def _has_binary(name: str) -> bool:
    return tool(name) is not None


def available() -> bool:
    return _has_binary("ffmpeg")


def in_protected_folder(path) -> bool:
    """True si el archivo esta en una carpeta que no se debe convertir."""
    return _rust.convert_in_protected_folder(Path(path), sorted(config.NEVER_CONVERT))


def needs_convert(path) -> bool:
    return _rust.convert_needs_convert(Path(path), sorted(config.NEVER_CONVERT))


def convert(
    path, quality="high", keep_original=False, target=None, *, conservar_original=None
) -> dict:
    """Convierte a mp3 conservando metadata y caratula. No toca carpetas protegidas.

    Si falla el primer intento se prueba sin la portada, que a veces rompe el
    mapeo. `conservar_original` es el nombre antiguo del parametro; se acepta
    para no romper a quien lo use por nombre, pero el bueno es `keep_original`.
    """
    if conservar_original is not None:
        keep_original = conservar_original
    path = Path(path)
    if not available():
        return {"ok": False, "reason": "falta ffmpeg"}
    return _rust.convert_convert(
        tool("ffmpeg") or "ffmpeg",
        path,
        quality if isinstance(quality, str) else "high",
        bool(keep_original),
        Path(target) if target else None,
        sorted(config.NEVER_CONVERT),
    )


# Una caratula no necesita mas de esto. Meter un png de 8 MB dentro de cada
# mp3 hincha la biblioteca y ralentiza la app para nada: a 1000 px ya se ve
# perfecta en cualquier pantalla.
COVER_MAX_SIDE, COVER_MAX_BYTES = _rust.convert_cover_limits()


def image_size(path) -> tuple[int, int]:
    """Ancho y alto de una imagen (jpg, png, webp, gif o bmp), o (0, 0) si
    no se puede saber.

    Se leen de su cabecera, sin decodificarla. Antes se le preguntaba a
    ffprobe: un proceso por caratula, y en el paquete de Windows 100 MB que
    solo servian para esto y para la duracion de las secuencias."""
    size = _rust.convert_image_size(path)
    if size is None:
        log.warning("no pude medir la imagen %s: %s", path, _rust.last_error())
        return (0, 0)
    return size


def ffmpeg() -> str | None:
    """La ruta de ffmpeg si esta, o None: lo que se le pasa a Rust, que lo
    lanza (en la app empaquetada vive junto al ejecutable)."""
    return (tool("ffmpeg") or "ffmpeg") if available() else None


def shrink_image(path, max_side=COVER_MAX_SIDE) -> tuple[bytes, str] | None:
    """Devuelve (datos, mime) de la imagen, encogida si hacia falta.

    Se mira el peso Y las medidas: un png de 3000x3000 puede pesar poco y aun
    asi es absurdo meterlo en cada mp3, porque hay que decodificarlo entero
    cada vez que se pinta la caratula.

    Si ya vale, se devuelve tal cual: reconvertir una jpg correcta solo le
    quita calidad sin ganar nada. Sin ffmpeg, tambien tal cual.
    """
    return _rust.convert_shrink_image(Path(path), int(max_side), ffmpeg())


def shrink_bytes(data: bytes, mime="image/jpeg", max_side=COVER_MAX_SIDE) -> tuple[bytes, str]:
    """Como `shrink_image`, pero para una imagen que ya esta en memoria.

    Las caratulas descargadas llegan como bytes; se posan en un temporal solo
    el tiempo de medirlas y encogerlas.
    """
    return _rust.convert_shrink_bytes(bytes(data), str(mime), int(max_side), ffmpeg())


def candidates(root=None) -> list[str]:
    """Archivos que se convertirian (respetando carpetas protegidas)."""
    from . import library as B

    root = root or config.LIBRARY
    return [r for r in B.audio_files(root) if needs_convert(r)]


def convert_batch(
    paths=None,
    quality="high",
    keep_original=False,
    dry_run=False,
    progress=None,
    *,
    conservar_original=None,
) -> dict:
    if conservar_original is not None:  # nombre antiguo del parametro
        keep_original = conservar_original
    paths = paths if paths is not None else candidates()
    done_items, failures, skipped = [], [], []
    for i, r in enumerate(paths, 1):
        if dry_run:
            done_items.append(
                {"source_path": r, "target": str(Path(r).with_suffix(".mp3")), "dry_run": True}
            )
        else:
            res = convert(r, quality, keep_original)
            (done_items if res["ok"] else (skipped if res.get("skipped") else failures)).append(res)
        if progress:
            progress(i, len(paths))
    saved = sum(h.get("before", 0) - h.get("after", 0) for h in done_items)
    return {
        "converted": len(done_items),
        "failures": len(failures),
        "skipped": len(skipped),
        "saved_bytes": saved,
        "detail": done_items,
        "errors": failures,
    }
