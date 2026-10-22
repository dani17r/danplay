"""Confirmaciones del asistente, el plan de descarga y lo que se hace al terminar.

Lo que no tiene vuelta atras (descargar, borrar) no lo decide el modelo ni el
cliente. La herramienta REGISTRA aqui lo que propone; la interfaz solo puede
decir «si» a un id que genero el servidor, y lo que se ejecuta entonces sale
del registro, nunca de lo que mande el cliente (revisores S1, S2, T4, F2):

- `register` guarda la propuesta con su resumen y su efecto (escritos aqui),
  deja sin valor la anterior de la misma herramienta y, si trae `plan`, busca
  en YouTube lo que falta en un hilo aparte: la tarjeta sale al momento con
  «buscando» y se va rellenando sola (T3, S6, F12).
- `claim` la gasta UNA sola vez, bajo cerrojo, y devuelve lo que hay que bajar
  y lo que hay que hacer al terminar, filtrado por las casillas marcadas.
- `apply_after` mete lo bajado en su sitio de la lista con UNA escritura
  (`playlists.place`), dentro del turno de descarga (S11, F1, F9, T1, T2).

Todo vive en memoria, con cerrojo, tope de entradas, tope de tiempo y tope de
filas: tras un reinicio no queda nada y la tarjeta se ve caducada.

Lo que se decide lo hace Rust (library/src/assistant/plan.rs): sanear lo que
se propone y escribir su resumen y su efecto, las filas de la tarjeta (qué es
cada cosa que se pide, lo que contestó YouTube, lo que se ve), lo que se baja
con las casillas marcadas y dónde va cada canción en su lista. Aquí queda el
registro con su cerrojo y sus topes, los hilos que buscan en YouTube con su
caché, y las funciones de la base, que Rust llama.
"""

import dataclasses
import logging
import secrets
import threading
import time
from concurrent.futures import FIRST_COMPLETED, Future, ThreadPoolExecutor, wait
from dataclasses import dataclass, field
from typing import Any

import danplay_core as _rust

from .. import library, playlists, youtube, ytdlp
from . import safe

log = logging.getLogger(__name__)

_CONSTANTS = _rust.plan_constants()

# Cuanto vale una propuesta sin contestar, cuantas se recuerdan, cuanto se
# baja de una vez (tools.py lo reexporta) y cuantos huecos puede tener lo que
# se hace al terminar.
TTL = 1800
MAX_ENTRIES = 50
MAX_DOWNLOADS: int = _CONSTANTS["max_downloads"]
MAX_SLOTS: int = _CONSTANTS["max_slots"]

# Filas por tarjeta: las mismas que se pueden bajar de una vez.
MAX_ROWS: int = _CONSTANTS["max_rows"]
# Lo mas largo que puede ser cada cosa que se pide bajar (texto o enlace).
MAX_ITEM: int = _CONSTANTS["max_item"]
MAX_ALREADY: int = _CONSTANTS["max_already"]
# Consultas a YouTube: ligeras (segundos por respuesta, sin reintentos), dos a
# la vez como mucho en TODA la app, y lo contestado se recuerda 10 minutos.
LOOKUP_TIMEOUT = 6
PARALLEL = 2
CACHE_TTL = 600
MAX_CACHE = 500
# Lo que dura una cancion normal: fuera de esto, el video encontrado es dudoso.
SHORTEST: int = _CONSTANTS["shortest"]
LONGEST: int = _CONSTANTS["longest"]

# Las que necesitan la aprobacion de la persona (las mismas que
# `chat.NEEDS_CONFIRMATION`; aqui no se importa tools para no hacer un ciclo).
TOOLS: tuple[str, ...] = _CONSTANTS["tools"]

# Por que ya no se puede aceptar una propuesta (lo lee la persona).
GONE_TEXT: dict[str, str] = _CONSTANTS["gone"]

NO_LISTS: str = _CONSTANTS["no_lists"]
NOT_YOUTUBE: str = _CONSTANTS["not_youtube"]
NOT_A_TITLE: str = _CONSTANTS["not_a_title"]
NOTHING: str = _CONSTANTS["nothing"]
NOT_FOUND: str = _CONSTANTS["not_found"]
NOT_ALIKE: str = _CONSTANTS["not_alike"]

