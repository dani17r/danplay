"""La rejilla de pulsos de una cancion para el metronomo del modo estudio.

La calcula Beat This! (`pulse.py`) en un proceso aparte, una vez: se guarda
en DATA_DIR/beats/, con la ruta, el tamaño y la fecha del archivo, y la
siguiente vez sale al momento. Si no se puede (sin el grafo, sin ffmpeg, una
cancion sin pulso), quien la pide sigue con el analisis de siempre de la app
de escritorio (`beats.rs`): el metronomo funciona igual, algo peor.

Calcularla tarda: unos 15-20 s por cancion en un portatil (la red, en la
CPU). Por eso la app no la espera: la pide con `request` (al momento lo que
haya; si no hay, a la cola de este modulo), suena con la de Rust (medio
segundo) y se pasa a esta cuando llega. `prepare` deja en cola la que vendra
despues, para que al pasar de cancion ya este.

Desde 1.28 se guarda tambien lo que dijo la red (`<clave>.npz`): con eso,
volver a sacar la rejilla de otra manera («Re-analizar el clic»: arreglar
los saltos de fase, tempo fijo, rellenar silencios, solo un tramo) tarda
milisegundos. Y si la cancion tiene la bateria separada, la red se pasa
tambien por ella y en cada instante manda la mas segura (`pulse.combine`):
con eso el clic se pega a la bateria sin perder la intro.
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

GRAPH = Path(__file__).resolve().parent / "data" / "pulse" / "beat_this.onnx"
# Lo que se guarda cambia si cambia la red o la forma de sacar la rejilla:
# con otro numero, lo guardado no vale y se calcula otra vez. 2: un solo nivel
# de pulso, afinado entre tramas, el «1» con modelo de compas y sin clic
# donde no hay pulso (ver pulse.grid). 3: los saltos de fase reparados, los
# huecos rellenos siguiendo a la red, tempo fijo para lo grabado a clic y la
# bateria separada, si la hay.
VERSION = 3
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

    return config.DATA_DIR / "beats" / f"{key_of(path)}.json"


def _raw_file(path: str, st=None) -> Path:
    """Donde va lo que dijo la red de un archivo."""
    from .pulse import key_of

    return config.DATA_DIR / "beats" / f"{key_of(path, st)}.npz"


def _command(path: str, raw: Path | None = None) -> list[str]:
    args = [str(GRAPH), convert.tool("ffmpeg") or "ffmpeg", path]
    if raw is not None:
        args.append(str(raw))
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

    folder = config.DATA_DIR / "beats"
    old = folder / f"{key_of(old_path, before)}.json"
    new = folder / f"{key_of(new_path, after)}.json"
    # lo que dijo la red va con ella
    old_raw, new_raw = old.with_suffix(".npz"), new.with_suffix(".npz")
    if old_raw != new_raw and old_raw.is_file():
        with contextlib.suppress(OSError):
            os.replace(old_raw, new_raw)
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
        saved = json.loads(_cache(path).read_text(encoding="utf-8"))
        return saved.get("version") == VERSION or bool(saved.get("exact"))
    return False


def _save(path: str, grid: dict, edits: list | None = None) -> None:
    cache = _cache(path)
    cache.parent.mkdir(parents=True, exist_ok=True)
    with contextlib.suppress(OSError):
        cache.write_text(
            json.dumps(
                {"version": VERSION, "path": path, "grid": grid, "edits": edits or []},
                separators=(",", ":"),
            ),
            encoding="utf-8",
        )


def save_exact(path: str, grid: dict) -> None:
    """Una rejilla que no se adivina: la del tempo de un proyecto (una
    secuencia de Ableton). Vale aunque cambie la forma de sacar las demas."""
    _save(path, grid)
    with contextlib.suppress(OSError, ValueError):
        cache = _cache(path)
        saved = json.loads(cache.read_text(encoding="utf-8"))
        saved["exact"] = True
        cache.write_text(json.dumps(saved, separators=(",", ":")), encoding="utf-8")


def _saved(path: str) -> dict | None:
    with contextlib.suppress(OSError, ValueError):
        saved = json.loads(_cache(path).read_text(encoding="utf-8"))
        if isinstance(saved, dict):
            return saved
    return None


def raw_of(path: str) -> dict | None:
    """Lo que dijo la red de ese archivo, si se guardo."""
    from . import pulse

    raw_path = _raw_file(path)
    return pulse.load_raw(raw_path) if raw_path.is_file() else None


def raw_by_key(key: str) -> dict | None:
    """Lo que dijo la red de un archivo por su clave (aunque ya no este)."""
    from . import pulse

    raw_path = config.DATA_DIR / "beats" / f"{key}.npz"
    return pulse.load_raw(raw_path) if raw_path.is_file() else None


def _drums_of(path: str) -> str | None:
    """La bateria separada de la cancion de esa ruta, si la tiene entera."""
    from . import library, stems

    with contextlib.suppress(Exception):
        brief = library.by_path(path)
        song = library.by_id(brief["id"]) if brief else None
        info = stems.info(song) if song else None
        if info and info.get("complete"):
            for t in info["tracks"]:
                if t["source"] == "drums" and t["exists"]:
                    return t["path"]
    return None


def _from_raw(raw: dict, *, mode="auto", edges=False, bpm=None, extra=None) -> dict:
    from . import pulse

    beat = raw["beat"] if extra is None else pulse.combine(raw["beat"], extra["beat"])
    try:
        return pulse.grid(
            beat,
            raw["down"],
            lowband=(raw["low"], raw["low_fps"]),
            mode=mode,
            edges=edges,
            duration=raw["seconds"],
            bpm=bpm,
        )
    except ValueError as e:
        raise BeatsError(str(e)) from e


def _network(path: str) -> dict:
    """Pasa la red por el archivo (en su proceso) y devuelve lo que dijo."""
    from . import pulse

    reason = available()
    if reason:
        raise BeatsError(reason)
    raw_path = _raw_file(path)
    raw_path.parent.mkdir(parents=True, exist_ok=True)
    answer, code, errors = _run(path, raw_path)
    if not answer.get("ok"):
        if errors.strip():
            log.warning("el detector de pulso fallo:\n%s", errors.strip()[-2000:])
        raise BeatsError(answer.get("error") or f"el detector se cerró sin terminar ({code})")
    raw = pulse.load_raw(raw_path)
    if raw is None:
        raise BeatsError("el detector no dejó lo que oyó")
    return raw


def grid_of(path: str) -> dict:
    """La rejilla del archivo: la guardada si la hay, o calculada ahora (de
    lo que dijo la red, si se guardo; si no, pasando la red)."""
    if not os.path.isfile(path):
        raise BeatsError("no encuentro el archivo")
    saved = _saved(path)
    if saved and (saved.get("version") == VERSION or saved.get("exact")) and saved.get("grid"):
        return saved["grid"]
    raw = raw_of(path) or _network(path)
    # con la bateria separada, la red tambien sobre ella (si ya se paso)
    drums = _drums_of(path)
    extra = raw_of(drums) if drums else None
    grid = _from_raw(raw, extra=extra)
    # lo que se re-analizo a mano (de una version anterior), otra vez encima
    edits = (saved or {}).get("edits") or []
    for e in edits:
        with contextlib.suppress(BeatsError, ValueError, KeyError, TypeError):
            grid = _apply(path, grid, raw, e, extra)
    _save(path, grid, edits)
    return grid


# ------------------------------------------------------------ re-analizar

REANALYZE_MODES = ("repair", "steady", "fill", "drums", "follow")


def _merge(old: dict, new_beats: list[float], start: float, end: float, raw: dict) -> dict:
    """La rejilla de antes con los pulsos de `start` a `end` cambiados por
    los nuevos, cosidos a los de los lados, y el «1» de toda otra vez."""
    import numpy as np

    from . import pulse

    beats = np.asarray(old["beats"], dtype=np.float64)
    keep_l = beats[beats < start]
    keep_r = beats[beats > end]
    inner = [t for t in new_beats if start <= t <= end]
    merged = np.sort(np.concatenate([keep_l, np.asarray(inner, dtype=np.float64), keep_r]))
    keep = np.concatenate([[True], np.diff(merged) > 0.05])
    merged = merged[keep]
    report: dict = {"region": [round(start, 2), round(end, 2)]}
    # las costuras: ¿cabe un numero entero de pulsos a cada lado?
    p = float(np.median(np.diff(merged))) if merged.size > 2 else 0.5
    joins = {}
    for side, (a, b) in {
        "left": (keep_l[-1] if keep_l.size else None, inner[0] if inner else None),
        "right": (inner[-1] if inner else None, keep_r[0] if keep_r.size else None),
    }.items():
        if a is None or b is None:
            continue
        n = float(b - a) / p
        joins[side] = bool(abs(n - round(n)) <= 0.25)
    report["joins"] = joins
    return pulse.finish(merged, raw["down"], report, meter=old.get("meter"))


def _apply(path: str, base: dict, raw: dict, edit: dict, extra=None) -> dict:
    """Una re-analisis sobre la rejilla `base`: de toda la cancion, o de un
    tramo (cosido a lo de los lados)."""
    import numpy as np

    mode = edit.get("mode", "repair")
    start, end = edit.get("start"), edit.get("end")
    if mode == "drums":
        drums = _drums_of(path)
        if not drums:
            raise BeatsError("esta canción no tiene la batería separada")
        extra = raw_of(drums) or _network(drums)
    engine = {
        "repair": "auto",
        "drums": "auto",
        "steady": "steady",
        "fill": "fill",
        "follow": "follow",
    }[mode]
    whole = _from_raw(
        raw,
        mode=engine,
        edges=mode == "fill",
        bpm=edit.get("bpm"),
        extra=extra if mode in ("drums", "repair", "fill", "steady") else None,
    )
    if start is None or end is None:
        return whole
    start, end = float(start), float(end)
    if mode == "steady":
        # tempo fijo entre los dos pulsos de los lados: cabe justo
        old = np.asarray(base["beats"], dtype=np.float64)
        left = old[old < start]
        right = old[old > end]
        if left.size and right.size:
            a, b = float(left[-1]), float(right[0])
            period = 60.0 / (edit.get("bpm") or whole["bpm"])
            k = max(1, round((b - a) / period))
            inner = [a + (b - a) * i / k for i in range(1, k)]
            return _merge(base, inner, a + 1e-6, b - 1e-6, raw)
    return _merge(base, whole["beats"], start, end, raw)


def changes(before: list[float], after: list[float], tol: float = 0.04) -> int:
    """En cuantos sitios suena distinto el clic: los trozos seguidos de
    pulsos que se movieron (mas de `tol`), aparecieron o se quitaron. Un
    hueco relleno o un salto de fase arreglado cuenta uno."""
    import numpy as np

    a = np.asarray(before, dtype=np.float64)
    b = np.asarray(after, dtype=np.float64)
    if not a.size or not b.size:
        return int(bool(a.size or b.size))

    def off(xs, ys):
        i = (
            np.clip(np.searchsorted(ys, xs), 1, ys.size - 1)
            if ys.size > 1
            else np.zeros(xs.size, int)
        )
        near = (
            np.minimum(np.abs(xs - ys[i - 1]), np.abs(xs - ys[i]))
            if ys.size > 1
            else np.abs(xs - ys[0])
        )
        return near > tol

    marks = np.concatenate([b[off(b, a)], a[off(a, b)]])
    if not marks.size:
        return 0
    marks.sort()
    # un trozo: cambios separados por menos de dos segundos
    return int(1 + np.sum(np.diff(marks) > 2.0))


def reanalyze(path: str, mode: str, start=None, end=None, bpm=None) -> dict:
    """Vuelve a sacar la rejilla de otra manera: 'repair' (arreglar los
    saltos de fase y rellenar los huecos a tempo; si va a clic, tempo fijo),
    'steady' (tempo fijo; `bpm`, si se da), 'fill' (ademas, clic en la intro
    y el final), 'drums' (con la bateria separada) o 'follow' (como antes de
    1.28). De toda la cancion, o de `start` a `end`. Se guarda (y se vuelve a
    aplicar si cambia la version) y se devuelve con lo que se arreglo."""
    if mode not in REANALYZE_MODES:
        raise BeatsError(f"no sé analizar así: {mode}")
    if not os.path.isfile(path):
        raise BeatsError("no encuentro el archivo")
    raw = raw_of(path) or _network(path)
    current = grid_of(path)
    edit: dict = {"mode": mode}
    if start is not None and end is not None and float(end) > float(start):
        edit.update(start=round(float(start), 3), end=round(float(end), 3))
    if bpm:
        edit["bpm"] = float(bpm)
    drums = _drums_of(path)
    extra = raw_of(drums) if drums else None
    grid = _apply(path, current, raw, edit, extra)
    grid["report"] = {
        **(grid.get("report") or {}),
        "fixed": changes(current["beats"], grid["beats"]),
    }
    saved = _saved(path) or {}
    edits = [] if "start" not in edit else list(saved.get("edits") or [])
    edits.append(edit)
    _save(path, grid, edits)
    return grid


def improve(path: str, *, drums: bool = True) -> dict:
    """La rejilla de una cancion, de nuevo y lo mejor que se pueda: la red
    sobre ella (si no estaba lo que oyo) y sobre su bateria separada (si la
    tiene). Para arreglar toda la biblioteca de una vez."""
    if not os.path.isfile(path):
        raise BeatsError("no encuentro el archivo")
    raw = raw_of(path) or _network(path)
    extra = None
    if drums:
        d = _drums_of(path)
        if d:
            with contextlib.suppress(BeatsError):
                extra = raw_of(d) or _network(d)
    grid = _from_raw(raw, extra=extra)
    saved = _saved(path) or {}
    edits = saved.get("edits") or []
    for e in edits:
        with contextlib.suppress(BeatsError, ValueError, KeyError, TypeError):
            grid = _apply(path, grid, raw, e, extra)
    _save(path, grid, edits)
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


def _run(path: str, raw: Path | None = None) -> tuple[dict, int, str]:
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
            _command(path, raw),
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
            t = threading.Thread(target=_work, name="danplay-beats", daemon=True)
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
