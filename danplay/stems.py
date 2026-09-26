"""Separar una cancion en pistas: bateria, bajo, voces, guitarra, piano y el
resto, cada una en su archivo.

Las pistas de una cancion van en una carpeta con su nombre, dentro de
`Separadas/` en la carpeta de la biblioteca de la que cuelga la cancion:

    Separadas/Miel San Marcos - Que Se Abra El Cielo/
        Bateria.flac  Voces.flac  Bajo.flac  Guitarra.flac  Piano.flac  Otros.flac
        .danplay-pistas.json   de que cancion son, con que modelo, que hay
        .danplay-ondas.json    la forma de onda de cada pista

Son archivos normales: se abren con cualquier programa (un DAW, Ableton, el
reproductor del sistema). Dentro de DanPlay suenan juntas en el modo
estudio, cada una con su volumen, y se pueden callar o dejar solas.

Como todo lo que el usuario crea, no depende de la base: la carpeta dice de
que cancion es (`.danplay-pistas.json`), y un escaneo vuelve a unirlas si el
indice se pierde. El escaneo no mete estas pistas en la biblioteca como
canciones sueltas: una carpeta con ese archivo se salta.

La separacion es Demucs v4 con ONNX Runtime (`separation.py`), en un proceso
aparte y con prioridad baja. Tarda en torno a lo que dura la cancion en un
portatil corriente, asi que va en una cola: se pueden pedir varias y se
separan una detras de otra mientras se sigue usando la app.
"""

import contextlib
import hashlib
import importlib.util
import json
import logging
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from collections.abc import Mapping
from pathlib import Path
from typing import Any

from . import config, convert
from .api import jobs

log = logging.getLogger(__name__)

GRAPHS = Path(__file__).resolve().parent / "data" / "separador"
FOLDER = "Separadas"
MANIFEST = ".danplay-pistas.json"
WAVES = ".danplay-ondas.json"
JOB = "separacion"
MIX_JOB = "mezcla"

# Cada fuente de Demucs en castellano: como se llama su archivo (sin tildes,
# como todos los de la casa) y como se enseña. En el orden del mezclador.
SOURCES = {
    "drums": ("Bateria", "Batería"),
    "vocals": ("Voces", "Voces"),
    "bass": ("Bajo", "Bajo"),
    "guitar": ("Guitarra", "Guitarra"),
    "piano": ("Piano", "Piano"),
    "other": ("Otros", "Otros"),
}

# Los modelos que se ofrecen. El de seis separa tambien guitarra y piano; el
# de cuatro deja esos dos en «Otros» y es algo mas limpio en lo demas.
MODELS = {
    "6": {
        "graph": "htdemucs_6s",
        "label": "6 pistas",
        "detail": "batería, voces, bajo, guitarra, piano y el resto",
    },
    "4": {
        "graph": "htdemucs",
        "label": "4 pistas",
        "detail": "batería, voces, bajo y el resto (algo más limpias)",
    },
}
DEFAULT_MODEL = "6"

USER_AGENT = "DanPlay (+https://github.com/dani17r/danplay)"
TIMEOUT = 60


class SeparateError(jobs.JobError):
    """Un fallo previsto, ya explicado en castellano."""


# ------------------------------------------------------------ el motor


def _manifest_of(model: str) -> dict:
    graph = MODELS[model]["graph"]
    return json.loads((GRAPHS / f"{graph}.json").read_text(encoding="utf-8"))


def _weights_dir() -> Path:
    return config.DATA_DIR / "separador"


def weights_path(model: str) -> Path:
    return _weights_dir() / _manifest_of(model)["weights"]["file"]


def installed(model: str) -> bool:
    """Los pesos de ese modelo ya estan bajados (y enteros: se comprobo su
    sha256 al bajarlos, y aqui el tamaño)."""
    try:
        want = _manifest_of(model)["weights"]["bytes"]
        return weights_path(model).stat().st_size == want
    except (OSError, KeyError, ValueError):
        return False


