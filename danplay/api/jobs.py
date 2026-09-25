"""Tareas largas en segundo plano (contrato A): escanear, importar,
convertir, buscar duplicados, poner yt-dlp al dia.

Antes esperaban dentro de la peticion, y el puente de Rust corta a los 60 s:
con una biblioteca grande la interfaz daba error aunque el trabajo siguiera
por detras. Ahora la peticion lo arranca en un hilo y contesta al momento;
la interfaz pregunta por `GET /api/jobs/{nombre}` hasta que `active` es
false, y ahi tiene `result` (lo que antes devolvia la ruta) o `error`.

Un trabajo de cada nombre a la vez: pedir otro igual mientras corre devuelve
el que ya esta en marcha. Lo terminado se guarda un rato (`TTL`) y se olvida.
"""

import copy
import logging
import threading
import time
from collections.abc import Callable

log = logging.getLogger(__name__)

# Cuanto se guarda un trabajo ya terminado antes de olvidarlo. Sin esto el
# registro crecia para siempre y /api/status lo devolvia entero cada vez.
TTL = 600.0


class JobError(Exception):
    """Un fallo previsto, con su explicacion ya en castellano: se enseña tal cual."""


_lock = threading.Lock()
_jobs: dict[str, dict] = {}

Progress = Callable[..., None]


def _fresh(name: str) -> dict:
    return {
        "name": name,
        "active": True,
        "done": 0,
        "total": 0,
        "message": "",
        "result": None,
        "error": "",
        "started": time.time(),
        "ended": None,
    }


def _expire(now: float) -> None:
    for key, job in list(_jobs.items()):
        if not job["active"] and now - (job["ended"] or now) > TTL:
            del _jobs[key]


def snapshot(name: str) -> dict | None:
    """Una copia del trabajo (nunca el dict vivo, que otro hilo va cambiando)."""
    with _lock:
        _expire(time.time())
        job = _jobs.get(name)
        return copy.deepcopy(job) if job else None


def snapshot_all() -> dict[str, dict]:
    with _lock:
        _expire(time.time())
        return copy.deepcopy(_jobs)


def active(name: str) -> bool:
    with _lock:
        job = _jobs.get(name)
        return bool(job and job["active"])


def start(
    name: str, work: Callable[[Progress], object], failure: str = "no se pudo terminar"
) -> tuple[dict, bool]:
    """Arranca `work(progress)` en un hilo. Devuelve (instantanea, ya_estaba).

    `progress(done=None, total=None, message=None)` va contando el avance.
    Lo que devuelva `work` queda en `result`; si falla, `error` lo dice en
    castellano (`failure`, y el motivo) y el detalle va al registro.
    """
    with _lock:
        _expire(time.time())
        current = _jobs.get(name)
        if current is not None and current["active"]:
            return copy.deepcopy(current), True
        job = _jobs[name] = _fresh(name)
        first = copy.deepcopy(job)
    threading.Thread(
        target=_run, args=(job, work, failure), name=f"danplay-{name}", daemon=True
    ).start()
    return first, False


def _run(job: dict, work, failure: str) -> None:
    def progress(done=None, total=None, message=None):
        with _lock:
            if done is not None:
                job["done"] = done
            if total is not None:
                job["total"] = total
            if message is not None:
                job["message"] = str(message)

    result, error = None, ""
    try:
        result = work(progress)
    except JobError as e:
        log.warning("el trabajo «%s» no pudo terminar: %s", job["name"], e)
        error = str(e)
    except Exception as e:
        log.warning("el trabajo «%s» fallo", job["name"], exc_info=True)
        error = f"{failure}: {e}" if str(e) else failure
    with _lock:
        job.update(active=False, result=result, error=error, ended=time.time())


def wait(name: str, timeout: float = 60.0) -> dict | None:
    """Espera a que termine (para las pruebas y la linea de ordenes)."""
    limit = time.monotonic() + timeout
    while time.monotonic() < limit:
        snap = snapshot(name)
        if snap is None or not snap["active"]:
            return snap
        time.sleep(0.02)
    return snapshot(name)
