"""La rejilla de pulsos de una cancion para el metronomo del modo estudio.

La calcula Beat This! (`pulse.py`) en un proceso aparte, una vez: se guarda
en DATA_DIR/pulso/, con la ruta, el tamaño y la fecha del archivo, y la
siguiente vez sale al momento. Si no se puede (sin el grafo, sin ffmpeg, una
cancion sin pulso), quien la pide sigue con el analisis de siempre de la app
de escritorio (`beats.rs`): el metronomo funciona igual, algo peor.

Calcularla tarda: unos 15-20 s por cancion en un portatil (la red, en la
CPU). Por eso la app no la espera: la pide con `request` (al momento lo que
haya; si no hay, a la cola de este modulo), suena con la de Rust (medio
segundo) y se pasa a esta cuando llega. `prepare` deja en cola la que vendra
despues, para que al pasar de cancion ya este.
"""

import contextlib
import json
import logging
import os
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

from . import config, convert

log = logging.getLogger(__name__)

GRAPH = Path(__file__).resolve().parent / "data" / "pulso" / "beat_this.onnx"
# Lo que se guarda cambia si cambia la red o la forma de sacar la rejilla:
# con otro numero, lo guardado no vale y se calcula otra vez. 2: un solo nivel
# de pulso, afinado entre tramas, el «1» con modelo de compas y sin clic
# donde no hay pulso (ver pulse.grid).
VERSION = 2
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


def retag(path: str, before, after) -> bool:
    """El archivo cambio solo de etiqueta (lo escribio DanPlay): la rejilla
    guardada con la clave de antes pasa a la de ahora, que el sonido es el
    mismo. Antes se perdia cada vez que se guardaba algo del modo estudio en
    la cancion, y el clic volvia a tardar 15-20 s. True si habia que mover."""
    return moved(path, path, before, after)


def moved(old_path: str, new_path: str, before, after) -> bool:
    """El archivo se movio o se renombro (lo hizo DanPlay: «Transformar»,
    llevarla a Pistas...), con su etiqueta reescrita o no: su rejilla sigue
    valiendo en la ruta nueva. `before`/`after`, sus `os.stat` de antes y de
    despues. True si habia que mover."""
    from .pulse import key_of

    folder = config.DATA_DIR / "pulso"
    old = folder / f"{key_of(old_path, before)}.json"
    new = folder / f"{key_of(new_path, after)}.json"
    if old == new or not old.is_file():
        return False
    with contextlib.suppress(OSError, ValueError):
        saved = json.loads(old.read_text(encoding="utf-8"))
        saved["path"] = new_path
        new.write_text(json.dumps(saved, separators=(",", ":")), encoding="utf-8")
        old.unlink()
        return True
    return False


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
    answer, code, errors = _run(path)
    if not answer.get("ok"):
        if errors.strip():
            log.warning("el detector de pulso fallo:\n%s", errors.strip()[-2000:])
        raise BeatsError(answer.get("error") or f"el detector se cerró sin terminar ({code})")
    grid = {k: v for k, v in answer.items() if k != "ok"}
    cache.parent.mkdir(parents=True, exist_ok=True)
    with contextlib.suppress(OSError):
        cache.write_text(
            json.dumps({"version": VERSION, "path": path, "grid": grid}, separators=(",", ":")),
            encoding="utf-8",
        )
    return grid


# --------------------------------------------------------- en segundo plano
# Un solo hilo: la red ya usa varios nucleos, y dos a la vez solo se
# estorbarian. La cola es corta: lo que se pide para ya va delante, lo que se
# prepara para despues, detras; lo que se queda fuera se pedira cuando toque.
MAX_QUEUE = 6
_WAKE = threading.Condition()
_QUEUE: list[str] = []
_BUSY: list[str] = []  # la que se esta calculando (una, o ninguna)
_FAILED: dict[str, str] = {}  # clave del archivo -> por que no
_PROGRESS: dict[str, float] = {}  # la que se calcula -> 0..1
_WORKER: list[threading.Thread] = []


