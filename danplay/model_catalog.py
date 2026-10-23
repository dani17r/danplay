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
    con `danplay-server --models-snapshot`) para el primer arranque sin red.

La lista de lo que el usuario PUEDE usar de verdad con su clave la da el
propio proveedor (`ai.list_models`); esto es el mapa completo, con precios y
capacidades, que se cruza con aquella.

Lo hace Rust (library/src/ai/catalog.rs: recortar, guardar, cargar y
consultar); la descarga la sigue haciendo Python, con urllib (las pruebas
cortan la red ahi).
"""

import json
import logging
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

import danplay_core as _rust

from . import config

log = logging.getLogger(__name__)

SOURCE = "https://models.dev/api.json"
CACHE = config.DATA_DIR / "models-catalog.json"
SNAPSHOT = Path(__file__).resolve().parent / "data" / "models-snapshot.json"
USER_AGENT = "DanPlay (+https://github.com/dani17r/danplay)"

# cada cuanto merece la pena volver a preguntar aunque nadie lo pida
BACKGROUND_MAX_AGE = 6 * 3600
# y como minimo entre dos comprobaciones seguidas (abrir y cerrar el modal)
MIN_INTERVAL = 60

_refreshing = False


def is_chat(mid: str, name: str = "") -> bool:
    """Si por el nombre parece un modelo de conversacion (los /models de los
    proveedores mezclan embeddings, voz, imagen y video)."""
    return _rust.catalog_is_chat(str(mid), str(name or ""))


def _trim(raw: dict, only: set | None = None) -> dict:
    """Solo lo que la app usa, con nombres cortos y estables."""
    return _rust.catalog_trim(raw, sorted(only) if only else None)


def load() -> dict:
    """El catalogo en memoria: la copia descargada, o la foto de la app."""
    return _rust.catalog_load(CACHE, SNAPSHOT)


def forget() -> None:
    """Olvida lo cargado (para las pruebas)."""
    _rust.catalog_forget()


def status() -> dict:
    return _rust.catalog_status(CACHE, SNAPSHOT, _refreshing)


def refresh(force: bool = False, timeout: float = 15) -> dict:
    """Consulta models.dev y actualiza la copia. Devuelve `status()`.

    Manda el ETag de la copia: si no ha cambiado, 304 y cero bytes. `force`
    salta el intervalo minimo, no el ETag (no hay motivo para bajar 4 MB que
    ya se tienen).
    """
    global _refreshing
    current = _rust.catalog_meta(CACHE, SNAPSHOT)
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
                body = r.read()
                etag = r.headers.get("ETag", "")
        except urllib.error.HTTPError as e:
            if e.code == 304:
                # sin cambios: se anota en memoria y ya. Reescribir 2 MB de
                # cache para mover una fecha no merece la pena
                _rust.catalog_mark_checked(CACHE, SNAPSHOT, now, "")
                return status()
            raise
        try:
            # los bytes tal cual: Rust lo recorta al leerlo (pasarlo antes por
            # json.loads costaba mas de 100 MB que el proceso no devolvia)
            n = _rust.catalog_store(CACHE, SNAPSHOT, body, etag or "", now)
        except ValueError:
            # lo que no es un objeto JSON: que falle como fallaba (el mismo
            # error y el mismo aviso)
            n = _rust.catalog_store(
                CACHE, SNAPSHOT, json.loads(body.decode("utf-8")), etag or "", now
            )
        log.info("catalogo de modelos actualizado: %d proveedores", n)
    except Exception as e:  # noqa: BLE001
        why = _describe(e)
        _rust.catalog_mark_checked(CACHE, SNAPSHOT, now, why)
        log.info("models.dev no disponible: %s", why)
    finally:
        _refreshing = False
    return status()


def _describe(e: Exception) -> str:
    return _rust.catalog_describe(str(e))


def refresh_in_background(max_age: float = BACKGROUND_MAX_AGE) -> bool:
    """Lanza una comprobacion si la copia tiene mas de `max_age` segundos.

    No bloquea: la interfaz pinta lo que hay y se entera de lo nuevo al
    volver a pedirlo. Devuelve si se ha lanzado algo.
    """
    d = _rust.catalog_meta(CACHE, SNAPSHOT)
    if _refreshing or time.time() - float(d.get("checked_at") or 0) < max_age:
        return False
    threading.Thread(target=refresh, name="danplay-models", daemon=True).start()
    return True


def models_for(models_dev_id: str | None) -> list[dict]:
    """Los modelos de un proveedor, por fecha (los nuevos primero)."""
    if not models_dev_id:
        return []
    return _rust.catalog_models_for(CACHE, SNAPSHOT, str(models_dev_id))


def lookup(models_dev_id: str | None, model_id: str) -> dict | None:
    """La ficha de un modelo concreto, o None si no se conoce."""
    if not models_dev_id or not model_id:
        return None
    return _rust.catalog_lookup(CACHE, SNAPSHOT, str(models_dev_id), str(model_id))


def recommend(models: list[dict]) -> dict:
    """Con que empezar: un modelo para conversar y otro barato para lo demas.

    Conversar exige herramientas y que no este obsoleto ni sea experimental
    ni especializado en otra cosa; entre los que quedan, el mas nuevo que no
    sea caro. El rapido sale de la gama «flash/mini/lite» reciente: el mas
    nuevo de los que cuestan como mucho el doble que el mas barato. Con
    precios desconocidos (un servidor local) no hay criterio: vacio.
    """
    return _rust.catalog_recommend(list(models), time.time())


def build_snapshot(path: Path = SNAPSHOT, only: set | None = None) -> int:
    """Descarga models.dev entera y deja la foto que viaja con la app.
    Devuelve cuantos modelos quedaron. Lo usaba scripts/update-models.py;
    ahora la saca el de Rust (`library::ai::catalog::snapshot`)."""
    req = urllib.request.Request(SOURCE, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as r:
        raw = json.loads(r.read().decode("utf-8"))
    data = {
        "source": "snapshot",
        "fetched_at": time.time(),
        "checked_at": 0,
        "providers": _trim(raw, only),
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8"
    )
    return sum(len(p["models"]) for p in data["providers"].values())