# Por que se dejo de buscar el resto del lote.
_CUT_TEXT: dict[str, str] = _CONSTANTS["cut"]

# El reloj de las caducidades (las pruebas lo cambian).
_now = time.monotonic


class Gone(Exception):
    """La propuesta ya no se puede aceptar: `state` dice por que (410)."""

    def __init__(self, state: str, message: str = ""):
        self.state = state
        super().__init__(message or GONE_TEXT.get(state, GONE_TEXT["unknown"]))


# Con esas casillas no queda nada que bajar (la propuesta sigue viva), y lo
# que se pide hacer al terminar no cuadra (no se registra ni se escribe). Las
# dos son `ValueError` y las lanza Rust.
NothingSelected = _rust.PlanNothingSelected
AfterError = _rust.PlanAfterError


@dataclass
class Entry:
    """Una propuesta registrada. `claim` devuelve una copia con `args` y
    `after` ya construidos desde el registro y filtrados por la seleccion.

    `card`: las filas de una descarga (las de la tarjeta, si lleva plan), en
    Rust; `search`: la señal para parar su busqueda en YouTube (None si no
    lleva plan)."""

    id: str
    tool: str
    args: dict
    summary: str
    effect: str
    after: dict | None = None
    chat_id: Any = None
    at: float = 0.0
    state: str = "live"
    trimmed: int = 0
    card: Any = field(default=None, repr=False)
    search: threading.Event | None = field(default=None, repr=False)
    guard: dict = field(default_factory=dict, repr=False)


_ENTRIES: dict[str, Entry] = {}
_LOCK = threading.RLock()
_CACHE: dict[str, tuple[float, dict]] = {}
_CACHE_LOCK = threading.Lock()
# Dos consultas a YouTube a la vez en toda la app, sean de la tarjeta que sean.
_SLOTS = threading.BoundedSemaphore(PARALLEL)
_SKIPPED = {"ok": False, "reason": "", "skipped": True}


def _short(e: BaseException) -> str:
    return safe.datum(str(e) or type(e).__name__, 160)


# ------------------------------------------------------------ el registro


def register(tool, args, *, plan=None, after=None, chat_id=None) -> dict:
    """Registra lo que el asistente propone y devuelve {id, tool, summary, effect}.

    El id, el resumen y el efecto los pone el servidor. `args` se sanea aqui
    (lo que sobre, incluidos un `after` o un `plan` metidos en `args`, se tira:
    S2). `after` solo lo fabrica el codigo (`fill_playlist`) y se comprueba
    (`AfterError` si no cuadra). Con `plan` (un dict, aunque vacio) se busca en
    YouTube lo que falta en un hilo aparte y el resultado lleva tambien
    `plan_state`. Deja sin valor (`superseded`) las propuestas vivas de la
    misma herramienta. `ValueError` si no hay nada que proponer.

    `plan` = {"rows": [{"item"?, "n", "label", "note", "checked"}] (alineadas
    con `args["items"]` o por `item`), "already": [{"n", "title", "song":
    {"id", "artist", "title"}}], "trimmed": int}.
    """
    draft = _rust.plan_draft(
        str(tool or ""),
        args,
        plan,
        after,
        library.by_id,
        playlists.by_id,
        playlists.by_name,
        playlists.existing_ids,
    )
    card = draft["card"]
    entry = Entry(
        id="",
        tool=draft["tool"],
        args=draft["args"],
        summary=draft["summary"],
        effect=draft["effect"],
        after=draft["after"],
        chat_id=chat_id,
        trimmed=draft["trimmed"],
        card=card,
        search=threading.Event() if card is not None and card.planned else None,
        guard=draft["guard"],
    )
    with _LOCK:
        entry.at = _now()
        _sweep(entry.at)
        entry.id = secrets.token_urlsafe(9)
        while entry.id in _ENTRIES:
            entry.id = secrets.token_urlsafe(9)
        for other in _ENTRIES.values():
            if other.state == "live" and other.tool == entry.tool:
                other.state = "superseded"
                _halt(other)
        _ENTRIES[entry.id] = entry
        _evict()
        searching = entry.search is not None and entry.card.searching()
        out = {"id": entry.id, "tool": entry.tool, "summary": entry.summary, "effect": entry.effect}
        if entry.search is not None:
            out["plan_state"] = entry.card.state()
    if searching:
        threading.Thread(target=_work, args=(entry,), name="danplay-plan", daemon=True).start()
    return out