def _run(path: str) -> tuple[dict, int, str]:
    """El detector en su proceso: (su respuesta, su codigo, lo que dijo por
    stderr). Lo que va diciendo de avance queda en `_PROGRESS` segun llega."""
    flags = 0
    if sys.platform == "win32":  # pragma: no cover - sin ventana de consola
        flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    answer: dict = {}
    # stderr a un archivo: por una tuberia que nadie lee, un detector que
    # hablase mucho se quedaria parado esperando a que se vaciara
    with tempfile.TemporaryFile() as errors:
        proc = subprocess.Popen(
            _command(path),
            stdout=subprocess.PIPE,
            stderr=errors,
            text=True,
            encoding="utf-8",
            errors="replace",
            creationflags=flags,
        )
        killed = threading.Event()

        def kill() -> None:
            killed.set()
            proc.kill()

        timer = threading.Timer(TIMEOUT, kill)
        timer.start()
        try:
            for line in proc.stdout or ():
                with contextlib.suppress(ValueError):
                    said = json.loads(line)
                    if isinstance(said, dict) and set(said) == {"progress"}:
                        with _WAKE:
                            _PROGRESS[path] = float(said["progress"])
                    elif isinstance(said, dict):
                        answer = said
            code = proc.wait()
        finally:
            timer.cancel()
            with _WAKE:
                _PROGRESS.pop(path, None)
        if killed.is_set():
            raise BeatsError("tarda demasiado")
        errors.seek(0)
        return answer, code, errors.read().decode("utf-8", "replace")


def progress(path: str) -> float | None:
    """Por donde va (0..1) la rejilla de esa ruta: 0 si espera en cola, None
    si no esta ni en cola ni calculandose."""
    with _WAKE:
        if path in _BUSY:
            return _PROGRESS.get(path, 0.0)
        return 0.0 if path in _QUEUE else None


def _key(path: str) -> str:
    """La clave del archivo (cambia si cambia el archivo), o la ruta si no esta."""
    from .pulse import key_of

    try:
        return key_of(path)
    except OSError:
        return path


def request(path: str, *, first: bool = True) -> dict | None:
    """La rejilla si ya esta guardada; si no, None y queda en cola (delante
    con `first`, para la que suena; detras sin el, para la que vendra).
    BeatsError si aqui no se puede o si ya fallo con este mismo archivo:
    entonces no hay nada que esperar."""
    if not os.path.isfile(path):
        raise BeatsError("no encuentro el archivo")
    with contextlib.suppress(BeatsError):
        if cached(path):
            return grid_of(path)
    reason = available()
    if reason:
        raise BeatsError(reason)
    with _WAKE:
        failed = _FAILED.get(_key(path))
        if failed:
            raise BeatsError(failed)
        if path not in _BUSY:
            if path in _QUEUE:
                _QUEUE.remove(path)
            if first:
                _QUEUE.insert(0, path)
            else:
                _QUEUE.append(path)
            del _QUEUE[MAX_QUEUE:]
        if not _WORKER:
            t = threading.Thread(target=_work, name="danplay-pulso", daemon=True)
            _WORKER.append(t)
            t.start()
        _WAKE.notify_all()
    return None


def prepare(path: str) -> bool:
    """Deja en cola, detras, la rejilla de una cancion que sonara despues.
    True si ya estaba o queda en cola; False si aqui no se puede."""
    try:
        request(path, first=False)
    except BeatsError:
        return False
    return True


def _work() -> None:
    while True:
        with _WAKE:
            while not _QUEUE:
                _WAKE.wait()
            path = _QUEUE.pop(0)
            _BUSY[:] = [path]
        try:
            grid_of(path)
        except BeatsError as e:
            with _WAKE:
                _FAILED[_key(path)] = str(e)
        except Exception:
            log.warning("el detector de pulso fallo con %s", path, exc_info=True)
            with _WAKE:
                _FAILED[_key(path)] = "el detector de pulso falló"
        finally:
            with _WAKE:
                _BUSY.clear()
                _WAKE.notify_all()


def idle(timeout: float = 30.0) -> bool:
    """Espera a que no quede nada en cola (para las pruebas)."""
    with _WAKE:
        return _WAKE.wait_for(lambda: not _QUEUE and not _BUSY, timeout)


def _reset() -> None:
    """Olvida la cola y los fallos (para las pruebas)."""
    with _WAKE:
        _QUEUE.clear()
        _FAILED.clear()