def missing() -> str:
    """Por que no se puede separar en este equipo, o vacio si se puede."""
    for module in ("numpy", "onnxruntime"):
        if importlib.util.find_spec(module) is None:
            return f"falta {module} en esta instalación de DanPlay"
    if not convert.tool("ffmpeg"):
        return "hace falta ffmpeg"
    if not all((GRAPHS / f"{m['graph']}.onnx").is_file() for m in MODELS.values()):
        return "faltan los archivos del separador en esta instalación"
    return ""


def status() -> dict:
    """Si se puede separar, que modelos hay (y si ya estan bajados), y la cola."""
    reason = missing()
    models = []
    for key, m in MODELS.items():
        item: dict[str, Any] = {"id": key, "label": m["label"], "detail": m["detail"]}
        with contextlib.suppress(OSError, ValueError, KeyError):
            manifest = _manifest_of(key)
            item["sources"] = [s for s in SOURCES if s in manifest["sources"]]
            item["bytes"] = manifest["weights"]["bytes"]
            item["installed"] = installed(key)
        models.append(item)
    with _lock:
        queue = [dict(q) for q in _queue]
        current = dict(_current) if _current else None
    return {
        "ok": not reason,
        "reason": reason,
        "models": models,
        "default": DEFAULT_MODEL,
        "folder": FOLDER,
        "current": current,
        "queue": queue,
    }


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


