"""Configuracion central. Todo se puede sobreescribir por variables de entorno.

Los datos del usuario (base de datos y claves) viven SIEMPRE en su home, nunca
dentro del programa: asi la app se puede empaquetar y repartir sin arrastrar
la biblioteca de nadie.
"""

import contextlib
import logging
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path

from dotenv import load_dotenv
from platformdirs import user_config_dir, user_data_dir, user_music_dir

log = logging.getLogger(__name__)

FROZEN = getattr(sys, "frozen", False)  # True dentro del .deb/.AppImage
PROJECT_ROOT = Path(sys.executable).parent if FROZEN else Path(__file__).resolve().parent.parent

# `platformdirs` da la carpeta correcta en cada sistema. En Linux respeta
# XDG_DATA_HOME / XDG_CONFIG_HOME y cae a ~/.local/share y ~/.config: son
# EXACTAMENTE las rutas que se usaban antes, asi que nadie pierde su base ni
# sus ajustes al actualizar. `appauthor=False` evita que en Windows aparezca
# una carpeta intermedia con el mismo nombre (AppData\Local\danplay\danplay).
#
# `DANPLAY_DATA_DIR` y `DANPLAY_CONFIG_DIR` las ponen en otro sitio: un
# DanPlay portatil, o un entorno aislado (las pruebas) en Windows y macOS,
# donde platformdirs no mira XDG_*: pregunta al sistema por sus carpetas.
_DATA_OVERRIDE = os.getenv("DANPLAY_DATA_DIR", "")
_CONFIG_OVERRIDE = os.getenv("DANPLAY_CONFIG_DIR", "")
DATA_DIR = (
    Path(_DATA_OVERRIDE).expanduser()
    if _DATA_OVERRIDE
    else Path(user_data_dir("danplay", appauthor=False))
)
CONFIG_DIR = (
    Path(_CONFIG_OVERRIDE).expanduser()
    if _CONFIG_OVERRIDE
    else Path(user_config_dir("danplay", appauthor=False))
)


def private_dir(path: Path) -> Path:
    """Crea la carpeta solo para su dueño (0700).

    A la que ya existia tambien se le quitan los permisos de los demas: ahi
    dentro van la base (con el historial del chat) y las claves de IA, y con
    la umask de siempre cualquier usuario del equipo podia leerlas.
    """
    path.mkdir(parents=True, exist_ok=True)
    if os.name == "posix":
        try:
            os.chmod(path, 0o700)
        except OSError:
            log.warning("no pude cerrar los permisos de %s", path, exc_info=True)
    return path