def _sweep(now: float) -> None:
    """Caduca las vivas pasado `TTL` y olvida del todo lo que pase de 2 x `TTL`."""
    for key, e in list(_ENTRIES.items()):
        age = now - e.at
        if age > 2 * TTL:
            del _ENTRIES[key]
            _halt(e)
        elif e.state == "live" and age > TTL:
            e.state = "expired"
            _halt(e)


def _evict() -> None:
    """Por encima del tope, fuera las mas viejas (primero las ya gastadas)."""
    while len(_ENTRIES) > MAX_ENTRIES:
        victim = next((k for k, e in _ENTRIES.items() if e.state != "live"), None)
        if victim is None:
            victim = next(iter(_ENTRIES))
        _halt(_ENTRIES.pop(victim))


def _halt(entry: Entry) -> None:
    if entry.search is not None:
        _stop(entry, "canceled", _CUT_TEXT["canceled"])


def status(entry_id) -> dict:
    """{"state": live|used|superseded|expired|unknown, "tool", "summary"}."""
    with _LOCK:
        _sweep(_now())
        e = _ENTRIES.get(str(entry_id or ""))
        if e is None:
            return {"state": "unknown", "tool": "", "summary": ""}
        return {"state": e.state, "tool": e.tool, "summary": e.summary}


def claim(entry_id, selection=None) -> Entry:
    """Gasta la propuesta: UNA vez, bajo cerrojo, antes de hacer nada.

    Devuelve una copia con `args` y `after` construidos desde el registro (y,
    en una descarga, filtrados por `selection`, indices de filas del plan).
    `Gone` si no esta viva (o si lo que se iba a borrar ya no es lo mismo);
    `NothingSelected` si con esas casillas no queda nada que bajar (entonces
    sigue viva). Dos clics a la vez: uno la gasta y el otro recibe `Gone`.
    """
    with _LOCK:
        _sweep(_now())
        entry = _ENTRIES.get(str(entry_id or ""))
        if entry is None:
            raise Gone("unknown")
        if entry.state != "live":
            raise Gone(entry.state)
        if entry.tool == "download_music":
            args, after = entry.card.select(entry.args, entry.after, selection)
        else:
            # si lo que se iba a borrar ya no es lo mismo (los ids se reutilizan)
            changed = _rust.plan_target_changed(
                entry.tool, entry.args, entry.guard, library.by_id, playlists.by_id
            )
            if changed:
                entry.state = "expired"
                raise Gone("expired", changed)
            args, after = dict(entry.args), None
        entry.state = "used"
        _halt(entry)
        return dataclasses.replace(entry, args=args, after=after, card=None, search=None, guard={})


# ------------------------------------------------------- el plan, buscando


def _cache_get(key: str) -> dict | None:
    with _CACHE_LOCK:
        hit = _CACHE.get(key)
        if hit is None:
            return None
        if _now() - hit[0] >= CACHE_TTL:
            del _CACHE[key]
            return None
        return hit[1]


def _cache_put(key: str, value: dict) -> None:
    with _CACHE_LOCK:
        _CACHE.pop(key, None)
        _CACHE[key] = (_now(), value)
        while len(_CACHE) > MAX_CACHE:
            del _CACHE[next(iter(_CACHE))]


