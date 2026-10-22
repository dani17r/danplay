"""Las guias habladas del modo estudio: lo que se oye en un punto de la
cancion («Coro», «Puente, cuatro tiempos», «Sube medio tono»), como la pista
«Guide» de una secuencia. Se escribe y la dice una voz, o se graba con el
microfono; queda en un archivo de DATA_DIR/guias/ y el motor lo hace sonar
justo ahi, por la salida de las guias.

Las voces, en el equipo y sin internet (salvo para bajarlas, una vez):

- Supertonic 3 (Supertone, OpenRAIL-M): diez voces naturales, cinco de
  mujer y cinco de hombre, que hablan castellano (y treinta idiomas mas).
  Lee el texto tal cual. Unos 400 MB, para todas.
- Piper (voces de rhasspy/piper-voices, cada una con su licencia): voces de
  España, Mexico y Argentina, de 28 a 115 MB cada una. Se entrenaron con los
  fonemas de espeak-ng; aqui los saca `habla` (espeak es GPL y no viaja).

Las dos son grafos ONNX que corren con ONNX Runtime, como el separador. Cada
archivo se comprueba con su sha256 al bajarlo (`data/voces.json`, con la
revision fijada: lo de HuggingFace no cambia por debajo).

Las voces, el sonido de cada guia (recortarla, su volumen, el WAV, su onda,
el editor) y el catalogo van en Rust (analisis/src/voces.rs,
analisis/src/guias.rs, biblioteca/src/guias.rs); aqui quedan las carpetas,
la bajada de las voces (con sus hilos) y la voz cargada.
"""

import functools
import hashlib
import json
import logging
import os
import threading
import time
import urllib.request
import uuid
from pathlib import Path

import danplay_core as _rust

from . import config, convert, habla

log = logging.getLogger(__name__)

CATALOG = Path(__file__).resolve().parent / "data" / "voces.json"
USER_AGENT = "DanPlay (+https://github.com/dani17r/danplay)"
TIMEOUT = 60
# Lo que cambia como suena una voz: con otro numero, las guias ya dichas no
# se reutilizan (se vuelven a decir al pedirlas).
VERSION = 1
# Una grabacion de mas de esto no es una guia.
MAX_SECONDS = 60.0
# Un audio importado como guía (una cuenta, un aviso, un trozo grabado
# aparte): hasta lo que dure la canción, y como mucho esto (el motor las
# guarda enteras en memoria).
MAX_IMPORT_SECONDS = 300.0
# Lo que queda alrededor de la voz al recortar el silencio (el de
# analisis/src/guias.rs).
MARGIN = 0.03
# Tras este rato sin decir nada, se suelta la voz cargada (la de Supertonic
# ocupa medio GB de memoria).
IDLE = 180.0
AUDIO_EXTS = {".wav", ".flac", ".mp3", ".ogg", ".opus", ".m4a", ".aac", ".webm"}


# No se pudo hacer la guia (por que, en castellano). Es la de Rust: la lanzan
# los dos lados.
CueError = _rust.CueError


def folder() -> Path:
    return config.DATA_DIR / "guias"


def _previews() -> Path:
    return folder() / "prueba"


def _voices_dir() -> Path:
    return config.DATA_DIR / "voces"


@functools.cache
def catalog() -> dict:
    return json.loads(CATALOG.read_text(encoding="utf-8"))


# ------------------------------------------------------------ las voces


@functools.cache
def _catalog():
    return _rust.CueCatalog(str(CATALOG))


def _find(voice_id: str) -> dict:
    """La voz por su id, con su motor y los archivos que necesita (cada uno
    con su URL y donde va)."""
    v = _catalog().find(str(_voices_dir()), voice_id) if isinstance(voice_id, str) else None
    if v is None:
        raise CueError("no conozco esa voz")
    for f in v["files"]:
        f["dest"] = Path(f["dest"])
    return v