def write_private(path: Path, text: str) -> None:
    """Escribe un archivo con secretos: atomico y solo para su dueño (0600).

    Va primero a un temporal de la misma carpeta, que ya nace con 0600 (no
    con la umask y un chmod despues, que dejaba un instante en que lo podia
    leer cualquiera), se fuerza al disco y se pone en su sitio con
    `os.replace`. Si algo se corta a medias queda el archivo de antes entero:
    un ai.json cortado se leia como «sin perfiles» y el siguiente guardado
    pisaba todas las claves.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
    except BaseException:
        with contextlib.suppress(OSError):
            os.unlink(tmp)
        raise
    if os.name == "posix":
        # que el cambio de nombre tambien llegue al disco
        with contextlib.suppress(OSError):
            folder = os.open(path.parent, os.O_RDONLY)
            try:
                os.fsync(folder)
            finally:
                os.close(folder)


private_dir(DATA_DIR)
private_dir(CONFIG_DIR)

ENV_FILE = CONFIG_DIR / "danplay.env"


def _migrate(old: Path, new: Path) -> None:
    """Copia un archivo de una instalacion anterior, una sola vez."""
    try:
        if old.is_file() and not new.exists():
            shutil.copy2(old, new)
            if os.name == "posix":
                os.chmod(new, 0o600)
    except OSError:
        log.warning("no se pudo migrar %s", old, exc_info=True)


# Migracion desde el nombre anterior del programa. Por platformdirs, como las
# rutas de ahora: con las rutas fijas de antes (~/.config/melodia, ...), un
# entorno aislado con XDG_* propios (las pruebas, `scripts/dev.sh --prueba`)
# se tragaba la base y las claves de IA del usuario de verdad. Para quien no
# tiene XDG definido la ruta es la misma que antes. Con las carpetas puestas a
# mano no se migra nada: un entorno aparte no tiene por que traerse lo de otro.
if not (_DATA_OVERRIDE or _CONFIG_OVERRIDE):
    _migrate(Path(user_config_dir("melodia", appauthor=False)) / "melodia.env", ENV_FILE)
    _migrate(
        Path(user_data_dir("melodia", appauthor=False)) / "melodia.db", DATA_DIR / "danplay.db"
    )

# El proyecto en desarrollo (su .env y su danplay.db) cuenta como respaldo y se
# migra una sola vez. `DANPLAY_PROJECT_ENV=0` lo apaga: las pruebas y los
# entornos aislados no tienen por que ver las claves de quien programa.
USE_PROJECT_ENV = os.getenv("DANPLAY_PROJECT_ENV", "1") not in ("0", "false", "no")
if USE_PROJECT_ENV:
    _migrate(PROJECT_ROOT / ".env", ENV_FILE)
    _migrate(PROJECT_ROOT / "danplay.db", DATA_DIR / "danplay.db")

load_dotenv(ENV_FILE)
if USE_PROJECT_ENV:
    load_dotenv(PROJECT_ROOT / ".env", override=False)  # el proyecto solo como respaldo


# Las variables se llamaban en castellano. Se leen las nuevas y, si no estan,
# las viejas: nadie tiene que tocar su danplay.env para que siga funcionando.
_ALIAS = {
    "DANPLAY_LIBRARY": "DANPLAY_BIBLIOTECA",
    "DANPLAY_INBOX": "DANPLAY_ENTRADA",
    "DANPLAY_CONVERT": "DANPLAY_CONVERTIR",
    "DANPLAY_QUALITY": "DANPLAY_CALIDAD",
    "DANPLAY_KEEP_ORIGINAL": "DANPLAY_CONSERVAR",
    "DANPLAY_NEVER_CONVERT": "DANPLAY_NO_CONVERTIR",
    "DANPLAY_AI": "DANPLAY_IA",
    "DANPLAY_DUP_THRESHOLD": "DANPLAY_UMBRAL_DUP",
    "DEEPINFRA_MODEL": "DEEPINFRA_MODELO",
    "DEEPINFRA_CHAT_MODEL": "DEEPINFRA_MODELO_CHAT",
}


def env(name: str, default: str = "") -> str:
    """Valor de una variable de entorno, aceptando tambien su nombre antiguo."""
    return os.getenv(name) or os.getenv(_ALIAS.get(name, name), default)


def _flag(name, default="0"):
    return env(name, default) not in ("0", "false", "no")


# Lo que puede ir sin comillas en el archivo: rutas y valores sencillos.
_PLAIN_VALUE = re.compile(r"[A-Za-z0-9_./:@+,-]*")


def dotenv_value(value: str) -> str:
    """Un valor escrito como lo relee python-dotenv, igual que se guardo.

    Sin comillas, lo que va detras de « #» es un comentario: la carpeta
    `/home/ana/Musica #2` se releia como `/home/ana/Musica`, y los espacios del
    final se perdian. Entre comillas simples no hay comentarios; la barra y la
    comilla se escapan (es lo mismo que hace el `set_key` de python-dotenv).
    """
    if _PLAIN_VALUE.fullmatch(value):
        return value
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"


def _key_of(line: str) -> str:
    """La variable que define una linea del archivo («export X=1» incluido)."""
    head = line.split("=", 1)[0].strip()
    return head[7:].strip() if head.startswith("export ") else head


def save_env(pairs: dict) -> None:
    """Persiste ajustes en el archivo del usuario.

    Los saltos de linea se quitan del valor: el archivo es una variable por
    linea, asi que un valor con un salto dentro (pegado sin querer, o metido a
    proposito desde Ajustes) escribia mas variables de las que se pedian. Se
    escribe con `write_private`: lleva claves de API.
    """
    lines = (
        ENV_FILE.read_text(encoding="utf-8", errors="replace").splitlines()
        if ENV_FILE.exists()
        else []
    )
    for key, value in pairs.items():
        value = str(value).replace("\r", " ").replace("\n", " ").strip()
        lines = [line for line in lines if _key_of(line) != key]
        lines.append(f"{key}={dotenv_value(value)}")
    write_private(ENV_FILE, "\n".join(lines) + "\n")


def _default_library() -> Path:
    """Donde esta la musica si nadie lo dice.

    ~/Musica si existe (lo de siempre); si no, la carpeta de musica del
    escritorio (XDG_MUSIC_DIR: en un sistema en castellano se llama con
    tilde, y sin esto no se encontraba); si no, ~/Music; y si no hay
    ninguna, ~/Musica.
    """
    home = Path.home()
    candidates = [home / "Musica"]
    with contextlib.suppress(Exception):
        candidates.append(Path(user_music_dir()))
    candidates.append(home / "Music")
    for folder in candidates:
        if folder.is_dir():
            return folder
    return home / "Musica"


# Carpeta con ffmpeg/ffprobe/fpcalc cuando no estan en el PATH: la del
# instalador (junto al ejecutable) o la que diga DANPLAY_TOOLS_DIR.
TOOLS_DIR = os.getenv("DANPLAY_TOOLS_DIR", "")


def exe_suffixes() -> tuple[str, ...]:
    """Que extensiones tiene un ejecutable en este sistema.

    Aparte para poder probarlo: cambiar `os.name` a mano en una prueba hace
    que pathlib se pase a rutas de Windows y ya no se puedan crear archivos.
    """
    return (".exe", "") if os.name == "nt" else ("",)


def find_tool(name: str) -> str | None:
    """Ruta de un binario externo, o None.

    Se mira primero en DANPLAY_TOOLS_DIR y junto al ejecutable (la app
    empaquetada lleva ffmpeg y fpcalc dentro, sobre todo en Windows), y por
    ultimo en el PATH del sistema.
    """
    candidates = []
    if TOOLS_DIR:
        candidates.append(Path(TOOLS_DIR))
    if FROZEN:
        candidates.append(Path(sys.executable).parent)
    suffixes = exe_suffixes()
    for folder in candidates:
        for suffix in suffixes:
            p = folder / (name + suffix)
            if p.is_file() and os.access(p, os.X_OK):
                return str(p)
    return shutil.which(name)


# --- rutas ---
LIBRARY = Path(env("DANPLAY_LIBRARY") or _default_library()).expanduser()
INBOX = LIBRARY / env("DANPLAY_INBOX", "Entrada")
ARTISTS_DIR = LIBRARY / "Artistas"
REVIEW_DIR = LIBRARY / "Revisar"  # lo que no se pudo identificar
DATABASE = DATA_DIR / "danplay.db"

# carpetas que no son de artista: se respetan tal cual, nunca se tocan
SPECIAL_FOLDERS = {"Entrada", "Revisar", "Secuencias", "Pistas", "Tutoriales y Play Along", "rolas"}

# --- IA ---
# Que proveedor y que modelos: en `providers` (perfiles en ai.json). Aqui solo
# el interruptor general. Las variables DEEPINFRA_* de antes se leen una vez
# para migrar la clave al perfil (providers._migrate_legacy).
AI_ENABLED = _flag("DANPLAY_AI", "1")

# --- AcoustID (huella acustica) ---
ACOUSTID_API_KEY = os.getenv("ACOUSTID_API_KEY", "")

# --- conversion de formatos ---
# la casilla de la app enciende/apaga esto
CONVERT_TO_MP3 = _flag("DANPLAY_CONVERT")
MP3_QUALITY = env("DANPLAY_QUALITY", "high")  # high | medium | variable
KEEP_ORIGINAL = _flag("DANPLAY_KEEP_ORIGINAL")
# carpetas que NUNCA se convierten: material de produccion, se pierde calidad
NEVER_CONVERT = set(
    filter(None, env("DANPLAY_NEVER_CONVERT", "Secuencias,Pistas,Multitracks,Stems").split(","))
)

# --- comportamiento ---
WRITE_TAGS = _flag("DANPLAY_TAGS", "1")
# en que se guardan las pistas separadas: "flac" (sin perdida) u "opus"
STEMS_FORMAT = env("DANPLAY_STEMS_FORMAT", "flac")
DUPLICATE_THRESHOLD = float(env("DANPLAY_DUP_THRESHOLD", "0.88"))
EXTENSIONS = {".mp3", ".wav", ".flac", ".m4a", ".ogg", ".opus", ".aac", ".wma"}


def summary() -> str:
    from . import ai  # aqui dentro: ai importa config

    ai_txt = f"{ai.provider_name()} · {ai.fast_model()}" if ai.available() else "desactivada"
    fp = "activa" if ACOUSTID_API_KEY else "sin API key"
    return (
        f"Biblioteca : {LIBRARY}\n"
        f"Entrada    : {INBOX}\n"
        f"Artistas   : {ARTISTS_DIR}\n"
        f"Huella     : {fp}\n"
        f"IA         : {ai_txt}\n"
        f"IA (chat)  : {ai.chat_model() if ai.available() else '-'}\n"
        f"Etiquetas  : {'si' if WRITE_TAGS else 'no'}\n"
        f"Convertir  : {'si (' + MP3_QUALITY + ')' if CONVERT_TO_MP3 else 'no'}"
        f"  | nunca: {', '.join(sorted(NEVER_CONVERT))}\n"
        f"Ajustes    : {ENV_FILE}\n"
        f"Datos      : {DATABASE}"
    )
