"""Identificacion por huella acustica: Chromaprint -> AcoustID -> MusicBrainz.

Reconoce la cancion por como suena, no por el nombre del archivo.
Necesita el binario `fpcalc` (paquete libchromaprint-tools) y una API key gratuita.

La huella y la consulta las hace el crate de Rust (library/src/fingerprint.rs,
antes pyacoustid); aqui queda la puerta de Python y lo que depende de
`config` (la clave, donde esta fpcalc).
"""

import logging
import os

import danplay_core as _rust

from . import config, convert

log = logging.getLogger(__name__)


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


def _match(key: str, path) -> list[tuple]:
    """Lo que AcoustID dice que suena como `path`: [(puntuacion, id, titulo,
    artista)]. Falla con una excepcion si no hay fpcalc, red o clave buena.
    Como con pyacoustid, la variable FPCALC manda sobre el que se encuentre
    (en la app empaquetada vive junto al ejecutable y no en el PATH)."""
    fpcalc = os.environ.get("FPCALC") or convert.tool("fpcalc") or "fpcalc"
    return _rust.fingerprint_match(key, fpcalc, os.path.abspath(os.path.expanduser(str(path))))


def identify(path, minimum=0.75) -> dict | None:
    """Devuelve {'artist','title','album','year','score','source'} o None."""
    if unavailable_reason():
        return None
    # Un error del servicio (clave invalida, cuota, caida) o de fpcalc no se
    # lleva por delante la importacion: no hay huella, y ya.
    try:
        results = list(_match(config.ACOUSTID_API_KEY, path))
    except Exception:
        log.warning("AcoustID no respondio para %s", path, exc_info=True)
        return None
    return _rust.fingerprint_best(results, minimum)
