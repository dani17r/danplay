"""Proveedores de IA: el catalogo, los perfiles guardados y cual esta activo.

Todos hablan el mismo protocolo (el «chat completions» de OpenAI), asi que un
solo cliente sirve para OpenAI, Anthropic, Google, DeepInfra, OpenRouter, un
Ollama en este equipo o un servidor privado. Lo que cambia es la URL, si
piden clave, alguna cabecera y que parametros toleran. Eso es lo que guarda
este modulo; la conversacion con el modelo vive en `ai.py`.

Los datos de cada proveedor que el usuario haya configurado se recuerdan
TODOS (no solo el activo), en `~/.config/danplay/ai.json` con permisos 0600:
cambiar de Ollama a OpenRouter y volver no obliga a pegar la clave otra vez.

Lo hace Rust (library/src/ai/providers.rs, con el catalogo en
providers/list.rs); esto es la puerta de Python, con los mismos nombres.
Lo guardado se lee una vez por ruta (`PROFILES_FILE`, que las pruebas
cambian) y se recuerda hasta `reload`.
"""

from typing import Any, TypedDict

import danplay_core as _rust

from . import config

PROFILES_FILE = config.CONFIG_DIR / "ai.json"

# Los grupos, el catalogo y el orden de los gratuitos (en Rust: list.rs)
GROUPS: list[dict] = _rust.providers_groups()
CATALOG: list[dict] = _rust.providers_catalog()
BY_ID = {p["id"]: p for p in CATALOG}

# En que orden se prueban los gratuitos con «Probar gratis, sin clave».
FREE_ORDER: list[str] = _rust.providers_free_order()

# Variables de entorno que mandan sobre lo guardado. Para la linea de
# ordenes, las pruebas y quien prefiera no tocar la app.
ENV_PROVIDER, ENV_KEY, ENV_URL = "DANPLAY_AI_PROVIDER", "DANPLAY_AI_KEY", "DANPLAY_AI_BASE_URL"
ENV_MODEL, ENV_CHAT_MODEL = "DANPLAY_AI_MODEL", "DANPLAY_AI_CHAT_MODEL"

DEFAULT_TIMEOUT = 60  # segundos por peticion; los razonadores tardan
_KEEP = ("key", "model", "chat_model", "base_url", "fields", "headers", "extra", "timeout", "name")


def catalog() -> list[dict]:
    """El catalogo, tal cual, para la interfaz."""
    return [dict(p) for p in CATALOG]


def mask(key: str) -> str:
    """Una clave enseñable: principio y final, nunca entera."""
    return _rust.providers_mask(str(key or ""))


def _masked(value):
    """Cabeceras y parametros extra, enmascarados como la clave (ahi tambien
    puede haber secretos: un «Authorization: Bearer …», un «api-key»)."""
    return _rust.providers_masked(value)


def _unmask(new, old):
    """Lo que vuelve de la interfaz con valores enmascarados, completado con
    lo guardado (donde el valor es justo la mascara del guardado)."""
    return _rust.providers_unmask(new, old)


def _read() -> dict:
    """Lo guardado en ai.json, con las claves (solo para el nucleo)."""
    return _rust.providers_read(PROFILES_FILE)


def reload() -> None:
    """Olvida lo leido (para las pruebas y para cuando cambia el archivo)."""
    _rust.providers_reload()


def profiles() -> dict:
    """Los perfiles guardados, con las claves enmascaradas."""
    return _rust.providers_profiles(PROFILES_FILE)


def public(pid: str, prof: dict) -> dict:
    """Un perfil sin secretos: lo unico que sale del nucleo."""
    return _rust.providers_public(pid, dict(prof))


