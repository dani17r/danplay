"""Los terminos y condiciones: si se aceptaron, y cuales.

El texto vive en la interfaz (desktop/src/data/terminos.js), que es quien
lo enseña; aqui solo se guarda que se aceptaron, que version y cuando, en un
archivo de los datos de DanPlay (`terminos.json`). Asi no se pierde aunque
el WebView olvide lo suyo, y vale para todas las ventanas.

Sin aceptarlos, la interfaz no deja separar en pistas, usar el asistente de
IA ni «Transformar».
"""

import json
import logging
import os
import re
import time

from . import __version__, config

log = logging.getLogger(__name__)

FILE = "terminos.json"
# una version de los terminos: su fecha (AAAA-MM-DD) o un numero
_VERSION = re.compile(r"^[0-9][0-9A-Za-z.\-]{0,31}$")


def _path():
    return config.DATA_DIR / FILE


def status() -> dict:
    """{accepted: version aceptada o "", at: cuando, app: con que DanPlay}."""
    try:
        data = json.loads(_path().read_text(encoding="utf-8"))
    except (OSError, ValueError):
        data = {}
    if not isinstance(data, dict):
        data = {}
    version = data.get("version")
    return {
        "accepted": version if isinstance(version, str) and _VERSION.match(version) else "",
        "at": data.get("at") if isinstance(data.get("at"), (int, float)) else None,
        "app": data.get("app") if isinstance(data.get("app"), str) else "",
    }


def accept(version: str) -> dict:
    """Guarda que se aceptaron los terminos de esa version."""
    if not isinstance(version, str) or not _VERSION.match(version):
        raise ValueError("version de los terminos no valida")
    path = _path()
    path.parent.mkdir(parents=True, exist_ok=True)
    data = {"version": version, "at": round(time.time()), "app": __version__}
    tmp = path.with_name(path.name + ".part")
    tmp.write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")
    os.replace(tmp, path)
    log.info("terminos aceptados (%s)", version)
    return status()
