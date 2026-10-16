"""En el nucleo empaquetado de Linux, los programas del sistema con las
bibliotecas del sistema.

PyInstaller arranca el nucleo con LD_LIBRARY_PATH apuntando a lo que lleva
dentro (/tmp/_MEI...): su libstdc++, su libssl... Y eso lo heredan los
procesos que lanza. El ffmpeg del sistema cargaba la libstdc++ del paquete
(la del equipo donde se compilo, mas vieja que la del sistema) y no
arrancaba («CXXABI_1.3.15 not found»): ni onda, ni separar, ni convertir.

Al propio nucleo ya no le hace falta (el cargador lee la variable al
arrancar), asi que se devuelve la que habia antes de PyInstaller (la guarda
en LD_LIBRARY_PATH_ORIG) para todo lo que se lance despues. Se instala al
importar `danplay`; fuera del paquete, o en Windows y macOS, no hace nada.
"""

import os
import sys


def restore(environ=None, frozen: bool | None = None, platform: str = sys.platform) -> bool:
    """Devuelve LD_LIBRARY_PATH a como estaba antes de PyInstaller. Devuelve
    si ha cambiado algo."""
    environ = os.environ if environ is None else environ
    frozen = bool(getattr(sys, "frozen", False)) if frozen is None else frozen
    if not frozen or not platform.startswith("linux") or "LD_LIBRARY_PATH" not in environ:
        return False
    original = environ.get("LD_LIBRARY_PATH_ORIG")
    if original:
        environ["LD_LIBRARY_PATH"] = original
    else:
        del environ["LD_LIBRARY_PATH"]
    return True