def _warm() -> str:
    """Carga yt-dlp y busca el motor de JS ANTES de abrir hilos (si no, cada
    hilo lo hacia a la vez). Devuelve por que no se puede buscar, o ""."""
    try:
        if ytdlp.module() is None:
            return youtube.unavailable_reason() or "falta yt-dlp"
        ytdlp.js_runtime()
    except Exception as e:
        log.info("no pude preparar yt-dlp para el plan", exc_info=True)
        return _short(e)
    return ""


def _lookup(
    query: str, key: str, cancel: threading.Event, blocked: threading.Event | None = None
) -> dict:
    """Una consulta ligera a YouTube (solo `youtube.info`), con cache y con
    el tope de dos a la vez en toda la app.

    `blocked` es la señal del lote: la consulta que recibe un 429, un «no eres
    un robot» o un fallo de red la levanta ella misma, y las que aun no han
    preguntado ya no preguntan. Antes solo se enteraba el hilo del plan al
    recoger el resultado (cada cuarto de segundo), y en un equipo lento el
    otro hilo ya habia lanzado varias consultas mas: justo lo que no hay que
    hacer cuando YouTube pide parar."""
    hit = _cache_get(key)
    if hit is not None:
        return hit

    def stop() -> bool:
        return cancel.is_set() or (blocked is not None and blocked.is_set())

    while not _SLOTS.acquire(timeout=0.25):
        if stop():
            return _SKIPPED
    try:
        if stop():
            return _SKIPPED
        try:
            res = youtube.info(query, 1, timeout=LOOKUP_TIMEOUT)
        except Exception as e:  # noqa: BLE001  (una consulta rara no tumba el lote)
            return {"ok": False, "reason": _short(e)}
    finally:
        _SLOTS.release()
    if not isinstance(res, dict):
        return {"ok": False, "reason": "YouTube no contestó"}
    if res.get("ok") or not youtube.failure_kind(str(res.get("reason") or "")):
        _cache_put(key, res)
    elif blocked is not None:
        blocked.set()
    return res


def _mine(title: str) -> str:
    """La cancion de la biblioteca que ya es esa, o "" (biblioteca primero)."""
    try:
        found = youtube.already_in_library(title)
    except Exception:
        log.debug("no pude mirar si ya la tienes", exc_info=True)
        return ""
    return _rust.plan_mine(found)


def _settle_row(entry: Entry, i: int, res: dict) -> None:
    """Apunta lo que contesto YouTube para la fila `i` (si sigue buscando)."""
    found = _rust.plan_answer(res)
    if isinstance(found, str):  # no se encontro, y por que
        with _LOCK:
            entry.card.miss(i, found)
        return
    with _LOCK:
        doubt = entry.card.doubt(i, found)
    if doubt is None:
        return
    mine = _mine(found.title)
    with _LOCK:
        entry.card.settle(i, found, doubt, mine)


def _stop(entry: Entry, kind: str, reason: str) -> None:
    """Para la busqueda: lo que aun estaba buscando queda «sin_buscar»."""
    with _LOCK:
        if entry.search is None:
            return
        entry.search.set()
        entry.card.stop(kind, reason)


def _result(fut: Future) -> dict:
    try:
        res = fut.result()
    except Exception as e:  # noqa: BLE001  (se cuenta como una fila sin resolver)
        return {"ok": False, "reason": _short(e)}
    return res if isinstance(res, dict) else {"ok": False, "reason": "YouTube no contestó"}


