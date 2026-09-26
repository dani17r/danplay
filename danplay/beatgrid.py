"""La rejilla de pulsos de una cancion para el metronomo del modo estudio.

La calcula Beat This! (`pulse.py`) en un proceso aparte, una vez: se guarda
en DATA_DIR/pulso/, con la ruta, el tamaño y la fecha del archivo, y la
siguiente vez sale al momento. Si no se puede (sin el grafo, sin ffmpeg, una
cancion sin pulso), quien la pide sigue con el analisis de siempre de la app
de escritorio (`beats.rs`): el metronomo funciona igual, algo peor.
"""

import contextlib
import json
import logging
import os
import subprocess
import sys
from pathlib import Path

from . import config, convert

log = logging.getLogger(__name__)

GRAPH = Path(__file__).resolve().parent / "data" / "pulso" / "beat_this.onnx"
# Lo que se guarda cambia si cambia la red o la forma de sacar la rejilla:
# con otro numero, lo guardado no vale y se calcula otra vez.
VERSION = 1
TIMEOUT = 600


class BeatsError(Exception):
    """No hay rejilla de Beat This! para esa cancion (por que, en castellano)."""


def available() -> str:
    """Por que no se puede usar aqui, o vacio si se puede."""
    import importlib.util

    for module in ("numpy", "onnxruntime"):
        if importlib.util.find_spec(module) is None:
            return f"falta {module} en esta instalación de DanPlay"
    if not convert.tool("ffmpeg"):
        return "hace falta ffmpeg"
    if not GRAPH.is_file():
        return "falta el detector de pulso en esta instalación"
    return ""


def _cache(path: str) -> Path:
    from .pulse import key_of

    return config.DATA_DIR / "pulso" / f"{key_of(path)}.json"


def _command(path: str) -> list[str]:
    args = [str(GRAPH), convert.tool("ffmpeg") or "ffmpeg", path]
    if config.FROZEN:
        return [sys.executable, "--pulso", *args]
    return [sys.executable, "-m", "danplay.pulse", *args]


def cached(path: str) -> bool:
    """Si ya hay rejilla guardada para ese archivo (vale aunque aqui no se
    pudiera calcular otra)."""
    with contextlib.suppress(OSError, ValueError):
        return json.loads(_cache(path).read_text(encoding="utf-8")).get("version") == VERSION
    return False


def grid_of(path: str) -> dict:
    """La rejilla del archivo: la guardada si la hay, o calculada ahora."""
    if not os.path.isfile(path):
        raise BeatsError("no encuentro el archivo")
    cache = _cache(path)
    with contextlib.suppress(OSError, ValueError):
        saved = json.loads(cache.read_text(encoding="utf-8"))
        if saved.get("version") == VERSION and saved.get("grid"):
            return saved["grid"]
    reason = available()
    if reason:
        raise BeatsError(reason)
    flags = 0
    if sys.platform == "win32":  # pragma: no cover - sin ventana de consola
        flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    try:
        r = subprocess.run(
            _command(path),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=TIMEOUT,
            check=False,
            creationflags=flags,
        )
    except subprocess.TimeoutExpired as e:
        raise BeatsError("tarda demasiado") from e
    answer: dict = {}
    for line in r.stdout.splitlines():
        with contextlib.suppress(ValueError):
            answer = json.loads(line)
    if not answer.get("ok"):
        if r.stderr.strip():
            log.warning("el detector de pulso fallo:\n%s", r.stderr.strip()[-2000:])
        raise BeatsError(
            answer.get("error") or f"el detector se cerró sin terminar ({r.returncode})"
        )
    grid = {k: v for k, v in answer.items() if k != "ok"}
    cache.parent.mkdir(parents=True, exist_ok=True)
    with contextlib.suppress(OSError):
        cache.write_text(
            json.dumps({"version": VERSION, "path": path, "grid": grid}, separators=(",", ":")),
            encoding="utf-8",
        )
    return grid