def _missing(voice: dict) -> list[dict]:
    """Los archivos de la voz que faltan (o estan a medias)."""
    files = voice["files"]
    return [files[i] for i in _rust.cues_missing([(str(f["dest"]), f["bytes"]) for f in files])]


def installed(voice_id: str) -> bool:
    try:
        return not _missing(_find(voice_id))
    except CueError:
        return False


def available() -> str:
    """Por que no se pueden decir guias aqui, o vacio si se puede."""
    import importlib.util

    for module in ("numpy", "onnxruntime"):
        if importlib.util.find_spec(module) is None:
            return f"falta {module} en esta instalación de DanPlay"
    return ""


_LOCK = threading.Lock()
# lo que se esta bajando, por paquete: {"got", "total", "error"}
_INSTALLS: dict[str, dict] = {}


def voices() -> list[dict]:
    """Todas las voces: las bajadas y las que se pueden bajar (con lo que
    pesa lo que les falta y, si se estan bajando, por donde van)."""
    with _LOCK:
        states = {k: (s["got"], s["total"], s.get("error") or "") for k, s in _INSTALLS.items()}
    return _catalog().voices(str(_voices_dir()), states)


def _why(e: Exception) -> str:
    code = getattr(e, "code", None)
    if isinstance(code, int):
        return f"el servidor contestó con un error {code}"
    low = str(e).lower()
    if "timed out" in low or "timeout" in low:
        return "tarda demasiado en responder"
    if "resolve" in low or "name or service" in low or "getaddrinfo" in low:
        return "sin conexión a internet"
    return str(e)[:160]


