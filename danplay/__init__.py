"""DanPlay - gestor de biblioteca musical con identificacion automatica."""

from . import no_console, system_libs

__version__ = "1.28.1"

# antes de lanzar nada: en Windows, ningun ffmpeg ni yt-dlp con ventana negra
no_console.install()
# y en el paquete de Linux, ffmpeg y los demas con las bibliotecas del sistema
system_libs.restore()