def _work(entry: Entry) -> None:
    """El hilo de una propuesta: busca sus filas en YouTube, dos a la vez.

    Corta el lote a la primera señal de 429, de «no eres un robot» o de red:
    seguir preguntando solo empeora las cosas. Al acabar o al cancelar, el
    grupo de hilos se suelta sin esperar y lo que no empezo no empieza.
    """
    search = entry.search
    if search is None:
        return
    problem = _warm()
    if problem:
        _stop(entry, "unavailable", safe.datum(problem, 160))
        return
    with _LOCK:
        remaining = entry.card.pending()
    pool = ThreadPoolExecutor(max_workers=PARALLEL, thread_name_prefix="danplay-plan")
    blocked = threading.Event()
    try:
        futures = {
            pool.submit(_lookup, query, key, search, blocked): i for i, query, key in remaining
        }
        pending = set(futures)
        while pending and not search.is_set():
            done, pending = wait(pending, timeout=0.25, return_when=FIRST_COMPLETED)
            cut = ""
            for fut in done:
                res = _result(fut)
                if res.get("skipped"):
                    continue
                kind = "" if res.get("ok") else youtube.failure_kind(str(res.get("reason") or ""))
                if kind:
                    cut = cut or kind
                else:
                    _settle_row(entry, futures[fut], res)
            if cut:
                _stop(entry, cut, _CUT_TEXT[cut])
                break
    except Exception:
        log.warning("la busqueda del plan de descarga fallo", exc_info=True)
        _stop(entry, "error", _CUT_TEXT["error"])
    finally:
        pool.shutdown(wait=False, cancel_futures=True)
        _stop(entry, "canceled", _CUT_TEXT["canceled"])


# ------------------------------------------------------- el plan, a la vista


def plan_view(entry_id) -> dict | None:
    """Lo que pinta la tarjeta (None si no hay plan con ese id).

    {"state": buscando|listo|parcial|error, "items": [{i, n, query, kind, url,
    title, channel, duration, state, checked, badge, note}], "already": [...],
    "unresolved": [{i, n, query, reason}], "effect", "trimmed", "reason"}.
    """
    with _LOCK:
        _sweep(_now())
        entry = _ENTRIES.get(str(entry_id or ""))
        if entry is None or entry.search is None:
            return None
        return entry.card.view(entry.effect)


def cancel(entry_id) -> dict | None:
    """Para la busqueda del plan (None si no hay plan con ese id).

    La propuesta NO se gasta: «Descargar» sigue valiendo con lo resuelto.
    Devuelve {"ok": True, "state": <estado del plan>}.
    """
    with _LOCK:
        _sweep(_now())
        entry = _ENTRIES.get(str(entry_id or ""))
        if entry is None or entry.search is None:
            return None
        _stop(entry, "canceled", _CUT_TEXT["canceled"])
        return {"ok": True, "state": entry.card.state()}


# ------------------------------------------------- al terminar la descarga


def apply_after(after, results) -> dict:
    """Mete lo bajado en su sitio de la lista: una sola escritura, sin duplicar.

    `after` es el de `claim`: {"playlist": {"id", "name"}, "slots": [{"n",
    "id"} | {"n", "item"}], "skipped": [...], "failed": [...]}. Cada resultado
    de `run_many` casa con su hueco por `requested`. La lista tiene que seguir
    existiendo y llamarse igual (R-NORM; los ids de lista se reutilizan) o no
    se escribe nada. El orden final se calcula en memoria y se escribe con UNA
    llamada a `playlists.place`, bajo `playlists.LOCK`. Idempotente.

    Devuelve {"state": "done"|"error", "playlist": {id, name}, "added": [ids],
    "placed": [{n, id}], "failed": [{n, item, label, reason}], "skipped":
    [{n, item, label}], "order": [ids]} (y "error" si `state` es "error").
    `placed` es cada hueco bajado que queda en la lista con su cancion, tambien
    si ya estaba: con eso la tabla del chat deja de decir «falta» en esa fila.
    """
    spec = _rust.plan_after_spec(after)
    if isinstance(spec, dict):  # no cuadra: no se escribe nada
        return spec
    canceled = youtube.canceled() or any(
        isinstance(r, dict) and r.get("canceled") for r in (results or [])
    )
    with playlists.LOCK:
        out, failure = _rust.plan_apply_after(
            spec,
            results,
            bool(canceled),
            playlists.by_id,
            playlists.songs,
            playlists.existing_ids,
            playlists.place,
        )
    if failure is not None:
        log.warning("no pude completar la lista al terminar la descarga", exc_info=failure)
    return out


def _reset() -> None:
    """Olvida todo (para las pruebas): para las busquedas y vacia la cache."""
    with _LOCK:
        for e in list(_ENTRIES.values()):
            _halt(e)
        _ENTRIES.clear()
    with _CACHE_LOCK:
        _CACHE.clear()
