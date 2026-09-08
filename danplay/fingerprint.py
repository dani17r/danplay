# -*- coding: utf-8 -*-
"""Identificacion por huella acustica: Chromaprint -> AcoustID -> MusicBrainz.

Reconoce la cancion por como suena, no por el nombre del archivo.
Necesita el binario `fpcalc` (paquete libchromaprint-tools) y una API key gratuita.
"""
import logging, os
from . import config, convert

log = logging.getLogger("danplay")


def available() -> bool:
    """Si `fpcalc` esta. Se mira cada vez (con la cache corta de `convert.tool`):
    instalarlo con la app abierta tiene que notarse sin reiniciar."""
    return convert.tool("fpcalc") is not None


def __getattr__(name):
    # `AVAILABLE` era una constante calculada al importar. Se mantiene el
    # nombre para quien lo lea, pero ya no se congela.
    if name == "AVAILABLE":
        return available()
    raise AttributeError(name)


def unavailable_reason() -> str:
    if not available():
        return "falta el binario fpcalc  ->  sudo apt install libchromaprint-tools"
    if not config.ACOUSTID_API_KEY:
        return "falta ACOUSTID_API_KEY en el .env  ->  https://acoustid.org/new-application"
    return ""


def identify(path, minimum=0.75) -> dict | None:
    """Devuelve {'artist','title','album','year','score','source'} o None."""
    if unavailable_reason():
        return None
    # pyacoustid lee la variable FPCALC al importarse: si el binario esta
    # junto al ejecutable y no en el PATH, hay que decirselo antes.
    fpcalc = convert.tool("fpcalc")
    if fpcalc and os.path.dirname(fpcalc):
        os.environ.setdefault("FPCALC", fpcalc)
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
    except Exception:                                       # noqa: BLE001
        log.warning("AcoustID no respondio para %s", path, exc_info=True)
        return None

    best = None
    for score, _rid, title, artist in results:
        if not title or not artist:
            continue
        if score < minimum:
            continue
        if best is None or score > best["score"]:
            best = {"artist": artist, "title": title, "album": "", "year": "",
                    "score": round(float(score), 3), "source": "fingerprint"}
    return best
