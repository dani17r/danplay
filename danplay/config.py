# -*- coding: utf-8 -*-
"""Configuracion central. Todo se puede sobreescribir por variables de entorno.

Los datos del usuario (base de datos y claves) viven SIEMPRE en su home, nunca
dentro del programa: asi la app se puede empaquetar y repartir sin arrastrar
la biblioteca de nadie.
"""
import logging, os, shutil, sys
from pathlib import Path
from dotenv import load_dotenv
from platformdirs import user_config_dir, user_data_dir

log = logging.getLogger("danplay")

FROZEN = getattr(sys, "frozen", False)          # True dentro del .deb/.AppImage
PROJECT_ROOT = (Path(sys.executable).parent if FROZEN
                 else Path(__file__).resolve().parent.parent)

# `platformdirs` da la carpeta correcta en cada sistema. En Linux respeta
# XDG_DATA_HOME / XDG_CONFIG_HOME y cae a ~/.local/share y ~/.config: son
# EXACTAMENTE las rutas que se usaban antes, asi que nadie pierde su base ni
# sus ajustes al actualizar. `appauthor=False` evita que en Windows aparezca
# una carpeta intermedia con el mismo nombre (AppData\Local\danplay\danplay).
DATA_DIR   = Path(user_data_dir("danplay", appauthor=False))
CONFIG_DIR = Path(user_config_dir("danplay", appauthor=False))
DATA_DIR.mkdir(parents=True, exist_ok=True)
CONFIG_DIR.mkdir(parents=True, exist_ok=True)

ENV_FILE = CONFIG_DIR / "danplay.env"

# migracion desde el nombre anterior del programa
for _old, _new in ((Path.home() / ".config/melodia/melodia.env", CONFIG_DIR / "danplay.env"),
                       (Path.home() / ".local/share/melodia/melodia.db", DATA_DIR / "danplay.db")):
    try:
        if _old.is_file() and not _new.exists():
            shutil.copy2(_old, _new)
    except OSError:
        log.warning("no se pudo migrar %s", _old, exc_info=True)

# migracion desde el proyecto en desarrollo, una sola vez
for source_path, target in ((PROJECT_ROOT / ".env", ENV_FILE),
                        (PROJECT_ROOT / "danplay.db", DATA_DIR / "danplay.db")):
    try:
        if source_path.is_file() and not target.exists():
            shutil.copy2(source_path, target)
    except OSError:
        log.warning("no se pudo copiar %s", source_path, exc_info=True)

load_dotenv(ENV_FILE)
load_dotenv(PROJECT_ROOT / ".env", override=False)   # el proyecto solo como respaldo


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


def env(name, default=""):
    """Valor de una variable de entorno, aceptando tambien su nombre antiguo."""
    return os.getenv(name) or os.getenv(_ALIAS.get(name, name), default)


def _flag(name, default="0"):
    return env(name, default) not in ("0", "false", "no")


def save_env(pairs: dict) -> None:
    """Persiste ajustes en el archivo del usuario.

    Los saltos de linea se quitan del valor: el archivo es una variable por
    linea, asi que un valor con un salto dentro (pegado sin querer, o metido a
    proposito desde Ajustes) escribia mas variables de las que se pedian.
    """
    lines = ENV_FILE.read_text().splitlines() if ENV_FILE.exists() else []
    for key, value in pairs.items():
        value = str(value).replace("\r", " ").replace("\n", " ").strip()
        lines = [l for l in lines if not l.startswith(key + "=")]
        lines.append(f"{key}={value}")
    ENV_FILE.write_text("\n".join(lines) + "\n")
    # el archivo lleva claves de API: solo lo lee su dueño. En Windows los
    # permisos POSIX no existen (el perfil del usuario ya es privado).
    if os.name == "posix":
        os.chmod(ENV_FILE, 0o600)


def _default_library() -> Path:
    """~/Musica si existe; si no, ~/Music (nombre en ingles); si no, ~/Musica."""
    home = Path.home()
    for name in ("Musica", "Music"):
        if (home / name).is_dir():
            return home / name
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
INBOX    = LIBRARY / env("DANPLAY_INBOX", "Entrada")
ARTISTS_DIR   = LIBRARY / "Artistas"
REVIEW_DIR    = LIBRARY / "Revisar"          # lo que no se pudo identificar
DATABASE = DATA_DIR / "danplay.db"

# carpetas que no son de artista: se respetan tal cual, nunca se tocan
SPECIAL_FOLDERS = {"Entrada", "Revisar", "Secuencias", "Pistas",
                       "Tutoriales y Play Along", "rolas"}

# --- IA ---
# Que proveedor y que modelos: en `providers` (perfiles en ai.json). Aqui solo
# el interruptor general. Las variables DEEPINFRA_* de antes se leen una vez
# para migrar la clave al perfil (providers._migrate_legacy).
AI_ENABLED          = _flag("DANPLAY_AI", "1")

# --- AcoustID (huella acustica) ---
ACOUSTID_API_KEY = os.getenv("ACOUSTID_API_KEY", "")

# --- conversion de formatos ---
# la casilla de la app enciende/apaga esto
CONVERT_TO_MP3 = _flag("DANPLAY_CONVERT")
MP3_QUALITY     = env("DANPLAY_QUALITY", "high")        # high | medium | variable
KEEP_ORIGINAL = _flag("DANPLAY_KEEP_ORIGINAL")
# carpetas que NUNCA se convierten: material de produccion, se pierde calidad
NEVER_CONVERT = set(filter(None, env(
    "DANPLAY_NEVER_CONVERT", "Secuencias,Pistas,Multitracks,Stems").split(",")))

# --- comportamiento ---
WRITE_TAGS   = _flag("DANPLAY_TAGS", "1")
DUPLICATE_THRESHOLD = float(env("DANPLAY_DUP_THRESHOLD", "0.88"))
EXTENSIONS = {".mp3", ".wav", ".flac", ".m4a", ".ogg", ".opus", ".aac", ".wma"}

def summary() -> str:
    from . import ai                       # aqui dentro: ai importa config
    ai_txt = f"{ai.provider_name()} · {ai.fast_model()}" if ai.available() else "desactivada"
    fp = "activa" if ACOUSTID_API_KEY else "sin API key"
    return (f"Biblioteca : {LIBRARY}\n"
            f"Entrada    : {INBOX}\n"
            f"Artistas   : {ARTISTS_DIR}\n"
            f"Huella     : {fp}\n"
            f"IA         : {ai_txt}\n"
            f"IA (chat)  : {ai.chat_model() if ai.available() else '-'}\n"
            f"Etiquetas  : {'si' if WRITE_TAGS else 'no'}\n"
            f"Convertir  : {'si (' + MP3_QUALITY + ')' if CONVERT_TO_MP3 else 'no'}"
            f"  | nunca: {', '.join(sorted(NEVER_CONVERT))}\n"
            f"Ajustes    : {ENV_FILE}\n"
            f"Datos      : {DATABASE}")
