"""Como se ven los avisos del nucleo.

Todos los modulos escriben en su propio registro (`getLogger(__name__)`,
«danplay.library.scan», «danplay.ytdlp»...) y aqui se decide adonde va: a la
consola, con la hora y el nivel, y al servir la app ademas a un archivo en
DATA_DIR/logs que rota solo (cuando algo falla con la app cerrada, o en el
equipo de otra persona, es lo unico que queda para saber que paso).
"""

import logging
import logging.handlers
from pathlib import Path

from . import config

FORMAT = "%(asctime)s %(levelname)s %(name)s: %(message)s"
DATEFMT = "%Y-%m-%d %H:%M:%S"
# un mega por archivo y tres de reserva: semanas de uso normal
MAX_BYTES = 1024 * 1024
BACKUPS = 3

_file_handler: logging.handlers.RotatingFileHandler | None = None


def log_file() -> Path:
    return config.DATA_DIR / "logs" / "danplay.log"


def setup(to_file: bool = False, level: int = logging.INFO) -> Path | None:
    """Avisos por consola (una vez) y, con `to_file`, tambien al archivo.

    Devuelve la ruta del archivo, o None si no se escribe (o no se pudo).
    """
    global _file_handler
    root = logging.getLogger()
    if not any(getattr(h, "_danplay", False) for h in root.handlers):
        console = logging.StreamHandler()
        console.setFormatter(logging.Formatter(FORMAT, DATEFMT))
        console._danplay = True  # type: ignore[attr-defined]
        root.addHandler(console)
    root.setLevel(level)
    if not to_file:
        return None
    if _file_handler is None:
        path = log_file()
        try:
            config.private_dir(path.parent)
            handler = logging.handlers.RotatingFileHandler(
                path, maxBytes=MAX_BYTES, backupCount=BACKUPS, encoding="utf-8"
            )
        except OSError:
            logging.getLogger(__name__).warning(
                "no puedo escribir el registro en %s", path, exc_info=True
            )
            return None
        handler.setFormatter(logging.Formatter(FORMAT, DATEFMT))
        root.addHandler(handler)
        _file_handler = handler
    return Path(_file_handler.baseFilename)