def _download(f: dict, progress) -> None:
    """Un archivo de una voz, comprobado con su sha256; a un temporal y a su
    sitio al final (cortarlo no deja nada roto)."""
    target: Path = f["dest"]
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = target.with_name(target.name + ".part")
    digest = hashlib.sha256()
    got = 0
    req = urllib.request.Request(f["url"], headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r, open(tmp, "wb") as out:
            while block := r.read(1 << 20):
                out.write(block)
                digest.update(block)
                got += len(block)
                if got > f["bytes"]:
                    raise CueError("la voz bajada pesa más de lo que debe")
                progress(len(block))
    except CueError:
        tmp.unlink(missing_ok=True)
        raise
    except Exception as e:
        tmp.unlink(missing_ok=True)
        raise CueError(f"no se pudo bajar la voz ({_why(e)})") from e
    if digest.hexdigest() != f["sha256"]:
        tmp.unlink(missing_ok=True)
        raise CueError("la voz bajada no es la que debe (sha256 distinto): no se usa")
    os.replace(tmp, target)


def install(voice_id: str, *, wait: bool = False) -> dict:
    """Baja lo que le falte a la voz, en segundo plano (o esperando, con
    `wait`). Devuelve como va: {installed, progress, error}."""
    v = _find(voice_id)
    missing = _missing(v)
    if not missing:
        return {"installed": True}
    pack = v["pack"]
    with _LOCK:
        running = pack in _INSTALLS and not _INSTALLS[pack].get("error")
        if not running:
            _INSTALLS[pack] = {"got": 0, "total": sum(f["bytes"] for f in missing), "error": ""}

    def work() -> None:
        def advance(n: int) -> None:
            with _LOCK:
                _INSTALLS[pack]["got"] += n

        try:
            for f in missing:
                _download(f, advance)
        except CueError as e:
            with _LOCK:
                _INSTALLS[pack]["error"] = str(e)
            return
        except Exception as e:
            log.warning("no se pudo bajar la voz %s", voice_id, exc_info=True)
            with _LOCK:
                _INSTALLS[pack]["error"] = f"no se pudo bajar la voz ({_why(e)})"
            return
        with _LOCK:
            _INSTALLS.pop(pack, None)

    if not running:
        if wait:
            work()
        else:
            threading.Thread(target=work, name="danplay-voz", daemon=True).start()
    with _LOCK:
        state = dict(_INSTALLS.get(pack) or {})
    if not state:
        return {"installed": True}
    if state.get("error"):
        raise CueError(state["error"])
    return {"installed": False, "progress": round(state["got"] / max(1, state["total"]), 3)}


# ------------------------------------------------------------ decir


class _Loaded:
    """La voz cargada (sus grafos en memoria), que se suelta sola tras un
    rato sin usarla."""

    def __init__(self) -> None:
        self.key = ""
        self.engine = None
        self.used = 0.0
        self.timer: threading.Timer | None = None

    def get(self, key: str, make):
        if self.key != key or self.engine is None:
            self.engine = None
            self.engine = make()
            self.key = key
        self.used = time.monotonic()
        if self.timer:
            self.timer.cancel()
        self.timer = threading.Timer(IDLE, self.release)
        self.timer.daemon = True
        self.timer.start()
        return self.engine

    def release(self) -> None:
        with _SPEAK:
            if time.monotonic() - self.used >= IDLE - 1:
                self.engine = None
                self.key = ""


_SPEAK = threading.Lock()
_LOADED = _Loaded()


def _onnxruntime() -> str:
    from . import pulse

    return pulse.onnxruntime_library()


def say(text: str, voice_id: str):
    """El texto dicho por esa voz: (muestras en mono, su frecuencia). La voz
    tiene que estar bajada."""
    v = _find(voice_id)
    if _missing(v):
        raise CueError("esa voz aún no está bajada")
    reason = available()
    if reason:
        raise CueError(reason)
    lang = "en" if v.get("lang", "es").startswith("en") else "es"
    spoken = habla.normalize(text, lang)
    if not spoken.strip(" ."):
        raise CueError("no hay nada que decir")
    with _SPEAK:
        try:
            if v["engine"] == "supertonic":
                root = _voices_dir() / "supertonic-3"
                engine = _LOADED.get(
                    "supertonic", lambda: _rust.CueSupertonic(_onnxruntime(), str(root))
                )
                return engine.say(spoken, v["style"], lang)
            model = _voices_dir() / "piper" / v["model"] / f"{v['model']}.onnx"
            engine = _LOADED.get(
                "piper-" + v["model"], lambda: _rust.CuePiper(_onnxruntime(), str(model))
            )
            return engine.say(spoken, v.get("speaker"))
        except CueError:
            raise
        except Exception as e:
            log.warning("la voz %s fallo", voice_id, exc_info=True)
            raise CueError("la voz no pudo decirlo") from e


# ------------------------------------------------------------ el archivo


def polish(wav, rate: int):
    """Lista para sonar en su punto: sin el silencio de delante y de detras
    (la guia empieza justo donde se puso), a -1 dBFS de pico y con un fundido
    cortito en los bordes (sin chasquidos)."""
    import numpy as np

    return _rust.cues_polish(np.asarray(wav, np.float32).reshape(-1), int(rate))


def write_wav(path: Path, wav, rate: int) -> None:
    import numpy as np

    x = np.asarray(wav)
    x = x.reshape(-1) if x.dtype == np.float32 else np.asarray(x, np.float64).reshape(-1)
    _rust.cues_write_wav(str(path), x, int(rate))


def _seconds(path: Path) -> float:
    return _rust.cues_seconds(str(path))


def _slug(text: str) -> str:
    return _rust.cues_slug(text)


def make(text: str, voice_id: str, *, preview: bool = False) -> dict:
    """La guia escrita, dicha con esa voz, en su archivo: {file, seconds}. La
    misma frase con la misma voz es el mismo archivo (no se dice otra vez).
    Con `preview`, en la carpeta de pruebas (que se vacia sola)."""
    return _rust.cues_make(
        str(text or ""), voice_id, VERSION, str(folder()), str(_previews()), bool(preview), say
    )


def _decode(
    path: str, longest: float = MAX_SECONDS, too_long: str = "es demasiado largo para una guía"
):
    """Un archivo de audio a mono: (muestras, frecuencia). WAV directamente;
    lo demas, con ffmpeg (hasta el doble de `longest`, para poder decir que
    es demasiado largo)."""
    return _rust.cues_decode(str(path), float(longest), too_long, convert.tool("ffmpeg"))


def import_recording(path: str, longest: float | None = None) -> dict:
    """Una guia grabada (lo que dejo la app al grabar, o un audio del
    equipo) a su sitio, recortada y a su volumen: {file, seconds}.
    `longest`: lo que dura la cancion (un audio importado no puede durar
    mas), y como mucho MAX_IMPORT_SECONDS; sin el, un minuto."""
    return _rust.cues_import_recording(
        str(path or ""),
        float(longest) if longest else None,
        float(MAX_SECONDS),
        float(MAX_IMPORT_SECONDS),
        sorted(AUDIO_EXTS),
        str(folder()),
        uuid.uuid4().hex[:12],
        _temp_dir(),
        convert.tool("ffmpeg"),
    )


def wave_of(file: str, buckets: int = 96) -> dict:
    """La onda de una guia, para verla en su bloque y en el editor: {seconds,
    peaks} (el pico de cada trozo, de 0 a 1, a su escala)."""
    own = own_file(file, previews=True)
    if not own or not os.path.isfile(own):
        raise CueError("esa guía no está")
    return _rust.cues_wave_of(own, int(buckets), float(MAX_IMPORT_SECONDS), convert.tool("ffmpeg"))


# lo que se le puede hacer a una guia grabada o importada, como en un editor
# (el tope de los fundidos, el de analisis/src/guias.rs)
EDIT_FADE_MAX = 5.0


def edit_clip(
    file: str,
    *,
    start: float = 0.0,
    end: float | None = None,
    fade_in: float = 0.0,
    fade_out: float = 0.0,
    denoise: bool = False,
    highpass: bool = False,
    normalize: bool = False,
    preview: bool = False,
) -> dict:
    """Una guia nueva a partir de otra (la de antes no se toca): recortada de
    `start` a `end` (segundos), con un fundido de entrada y otro de salida,
    sin el ruido de fondo (`denoise`), sin los graves y golpes de microfono
    (`highpass`) y a su volumen (`normalize`). {file, seconds}. Con
    `preview`, en la carpeta de pruebas (para escucharla antes)."""
    own = own_file(file, previews=True)
    if not own or not os.path.isfile(own):
        raise CueError("esa guía no está")
    return _rust.cues_edit_clip(
        own,
        float(start or 0.0),
        None if end is None else float(end),
        float(fade_in or 0.0),
        float(fade_out or 0.0),
        bool(denoise),
        bool(highpass),
        bool(normalize),
        str(folder()),
        str(_previews()),
        bool(preview),
        float(MAX_IMPORT_SECONDS),
        convert.tool("ffmpeg"),
        _ffmpeg_filter,
    )


def _ffmpeg_filter(x, rate: int, chain: str):
    """Las muestras (mono) por un filtro de ffmpeg (quitar ruido, graves)."""
    import numpy as np

    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise CueError("hace falta ffmpeg para limpiar la guía")
    return _rust.cues_ffmpeg_filter(ffmpeg, np.asarray(x, np.float32).reshape(-1), int(rate), chain)


def _temp_dir() -> str:
    import tempfile

    return tempfile.gettempdir()


def own_file(path, *, previews: bool = False) -> str | None:
    """La ruta, si es una guia de DanPlay (de su carpeta; con `previews`,
    tambien las de prueba); si no, None."""
    if isinstance(path, str) and "\0" in path:
        return None
    return _rust.songs_own_cue_file(path, str(folder()), previews)


def audio(path: str) -> bytes:
    """El archivo de una guia, para escucharla."""
    own = own_file(path, previews=True)
    if not own or not os.path.isfile(own):
        raise CueError("no encuentro esa guía")
    return Path(own).read_bytes()


def sweep_previews(max_age: float = 3600.0) -> int:
    """Quita las pruebas de hace mas de una hora. Devuelve cuantas."""
    return _rust.cues_sweep_previews(str(_previews()), float(max_age))
