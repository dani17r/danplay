# -*- coding: utf-8 -*-
"""Identificacion por huella acustica: Chromaprint -> AcoustID -> MusicBrainz.

Reconoce la cancion por como suena, no por el nombre del archivo.
Necesita el binario `fpcalc` (paquete libchromaprint-tools) y una API key gratuita.
"""
import shutil
from . import config

AVAILABLE = shutil.which("fpcalc") is not None


def unavailable_reason() -> str:
    if not AVAILABLE:
        return "falta el binario fpcalc  ->  sudo apt install libchromaprint-tools"
    if not config.ACOUSTID_API_KEY:
        return "falta ACOUSTID_API_KEY en el .env  ->  https://acoustid.org/new-application"
    return ""


def identify(path, minimo=0.75) -> dict | None:
    """Devuelve {'artista','titulo','album','anio','puntuacion','fuente'} o None."""
    if unavailable_reason():
        return None
    try:
        import acoustid
    except ImportError:
        return None
    # OJO: `match` devuelve un GENERADOR. La peticion de red y el parseo de la
    # respuesta ocurren al recorrerlo, no al llamarlo. Si el try solo envuelve
    # la llamada, un error del servicio (clave invalida, cuota, caida) escapa
    # al recorrer y se lleva por delante toda la importacion.
    try:
        results = list(acoustid.match(config.ACOUSTID_API_KEY, str(path),
                                      meta="recordings releases"))
    except Exception:
        return None

    best = None
    for score, _rid, title, artist in results:
        if not title or not artist:
            continue
        if score < minimo:
            continue
        if best is None or score > best["score"]:
            best = {"artist": artist, "title": title, "album": "", "year": "",
                    "score": round(float(score), 3), "source": "fingerprint"}
    return best
