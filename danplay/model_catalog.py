# -*- coding: utf-8 -*-
"""El catalogo de modelos de todo el mundo, siempre al dia.

Los nombres de modelo caducan en meses (OpenAI cambio toda su nomenclatura,
Mistral retiro los alias «-latest», DeepSeek jubila «deepseek-chat»), asi que
dejarlos escritos en el codigo es condenar la app a enseñar modelos muertos.
En vez de eso se consulta models.dev: una base de datos abierta (codigo en
GitHub, mantenida por la comunidad) que publica UN json con 200+ proveedores
y miles de modelos, con el id exacto que usa cada proveedor, si el modelo
sabe usar herramientas, si acepta JSON y temperatura, precio, contexto,
fecha y si esta obsoleto.

Como se mantiene al dia:
  - se descarga con peticion condicional (ETag): si no ha cambiado, el
    servidor contesta 304 con cero bytes. Por eso se puede comprobar CADA VEZ
    que el usuario abre el apartado de IA sin coste, y traer la lista nueva
    en cuanto ellos la publiquen;
  - la copia vive en la carpeta de datos del usuario; sin internet se usa esa;
  - y la app lleva dentro una foto (`data/models-snapshot.json`, regenerada
    con scripts/actualizar-modelos.py) para el primer arranque sin red.

La lista de lo que el usuario PUEDE usar de verdad con su clave la da el
propio proveedor (`ai.list_models`); esto es el mapa completo, con precios y
capacidades, que se cruza con aquella.
"""
import json
import logging
import os
import re
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from . import config

log = logging.getLogger("danplay.models")

SOURCE = "https://models.dev/api.json"
CACHE = config.DATA_DIR / "models-catalog.json"
SNAPSHOT = Path(__file__).resolve().parent / "data" / "models-snapshot.json"
USER_AGENT = "DanPlay (+https://github.com/dani17r/danplay)"

# cada cuanto merece la pena volver a preguntar aunque nadie lo pida
BACKGROUND_MAX_AGE = 6 * 3600
# y como minimo entre dos comprobaciones seguidas (abrir y cerrar el modal)
MIN_INTERVAL = 60

# Lo que no es un modelo de conversacion: embeddings, voz, imagen, moderacion.
# models.dev trae las modalidades, pero no siempre; el nombre remata.
_NOT_CHAT = re.compile(r"embed|\btts\b|text-to-speech|whisper|transcri|speech|"
                       r"\bimage\b|imagen|dall-e|sora|veo\b|lyria|moderation|guard|"
                       r"rerank|\baudio\b|video|realtime|\bocr\b|colpali|clip\b",
                       re.IGNORECASE)

_lock = threading.Lock()
_loaded: dict | None = None
_refreshing = False


# ------------------------------------------------------------------ recorte

def _trim_model(mid: str, m: dict) -> dict | None:
    """Solo lo que la app usa, con nombres cortos y estables."""
    mods = m.get("modalities") or {}
    outs = mods.get("output") or []
    ins = mods.get("input") or []
    if outs and "text" not in outs:
        return None
    if ins and "text" not in ins:
        return None
    if _NOT_CHAT.search(mid) or _NOT_CHAT.search(str(m.get("name") or "")):
        return None
    cost = m.get("cost") or {}
    limit = m.get("limit") or {}

    def num(v):
        try:
            return float(v) if v is not None else None
        except (TypeError, ValueError):
            return None

    return {"id": mid, "name": str(m.get("name") or mid),
            "tools": bool(m.get("tool_call")),
            "json": m.get("structured_output"),
            "temperature": m.get("temperature"),
            "reasoning": bool(m.get("reasoning")),
            "cost_in": num(cost.get("input")), "cost_out": num(cost.get("output")),
            "context": limit.get("context"), "output": limit.get("output"),
            "released": str(m.get("release_date") or ""),
            "deprecated": str(m.get("status") or "") == "deprecated",
            "open": bool(m.get("open_weights")),
            "input": [x for x in ins if x != "text"],
            "family": str(m.get("family") or "")}


