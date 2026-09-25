"""Lo que comparten varias rutas."""

import logging
import mimetypes
import os
import threading
from pathlib import Path
from typing import Any

from .. import config

log = logging.getLogger(__name__)


# Cada formato con su tipo. El navegador rechaza un .flac anunciado como mp3.
AUDIO_TYPES = {
    ".mp3": "audio/mpeg",
    ".flac": "audio/flac",
    ".ogg": "audio/ogg",
    ".opus": "audio/opus",
    ".m4a": "audio/mp4",
    ".aac": "audio/aac",
    ".wav": "audio/wav",
    ".wma": "audio/x-ms-wma",
}


def audio_type(path) -> str:
    ext = os.path.splitext(str(path))[1].lower()
    return AUDIO_TYPES.get(ext) or mimetypes.guess_type(str(path))[0] or "application/octet-stream"


def _in_background(name: str, fn) -> None:
    """Arranca `fn` en un hilo propio y vuelve enseguida (una descarga tarda
    minutos). Lo que falle se apunta en el registro."""

    def run():
        try:
            fn()
        except Exception:
            log.warning("fallo en segundo plano (%s)", name, exc_info=True)

    threading.Thread(target=run, name=name, daemon=True).start()


def _started(snap: dict, already: bool) -> dict:
    """La respuesta de una ruta que arranca un trabajo (contrato A)."""
    out: dict[str, Any] = {"job": snap}
    if already:
        out["already_running"] = True
    return out


def _relative(path) -> str:
    """La ruta relativa a la biblioteca si cuelga de ella; si no, entera."""
    try:
        return str(Path(path).relative_to(config.LIBRARY))
    except ValueError:
        return str(path)