def download(model: str, progress=None, cancelled=None) -> Path:
    """Baja los pesos del modelo del autor (HuggingFace), una sola vez.

    Se comprueba el sha256 que viaja con la app: un archivo cambiado o a
    medias no se usa. Se escribe a un temporal y se pone en su sitio al
    final, asi que cortarlo no deja nada roto.
    """
    if installed(model):
        return weights_path(model)
    info = _manifest_of(model)["weights"]
    target = weights_path(model)
    target.parent.mkdir(parents=True, exist_ok=True)
    part = target.with_suffix(".part")
    digest = hashlib.sha256()
    got = 0
    req = urllib.request.Request(info["url"], headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r, open(part, "wb") as f:
            while True:
                if cancelled and cancelled():
                    raise SeparateError("cancelado")
                block = r.read(1 << 20)
                if not block:
                    break
                f.write(block)
                digest.update(block)
                got += len(block)
                if got > info["bytes"] + (1 << 20):
                    raise SeparateError("la descarga del separador pesa más de lo que debe")
                if progress:
                    progress(got, info["bytes"])
    except SeparateError:
        part.unlink(missing_ok=True)
        raise
    except Exception as e:
        part.unlink(missing_ok=True)
        raise SeparateError(f"no se pudo bajar el separador ({_why(e)})") from e
    if digest.hexdigest() != info["sha256"]:
        part.unlink(missing_ok=True)
        raise SeparateError("el separador bajado no es el que debe (sha256 distinto): no se usa")
    os.replace(part, target)
    return target


def remove_weights(model: str) -> bool:
    """Borra los pesos bajados de ese modelo (se vuelven a bajar si hacen falta)."""
    try:
        weights_path(model).unlink()
        return True
    except FileNotFoundError:
        return False


# ------------------------------------------------------------ la carpeta


def _stems_of(song: Mapping[str, Any]) -> dict:
    try:
        data = json.loads(song.get("stems") or "{}")
    except (TypeError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def folder_of(song: Mapping[str, Any]) -> Path | None:
    """La carpeta de las pistas de la cancion, si tiene."""
    rel = _stems_of(song).get("folder")
    if not rel:
        return None
    return Path(song.get("root") or config.LIBRARY) / rel


def read_manifest(folder: Path) -> dict | None:
    try:
        data = json.loads((folder / MANIFEST).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def info(song: Mapping[str, Any]) -> dict | None:
    """Las pistas de la cancion tal como estan en el disco, o None.

    Cada pista con su ruta, su nombre y si el archivo sigue ahi; y la onda
    de cada una, si esta. Si la carpeta ya no esta (se borro a mano), None.
    """
    folder = folder_of(song)
    if folder is None:
        return None
    manifest = read_manifest(folder)
    if manifest is None:
        return None
    try:
        waves = json.loads((folder / WAVES).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        waves = {}
    tracks = []
    for t in manifest.get("tracks") or []:
        path = folder / str(t.get("file") or "")
        source = str(t.get("source") or "")
        tracks.append(
            {
                "source": source,
                "name": t.get("name") or SOURCES.get(source, (source, source))[1],
                "file": t.get("file"),
                "path": str(path),
                "exists": path.is_file(),
                "wave": (waves.get("tracks") or {}).get(source),
            }
        )
    return {
        "folder": str(folder),
        "model": manifest.get("model", ""),
        "created": manifest.get("created"),
        "tracks": tracks,
        "complete": bool(tracks) and all(t["exists"] for t in tracks),
    }


def _safe_name(text: str) -> str:
    bad = '<>:"/\\|?*'
    clean = "".join("_" if c in bad or ord(c) < 32 else c for c in text).strip(" .")
    return clean[:150] or "Cancion"


def _target_for(song: Mapping[str, Any]) -> Path:
    """Donde van las pistas de esta cancion: `Separadas/<nombre del archivo>`.

    Si ya hay una carpeta con ese nombre de OTRA cancion (dos archivos que se
    llaman igual en carpetas distintas), se le pone un numero.
    """
    base = Path(song["root"]) / FOLDER
    name = _safe_name(Path(song["file"]).stem)
    candidate, n = base / name, 2
    while candidate.exists():
        manifest = read_manifest(candidate)
        if manifest and _is_source(manifest, song):
            return candidate
        candidate, n = base / f"{name} ({n})", n + 1
    return candidate


def _is_source(manifest: dict, song: Mapping[str, Any]) -> bool:
    src = manifest.get("source") or {}
    return src.get("file") == song.get("file") and src.get("folder") == song.get("folder")


def is_stems_dir(path) -> bool:
    """La carpeta es de pistas separadas (el escaneo no la mete en la biblioteca)."""
    return os.path.isfile(os.path.join(path, MANIFEST))


# ------------------------------------------------------------ separar

_lock = threading.Lock()
_queue: list[dict] = []  # [{id, title, model}] lo que espera
_current: dict | None = None  # lo que se esta separando ahora
_child: subprocess.Popen | None = None
_cancel = threading.Event()
# La cola esta andando (`_drain`). Se mira y se cambia con `_lock`, junto con
# la cola: asi una cancion que se pide justo cuando la cola decide que ha
# acabado no se queda esperando para siempre.
_draining = False


def _command(job_file: str) -> list[str]:
    """Como se lanza el proceso que separa: el propio nucleo empaquetado con
    `--separar`, o este Python con el modulo del motor."""
    if config.FROZEN:
        return [sys.executable, "--separar", job_file]
    return [sys.executable, "-m", "danplay.separation", job_file]


def _title(song: Mapping[str, Any]) -> str:
    if song.get("artist") and song.get("title"):
        return f"{song['artist']} - {song['title']}"
    return song.get("title") or Path(song.get("file") or "").stem


def _run_child(job: dict, progress) -> dict:
    """Lanza el proceso que separa y va contando lo que dice."""
    global _child
    with tempfile.TemporaryDirectory(prefix="danplay-separar-") as tmp:
        job_file = os.path.join(tmp, "trabajo.json")
        Path(job_file).write_text(json.dumps(job), encoding="utf-8")
        flags = 0
        if sys.platform == "win32":  # pragma: no cover - sin ventana de consola
            flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
        child = subprocess.Popen(
            _command(job_file),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            creationflags=flags,
        )
        with _lock:
            _child = child
        errors: list[str] = []

        def drain():  # lo que diga por stderr, para el registro si falla
            assert child.stderr is not None
            for line in child.stderr:
                errors.append(line.rstrip())
                del errors[:-30]

        reader = threading.Thread(target=drain, daemon=True)
        reader.start()
        result: dict = {}
        failure = ""
        try:
            assert child.stdout is not None
            for line in child.stdout:
                try:
                    message = json.loads(line)
                except ValueError:
                    continue
                if "done" in message:
                    progress(message["done"], message["total"])
                elif message.get("step"):
                    progress(step=message["step"], seconds=message.get("seconds"))
                elif message.get("error"):
                    failure = message["error"]
                elif message.get("ok"):
                    result = message
            child.wait()
        finally:
            with _lock:
                _child = None
            if child.poll() is None:
                child.kill()
                child.wait()
        reader.join(timeout=2)
        if _cancel.is_set():
            raise SeparateError("cancelado")
        if not result:
            if errors:
                log.warning("el separador fallo:\n%s", "\n".join(errors))
            raise SeparateError(
                failure or f"el separador se cerró sin terminar ({child.returncode})"
            )
        return result


def _write_manifest(folder: Path, song: Mapping[str, Any], model: str, sources: list[str]) -> None:
    from . import __version__

    manifest = {
        "version": 1,
        "app": f"DanPlay {__version__}",
        "model": MODELS[model]["graph"],
        "created": round(time.time(), 1),
        "source": {
            "file": song["file"],
            "folder": song.get("folder") or "",
            "size": song.get("size") or 0,
            "duration": song.get("duration") or 0,
            "artist": song.get("artist") or "",
            "title": song.get("title") or "",
        },
        "tracks": [
            {"source": s, "file": f"{SOURCES[s][0]}.flac", "name": SOURCES[s][1]}
            for s in SOURCES
            if s in sources
        ],
    }
    (folder / MANIFEST).write_text(
        json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
    )


def _sweep(folder: Path) -> None:
    """Quita lo que dejo a medias una separacion que no acabo (la app se
    cerro, se fue la luz): la cola es una, asi que si ahora empieza otra,
    cualquier carpeta de trabajo que quede es de una que ya no sigue."""
    for leftover in folder.glob(".separando-*"):
        shutil.rmtree(leftover, ignore_errors=True)


def separate_song(cid: int, model: str = DEFAULT_MODEL, progress=None) -> dict:
    """Separa una cancion de la biblioteca y deja sus pistas en su carpeta.

    Se separa en una carpeta oculta junto a la de destino (el escaneo se
    salta las ocultas, asi que nunca ve pistas a medias) y al terminar se
    pone en su sitio de golpe. Si la cancion ya tenia pistas, las de antes
    van a la papelera cuando las nuevas ya estan.
    """
    from . import library

    progress = progress or (lambda *a, **k: None)
    song = library.by_id(cid)
    if not song:
        raise SeparateError("esa canción ya no está en la biblioteca")
    if not os.path.isfile(song["path"]):
        raise SeparateError(f"no encuentro «{song['file']}»")
    if model not in MODELS:
        raise SeparateError(f"no hay un separador «{model}»")
    reason = missing()
    if reason:
        raise SeparateError(f"no se puede separar: {reason}")
    manifest = _manifest_of(model)
    if not installed(model):
        progress(step="download")
        download(model, lambda got, total: progress(got, total, step="download"), _cancel.is_set)
    target = _target_for(song)
    target.parent.mkdir(parents=True, exist_ok=True)
    _sweep(target.parent)
    work = Path(tempfile.mkdtemp(prefix=".separando-", dir=target.parent))
    try:
        job = {
            "input": song["path"],
            "output": str(work),
            "files": {s: f"{SOURCES[s][0]}.flac" for s in manifest["sources"]},
            "graph": str(GRAPHS / f"{MODELS[model]['graph']}.onnx"),
            "manifest": str(GRAPHS / f"{MODELS[model]['graph']}.json"),
            "weights": str(weights_path(model)),
            "ffmpeg": convert.tool("ffmpeg"),
            "waves": str(work / WAVES),
        }
        result = _run_child(job, progress)
        _write_manifest(work, song, model, manifest["sources"])
        old = None
        if target.exists():
            old = target.with_name(f".antes-{target.name}-{int(time.time())}")
            os.replace(target, old)
        os.replace(work, target)
    except BaseException:
        shutil.rmtree(work, ignore_errors=True)
        raise
    if old is not None:
        # las pistas de antes (otro modelo, otra version): a la papelera
        r = library.trash_path(old)
        if not r["ok"]:
            log.warning("no pude mandar a la papelera las pistas viejas %s", old)
    rel = os.path.relpath(target, song["root"])
    library.update(cid, stems=json.dumps({"folder": rel, "model": MODELS[model]["graph"]}))
    return {"id": cid, "title": _title(song), "folder": str(target), **result}


def request(cid: int, model: str = DEFAULT_MODEL) -> dict:
    """Pide separar una cancion: a la cola, y la cola a andar si no lo esta.

    Devuelve el estado (`status`). Pedir otra vez una que ya espera o se
    esta separando no la repite.
    """
    from . import library

    song = library.by_id(cid)
    if not song:
        raise SeparateError("esa canción ya no está en la biblioteca")
    if model not in MODELS:
        raise SeparateError(f"no hay un separador «{model}»")
    reason = missing()
    if reason:
        raise SeparateError(f"no se puede separar: {reason}")
    global _draining
    with _lock:
        busy = (_current and _current["id"] == cid) or any(q["id"] == cid for q in _queue)
        if not busy:
            _queue.append({"id": cid, "title": _title(song), "model": model})
        wake = not _draining
        if wake:
            _draining = True
            _cancel.clear()
    if wake:
        # la tanda anterior pudo decidir que acababa y no haber cerrado aun
        # su trabajo: son microsegundos, pero sin esperar no arrancaria otro
        limit = time.monotonic() + 5
        while jobs.active(JOB) and time.monotonic() < limit:
            time.sleep(0.01)
        jobs.start(JOB, _drain, failure="no se pudo separar")
    return status()


def _note(done, total, step) -> None:
    """Como va la que se esta separando, para `status`."""
    with _lock:
        if _current is not None:
            _current.update(done=done or 0, total=total or 0, step=step or "separate")


def _drain(progress) -> dict:
    """El trabajo de la cola: separa lo que haya, una detras de otra."""
    global _draining
    try:
        return _drain_queue(progress)
    finally:
        with _lock:
            _draining = False


def _drain_queue(progress) -> dict:
    global _current, _draining
    done, failed = [], []
    while True:
        with _lock:
            if not _queue or _cancel.is_set():
                _current = None
                _queue.clear()
                _draining = False
                break
            _current = _queue.pop(0)
            item = dict(_current)
            left = len(_queue)
        count = len(done) + len(failed) + 1
        total = count + left
        where = f" ({count} de {total})" if total > 1 else ""

        def report(done_=None, total_=None, step=None, seconds=None, _item=item, _where=where):
            if step == "download":
                progress(done_, total_, f"Bajando el separador (una sola vez)…{_where}")
            elif step in ("load", "decode"):
                progress(0, 0, f"Preparando «{_item['title']}»…{_where}")
            elif step == "separate":
                progress(0, 0, f"Separando «{_item['title']}»…{_where}")
            elif done_ is not None:
                progress(done_, total_, f"Separando «{_item['title']}»{_where}")
            _note(done_, total_, step)

        try:
            done.append(separate_song(item["id"], item["model"], report))
        except SeparateError as e:
            if _cancel.is_set():
                break
            failed.append({**item, "error": str(e)})
        except Exception as e:
            log.warning("no se pudo separar %s", item, exc_info=True)
            failed.append({**item, "error": str(e) or e.__class__.__name__})
    with _lock:
        _current = None
    if _cancel.is_set() and not done:
        raise SeparateError("cancelado")
    if failed and not done:
        raise SeparateError(failed[0]["error"])
    return {"separated": done, "failed": failed, "cancelled": _cancel.is_set()}


def cancel() -> dict:
    """Para la separacion en marcha y vacia la cola."""
    _cancel.set()
    with _lock:
        _queue.clear()
        child = _child
    if child is not None and child.poll() is None:
        child.terminate()
    return status()


def unqueue(cid: int) -> dict:
    """Quita de la cola una cancion que aun espera."""
    with _lock:
        _queue[:] = [q for q in _queue if q["id"] != cid]
    return status()


# ------------------------------------------------------------ la biblioteca


def forget(song: Mapping[str, Any]) -> dict:
    """Manda a la papelera las pistas de la cancion y las olvida."""
    from . import library

    folder = folder_of(song)
    if folder is not None and folder.exists():
        if not library.within_roots(folder):
            return {"ok": False, "error": "esa carpeta está fuera de la biblioteca"}
        r = library.trash_path(folder)
        if not r["ok"]:
            return r
    library.update(song["id"], stems="")
    return {"ok": True, "folder": str(folder) if folder else ""}


def relink(found: dict[str, dict]) -> int:
    """Une con su cancion las carpetas de pistas que encontro el escaneo.

    `found`: {carpeta: manifiesto}. Para las canciones sin pistas en el
    indice (la base se perdio, o llegaron de otro equipo): se busca la de la
    misma carpeta y nombre, o si no la del mismo nombre y tamaño. Y al reves:
    la que apunta a una carpeta que ya no esta, la olvida.
    """
    from .library import db

    n = 0
    with db.connect() as conn:
        linked = {}
        for r in conn.execute("SELECT id, root, stems FROM songs WHERE COALESCE(stems,'') != ''"):
            with contextlib.suppress(ValueError, TypeError, AttributeError):
                rel = json.loads(r["stems"]).get("folder") or ""
                path = os.path.normpath(os.path.join(r["root"], rel))
                if rel and os.path.isfile(os.path.join(path, MANIFEST)):
                    linked[path] = r["id"]
                    continue
            conn.execute("UPDATE songs SET stems='' WHERE id=?", (r["id"],))
            n += 1
        for folder, manifest in found.items():
            folder = os.path.normpath(folder)
            if folder in linked:
                continue
            src = manifest.get("source") or {}
            if not src.get("file"):
                continue
            row = conn.execute(
                "SELECT id, root FROM songs WHERE file=? AND folder=? "
                "AND COALESCE(stems,'')='' ORDER BY id LIMIT 1",
                (src["file"], src.get("folder") or ""),
            ).fetchone()
            if row is None and src.get("size"):
                row = conn.execute(
                    "SELECT id, root FROM songs WHERE file=? AND size=? "
                    "AND COALESCE(stems,'')='' ORDER BY id LIMIT 1",
                    (src["file"], src["size"]),
                ).fetchone()
            if row is None:
                continue
            rel = os.path.relpath(folder, row["root"])
            data = {"folder": rel, "model": manifest.get("model", "")}
            conn.execute("UPDATE songs SET stems=? WHERE id=?", (json.dumps(data), row["id"]))
            linked[folder] = row["id"]
            n += 1
    return n


def stem_paths() -> set[str]:
    """Las rutas de todas las pistas separadas del indice (para no podar su
    forma de onda al escanear)."""
    from .library import db

    out: set[str] = set()
    with db.connect() as conn:
        for r in conn.execute("SELECT root, stems FROM songs WHERE COALESCE(stems,'') != ''"):
            with contextlib.suppress(ValueError, TypeError, AttributeError):
                folder = Path(r["root"]) / json.loads(r["stems"])["folder"]
                manifest = read_manifest(folder) or {}
                out.update(str(folder / t["file"]) for t in manifest.get("tracks") or [])
    return out


# ------------------------------------------------------------ exportar una mezcla

MIX_FORMATS = {
    "mp3": ["-c:a", "libmp3lame", "-b:a", "320k"],
    "flac": ["-c:a", "flac"],
    "wav": ["-c:a", "pcm_s16le"],
}


def _has_rubberband(ffmpeg: str) -> bool:
    try:
        r = subprocess.run(
            [ffmpeg, "-hide_banner", "-filters"], capture_output=True, text=True, check=False
        )
    except OSError:
        return False
    return " rubberband " in r.stdout


def _tempo_filter(tempo: float) -> str:
    """atempo solo admite de 0,5 a 2: fuera de eso se encadenan (como Rust)."""
    parts, left = [], min(3.0, max(0.25, tempo))
    while left < 0.5 - 1e-6:
        parts.append("atempo=0.5")
        left /= 0.5
    while left > 2.0 + 1e-6:
        parts.append("atempo=2.0")
        left /= 2.0
    parts.append(f"atempo={left:.4f}")
    return ",".join(parts)


def _speed_pitch(speed: float, pitch: float, rubberband: bool) -> str:
    """El mismo filtro que usa el reproductor (transcode.rs) para velocidad y tono."""
    if abs(pitch) <= 1e-3:
        return "" if abs(speed - 1) <= 1e-4 else _tempo_filter(speed)
    factor = 2 ** (max(-12.0, min(12.0, pitch)) / 12)
    if rubberband:
        return f"rubberband=tempo={speed:.4f}:pitch={factor:.5f}"
    return f"asetrate=44100*{factor:.5f},aresample=44100,{_tempo_filter(speed / factor)}"


def gains(gain: float, pan: float) -> tuple[float, float]:
    """Volumen y panorama de una pista en lo que se le aplica a cada canal.

    Es un balance, como el de un equipo de musica: al centro, los dos
    canales tal cual; hacia un lado, el otro baja hasta callar. Es lo mismo
    que hace el reproductor, para que lo exportado suene como lo que se oye.
    """
    pan = max(-1.0, min(1.0, pan))
    gain = max(0.0, gain)
    return gain * min(1.0, 1.0 - pan), gain * min(1.0, 1.0 + pan)


def mix_filter(inputs: list[tuple[float, float]], speed=1.0, pitch=0.0, rubberband=True) -> str:
    """El filtro de ffmpeg que junta las pistas: cada una con su volumen y su
    panorama, sumadas sin normalizar (como suenan en el reproductor), con un
    limitador al final para que la suma no se recorte, y la velocidad y el
    tono si se pidieron."""
    chains = []
    for i, (left, right) in enumerate(inputs):
        chains.append(
            f"[{i}:a]aformat=channel_layouts=stereo,pan=stereo|c0={left:.4f}*c0|c1={right:.4f}*c1[a{i}]"
        )
    joined = "".join(f"[a{i}]" for i in range(len(inputs)))
    tail = f"{joined}amix=inputs={len(inputs)}:normalize=0:dropout_transition=0"
    extra = _speed_pitch(speed, pitch, rubberband)
    if extra:
        tail += "," + extra
    tail += ",alimiter=limit=0.98:level=0[out]"
    return ";".join([*chains, tail])


def export_mix(
    cid: int,
    tracks: list[dict],
    target: str,
    fmt: str | None = None,
    speed: float = 1.0,
    pitch: float = 0.0,
    progress=None,
) -> dict:
    """Guarda en `target` la mezcla de las pistas de la cancion.

    `tracks`: [{source, gain, pan}] las que suenan (las calladas no hacen
    falta). Para practicar fuera de DanPlay: la cancion sin bateria para el
    movil, o a menos velocidad.

    Solo dentro de la biblioteca, y sin pisar ninguna cancion de ella: por
    aqui no se puede escribir en cualquier sitio del disco. Si cae fuera de
    una carpeta de pistas, entra en la biblioteca como una cancion mas.
    """
    from . import library

    song = library.by_id(cid)
    if not song:
        raise SeparateError("esa canción ya no está en la biblioteca")
    data = info(song)
    if not data or not data["complete"]:
        raise SeparateError("esta canción no tiene sus pistas separadas completas")
    by_source = {t["source"]: t for t in data["tracks"]}
    chosen = []
    for t in tracks:
        track = by_source.get(t.get("source"))
        if track is None:
            raise SeparateError(f"no hay una pista «{t.get('source')}»")
        left, right = gains(float(t.get("gain", 1.0)), float(t.get("pan", 0.0)))
        if left > 1e-4 or right > 1e-4:
            chosen.append((track["path"], (left, right)))
    if not chosen:
        raise SeparateError("todas las pistas están calladas: no hay nada que guardar")
    path = Path(os.path.abspath(os.path.expanduser(target)))
    fmt = (fmt or path.suffix.lstrip(".")).lower()
    if fmt not in MIX_FORMATS:
        raise SeparateError("solo se puede guardar en mp3, flac o wav")
    if path.suffix.lower() != f".{fmt}":
        path = path.with_name(path.name + f".{fmt}")
    if not path.parent.is_dir():
        raise SeparateError("esa carpeta no existe")
    if not library.within_roots(path):
        raise SeparateError("la mezcla tiene que ir dentro de tu biblioteca")
    if library.by_path(str(path)):
        raise SeparateError("ahí ya hay una canción de tu biblioteca: elige otro nombre")
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise SeparateError("hace falta ffmpeg")
    speed = max(0.25, min(3.0, float(speed or 1.0)))
    pitch = max(-12.0, min(12.0, float(pitch or 0.0)))
    muted = [
        SOURCES.get(t["source"], ("", t["name"]))[1].lower()
        for t in data["tracks"]
        if t["path"] not in {p for p, _ in chosen}
    ]
    title = song.get("title") or Path(song["file"]).stem
    if muted:
        title += " (sin " + " ni ".join(muted) + ")" if len(muted) <= 2 else " (mezcla)"
    cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-y"]
    for p, _ in chosen:
        cmd += ["-i", p]
    cmd += [
        "-filter_complex",
        mix_filter([g for _, g in chosen], speed, pitch, _has_rubberband(ffmpeg)),
    ]
    cmd += ["-map", "[out]", "-ar", "44100", *MIX_FORMATS[fmt]]
    if song.get("artist"):
        cmd += ["-metadata", f"artist={song['artist']}"]
    cmd += ["-metadata", f"title={title}", "-progress", "pipe:1", "-nostats"]
    tmp = path.with_name(f".{path.stem}.parte{path.suffix}")
    cmd.append(str(tmp))
    total = (song.get("duration") or 0) / speed
    child = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    assert child.stdout is not None
    for line in child.stdout:
        if line.startswith("out_time_us=") and progress and total > 0:
            with contextlib.suppress(ValueError):
                progress(min(total, int(line.split("=", 1)[1]) / 1e6), total)
    err = child.stderr.read() if child.stderr else ""
    if child.wait() != 0:
        tmp.unlink(missing_ok=True)
        raise SeparateError("ffmpeg no pudo guardar la mezcla: " + err.strip()[-200:])
    os.replace(tmp, path)
    indexed = None
    inside_stems = any(is_stems_dir(parent) for parent in path.parents)
    if not inside_stems:
        row = library.index_file(str(path))
        indexed = row["id"] if row else None
    return {"path": str(path), "name": path.name, "id": indexed, "title": title}