class Profile(TypedDict):
    """Un perfil RESUELTO: lo que necesita el cliente para hablar con el
    modelo (ver `resolve`). Lo guardado en ai.json es otra cosa: lo que
    escribio la persona, sin URL definitiva ni valores por defecto."""

    id: str
    provider: str
    name: str
    base_url: str
    key: str
    key_required: bool
    headers: dict[str, str]
    model: str
    chat_model: str
    extra: dict[str, Any]
    timeout: float
    quirks: dict[str, Any]
    models_dev: str | None
    local: bool
    # cuantas veces reintenta el SDK un fallo de conexion (None = lo normal)
    retries: int | None


def active_id() -> str:
    return _rust.providers_active_id(PROFILES_FILE)


def active() -> Profile | None:
    """El perfil activo RESUELTO (URL rellena, clave, cabeceras, modelos), o
    None si no hay ninguno. Las variables DANPLAY_AI_* mandan sobre el
    archivo, campo a campo."""
    return _rust.providers_active(PROFILES_FILE)  # type: ignore[return-value]


def resolve(pid: str, prof: dict) -> Profile | None:
    """Convierte lo guardado (o un borrador de la interfaz) en lo que necesita
    el cliente: URL definitiva, clave, cabeceras, modelos y tiempo limite."""
    return _rust.providers_resolve(pid, dict(prof))  # type: ignore[return-value]


def usable(prof: Profile | None) -> tuple[bool, str]:
    """Si con este perfil se puede llamar al modelo, y si no, por que."""
    return _rust.providers_usable(dict(prof) if prof else None)


def save_profile(data: dict, activate: bool = True) -> str:
    """Guarda (o completa) un perfil y devuelve su id.

    Lo que no venga se conserva: mandar solo `model` no borra la clave. Una
    clave vacia tampoco la borra (la caja de la clave llega vacia cuando el
    usuario no la ha vuelto a escribir); para quitarla esta `delete_profile`.
    Las cabeceras y los parametros extra que vuelven enmascarados (ver
    `public`) conservan tambien su valor guardado. `ValueError` si el
    proveedor no existe.
    """
    return _rust.providers_save_profile(PROFILES_FILE, dict(data), bool(activate))


def delete_profile(pid: str) -> None:
    _rust.providers_delete_profile(PROFILES_FILE, str(pid))


def activate(pid: str) -> None:
    """Lo activa (un local sin clave vale tal cual). `ValueError` si no esta
    configurado ni en el catalogo."""
    _rust.providers_activate(PROFILES_FILE, str(pid))


def fallback_enabled() -> bool:
    """Si, cuando el activo falla, se prueba con los demas configurados."""
    return _rust.providers_fallback_enabled(PROFILES_FILE)


def set_fallback(enabled: bool) -> None:
    _rust.providers_set_fallback(PROFILES_FILE, bool(enabled))


def budget() -> float:
    """Tope de gasto al mes en dolares (0 = sin aviso). Solo avisa: la IA no
    se corta sola, que a mitad de un ensayo seria peor."""
    return _rust.providers_budget(PROFILES_FILE)


def set_budget(value) -> None:
    _rust.providers_set_budget(PROFILES_FILE, value)


def fallbacks(active_id: str = "") -> list[Profile]:
    """Los demas perfiles usables, resueltos, en el orden en que se
    guardaron. Un respaldo sin clave (un local apagado) tambien cuenta: el
    cliente lo descarta en un momento si no responde."""
    return _rust.providers_fallbacks(PROFILES_FILE, active_id or "")  # type: ignore[return-value]


def with_saved_key(draft: dict) -> dict:
    """Un borrador de la interfaz, completado con lo guardado que no vuelve
    entero: la clave (la caja llega vacia: nunca se enseña) y los valores
    enmascarados de cabeceras y parametros extra.

    Solo si el borrador va AL MISMO SITIO que el perfil guardado. «Probar»
    con la URL cambiada (`{"provider": "openai", "base_url": "https://otro/v1"}`)
    le mandaba la clave de OpenAI a ese otro servidor.
    """
    return _rust.providers_with_saved_key(PROFILES_FILE, dict(draft or {}))
