# -*- coding: utf-8 -*-
"""Configuracion central. Todo se puede sobreescribir por variables de entorno.

Los datos del usuario (base de datos y claves) viven SIEMPRE en su home, nunca
dentro del programa: asi la app se puede empaquetar y repartir sin arrastrar
la biblioteca de nadie.
"""
import os, shutil, sys
from pathlib import Path
from dotenv import load_dotenv

FROZEN = getattr(sys, "frozen", False)          # True dentro del .deb/.AppImage
PROJECT_ROOT = (Path(sys.executable).parent if FROZEN
                 else Path(__file__).resolve().parent.parent)

def _xdg(var, default):
    return Path(os.getenv(var) or Path.home() / default) / "danplay"

DATA_DIR  = _xdg("XDG_DATA_HOME",   ".local/share")
CONFIG_DIR = _xdg("XDG_CONFIG_HOME", ".config")
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
        pass

# migracion desde el proyecto en desarrollo, una sola vez
for source_path, target in ((PROJECT_ROOT / ".env", ENV_FILE),
                        (PROJECT_ROOT / "danplay.db", DATA_DIR / "danplay.db")):
    try:
        if source_path.is_file() and not target.exists():
            shutil.copy2(source_path, target)
    except OSError:
        pass

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
    os.chmod(ENV_FILE, 0o600)


# --- rutas ---
LIBRARY = Path(env("DANPLAY_LIBRARY") or (Path.home() / "Musica"))
INBOX    = LIBRARY / env("DANPLAY_INBOX", "Entrada")
ARTISTS_DIR   = LIBRARY / "Artistas"
REVIEW_DIR    = LIBRARY / "Revisar"          # lo que no se pudo identificar
DATABASE = DATA_DIR / "danplay.db"

# carpetas que no son de artista: se respetan tal cual, nunca se tocan
SPECIAL_FOLDERS = {"Entrada", "Revisar", "Secuencias", "Pistas",
                       "Tutoriales y Play Along", "rolas"}

# --- DeepInfra (compatible con la API de OpenAI) ---
DEEPINFRA_API_KEY  = os.getenv("DEEPINFRA_API_KEY", "")
DEEPINFRA_BASE_URL = os.getenv("DEEPINFRA_BASE_URL", "https://api.deepinfra.com/v1/openai")
DEEPINFRA_MODEL   = env("DEEPINFRA_MODEL", "google/gemini-3.1-flash-lite")
# El chat usa otro modelo: gemini no soporta el ida y vuelta de herramientas por
# la API compatible con OpenAI (le falta el thought_signature de Google).
DEEPINFRA_CHAT_MODEL = env("DEEPINFRA_CHAT_MODEL",
                           "Qwen/Qwen3-Next-80B-A3B-Instruct")
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
    ai_txt = f"{DEEPINFRA_MODEL}" if (AI_ENABLED and DEEPINFRA_API_KEY) else "desactivada"
    fp = "activa" if ACOUSTID_API_KEY else "sin API key"
    return (f"Biblioteca : {LIBRARY}\n"
            f"Entrada    : {INBOX}\n"
            f"Artistas   : {ARTISTS_DIR}\n"
            f"Huella     : {fp}\n"
            f"IA         : {ai_txt}\n"
            f"IA (chat)  : {DEEPINFRA_CHAT_MODEL if AI_ENABLED and DEEPINFRA_API_KEY else '-'}\n"
            f"Etiquetas  : {'si' if WRITE_TAGS else 'no'}\n"
            f"Convertir  : {'si (' + MP3_QUALITY + ')' if CONVERT_TO_MP3 else 'no'}"
            f"  | nunca: {', '.join(sorted(NEVER_CONVERT))}\n"
            f"Ajustes    : {ENV_FILE}\n"
            f"Datos      : {DATABASE}")