def _trim(raw: dict, only: set | None = None) -> dict:
    out = {}
    for pid, p in raw.items():
        if not isinstance(p, dict) or (only and pid not in only):
            continue
        models = {}
        for mid, m in (p.get("models") or {}).items():
            t = _trim_model(mid, m) if isinstance(m, dict) else None
            if t:
                models[mid] = t
        if models:
            out[pid] = {"name": str(p.get("name") or pid), "doc": p.get("doc") or "",
                        "api": p.get("api") or "", "models": models}
    return out


# ------------------------------------------------------------------- carga

def _read_json(path: Path) -> dict | None:
    try:
        if path.is_file():
            d = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(d, dict) and isinstance(d.get("providers"), dict):
                return d
    except (OSError, ValueError):
        log.warning("no se pudo leer %s", path, exc_info=True)
    return None


def load() -> dict:
    """El catalogo en memoria: la copia descargada, o la foto de la app."""
    global _loaded
    with _lock:
        if _loaded is None:
            _loaded = _read_json(CACHE) or _read_json(SNAPSHOT) or \
                {"source": "none", "providers": {}, "fetched_at": 0, "checked_at": 0}
        return _loaded


def forget() -> None:
    """Olvida lo cargado (para las pruebas)."""
    global _loaded
    with _lock:
        _loaded = None


def status() -> dict:
    d = load()
    n = sum(len(p["models"]) for p in d["providers"].values())
    return {"source": d.get("source", "none"), "providers": len(d["providers"]),
            "models": n, "fetched_at": d.get("fetched_at") or 0,
            "checked_at": d.get("checked_at") or 0, "error": d.get("error") or "",
            "refreshing": _refreshing}


# ----------------------------------------------------------------- descarga

def refresh(force: bool = False, timeout: float = 15) -> dict:
    """Consulta models.dev y actualiza la copia. Devuelve `status()`.

    Manda el ETag de la copia: si no ha cambiado, 304 y cero bytes. `force`
    salta el intervalo minimo, no el ETag (no hay motivo para bajar 4 MB que
    ya se tienen).
    """
    global _loaded, _refreshing
    current = load()
    now = time.time()
    if not force and now - float(current.get("checked_at") or 0) < MIN_INTERVAL:
        return status()
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    if current.get("etag") and current.get("source") == "models.dev":
        headers["If-None-Match"] = current["etag"]
    req = urllib.request.Request(SOURCE, headers=headers)
    _refreshing = True
    try:
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                raw = json.loads(r.read().decode("utf-8"))
                etag = r.headers.get("ETag", "")
        except urllib.error.HTTPError as e:
            if e.code == 304:
                # sin cambios: se anota en memoria y ya. Reescribir 2 MB de
                # cache para mover una fecha no merece la pena; al arrancar
                # de nuevo se vuelve a preguntar, que es lo que se quiere.
                with _lock:
                    current["checked_at"] = now
                    current["error"] = ""
                return status()
            raise
        data = {"source": "models.dev", "etag": etag, "fetched_at": now, "checked_at": now,
                "error": "", "providers": _trim(raw)}
        with _lock:
            _loaded = data
        _save(data)
        log.info("catalogo de modelos actualizado: %d proveedores", len(data["providers"]))
    except Exception as e:                                   # noqa: BLE001
        with _lock:
            current["checked_at"] = now
            current["error"] = _describe(e)
        log.info("models.dev no disponible: %s", current["error"])
    finally:
        _refreshing = False
    return status()


def _describe(e: Exception) -> str:
    t = str(e)
    low = t.lower()
    if "timed out" in low or "timeout" in low:
        return "models.dev tarda demasiado en responder"
    if "resolve" in low or "name or service" in low or "nodename" in low or "getaddrinfo" in low:
        return "sin conexion a internet"
    if "connection" in low or "network" in low or "unreachable" in low:
        return "no hay conexion con models.dev"
    return t[:120]


def _save(data: dict) -> None:
    try:
        CACHE.parent.mkdir(parents=True, exist_ok=True)
        tmp = CACHE.with_suffix(".tmp")
        tmp.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
        os.replace(tmp, CACHE)
    except OSError:
        log.warning("no se pudo guardar el catalogo de modelos", exc_info=True)


