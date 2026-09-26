"""Estado del nucleo, ajustes y como van los trabajos largos."""

import logging
from pathlib import Path
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException

from ... import ai, config, convert, duplicates, fingerprint, library, providers, youtube
from .. import jobs
from ..models import SettingsIn

log = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/status")
def status():
    e = library.stats_of()
    folders = library.list_folders()
    return {
        "configured": bool(folders),
        "folders": len(folders),
        # carpetas gestionadas que ya no estan donde estaban (se movieron,
        # o un disco sin montar): sus canciones estan apartadas, y la
        # interfaz lo dice en vez de enseñar una biblioteca vacia sin mas
        "missing_folders": [f["path"] for f in folders if f["active"] and not f["exists"]],
        "library": str(config.LIBRARY),
        "inbox": str(config.INBOX),
        "model": ai.fast_model(),
        "provider": ai.provider_name(),
        "ai": ai.available(),
        "fingerprint": not fingerprint.unavailable_reason(),
        "fingerprint_reason": fingerprint.unavailable_reason(),
        "convert": config.CONVERT_TO_MP3,
        "quality": config.MP3_QUALITY,
        "never_convert": sorted(config.NEVER_CONVERT),
        "rust": duplicates.RUST,
        "ffmpeg": convert.available(),
        "stats": e,
        "youtube": youtube.available(),
        "youtube_reason": youtube.unavailable_reason(),
        # cuantas veces ha cambiado algo que se enseña: Rust lo vigila y
        # avisa a las ventanas cuando se mueve (`danplay://changed`)
        "revision": library.revision(),
        "jobs": jobs.snapshot_all(),
    }


@router.get("/api/settings")
def settings():
    p = ai.profile()
    return {
        "convert_mp3": config.CONVERT_TO_MP3,
        "quality": config.MP3_QUALITY,
        "keep_original": config.KEEP_ORIGINAL,
        "stems_format": config.STEMS_FORMAT,
        "write_tags": config.WRITE_TAGS,
        "ai_enabled": config.AI_ENABLED,
        "model": ai.fast_model(),
        "chat_model": ai.chat_model(),
        "provider": p["id"] if p else "",
        "provider_name": p["name"] if p else "",
        "library": str(config.LIBRARY),
        "ai_key": providers.mask(p["key"]) if p else "",
        "has_ai_key": bool(p and p["key"]),
        "ai_ready": ai.available(),
        "ai_reason": ai.unavailable_reason(),
        "fingerprint_key": bool(config.ACOUSTID_API_KEY),
        "settings_file": str(config.ENV_FILE),
        "ai_file": str(providers.PROFILES_FILE),
    }


@router.post("/api/settings")
def save_settings(body: Annotated[SettingsIn, Body()]):
    """Las casillas y campos de la app. Se aplican en caliente y se persisten."""
    flags = {
        "convert_mp3": ("CONVERT_TO_MP3", "DANPLAY_CONVERT"),
        "keep_original": ("KEEP_ORIGINAL", "DANPLAY_KEEP_ORIGINAL"),
        "write_tags": ("WRITE_TAGS", "DANPLAY_TAGS"),
        "ai_enabled": ("AI_ENABLED", "DANPLAY_AI"),
    }
    texts = {
        "quality": ("MP3_QUALITY", "DANPLAY_QUALITY"),
        "stems_format": ("STEMS_FORMAT", "DANPLAY_STEMS_FORMAT"),
        "fingerprint_key": ("ACOUSTID_API_KEY", "ACOUSTID_API_KEY"),
    }
    save = {}
    profile_patch = {}
    for k, v in body.model_dump(exclude_none=True).items():
        if k in flags:
            attr, env = flags[k]
            setattr(config, attr, bool(v))
            save[env] = "1" if v else "0"
        elif k in texts and isinstance(v, str) and v.strip():
            attr, env = texts[k]
            setattr(config, attr, v.strip())
            save[env] = v.strip()
        elif k in ("model", "ai_key") and isinstance(v, str) and v.strip():
            profile_patch["key" if k == "ai_key" else "model"] = v.strip()
        elif k == "library" and v:
            path = Path(v).expanduser()
            if not path.is_dir():
                raise HTTPException(400, "esa carpeta no existe")
            config.LIBRARY = path
            config.INBOX = path / "Entrada"
            config.ARTISTS_DIR = path / "Artistas"
            config.REVIEW_DIR = path / "Revisar"
            save["DANPLAY_LIBRARY"] = str(path)
    if save:
        config.save_env(save)
    if profile_patch:
        # sin proveedor elegido, la clave suelta va a DeepInfra: es lo que
        # significaba antes «clave de IA»
        pid = providers.active_id() or "deepinfra"
        providers.save_profile(
            {"id": pid, "provider": pid if pid in providers.BY_ID else "custom", **profile_patch}
        )
        ai.reset_client()  # fuerza recrear con la clave nueva
    return settings()


@router.get("/api/jobs/{name}")
def job_status(name: str):
    """Como va un trabajo largo: {name, active, done, total, message, result,
    error, started, ended}. La interfaz pregunta hasta que `active` es false."""
    snap = jobs.snapshot(name)
    if snap is None:
        raise HTTPException(404, "no hay ningun trabajo con ese nombre")
    return snap
