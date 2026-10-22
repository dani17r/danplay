"""Los terminos y condiciones: si se aceptaron, y cuales.

El texto vive en la interfaz (desktop/src/data/terminos.js), que es quien
lo enseña; aqui solo se guarda que se aceptaron, que version y cuando, en un
archivo de los datos de DanPlay (`terminos.json`). Asi no se pierde aunque
el WebView olvide lo suyo, y vale para todas las ventanas.

Sin aceptarlos, la interfaz no deja separar en pistas, usar el asistente de
IA ni «Transformar».
"""

import logging

import danplay_core as _rust

from . import __version__, config

log = logging.getLogger(__name__)

FILE = "terminos.json"


def _path():
    return config.DATA_DIR / FILE


def status() -> dict:
    """{accepted: version aceptada o "", at: cuando, app: con que DanPlay}.
    Lo lee Rust (biblioteca/src/terminos.rs)."""
    return _rust.terms_status(config.DATA_DIR)


def accept(version: str) -> dict:
    """Guarda que se aceptaron los terminos de esa version. `ValueError` si
    la version no vale (una fecha AAAA-MM-DD o un numero)."""
    if not isinstance(version, str):
        raise ValueError("version de los terminos no valida")
    out = _rust.terms_accept(config.DATA_DIR, version, __version__)
    log.info("terminos aceptados (%s)", version)
    return out