def refresh_in_background(max_age: float = BACKGROUND_MAX_AGE) -> bool:
    """Lanza una comprobacion si la copia tiene mas de `max_age` segundos.

    No bloquea: la interfaz pinta lo que hay y se entera de lo nuevo al
    volver a pedirlo. Devuelve si se ha lanzado algo.
    """
    d = load()
    if _refreshing or time.time() - float(d.get("checked_at") or 0) < max_age:
        return False
    threading.Thread(target=refresh, name="danplay-models", daemon=True).start()
    return True


# ----------------------------------------------------------------- consulta

def models_for(models_dev_id: str | None) -> list[dict]:
    """Los modelos de un proveedor, por fecha (los nuevos primero)."""
    if not models_dev_id:
        return []
    p = load()["providers"].get(models_dev_id)
    if not p:
        return []
    return sorted(p["models"].values(), key=lambda m: (m["released"], m["id"]), reverse=True)


def lookup(models_dev_id: str | None, model_id: str) -> dict | None:
    """La ficha de un modelo concreto, o None si no se conoce."""
    if not models_dev_id or not model_id:
        return None
    p = load()["providers"].get(models_dev_id)
    if not p:
        return None
    m = p["models"].get(model_id)
    if m:
        return m
    # OpenRouter y Hugging Face admiten sufijos (:free, :nitro, :groq)
    return p["models"].get(model_id.split(":")[0])


# Lo que suena a «escalon barato de una familia seria»: para identificar
# nombres de archivo sobra con eso, y es mucho mejor que el modelo mas
# barato de todos, que suele ser diminuto y no conoce a los artistas.
_CHEAP_TIER = re.compile(r"flash|lite|mini|nano|small|haiku|instant|turbo|fast", re.I)
_UNSTABLE = re.compile(r"exp\b|experimental|preview|beta|alpha|\brc\b|nightly|dev\b|:free$", re.I)
# especializados en otra cosa (programar, ver imagenes, matematicas): para
# hablar de musica hay mejores
_SPECIALIZED = re.compile(r"code|coder|coding|\bbuild\b|math|vision|\bvl\b|omni|guard|safety|"
                          r"\bagent|search|research|thinking", re.I)


def recommend(models: list[dict]) -> dict:
    """Con que empezar: un modelo para conversar y otro barato para lo demas.

    Conversar exige herramientas y que no este obsoleto ni sea experimental
    ni especializado en otra cosa; entre los que quedan, el mas nuevo que no
    sea caro. El rapido sale de la gama «flash/mini/lite» reciente: el mas
    nuevo de los que cuestan como mucho el doble que el mas barato. Con
    precios desconocidos (un servidor local) no hay criterio: vacio.
    """
    ok = [m for m in models if not m["deprecated"] and m["released"] and m["tools"]
          and m["cost_out"] is not None and not _UNSTABLE.search(m["id"])
          and not _SPECIALIZED.search(m["id"]) and not _SPECIALIZED.search(m["name"])]
    recent = sorted((m for m in ok if m["released"] >= _months_ago(18)),
                    key=lambda m: m["released"], reverse=True)
    chat = next((m for m in recent if m["cost_out"] <= 15), None) or (recent[0] if recent else None)
    cheap = [m for m in recent if _CHEAP_TIER.search(m["id"]) or _CHEAP_TIER.search(m["name"])] or recent
    fast = None
    if cheap:
        floor = min(m["cost_out"] for m in cheap)
        fast = next(m for m in cheap if m["cost_out"] <= max(floor * 2, 0.05))
    return {"chat": chat["id"] if chat else "", "fast": fast["id"] if fast else ""}


def _months_ago(n: int) -> str:
    t = time.gmtime(time.time() - n * 30 * 86400)
    return time.strftime("%Y-%m-%d", t)


def build_snapshot(path: Path = SNAPSHOT, only: set | None = None) -> int:
    """Descarga models.dev entera y deja la foto que viaja con la app.
    Devuelve cuantos modelos quedaron. Lo usa scripts/actualizar-modelos.py."""
    req = urllib.request.Request(SOURCE, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as r:
        raw = json.loads(r.read().decode("utf-8"))
    data = {"source": "snapshot", "fetched_at": time.time(), "checked_at": 0,
            "providers": _trim(raw, only)}
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n",
                    encoding="utf-8")
    return sum(len(p["models"]) for p in data["providers"].values())
