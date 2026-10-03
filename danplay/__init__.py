"""DanPlay - gestor de biblioteca musical con identificacion automatica."""

from . import no_console

__version__ = "1.20.1"

# antes de lanzar nada: en Windows, ningun ffmpeg ni yt-dlp con ventana negra
no_console.install()
