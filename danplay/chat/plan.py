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
"""

import dataclasses
import logging
import re
import secrets
import threading
import time
from concurrent.futures import FIRST_COMPLETED, Future, ThreadPoolExecutor, wait
from dataclasses import dataclass, field
from typing import Any

from .. import library, names, playlists, youtube, ytdlp
from . import intake, safe

log = logging.getLogger(__name__)

# Cuanto vale una propuesta sin contestar, cuantas se recuerdan, cuanto se
# baja de una vez (tools.py lo reexporta) y cuantos huecos puede tener lo que
# se hace al terminar.
TTL = 1800
MAX_ENTRIES = 50
MAX_DOWNLOADS = 25
MAX_SLOTS = 50

# Filas por tarjeta: las mismas que se pueden bajar de una vez.
MAX_ROWS = MAX_DOWNLOADS
# Lo mas largo que puede ser cada cosa que se pide bajar (texto o enlace).
MAX_ITEM = 300
MAX_ALREADY = 60
# Consultas a YouTube: ligeras (segundos por respuesta, sin reintentos), dos a
# la vez como mucho en TODA la app, y lo contestado se recuerda 10 minutos.
LOOKUP_TIMEOUT = 6
PARALLEL = 2
CACHE_TTL = 600
MAX_CACHE = 500
# Lo que dura una cancion normal: fuera de esto, el video encontrado es dudoso.
SHORTEST = 60
LONGEST = 12 * 60

# Las que necesitan la aprobacion de la persona (las mismas que
# `chat.NEEDS_CONFIRMATION`; aqui no se importa tools para no hacer un ciclo).
TOOLS = ("download_music", "delete_song", "delete_playlist")

# Por que ya no se puede aceptar una propuesta (lo lee la persona).
GONE_TEXT = {
    "used": "Esa propuesta ya se atendió.",
    "superseded": "Hay una propuesta más nueva: usa esa.",
    "expired": "Esa propuesta caducó: pídemela otra vez.",
    "unknown": "No conozco esa propuesta (¿se reinició la app?): pídemela otra vez.",
}

NO_LISTS = "no bajo listas enteras desde una lista pegada"
NOT_YOUTUBE = "no es un enlace de YouTube"
NOT_A_TITLE = "eso no es un título"
NOTHING = "no hay nada que buscar"
NOT_FOUND = "no la encontré en YouTube"
NOT_ALIKE = "el título no se parece a lo pedido"

# Por que se dejo de buscar el resto del lote.
_CUT_TEXT = {
    "rate": "YouTube pide esperar (demasiadas consultas); no busqué el resto",
    "bot": "YouTube pide verificar que no eres un robot; no busqué el resto",
    "network": "no hay conexión con YouTube; no busqué el resto",
    "error": "no pude terminar de buscar",
    "canceled": "búsqueda parada",
}
# Los cortes que dejan el plan en «error» si no se encontro nada.
_BLOCKING = frozenset({"rate", "bot", "network", "unavailable", "error"})

# El reloj de las caducidades (las pruebas lo cambian).
_now = time.monotonic


class Gone(Exception):
    """La propuesta ya no se puede aceptar: `state` dice por que (410)."""

    def __init__(self, state: str, message: str = ""):
        self.state = state
        super().__init__(message or GONE_TEXT.get(state, GONE_TEXT["unknown"]))


class NothingSelected(ValueError):
    """Con esas casillas no queda nada que bajar (la propuesta sigue viva)."""


class AfterError(ValueError):
    """Lo que se pide hacer al terminar no cuadra: no se registra ni se escribe."""


@dataclass
class _Row:
    """Una fila de la tarjeta: una cadena de `args.items` y lo que se sabe de ella.

    `item` es la cadena tal cual se registro (con ella casa el `after`);
    `target`, lo que se bajaria si no se resuelve (la URL canonica o el texto
    en forma de busqueda; vacio si se rechaza). `state`: buscando, encontrada,
    dudosa, no_encontrada, rechazada o sin_buscar.
    """

    item: str
    target: str
    kind: str
    reason: str = ""
    state: str = "sin_buscar"
    label: str = ""
    n: int | None = None
    hint: str = ""
    sticky: bool | None = None
    video: dict | None = None
    doubt: str = ""
    mine: str = ""


@dataclass
class _Search:
    """La busqueda en YouTube de una propuesta: se para con `cancel`."""

    cancel: threading.Event = field(default_factory=threading.Event)
    stop_kind: str = ""
    stop_reason: str = ""
    already: list = field(default_factory=list)
    trimmed: int = 0


@dataclass
class Entry:
    """Una propuesta registrada. `claim` devuelve una copia con `args` y
    `after` ya construidos desde el registro y filtrados por la seleccion."""

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
    rows: list[_Row] = field(default_factory=list, repr=False)
    search: _Search | None = field(default=None, repr=False)
    guard: dict = field(default_factory=dict, repr=False)


_ENTRIES: dict[str, Entry] = {}
_LOCK = threading.RLock()
_CACHE: dict[str, tuple[float, dict]] = {}
_CACHE_LOCK = threading.Lock()
# Dos consultas a YouTube a la vez en toda la app, sean de la tarjeta que sean.
_SLOTS = threading.BoundedSemaphore(PARALLEL)
_SKIPPED = {"ok": False, "reason": "", "skipped": True}


# ------------------------------------------------------------------ utiles


def _int(value) -> int | None:
    """Un entero de verdad (ni booleano ni texto), o None."""
    if isinstance(value, bool) or not isinstance(value, int):
        return None
    return value


def _n(value) -> int | None:
    n = _int(value)
    return n if n is not None and n >= 0 else None


def _short(e: BaseException) -> str:
    return safe.datum(str(e) or type(e).__name__, 160)


def _mmss(seconds: int) -> str:
    return f"{seconds // 60}:{seconds % 60:02d}"


def _plural(n: int, one: str, many: str) -> str:
    return one if n == 1 else many


# ---------------------------------------------------- que se pide bajar


_DANGEROUS = re.compile(
    r"(?i)^(?:javascript|vbscript|data|file|blob|about|ftps?|sftp|wss?|mailto|tel|"
    r"view-source|chrome|intent)\s*:"
)
_SEARCH_KEY = re.compile(r"(?i)^[a-z0-9]*search[a-z0-9]*\s*:")
_DOMAIN = re.compile(r"(?i)^(?:[a-z0-9-]+\.)+[a-z]{2,}$")


def _is_link(text: str) -> bool:
    """Si parece una direccion (de donde sea) y no un titulo."""
    if youtube.host_of(text) or "://" in text or text.startswith("//"):
        return True
    if _DANGEROUS.match(text):
        return True
    if any(c.isspace() for c in text):
        return False
    return bool(_DOMAIN.match(text)) or ("." in text and any(c in text for c in "/@\\?#"))


def _classify(item: str) -> tuple[str, str, str]:
    """(lo que se bajaria, tipo, por que no) de una cadena de `args.items`.

    Tipo: «video» (un enlace de UN video, en su forma canonica), «lista» (un
    enlace de lista: se rechaza), «otro» (algo que no es de YouTube: se
    rechaza) o «texto» (se busca en YouTube en forma R-NORM). Las URL nunca
    pasan por `search_form` (S4, F14).
    """
    canon = intake.clean_youtube_url(item)
    if canon.startswith(intake.WATCH):
        return canon, "video", ""
    if canon.startswith(intake.PLAYLIST):
        return "", "lista", NO_LISTS
    if _is_link(item):
        return "", "otro", NOT_YOUTUBE
    if _SEARCH_KEY.match(item):
        return "", "otro", NOT_A_TITLE
    form = names.search_form(item)
    if not form:
        return "", "texto", NOTHING
    return form, "texto", ""


def _items(args: dict) -> tuple[list[str], int]:
    """Las cadenas a bajar, saneadas: sin vacias ni repetidas (sin `.lower()`:
    los ids de video distinguen mayusculas), como mucho `MAX_ITEM` letras cada
    una y `MAX_DOWNLOADS` en total. Devuelve (cadenas, cuantas sobraron)."""
    raw = args.get("items")
    if isinstance(raw, str):
        raw = [raw]
    elif not isinstance(raw, (list, tuple)):
        raw = []
    if not raw and isinstance(args.get("query"), str):
        raw = [args["query"]]
    out: list[str] = []
    for x in raw:
        if not isinstance(x, (str, int)) or isinstance(x, bool):
            continue
        s = str(x).strip()[:MAX_ITEM].strip()
        if s and s not in out:
            out.append(s)
    return out[:MAX_DOWNLOADS], max(0, len(out) - MAX_DOWNLOADS)


def _item_key(value) -> str:
    return str(value).strip()[:MAX_ITEM].strip() if isinstance(value, str) else ""


def _apply_meta(rows: list[_Row], metas) -> None:
    """Lo que quien registra sabe de cada fila (de la lista pegada): numero,
    titulo legible, nota y si va marcada. Por `item` si lo trae; si no, por
    posicion."""
    if not isinstance(metas, list):
        return
    metas = metas[: MAX_ROWS * 4]
    by_item = {}
    for m in metas:
        if isinstance(m, dict) and _item_key(m.get("item")):
            by_item.setdefault(_item_key(m.get("item")), m)
    for i, row in enumerate(rows):
        m = by_item.get(row.item)
        if m is None and i < len(metas) and isinstance(metas[i], dict):
            m = None if _item_key(metas[i].get("item")) else metas[i]
        if not m:
            continue
        row.n = _n(m.get("n"))
        row.label = safe.datum(m.get("label") or m.get("title") or "", 120)
        row.hint = safe.datum(m.get("note") or "", 160)
        if isinstance(m.get("checked"), bool):
            row.sticky = m["checked"]


def _already(raw) -> list[dict]:
    """Lo que ya estaba en la biblioteca, para enseñarlo en la tarjeta."""
    out = []
    for x in raw if isinstance(raw, list) else []:
        if len(out) >= MAX_ALREADY:
            break
        if not isinstance(x, dict):
            continue
        song = x.get("song") if isinstance(x.get("song"), dict) else None
        sid = _int(song.get("id")) if song else None
        out.append(
            {
                "n": _n(x.get("n")),
                "title": safe.datum(x.get("title") or "", 120),
                "song": (
                    {
                        "id": sid,
                        "artist": safe.datum(song.get("artist") or "", 80),
                        "title": safe.datum(song.get("title") or "", 120),
                    }
                    if song is not None and sid is not None
                    else None
                ),
            }
        )
    return out


# ---------------------------------------------------- lo que se hace al terminar


def _check_after(after, items: list[str]) -> dict:
    """El `after` de `register`, comprobado contra el registro (S1, S2).

    Forma: {"playlist": {"id", "name"}, "slots": [{"n", "id"} | {"n", "item"}]}.
    La lista tiene que existir y llamarse asi (R-NORM); como mucho `MAX_SLOTS`
    huecos; cada `id` una cancion de verdad; cada `item` una de las cadenas de
    `args.items`. Lo repetido se queda una vez. Lo que no cuadre: `AfterError`.
    """
    if not isinstance(after, dict) or set(after) - {"playlist", "slots"}:
        raise AfterError("el «después» no tiene la forma esperada")
    pl = after.get("playlist")
    if not isinstance(pl, dict) or set(pl) - {"id", "name"}:
        raise AfterError("el «después» no dice a qué lista")
    pid, name = _int(pl.get("id")), pl.get("name")
    if pid is None or not isinstance(name, str) or not name.strip() or len(name) > 200:
        raise AfterError("el «después» no dice a qué lista")
    current = playlists.by_id(pid)
    if current is None:
        raise AfterError("no existe esa lista")
    if names.search_form(current["name"]) != names.search_form(name):
        raise AfterError("esa lista no se llama así")
    slots = after.get("slots")
    if not isinstance(slots, list) or not slots or len(slots) > MAX_SLOTS:
        raise AfterError(f"el «después» tiene que llevar entre 1 y {MAX_SLOTS} huecos")
    wanted = set(items)
    out: list[dict] = []
    seen_ids: set[int] = set()
    seen_items: set[str] = set()
    for s in slots:
        if not isinstance(s, dict) or set(s) - {"n", "id", "item"} or ("id" in s) == ("item" in s):
            raise AfterError("un hueco del «después» no tiene la forma esperada")
        if s.get("n") is not None and _n(s.get("n")) is None:
            raise AfterError("un hueco del «después» tiene un número raro")
        n = _n(s.get("n"))
        if "id" in s:
            sid = _int(s["id"])
            if sid is None:
                raise AfterError("un hueco del «después» tiene un id raro")
            if sid not in seen_ids:
                seen_ids.add(sid)
                out.append({"n": n, "id": sid})
            continue
        item = _item_key(s["item"])
        if not item or item not in wanted:
            raise AfterError("un hueco del «después» pide algo que no se va a bajar")
        if item not in seen_items:
            seen_items.add(item)
            out.append({"n": n, "item": item})
    alive = set(playlists.existing_ids(sorted(seen_ids)))
    if seen_ids - alive:
        raise AfterError("el «después» nombra canciones que no existen")
    if not seen_items:
        raise AfterError("el «después» no lleva nada que bajar")
    return {"playlist": {"id": pid, "name": current["name"]}, "slots": out}


def _after_spec(after) -> dict:
    """El `after` que recibe `apply_after` (el de `claim`), con sus topes."""
    if not isinstance(after, dict):
        raise AfterError("no hay lista a la que añadir")
    pl = after.get("playlist")
    pid = _int(pl.get("id")) if isinstance(pl, dict) else None
    name = pl.get("name") if isinstance(pl, dict) else None
    if pid is None or not isinstance(name, str) or not name.strip():
        raise AfterError("no hay lista a la que añadir")
    slots = after.get("slots")
    if not isinstance(slots, list) or len(slots) > MAX_SLOTS:
        raise AfterError(f"el «después» tiene que llevar como mucho {MAX_SLOTS} huecos")
    clean: list[dict] = []
    for s in slots:
        if not isinstance(s, dict) or ("id" in s) == ("item" in s):
            raise AfterError("un hueco del «después» no tiene la forma esperada")
        if "id" in s:
            sid = _int(s["id"])
            if sid is None:
                raise AfterError("un hueco del «después» tiene un id raro")
            clean.append({"n": _n(s.get("n")), "id": sid})
            continue
        item = _item_key(s["item"])
        if not item:
            raise AfterError("un hueco del «después» no dice qué se bajó")
        label = safe.datum(s.get("label") or item, 120)
        clean.append({"n": _n(s.get("n")), "item": item, "label": label})
    return {
        "playlist": {"id": pid, "name": name},
        "slots": clean,
        "skipped": _marks(after.get("skipped"), with_reason=False),
        "failed": _marks(after.get("failed"), with_reason=True),
    }


def _marks(raw, *, with_reason: bool) -> list[dict]:
    """Los huecos que `claim` ya dejo fuera (casillas quitadas, rechazados)."""
    out = []
    for x in raw if isinstance(raw, list) else []:
        if len(out) >= MAX_SLOTS:
            break
        if not isinstance(x, dict):
            continue
        item = safe.datum(x.get("item") or "", MAX_ITEM)
        mark = {"n": _n(x.get("n")), "item": item, "label": safe.datum(x.get("label") or item, 120)}
        if with_reason:
            mark["reason"] = safe.datum(x.get("reason") or "", 160)
        out.append(mark)
    return out


# ------------------------------------------------------------ el registro


def _download_entry(args: dict, plan, after) -> Entry:
    items, trimmed = _items(args)
    if not items:
        raise ValueError("no hay nada que bajar")
    rows = []
    for item in items:
        target, kind, reason = _classify(item)
        state = "rechazada" if reason else "sin_buscar"
        rows.append(_Row(item=item, target=target, kind=kind, reason=reason, state=state))
    usable = [r for r in rows if not r.reason]
    if not usable:
        raise ValueError(rows[0].reason)
    quality = args.get("quality")
    clean = {
        "items": items,
        "quality": quality if quality in ("high", "medium", "variable") else None,
        "file_it": args.get("file_it", True) is not False,
        # solo un `true` de verdad: «"false"» como texto tambien es verdadero
        "force": args.get("force") is True,
    }
    checked_after = _check_after(after, items) if after is not None else None
    search = None
    if plan is not None:
        if not isinstance(plan, dict):
            raise ValueError("el plan tiene que ser un diccionario")
        extra = _n(plan.get("trimmed")) or 0
        search = _Search(already=_already(plan.get("already")), trimmed=trimmed + extra)
        _apply_meta(rows, plan.get("rows"))
        for r in rows:
            if r.state == "sin_buscar":
                r.state = "buscando"
    n = len(usable)
    summary = f"Descargar {n} {_plural(n, 'tema', 'temas')} de YouTube"
    if clean["force"]:
        summary += " aunque ya los tengas"
    if checked_after is not None:
        k = sum(1 for s in checked_after["slots"] if "item" in s)
        where = safe.datum(checked_after["playlist"]["name"], 60)
        effect = (
            f"Al terminar: la añado a «{where}»"
            if k == 1
            else f"Al terminar: añado {k} a «{where}»"
        )
    elif clean["file_it"]:
        effect = "Al terminar: quedan en tu biblioteca"
    else:
        effect = "Al terminar: quedan en Entrada/ sin archivar"
    return Entry(
        id="",
        tool="download_music",
        args=clean,
        summary=summary,
        effect=effect,
        after=checked_after,
        trimmed=trimmed,
        rows=rows,
        search=search,
    )


def _delete_song_entry(args: dict) -> Entry:
    sid = _int(args.get("id"))
    if sid is None and isinstance(args.get("id"), str) and args["id"].strip().isdigit():
        sid = int(args["id"].strip())
    song = library.by_id(sid) if sid is not None else None
    if not song or sid is None:
        raise ValueError("no existe esa canción")
    who = " - ".join(x for x in (song.get("artist"), song.get("title")) if x) or song.get("file")
    return Entry(
        id="",
        tool="delete_song",
        args={"id": sid},
        summary=f"Mandar «{safe.datum(who or sid, 80)}» a la papelera",
        effect="Sale de la biblioteca y el archivo va a la papelera del sistema",
        guard={"path": song.get("path")},
    )


def _delete_playlist_entry(args: dict) -> Entry:
    name = str(args.get("name") or args.get("playlist") or "").strip()
    pl = playlists.by_name(name) if name else None
    if pl is None:
        raw = args.get("playlist_id", args.get("id"))
        pid = _int(raw)
        if pid is None and isinstance(raw, str) and raw.strip().isdigit():
            pid = int(raw.strip())
        pl = playlists.by_id(pid) if pid is not None else None
    if pl is None:
        raise ValueError("no existe esa lista")
    n = int(pl.get("n") or 0)
    return Entry(
        id="",
        tool="delete_playlist",
        args={"playlist_id": pl["id"], "name": pl["name"]},
        summary=f"Borrar la lista «{safe.datum(pl['name'], 60)}» ({n} {_plural(n, 'tema', 'temas')})",
        effect="Las canciones siguen en tu biblioteca",
        guard={"form": names.search_form(pl["name"])},
    )


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
    tool = str(tool or "")
    if tool not in TOOLS:
        raise ValueError(f"«{tool}» no necesita confirmación")
    if not isinstance(args, dict):
        raise ValueError("los argumentos tienen que ser un diccionario")
    if tool == "download_music":
        entry = _download_entry(args, plan, after)
    elif plan is not None or after is not None:
        raise ValueError("solo una descarga lleva plan o «después»")
    elif tool == "delete_song":
        entry = _delete_song_entry(args)
    else:
        entry = _delete_playlist_entry(args)
    entry.chat_id = chat_id
    with _LOCK:
        entry.at = _now()
        _sweep(entry.at)
        entry.id = secrets.token_urlsafe(9)
        while entry.id in _ENTRIES:
            entry.id = secrets.token_urlsafe(9)
        for other in _ENTRIES.values():
            if other.state == "live" and other.tool == tool:
                other.state = "superseded"
                _halt(other)
        _ENTRIES[entry.id] = entry
        _evict()
        searching = entry.search is not None and any(r.state == "buscando" for r in entry.rows)
        out = {"id": entry.id, "tool": tool, "summary": entry.summary, "effect": entry.effect}
        if entry.search is not None:
            out["plan_state"] = _plan_state(entry)
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


def _target_changed(entry: Entry) -> str:
    """Si lo que se iba a borrar ya no es lo mismo (los ids se reutilizan)."""
    if entry.tool == "delete_song":
        song = library.by_id(entry.args["id"])
        if not song or song.get("path") != entry.guard.get("path"):
            return "Esa canción ya no está o ya no es la misma."
    elif entry.tool == "delete_playlist":
        pl = playlists.by_id(entry.args["playlist_id"])
        if not pl or names.search_form(pl["name"]) != entry.guard.get("form"):
            return "Esa lista ya no está o ya no es la misma."
    return ""


def _label(row: _Row) -> str:
    return row.label or (row.video or {}).get("title") or safe.datum(row.item, 120)


def _selected(entry: Entry, selection) -> tuple[dict, dict | None]:
    """`args` y `after` de una descarga, construidos desde el registro.

    `selection` son indices de filas; sin ella, las marcadas por defecto. Una
    fila resuelta se baja por el enlace del video encontrado (lo que la
    persona vio); una sin resolver, por su enlace o por su texto; una
    rechazada, nunca. En el `after` cada `item` pasa a ser la cadena que de
    verdad se baja (con ella llega `requested`), y lo no marcado va a
    `skipped`.
    """
    rows = entry.rows
    if selection is None:
        chosen = [i for i, r in enumerate(rows) if _checked(r)]
    else:
        given = selection if isinstance(selection, (list, tuple, set, frozenset)) else ()
        picked = {i for i in given if _int(i) is not None and 0 <= i < len(rows)}
        chosen = sorted(picked)
    items: list[str] = []
    target_of: dict[str, str] = {}
    refused: dict[str, str] = {}
    for i in chosen:
        r = rows[i]
        url = (r.video or {}).get("url") or ""
        target = url or r.target
        if r.state == "rechazada" or not target:
            refused[r.item] = r.reason or NOT_YOUTUBE
            continue
        target_of[r.item] = target
        if target not in items:
            items.append(target)
    if not items:
        raise NothingSelected("no hay nada marcado que se pueda bajar")
    args = {**entry.args, "items": items}
    if entry.after is None:
        return args, None
    by_item = {r.item: r for r in rows}
    slots, skipped, failed = [], [], []
    for s in entry.after["slots"]:
        if "id" in s:
            slots.append(dict(s))
            continue
        item = s["item"]
        row = by_item.get(item)
        label = _label(row) if row else safe.datum(item, 120)
        if item in target_of:
            slots.append({"n": s["n"], "item": target_of[item], "label": label})
        elif item in refused:
            failed.append({"n": s["n"], "item": item, "label": label, "reason": refused[item]})
        else:
            skipped.append({"n": s["n"], "item": item, "label": label})
    after = {
        "playlist": dict(entry.after["playlist"]),
        "slots": slots,
        "skipped": skipped,
        "failed": failed,
    }
    return args, after


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
            args, after = _selected(entry, selection)
        else:
            changed = _target_changed(entry)
            if changed:
                entry.state = "expired"
                raise Gone("expired", changed)
            args, after = dict(entry.args), None
        entry.state = "used"
        _halt(entry)
        return dataclasses.replace(entry, args=args, after=after, rows=[], search=None, guard={})


# ------------------------------------------------------- el plan, buscando


def _cache_key(row: _Row) -> str:
    # las URL nunca pasan por R-NORM; el texto ya va en su forma de busqueda
    return ("v:" if row.kind == "video" else "q:") + row.target


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


def _video(first: dict) -> dict | None:
    """El video encontrado, con su enlace canonico (o None si no lo tiene)."""
    vid = str(first.get("id") or "")
    url = intake.clean_youtube_url(intake.WATCH + vid) if vid else ""
    if not url.startswith(intake.WATCH):
        url = intake.clean_youtube_url(str(first.get("url") or ""))
    if not url.startswith(intake.WATCH):
        return None
    try:
        duration = max(0, int(float(first.get("duration") or 0)))
    except (TypeError, ValueError, OverflowError):
        duration = 0
    return {
        "url": url,
        "title": safe.datum(first.get("title") or "", 120),
        "channel": safe.datum(first.get("channel") or "", 80),
        "duration": duration,
    }


def _doubt(asked: str, first: dict, duration: int) -> str:
    """Por que el video encontrado puede no ser lo pedido, o "".

    Menos de la mitad de las palabras pedidas (R-NORM, ñ como n) en el titulo
    o el canal, o una duracion que no es la de una cancion.
    """
    want = [names.fold_enye(t) for t in names.search_tokens(asked)]
    if want:
        found = f"{first.get('title') or ''} {first.get('channel') or ''}"
        have = {names.fold_enye(t) for t in names.search_tokens(found)}
        if 2 * sum(1 for t in want if t in have) < len(want):
            return NOT_ALIKE
    if duration and duration < SHORTEST:
        return f"dura {_mmss(duration)} (muy corta)"
    if duration > LONGEST:
        return f"dura {_mmss(duration)} (muy larga)"
    return ""


def _mine(title: str) -> str:
    """La cancion de la biblioteca que ya es esa, o "" (biblioteca primero)."""
    try:
        found = youtube.already_in_library(title)
    except Exception:
        log.debug("no pude mirar si ya la tienes", exc_info=True)
        return ""
    if not found:
        return ""
    s = found[0]
    who = " - ".join(x for x in (s.get("artist"), s.get("title")) if x) or s.get("file") or ""
    return safe.datum(who, 80)


def _settle_row(entry: Entry, i: int, res: dict) -> None:
    """Apunta lo que contesto YouTube para la fila `i` (si sigue buscando)."""
    items = res.get("items") if res.get("ok") else None
    first = items[0] if isinstance(items, list) and items and isinstance(items[0], dict) else None
    video = _video(first) if first is not None else None
    if first is None or video is None:
        reason = NOT_FOUND
        if not res.get("ok") and res.get("reason"):
            reason = safe.datum(str(res["reason"]), 120)
        with _LOCK:
            row = entry.rows[i]
            if row.state == "buscando":
                row.state, row.reason = "no_encontrada", reason
        return
    with _LOCK:
        row = entry.rows[i]
        if row.state != "buscando":
            return
        asked = row.label or (row.target if row.kind == "texto" else "")
    doubt = _doubt(asked, first, video["duration"])
    mine = _mine(str(first.get("title") or ""))
    with _LOCK:
        if row.state != "buscando":
            return
        row.video, row.doubt, row.mine = video, doubt, mine
        row.state = "dudosa" if doubt else "encontrada"


def _stop(entry: Entry, kind: str, reason: str) -> None:
    """Para la busqueda: lo que aun estaba buscando queda «sin_buscar»."""
    with _LOCK:
        search = entry.search
        if search is None:
            return
        search.cancel.set()
        waiting = [r for r in entry.rows if r.state == "buscando"]
        if waiting and not search.stop_kind:
            search.stop_kind, search.stop_reason = kind, reason
        for r in waiting:
            r.state, r.reason = "sin_buscar", search.stop_reason or reason


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
        todo = [
            (i, r.target, _cache_key(r)) for i, r in enumerate(entry.rows) if r.state == "buscando"
        ]
    pool = ThreadPoolExecutor(max_workers=PARALLEL, thread_name_prefix="danplay-plan")
    blocked = threading.Event()
    try:
        futures = {
            pool.submit(_lookup, query, key, search.cancel, blocked): i for i, query, key in todo
        }
        pending = set(futures)
        while pending and not search.cancel.is_set():
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


def _checked(row: _Row) -> bool:
    if row.state == "rechazada":
        return False
    if row.sticky is not None:
        return row.sticky
    return row.state not in ("dudosa", "no_encontrada")


def _badge(row: _Row) -> str:
    if row.state == "rechazada":
        return "lista entera" if row.kind == "lista" else "no vale"
    if row.state == "no_encontrada":
        return "no encontrada"
    if row.state == "dudosa":
        return "no se parece"
    if row.state == "sin_buscar":
        return "sin comprobar"
    return "ya la tienes" if row.mine else ""


def _note(row: _Row) -> str:
    parts = [row.hint]
    if row.state in ("rechazada", "no_encontrada", "sin_buscar"):
        parts.append(row.reason)
    elif row.state == "dudosa":
        parts.append(row.doubt)
    if row.mine:
        parts.append(f"ya la tienes: «{row.mine}»")
    return " · ".join(p for p in parts if p)


def _row_view(i: int, row: _Row) -> dict:
    video = row.video or {}
    return {
        "i": i,
        "n": row.n,
        "query": safe.datum(row.label or row.item, 120),
        "kind": row.kind,
        # «ver»: SOLO el enlace canonico de un video
        "url": video.get("url") or (row.target if row.kind == "video" else ""),
        "title": video.get("title", ""),
        "channel": video.get("channel", ""),
        "duration": video.get("duration", 0),
        "state": row.state,
        "checked": _checked(row),
        "badge": _badge(row),
        "note": _note(row),
    }


def _plan_state(entry: Entry) -> str:
    rows = entry.rows
    if any(r.state == "buscando" for r in rows):
        return "buscando"
    resolved = [r for r in rows if r.state in ("encontrada", "dudosa")]
    if len(resolved) == len(rows):
        return "listo"
    search = entry.search
    blocked = search is not None and search.stop_kind in _BLOCKING
    if not resolved and (blocked or all(r.state == "rechazada" for r in rows)):
        return "error"
    return "parcial"


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
        rows = [_row_view(i, r) for i, r in enumerate(entry.rows)]
        unresolved = [
            {"i": i, "n": r.n, "query": safe.datum(r.label or r.item, 120), "reason": r.reason}
            for i, r in enumerate(entry.rows)
            if r.state in ("no_encontrada", "rechazada", "sin_buscar")
        ]
        return {
            "state": _plan_state(entry),
            "items": rows,
            "already": [dict(a) for a in entry.search.already],
            "unresolved": unresolved,
            "effect": entry.effect,
            "trimmed": entry.search.trimmed,
            "reason": entry.search.stop_reason,
        }


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
        return {"ok": True, "state": _plan_state(entry)}


# ------------------------------------------------- al terminar la descarga


def _songs_from(results) -> dict[str, tuple[int | None, str]]:
    """{lo pedido: (cancion que quedo, o None, y por que no)} de `run_many`.

    Bajada: su `id`. Ya la tenias: la primera de `matches`. Si la misma
    peticion dio varias, gana la primera que dejo cancion.
    """
    got: dict[str, tuple[int | None, str]] = {}
    for r in results if isinstance(results, list) else []:
        if not isinstance(r, dict):
            continue
        asked = _item_key(r.get("requested"))
        if not asked:
            continue
        sid = None
        if r.get("ok"):
            sid = _int(r.get("id"))
        elif r.get("already_there"):
            matches = r.get("matches")
            first = matches[0] if isinstance(matches, list) and matches else None
            sid = _int(first.get("id")) if isinstance(first, dict) else None
        if sid is not None:
            if got.get(asked, (None, ""))[0] is None:
                got[asked] = (sid, "")
            continue
        if asked in got:
            continue
        if r.get("canceled"):
            why = "cancelada"
        elif r.get("ok"):
            why = "se bajó pero no entró en la biblioteca"
        else:
            why = safe.datum(str(r.get("reason") or "falló"), 160)
        got[asked] = (None, why)
    return got


def _anchor(
    k: int, slots: list[dict], slot_song: dict[int, int], final: list[int], in_list: set[int]
) -> int:
    """Donde va el hueco `k`: tras el hueco anterior cuya cancion este AHORA
    en la lista; si no hay, antes del siguiente que este; si no, al final."""
    for j in range(k - 1, -1, -1):
        song = slot_song.get(j)
        if song is not None and song in in_list:
            return final.index(song) + 1
    for j in range(k + 1, len(slots)):
        song = slot_song.get(j)
        if song is not None and song in in_list:
            return final.index(song)
    return len(final)


def apply_after(after, results) -> dict:
    """Mete lo bajado en su sitio de la lista: una sola escritura, sin duplicar.

    `after` es el de `claim`: {"playlist": {"id", "name"}, "slots": [{"n",
    "id"} | {"n", "item"}], "skipped": [...], "failed": [...]}. Cada resultado
    de `run_many` casa con su hueco por `requested`. La lista tiene que seguir
    existiendo y llamarse igual (R-NORM; los ids de lista se reutilizan) o no
    se escribe nada. El orden final se calcula en memoria y se escribe con UNA
    llamada a `playlists.place`, bajo `playlists.LOCK`. Idempotente.

    Devuelve {"state": "done"|"error", "playlist": {id, name}, "added": [ids],
    "failed": [{n, item, label, reason}], "skipped": [{n, item, label}],
    "order": [ids]} (y "error" si `state` es "error").
    """
    try:
        spec = _after_spec(after)
    except AfterError as e:
        return {
            "state": "error",
            "error": str(e),
            "playlist": None,
            "added": [],
            "failed": [],
            "skipped": [],
            "order": [],
        }
    pl = spec["playlist"]
    out: dict[str, Any] = {
        "state": "done",
        "playlist": {"id": pl["id"], "name": pl["name"]},
        "added": [],
        "failed": list(spec["failed"]),
        "skipped": list(spec["skipped"]),
        "order": [],
    }
    slots = spec["slots"]
    canceled = youtube.canceled() or any(
        isinstance(r, dict) and r.get("canceled") for r in (results or [])
    )
    got = _songs_from(results)

    def fail(slot: dict, why: str) -> None:
        out["failed"].append(
            {"n": slot["n"], "item": slot["item"], "label": slot["label"], "reason": why}
        )

    try:
        with playlists.LOCK:
            now = playlists.by_id(pl["id"])
            if now is None or names.search_form(now["name"]) != names.search_form(pl["name"]):
                where = safe.datum(pl["name"], 60)
                why = (
                    f"ya no existe la lista «{where}»"
                    if now is None
                    else f"la lista «{where}» cambió de nombre"
                )
                out["state"], out["error"] = "error", why
                for s in slots:
                    if "item" in s:
                        fail(s, why)
                return out
            current = [s["id"] for s in playlists.songs(pl["id"], light=True)]
            slot_song: dict[int, int] = {}
            for k, s in enumerate(slots):
                if "id" in s:
                    slot_song[k] = s["id"]
                    continue
                sid, why = got.get(s["item"], (None, ""))
                if sid is None:
                    fail(s, why or ("cancelada" if canceled else "no se llegó a bajar"))
                else:
                    slot_song[k] = sid
            fresh = [slot_song[k] for k, s in enumerate(slots) if "item" in s and k in slot_song]
            alive = set(playlists.existing_ids(fresh))
            for k, s in enumerate(slots):
                if "item" in s and k in slot_song and slot_song[k] not in alive:
                    fail(s, "la canción ya no está en la biblioteca")
                    del slot_song[k]
            final = list(current)
            in_list = set(final)
            for k, s in enumerate(slots):
                if "item" not in s or k not in slot_song or slot_song[k] in in_list:
                    continue
                song = slot_song[k]
                final.insert(_anchor(k, slots, slot_song, final, in_list), song)
                in_list.add(song)
                out["added"].append(song)
            if out["added"]:
                placed = playlists.place(pl["id"], final)
                out["order"] = placed["order"]
                shown = set(placed["order"])
                out["added"] = [song for song in out["added"] if song in shown]
            else:
                out["order"] = current
    except Exception as e:
        log.warning("no pude completar la lista al terminar la descarga", exc_info=True)
        out["state"], out["error"] = "error", _short(e)
    return out


def _reset() -> None:
    """Olvida todo (para las pruebas): para las busquedas y vacia la cache."""
    with _LOCK:
        for e in list(_ENTRIES.values()):
            _halt(e)
        _ENTRIES.clear()
    with _CACHE_LOCK:
        _CACHE.clear()
