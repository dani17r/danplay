"""Proveedores de IA: el catalogo, los perfiles guardados, probarlos y lo que
gastan (docs/CONTRATO-INTERNO.md §5)."""

import logging
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Query

from ... import ai, config, library, model_catalog, providers
from ..models import AiActivateIn, AiBudgetIn, AiFallbackIn, AiProfileIn

log = logging.getLogger(__name__)

router = APIRouter()


@router.post("/api/settings/check-ai")
def check_ai():
    """Prueba el proveedor activo. Lo usa el boton «Probar» de Ajustes."""
    return ai.check()


def _ai_overview() -> dict:
    """Lo que necesita el modal de IA: catalogo, perfiles guardados (sin
    claves) y el estado del catalogo de modelos."""
    p = ai.profile()
    return {
        "catalog": providers.catalog(),
        "groups": providers.GROUPS,
        "profiles": providers.profiles(),
        "active": providers.active_id(),
        "active_profile": (
            {
                "id": p["id"],
                "provider": p["provider"],
                "name": p["name"],
                "model": p["model"],
                "chat_model": p["chat_model"],
                "base_url": p["base_url"],
                "local": p["local"],
            }
            if p
            else None
        ),
        "ai_enabled": config.AI_ENABLED,
        "ai_ready": ai.available(),
        "ai_reason": ai.unavailable_reason(),
        "fallback": providers.fallback_enabled(),
        "fallbacks": [
            {"id": f["id"], "name": f["name"], "chat_model": f["chat_model"]}
            for f in providers.fallbacks(providers.active_id())
        ],
        "catalog_status": model_catalog.status(),
    }


# Las rutas son `def` y no `async def`: FastAPI las corre en su grupo de
# hilos. Con `async def`, lo que hacian sin `await` (leer ai.json, cargar el
# catalogo de modelos de 2,5 MB, consultar la base) paraba el bucle entero y
# con el todas las demas peticiones.


@router.get("/api/ai/providers")
def ai_providers(refresh: Annotated[bool, Query()] = False):
    """Con `refresh`, espera a consultar models.dev (el boton «Actualizar»);
    si no, lo consulta en segundo plano si hace rato de la ultima vez. En
    los dos casos la peticion es condicional: sin cambios, cero bytes."""
    if refresh:
        model_catalog.refresh(force=True)
    else:
        model_catalog.refresh_in_background(max_age=model_catalog.MIN_INTERVAL)
    return _ai_overview()


@router.post("/api/ai/profile")
def ai_save_profile(body: Annotated[AiProfileIn, Body()]):
    data = body.model_dump(exclude_none=True)
    activate = data.pop("activate", True)
    try:
        pid = providers.save_profile(data, activate=activate)
    except ValueError as e:
        raise HTTPException(400, str(e)) from e
    ai.reset_client()
    out = _ai_overview()
    out["saved"] = pid
    return out


@router.delete("/api/ai/profile/{pid}")
def ai_delete_profile(pid: str):
    providers.delete_profile(pid)
    ai.reset_client()
    return _ai_overview()


@router.post("/api/ai/activate")
def ai_activate(body: Annotated[AiActivateIn, Body()]):
    try:
        providers.activate(body.id)
    except ValueError as e:
        raise HTTPException(400, str(e)) from e
    ai.reset_client()
    return _ai_overview()


@router.post("/api/ai/free")
def ai_free():
    """«Probar gratis, sin clave»: prueba los servicios gratuitos por orden y
    deja activo el primero que responda. Tarda lo que tarden en contestar."""
    r = ai.try_free()
    out = _ai_overview()
    out["free"] = r
    return out


@router.get("/api/ai/usage")
def ai_usage():
    """Lo que gasta la IA: hoy, este mes, en total y por proveedor (tokens y
    coste), y el presupuesto mensual si lo hay."""
    out = library.ai_usage_summary()
    out["budget"] = providers.budget()
    out["over_budget"] = bool(out["budget"] and out["month"]["cost"] > out["budget"])
    return out


@router.post("/api/ai/budget")
def ai_budget(body: Annotated[AiBudgetIn, Body()]):
    """Avisar al pasar de tantos dolares al mes (0 = sin aviso)."""
    providers.set_budget(body.dollars)
    return ai_usage()


@router.post("/api/ai/fallback")
def ai_fallback(body: Annotated[AiFallbackIn, Body()]):
    """Si, cuando el proveedor activo falla, se usan los demas configurados."""
    providers.set_fallback(body.enabled)
    ai.reset_client()
    return _ai_overview()


@router.post("/api/ai/check")
def ai_check(body: Annotated[AiProfileIn, Body()]):
    """Prueba lo que hay en el formulario SIN guardarlo: clave, URL, los dos
    modelos y si el de conversacion sabe usar herramientas."""
    draft = body.model_dump(exclude_none=True)
    draft.pop("activate", None)
    return ai.check(draft)


@router.post("/api/ai/models")
def ai_models(body: Annotated[AiProfileIn, Body()]):
    """Los modelos que ofrece ese proveedor con esa clave (su /models), mas
    los que conoce el catalogo aunque el proveedor no los liste."""
    draft = body.model_dump(exclude_none=True)
    draft.pop("activate", None)
    live = ai.list_models(draft)
    p = providers.BY_ID.get(body.provider) or providers.BY_ID["custom"]
    known = model_catalog.models_for(p["models_dev"])
    live["catalog"] = known
    live["suggest"] = dict(p["suggest"])
    recommended = model_catalog.recommend(known)
    for k, v in recommended.items():
        if v:
            live["suggest"][k] = v
    live["catalog_status"] = model_catalog.status()
    return live
