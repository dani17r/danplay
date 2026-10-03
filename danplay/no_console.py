"""En Windows, que ningun proceso que lance el nucleo abra una consola.

El nucleo empaquetado es una aplicacion de ventanas (packaging/core.spec,
`console=False`): no tiene consola. Y cuando un programa sin consola lanza
uno de consola (ffmpeg, fpcalc, yt-dlp), Windows le abre una nueva: una
ventana negra por cada onda, cada conversion, cada huella. Con la app
analizando la biblioteca salian a decenas. En Linux no pasa nada de esto.

Lo que lo evita es la marca CREATE_NO_WINDOW, y habria que acordarse de ella
en cada llamada (las librerias no la ponen). Asi que aqui se pone por defecto
a todos los procesos: lo que pida una consola a proposito
(CREATE_NEW_CONSOLE, DETACHED_PROCESS) se respeta. Se instala al importar
`danplay`; en Linux y macOS no hace nada.
"""

import functools
import subprocess
import sys

CREATE_NO_WINDOW = 0x08000000
# con cualquiera de estas el que llama quiere una consola (o ninguna) a
# proposito, y CREATE_NO_WINDOW no se puede combinar con ellas
_CONSOLE_ON_PURPOSE = 0x00000010 | 0x00000008  # CREATE_NEW_CONSOLE | DETACHED_PROCESS


def hidden(flags: int | None) -> int:
    """Las `creationflags` que se piden, con CREATE_NO_WINDOW si no piden consola."""
    flags = flags or 0
    if flags & _CONSOLE_ON_PURPOSE:
        return flags
    return flags | CREATE_NO_WINDOW


def install(platform: str = sys.platform, popen: type = subprocess.Popen) -> bool:
    """Hace que `popen` (y con el `run`, `check_output` y las subclases de
    las librerias) lance sin ventana. Devuelve si ha cambiado algo."""
    if platform != "win32" or getattr(popen, "_danplay_no_console", False):
        return False
    original = popen.__init__

    @functools.wraps(original)
    def __init__(self, *args, **kwargs):
        kwargs["creationflags"] = hidden(kwargs.get("creationflags"))
        original(self, *args, **kwargs)

    popen.__init__ = __init__
    popen._danplay_no_console = True  # type: ignore[attr-defined]
    return True
