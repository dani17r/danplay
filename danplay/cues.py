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
"""

import contextlib
import functools
import hashlib
import json
import logging
import os
import re
import shutil
import subprocess
import threading
import time
import unicodedata
import urllib.request
import uuid
import wave
from pathlib import Path

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
# Lo que queda alrededor de la voz al recortar el silencio.
MARGIN = 0.03
# Tras este rato sin decir nada, se suelta la voz cargada (la de Supertonic
# ocupa medio GB de memoria).
IDLE = 180.0
AUDIO_EXTS = {".wav", ".flac", ".mp3", ".ogg", ".opus", ".m4a", ".aac", ".webm"}


class CueError(Exception):
    """No se pudo hacer la guia (por que, en castellano)."""


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


def _find(voice_id: str) -> dict:
    """La voz por su id, con su motor y los archivos que necesita (cada uno
    con su URL y donde va)."""
    cat = catalog()
    st = cat["supertonic"]
    for v in st["voices"]:
        if v["id"] == voice_id:
            files = [
                {
                    **f,
                    "url": st["base"] + f["path"],
                    "dest": _voices_dir() / "supertonic-3" / f["path"],
                }
                for f in st["files"]
            ]
            return {
                **v,
                "engine": "supertonic",
                "pack": "supertonic-3",
                "lang": "es",
                "licence": st["licence"],
                "files": files,
            }
    pi = cat["piper"]
    for v in pi["voices"]:
        if v["id"] == voice_id:
            files = [
                {
                    **f,
                    "url": pi["base"] + f["path"],
                    "dest": _voices_dir() / "piper" / v["model"] / Path(f["path"]).name,
                }
                for f in v["files"]
            ]
            return {**v, "engine": "piper", "pack": "piper-" + v["model"], "files": files}
    raise CueError("no conozco esa voz")


def _missing(voice: dict) -> list[dict]:
    """Los archivos de la voz que faltan (o estan a medias)."""
    out = []
    for f in voice["files"]:
        try:
            if f["dest"].stat().st_size == f["bytes"]:
                continue
        except OSError:
            pass
        out.append(f)
    return out


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
    out = []
    cat = catalog()
    ids = [v["id"] for v in cat["supertonic"]["voices"]] + [v["id"] for v in cat["piper"]["voices"]]
    for voice_id in ids:
        v = _find(voice_id)
        missing = _missing(v)
        with _LOCK:
            state = dict(_INSTALLS.get(v["pack"]) or {})
        item = {
            "id": v["id"],
            "name": v["name"],
            "lang": {
                "es": "castellano",
                "es-ES": "España",
                "es-MX": "México",
                "es-AR": "Argentina",
            }.get(v.get("lang", "es"), v.get("lang", "es")),
            "gender": v.get("gender", ""),
            "engine": v["engine"],
            "licence": v.get("licence", ""),
            "installed": not missing,
            "size": sum(f["bytes"] for f in missing),
        }
        if state and not item["installed"]:
            item["progress"] = round(state["got"] / max(1, state["total"]), 3)
            if state.get("error"):
                item["error"] = state["error"]
        out.append(item)
    return out


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


def _session(path: Path):
    import onnxruntime as ort

    opts = ort.SessionOptions()
    opts.log_severity_level = 3
    return ort.InferenceSession(str(path), sess_options=opts, providers=["CPUExecutionProvider"])


class _Supertonic:
    STEPS = 8
    SPEED = 1.05

    def __init__(self, root: Path) -> None:
        onnx = root / "onnx"
        self.root = root
        self.cfg = json.loads((onnx / "tts.json").read_text(encoding="utf-8"))
        self.indexer = json.loads((onnx / "unicode_indexer.json").read_text(encoding="utf-8"))
        self.dp = _session(onnx / "duration_predictor.onnx")
        self.enc = _session(onnx / "text_encoder.onnx")
        self.est = _session(onnx / "vector_estimator.onnx")
        self.voc = _session(onnx / "vocoder.onnx")
        self.rate = int(self.cfg["ae"]["sample_rate"])
        ttl = self.cfg["ttl"]
        self.chunk = int(self.cfg["ae"]["base_chunk_size"]) * int(ttl["chunk_compress_factor"])
        self.ldim = int(ttl["latent_dim"]) * int(ttl["chunk_compress_factor"])

    @functools.cache  # noqa: B019 - una por estilo, mientras viva la voz
    def style(self, name: str):
        import numpy as np

        v = json.loads((self.root / "voice_styles" / f"{name}.json").read_text(encoding="utf-8"))

        def arr(k):
            return np.array(v[k]["data"], np.float32).reshape(v[k]["dims"])

        return arr("style_ttl"), arr("style_dp")

    def say(self, text: str, style: str, lang: str = "es"):
        import numpy as np

        text = unicodedata.normalize("NFKD", text)
        ids = [
            self.indexer[ord(c)] if ord(c) < len(self.indexer) else -1
            for c in f"<{lang}>{text}</{lang}>"
        ]
        ids = np.array([[i for i in ids if i >= 0]], np.int64)
        tmask = np.ones((1, 1, ids.shape[1]), np.float32)
        s_ttl, s_dp = self.style(style)
        dur = (
            np.asarray(
                self.dp.run(None, {"text_ids": ids, "style_dp": s_dp, "text_mask": tmask})[0]
            )
            / self.SPEED
        )
        emb = self.enc.run(None, {"text_ids": ids, "style_ttl": s_ttl, "text_mask": tmask})[0]
        seconds = float(np.ravel(dur)[0])
        n_lat = max(1, int(np.ceil(seconds * self.rate / self.chunk)))
        # siempre la misma semilla: la misma frase suena igual cada vez
        x = np.random.default_rng(0).standard_normal((1, self.ldim, n_lat)).astype(np.float32)
        lmask = np.ones((1, 1, n_lat), np.float32)
        total = np.array([self.STEPS], np.float32)
        for i in range(self.STEPS):
            x = self.est.run(
                None,
                {
                    "noisy_latent": x,
                    "text_emb": emb,
                    "style_ttl": s_ttl,
                    "latent_mask": lmask,
                    "text_mask": tmask,
                    "current_step": np.array([i], np.float32),
                    "total_step": total,
                },
            )[0]
        wav = np.asarray(self.voc.run(None, {"latent": x})[0])[0]
        return np.asarray(wav[: int(seconds * self.rate)], np.float32), self.rate


class _Piper:
    def __init__(self, model: Path) -> None:
        self.cfg = json.loads(model.with_name(model.name + ".json").read_text(encoding="utf-8"))
        self.sess = _session(model)
        self.rate = int(self.cfg["audio"]["sample_rate"])

    def say(self, text: str, speaker: int | None):
        import numpy as np

        phones = habla.phonemes(text, self.cfg.get("espeak", {}).get("voice", "es"))
        ids = habla.piper_ids(phones, self.cfg["phoneme_id_map"])
        inf = self.cfg.get("inference", {})
        feeds = {
            "input": np.array([ids], np.int64),
            "input_lengths": np.array([len(ids)], np.int64),
            "scales": np.array(
                [
                    inf.get("noise_scale", 0.667),
                    inf.get("length_scale", 1.0),
                    inf.get("noise_w", 0.8),
                ],
                np.float32,
            ),
        }
        if int(self.cfg.get("num_speakers", 1)) > 1:
            feeds["sid"] = np.array([speaker or 0], np.int64)
        return np.asarray(self.sess.run(None, feeds)[0], np.float32).reshape(-1), self.rate


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
                engine = _LOADED.get("supertonic", lambda: _Supertonic(root))
                return engine.say(spoken, v["style"], lang)
            model = _voices_dir() / "piper" / v["model"] / f"{v['model']}.onnx"
            engine = _LOADED.get("piper-" + v["model"], lambda: _Piper(model))
            return engine.say(spoken, v.get("speaker"))
        except CueError:
            raise
        except Exception as e:
            log.warning("la voz %s fallo", voice_id, exc_info=True)
            raise CueError("la voz no pudo decirlo") from e


# ------------------------------------------------------------ el archivo


def polish(wav, rate: int):
    """Lista para sonar en su punto: sin el silencio de delante y de detras
    (la guia empieza justo donde se puso), a -1 dBFS de pico y con un
    fundido cortito en los bordes (sin chasquidos)."""
    import numpy as np

    wav = np.nan_to_num(np.asarray(wav, np.float32).reshape(-1))
    if not wav.size:
        return wav
    peak = float(np.max(np.abs(wav)))
    if peak <= 1e-6:
        return wav[:0]
    # el silencio: por debajo de -45 dB del pico, en ventanas de 10 ms
    win = max(1, rate // 100)
    n = wav.size // win
    if n > 2:
        rms = np.sqrt(np.mean(wav[: n * win].reshape(n, win) ** 2, axis=1))
        loud = np.flatnonzero(rms > peak * 10 ** (-45 / 20))
        if loud.size:
            margin = int(MARGIN * rate)
            a = max(0, loud[0] * win - margin)
            b = min(wav.size, (loud[-1] + 1) * win + margin)
            wav = wav[a:b]
    wav = wav * (10 ** (-1 / 20) / peak)
    fade = min(wav.size // 2, int(0.008 * rate))
    if fade > 0:
        ramp = np.linspace(0.0, 1.0, fade, dtype=np.float32)
        wav[:fade] *= ramp
        wav[-fade:] *= ramp[::-1]
    return wav.astype(np.float32)


def write_wav(path: Path, wav, rate: int) -> None:
    import numpy as np

    path.parent.mkdir(parents=True, exist_ok=True)
    pcm = (np.clip(wav, -1, 1) * 32767).astype("<i2")
    tmp = path.with_name(path.name + ".part")
    with wave.open(str(tmp), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(int(rate))
        w.writeframes(pcm.tobytes())
    os.replace(tmp, path)


def _seconds(path: Path) -> float:
    with contextlib.suppress(OSError, wave.Error, EOFError), wave.open(str(path), "rb") as w:
        return round(w.getnframes() / max(1, w.getframerate()), 2)
    return 0.0


def _slug(text: str) -> str:
    ascii_text = unicodedata.normalize("NFKD", text.lower()).encode("ascii", "ignore").decode()
    s = re.sub(r"[^a-z0-9]+", "-", ascii_text)
    return s.strip("-")[:32] or "guia"


def make(text: str, voice_id: str, *, preview: bool = False) -> dict:
    """La guia escrita, dicha con esa voz, en su archivo: {file, seconds}. La
    misma frase con la misma voz es el mismo archivo (no se dice otra vez).
    Con `preview`, en la carpeta de pruebas (que se vacia sola)."""
    text = str(text or "").strip()[:300]
    if not text:
        raise CueError("no hay nada que decir")
    key = hashlib.sha1(f"{VERSION}|{voice_id}|{text}".encode()).hexdigest()[:12]  # noqa: S324
    target = (_previews() if preview else folder()) / f"{_slug(text)}-{key}.wav"
    if not target.is_file():
        # ya dicha como prueba: se aprovecha
        tried = _previews() / target.name
        if not preview and tried.is_file():
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(tried, target)
        else:
            wav, rate = say(text, voice_id)
            wav = polish(wav, rate)
            if not wav.size:
                raise CueError("la voz no dijo nada")
            write_wav(target, wav, rate)
    if preview:
        sweep_previews()
    return {"file": str(target), "seconds": _seconds(target)}


def _decode(path: str):
    """Un archivo de audio a mono: (muestras, frecuencia). WAV directamente;
    lo demas, con ffmpeg."""
    import numpy as np

    with contextlib.suppress(wave.Error, EOFError, OSError), wave.open(path, "rb") as w:
        width, channels, rate = w.getsampwidth(), w.getnchannels(), w.getframerate()
        if w.getnframes() > MAX_SECONDS * rate * 4:
            raise CueError("la grabación es demasiado larga para una guía")
        data = w.readframes(w.getnframes())
        if width == 2:
            x = np.frombuffer(data, "<i2").astype(np.float32) / 32768
        elif width == 4:
            x = np.frombuffer(data, "<i4").astype(np.float32) / 2147483648
        elif width == 1:
            x = (np.frombuffer(data, np.uint8).astype(np.float32) - 128) / 128
        else:
            x = None
        if x is not None:
            return x.reshape(-1, channels).mean(axis=1), rate
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise CueError("hace falta ffmpeg para leer esa grabación")
    rate = 44100
    r = subprocess.run(
        [
            ffmpeg,
            "-v",
            "error",
            "-nostdin",
            "-i",
            path,
            "-t",
            str(MAX_SECONDS * 2),
            "-ac",
            "1",
            "-ar",
            str(rate),
            "-f",
            "f32le",
            "-",
        ],
        capture_output=True,
        timeout=120,
        check=False,
    )
    if r.returncode != 0 or not r.stdout:
        raise CueError("no se pudo leer la grabación")
    return np.frombuffer(r.stdout, np.float32), rate


def import_recording(path: str) -> dict:
    """Una guia grabada (lo que dejo la app al grabar, o un audio del
    equipo) a su sitio, recortada y a su volumen: {file, seconds}."""
    src = Path(str(path or ""))
    if not src.is_file() or src.suffix.lower() not in AUDIO_EXTS:
        raise CueError("no encuentro la grabación")
    if src.stat().st_size > 200 * 1024 * 1024:
        raise CueError("la grabación es demasiado grande para una guía")
    wav, rate = _decode(str(src))
    if wav.size > MAX_SECONDS * rate:
        raise CueError("la grabación es demasiado larga para una guía (un minuto como mucho)")
    wav = polish(wav, rate)
    if not wav.size:
        raise CueError("la grabación está en silencio")
    target = folder() / f"grabada-{uuid.uuid4().hex[:12]}.wav"
    write_wav(target, wav, rate)
    # la temporal de la app ya no hace falta
    if src.parent.resolve() == Path(_temp_dir()).resolve() and src.name.startswith("danplay-guia-"):
        with contextlib.suppress(OSError):
            src.unlink()
    return {"file": str(target), "seconds": _seconds(target)}


def _temp_dir() -> str:
    import tempfile

    return tempfile.gettempdir()


def own_file(path, *, previews: bool = False) -> str | None:
    """La ruta, si es una guia de DanPlay (de su carpeta; con `previews`,
    tambien las de prueba); si no, None."""
    if not path or not isinstance(path, str) or len(path) > 1024:
        return None
    try:
        p = Path(path).resolve()
        base = folder().resolve()
    except (OSError, ValueError):
        return None
    if p.suffix.lower() != ".wav":
        return None
    if p.parent == base or (previews and p.parent == base / "prueba"):
        return str(p)
    return None


def audio(path: str) -> bytes:
    """El archivo de una guia, para escucharla."""
    own = own_file(path, previews=True)
    if not own or not os.path.isfile(own):
        raise CueError("no encuentro esa guía")
    return Path(own).read_bytes()


def sweep_previews(max_age: float = 3600.0) -> int:
    """Quita las pruebas de hace mas de una hora. Devuelve cuantas."""
    n = 0
    now = time.time()
    with contextlib.suppress(OSError):
        for f in _previews().iterdir():
            with contextlib.suppress(OSError):
                if now - f.stat().st_mtime > max_age:
                    f.unlink()
                    n += 1
    return n
