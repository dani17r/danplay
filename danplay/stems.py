"""Separar una cancion en pistas: bateria, bajo, voces, guitarra, piano y el
resto, cada una en su archivo.

Las pistas de una cancion van en una carpeta con su nombre, dentro de
`Separadas/` en la carpeta de la biblioteca de la que cuelga la cancion:

    Separadas/Miel San Marcos - Que Se Abra El Cielo/
        Bateria.flac  Voces.flac  Bajo.flac  Guitarra.flac  Piano.flac  Otros.flac
        .danplay-pistas.json   de que cancion son, con que redes, que hay
        .danplay-ondas.json    la forma de onda de cada pista

Son archivos normales: se abren con cualquier programa (un DAW, Ableton, el
reproductor del sistema). Dentro de DanPlay suenan juntas en el modo
estudio, cada una con su volumen, y se pueden callar o dejar solas.

Solo salen las que estan: una cancion sin piano no tiene pista de piano (lo
poco que la red le atribuya va a «Otros»). Y «Otros» es la cancion menos
todas las demas, asi que juntas suenan exactamente como la cancion.

Se separa en dos pasadas, con Demucs v4 y ONNX Runtime (`separation.py`), en
un proceso aparte y con prioridad baja:

1. la rapida (htdemucs_6s): una red que saca las seis fuentes de una vez. En
   unos minutos las pistas ya estan y se pueden usar;
2. la buena: la bateria y el bajo otra vez, cada uno con su especialista
   (htdemucs_ft, una red afinada para esa sola fuente). Tarda el doble que
   la rapida y los cambia por otros mas limpios, sin cortar lo que suena.
   La voz no: su especialista no la saca mejor que la rapida. Lo medido,
   con scripts/evaluar-separador.py, en docs/ARQUITECTURA.md.

Va en una cola: se pueden pedir varias y se separan una detras de otra
mientras se sigue usando la app. Las rapidas de todas van antes que
cualquier mejora: un repertorio entero se puede ensayar cuanto antes.

Como todo lo que el usuario crea, no depende de la base: la carpeta dice de
que cancion es (`.danplay-pistas.json`), y un escaneo vuelve a unirlas si el
indice se pierde. El escaneo no mete estas pistas en la biblioteca como
canciones sueltas: una carpeta con ese archivo se salta.
"""

import contextlib
import functools
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
REST = "other"  # la que es «lo que queda»: la cancion menos las demas

# La pasada rapida, y la red de los especialistas de la buena con las fuentes
# que mejoran. Medido en MUSDB18 (SDR): bateria 9,2 → 9,5 dB, bajo 7,7 →
# 8,8; la voz, 8,5 con los dos, asi que se queda la de la rapida. La
# guitarra y el piano no tienen especialista: tambien de la rapida.
FAST = "htdemucs_6s"
BEST = "htdemucs"
REFINE = ("drums", "bass")

# Como estan las pistas de una cancion (en su manifiesto): de la rapida, que
# se mejora luego, o ya de la buena. Las de 1.16.0 no lo dicen: se hicieron
# con una sola red, de seis o de cuatro, y se pueden separar otra vez.
QUICK, FINAL = "rapida", "mejor"

FORMATS = ("flac", "opus")

USER_AGENT = "DanPlay (+https://github.com/dani17r/danplay)"
TIMEOUT = 60


class SeparateError(jobs.JobError):
    """Un fallo previsto, ya explicado en castellano."""


# ------------------------------------------------------------ el motor


@functools.cache
def _graph(name: str) -> dict:
    """La receta de un grafo (`<nombre>.json`): sus fuentes y sus pesos."""
    return json.loads((GRAPHS / f"{name}.json").read_text(encoding="utf-8"))


def parts() -> dict[str, dict]:
    """Cada archivo de pesos que usa el separador, por nombre: el de la
    pasada rapida y el de cada especialista. Con su grafo, su URL, su sha256
    y lo que pesa."""
    out = {FAST: {"graph": FAST, **_graph(FAST)["weights"]}}
    specialists = _graph(BEST).get("specialists") or {}
    for source in REFINE:
        out[source] = {"graph": BEST, **specialists[source]}
    return out


def _weights_dir() -> Path:
    return config.DATA_DIR / "separador"


def weights_path(part: str) -> Path:
    return _weights_dir() / parts()[part]["file"]


def installed(part: str) -> bool:
    """Los pesos de esa parte ya estan bajados (y enteros: se comprobo su
    sha256 al bajarlos, y aqui el tamaño)."""
    try:
        return weights_path(part).stat().st_size == parts()[part]["bytes"]
    except (OSError, KeyError, ValueError):
        return False


def missing() -> str:
    """Por que no se puede separar en este equipo, o vacio si se puede."""
    for module in ("numpy", "onnxruntime"):
        if importlib.util.find_spec(module) is None:
            return f"falta {module} en esta instalación de DanPlay"
    if not convert.tool("ffmpeg"):
        return "hace falta ffmpeg"
    try:
        known = parts()
    except (OSError, ValueError, KeyError):
        return "faltan los archivos del separador en esta instalación"
    if not all((GRAPHS / f"{p['graph']}.onnx").is_file() for p in known.values()):
        return "faltan los archivos del separador en esta instalación"
    return ""


@functools.cache
def _has_opus(ffmpeg: str) -> bool:
    try:
        r = subprocess.run(
            [ffmpeg, "-hide_banner", "-encoders"], capture_output=True, text=True, check=False
        )
    except OSError:
        return False
    return " libopus " in r.stdout


def stems_format() -> str:
    """En que se guardan las pistas: FLAC (sin perdida) u Opus, si se eligio
    y este ffmpeg lo sabe hacer."""
    chosen = config.STEMS_FORMAT if config.STEMS_FORMAT in FORMATS else "flac"
    ffmpeg = convert.tool("ffmpeg")
    if chosen == "opus" and not (ffmpeg and _has_opus(ffmpeg)):
        return "flac"
    return chosen


def status() -> dict:
    """Si se puede separar, lo que pesa el separador (y lo que falta por
    bajar), en que se guardan las pistas, la cola y lo ultimo que acabo."""
    reason = missing()
    total = pending = 0
    with contextlib.suppress(OSError, ValueError, KeyError):
        for key, part in parts().items():
            total += part["bytes"]
            if not installed(key):
                pending += part["bytes"]
    ffmpeg = convert.tool("ffmpeg")
    with _lock:
        queue = [dict(q) for q in _queue]
        current = dict(_current) if _current else None
        events = [dict(e) for e in _events]
    return {
        "ok": not reason,
        "reason": reason,
        "bytes": total,
        "pending": pending,
        "installed": bool(total) and not pending,
        "folder": FOLDER,
        "format": stems_format(),
        "opus": bool(ffmpeg and _has_opus(ffmpeg)),
        "current": current,
        "queue": queue,
        "events": events,
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


def download(part: str, progress=None, cancelled=None) -> Path:
    """Baja los pesos de una parte del separador (del autor, en HuggingFace),
    una sola vez.

    Se comprueba el sha256 que viaja con la app: un archivo cambiado o a
    medias no se usa. Se escribe a un temporal y se pone en su sitio al
    final, asi que cortarlo no deja nada roto.
    """
    if installed(part):
        return weights_path(part)
    info = parts()[part]
    target = weights_path(part)
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = target.with_suffix(".part")
    digest = hashlib.sha256()
    got = 0
    req = urllib.request.Request(info["url"], headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r, open(tmp, "wb") as f:
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
        tmp.unlink(missing_ok=True)
        raise
    except Exception as e:
        tmp.unlink(missing_ok=True)
        raise SeparateError(f"no se pudo bajar el separador ({_why(e)})") from e
    if digest.hexdigest() != info["sha256"]:
        tmp.unlink(missing_ok=True)
        raise SeparateError("el separador bajado no es el que debe (sha256 distinto): no se usa")
    os.replace(tmp, target)
    return target


def _fetch(needed: list[str], progress) -> None:
    """Baja lo que falte de esas partes, contando el total como una descarga.
    Y quita lo que ya no usa ninguna (los pesos de 1.16.0 de cuatro pistas)."""
    todo = [p for p in needed if not installed(p)]
    if not todo:
        return
    known = {part["file"] for part in parts().values()}
    with contextlib.suppress(OSError):
        for old in _weights_dir().glob("*.safetensors"):
            if old.name not in known:
                old.unlink()
    sizes = {p: parts()[p]["bytes"] for p in todo}
    total, before = sum(sizes.values()), 0
    progress(step="download")
    for p in todo:
        download(
            p,
            lambda got, _t, _b=before: progress(_b + got, total, step="download"),
            _cancel.is_set,
        )
        before += sizes[p]


def remove_weights() -> int:
    """Borra todo lo bajado del separador (se vuelve a bajar al separar).
    Devuelve cuantos archivos quito."""
    n = 0
    with contextlib.suppress(OSError):
        for f in _weights_dir().iterdir():
            if f.suffix in (".safetensors", ".part"):
                f.unlink()
                n += 1
    return n


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
    quality = manifest.get("quality") or ""
    return {
        "folder": str(folder),
        "model": manifest.get("model", ""),
        "created": manifest.get("created"),
        # rapida (se esta mejorando, o se quedo a medias), mejor, o vacio
        # si se separaron antes de que hubiera dos pasadas (1.16.0)
        "quality": quality,
        "best": quality == FINAL,
        "tracks": tracks,
        "dropped": [s for s in manifest.get("dropped") or [] if s in SOURCES],
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
# Lo que espera: [{id, title, stage}]. `stage` es "separate" (la pasada
# rapida, y detras la buena) o "refine" (solo la buena, sobre las rapidas).
_queue: list[dict] = []
_current: dict | None = None  # lo que se esta separando ahora
_child: subprocess.Popen | None = None
_cancel = threading.Event()
# La cola esta andando (`_drain`). Se mira y se cambia con `_lock`, junto con
# la cola: asi una cancion que se pide justo cuando la cola decide que ha
# acabado no se queda esperando para siempre.
_draining = False
# Lo ultimo que acabo, bien o mal: [{seq, id, title, stage, ok, error}]. La
# interfaz se queda con el `seq` mas alto que vio y avisa de lo nuevo: asi se
# entera de que una cancion ya tiene sus pistas aunque siga en la cola (para
# mejorarlas).
_events: list[dict] = []
_seq = 0
EVENTS = 50


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


def _write_manifest(
    folder: Path, song: Mapping[str, Any], result: dict, quality: str, created: float
) -> None:
    from . import __version__

    tracks = result.get("tracks") or {}
    levels = result.get("levels") or {}
    manifest = {
        "version": 2,
        "app": f"DanPlay {__version__}",
        "model": FAST if quality == QUICK else f"{FAST}+htdemucs_ft",
        "quality": quality,
        "created": round(created, 1),
        "updated": round(time.time(), 1),
        "source": {
            "file": song["file"],
            "folder": song.get("folder") or "",
            "size": song.get("size") or 0,
            "duration": song.get("duration") or 0,
            "artist": song.get("artist") or "",
            "title": song.get("title") or "",
        },
        "tracks": [
            {"source": s, "file": tracks[s], "name": SOURCES[s][1]} for s in SOURCES if s in tracks
        ],
        # las que la red casi no encontro y no llegaron a pista (su poco va
        # en «Otros»), y lo que sonaba cada una: por que se quedo o no
        "dropped": [s for s in SOURCES if s in levels and s not in tracks],
        "levels": {s: levels[s] for s in SOURCES if s in levels},
    }
    (folder / MANIFEST).write_text(
        json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
    )


def _sweep(folder: Path) -> None:
    """Quita lo que dejo a medias una separacion que no acabo (la app se
    cerro, se fue la luz): la cola es una, asi que si ahora empieza otra,
    cualquier carpeta de trabajo que quede es de una que ya no sigue. Y las
    pistas rapidas que ya se cambiaron por las buenas, si no se pudieron
    borrar en su momento."""
    for pattern in (".separando-*", ".rapidas-*"):
        for leftover in folder.glob(pattern):
            shutil.rmtree(leftover, ignore_errors=True)


def _song_for(cid: int) -> dict:
    from . import library

    song = library.by_id(cid)
    if not song:
        raise SeparateError("esa canción ya no está en la biblioteca")
    if not os.path.isfile(song["path"]):
        raise SeparateError(f"no encuentro «{song['file']}»")
    reason = missing()
    if reason:
        raise SeparateError(f"no se puede separar: {reason}")
    return dict(song)


def _flac_names() -> dict[str, str]:
    return {s: f"{SOURCES[s][0]}.flac" for s in SOURCES}


def _job(song: Mapping[str, Any], work: Path, nets: list[dict], **extra) -> dict:
    return {
        "input": song["path"],
        "output": str(work),
        "files": _flac_names(),
        "rest": REST,
        "nets": nets,
        "ffmpeg": convert.tool("ffmpeg"),
        "waves": str(work / WAVES),
        **extra,
    }


def _net(part: str, take: list[str]) -> dict:
    graph = parts()[part]["graph"]
    return {
        "graph": str(GRAPHS / f"{graph}.onnx"),
        "manifest": str(GRAPHS / f"{graph}.json"),
        "weights": str(weights_path(part)),
        "take": take,
    }


def _publish(work: Path, target: Path, old_prefix: str) -> Path | None:
    """Pone la carpeta nueva en el sitio de la de antes, de golpe. Devuelve
    donde quedo la de antes (oculta), si habia."""
    old = None
    if target.exists():
        old = target.with_name(f"{old_prefix}-{target.name}-{int(time.time())}")
        os.replace(target, old)
    os.replace(work, target)
    return old


def _needs_refine(tracks: dict) -> bool:
    """Si hay algo que mejorar: alguna de las que tienen especialista, o
    pasarlas a Opus (que se hace al final, con todas ya buenas)."""
    return any(s in tracks for s in REFINE) or stems_format() != "flac"


def separate_song(cid: int, progress=None) -> dict:
    """La pasada rapida: separa una cancion de la biblioteca y deja sus pistas
    en su carpeta, ya para usar. Devuelve si hay que mejorarlas (`refine`).

    Se separa en una carpeta oculta junto a la de destino (el escaneo se
    salta las ocultas, asi que nunca ve pistas a medias) y al terminar se
    pone en su sitio de golpe. Si la cancion ya tenia pistas, las de antes
    van a la papelera cuando las nuevas ya estan.
    """
    from . import library

    progress = progress or (lambda *a, **k: None)
    song = _song_for(cid)
    _fetch([FAST], progress)
    target = _target_for(song)
    target.parent.mkdir(parents=True, exist_ok=True)
    _sweep(target.parent)
    work = Path(tempfile.mkdtemp(prefix=".separando-", dir=target.parent))
    try:
        take = [s for s in _graph(FAST)["sources"] if s != REST]
        result = _run_child(_job(song, work, [_net(FAST, take)]), progress)
        refine = _needs_refine(result["tracks"])
        quality = QUICK if refine else FINAL
        _write_manifest(work, song, result, quality, time.time())
        old = _publish(work, target, ".antes")
    except BaseException:
        shutil.rmtree(work, ignore_errors=True)
        raise
    if old is not None:
        # las pistas de antes (otra version, otra separacion): a la papelera
        r = library.trash_path(old)
        if not r["ok"]:
            log.warning("no pude mandar a la papelera las pistas viejas %s", old)
    rel = os.path.relpath(target, song["root"])
    library.update(cid, stems=json.dumps({"folder": rel, "model": FAST, "quality": quality}))
    return {"id": cid, "title": _title(song), "folder": str(target), "refine": refine, **result}


def refine_song(cid: int, progress=None) -> dict:
    """La pasada buena, sobre las pistas rapidas de la cancion: la bateria y
    el bajo, cada uno con su especialista; «Otros», otra vez lo que queda. La
    voz, la guitarra y el piano se quedan como estaban.

    Tambien se hace aparte y se pone en su sitio de golpe. Las rapidas no van
    a la papelera: son un paso intermedio, no algo que el usuario hiciera.
    """
    from . import library

    progress = progress or (lambda *a, **k: None)
    song = _song_for(cid)
    target = folder_of(song)
    manifest = read_manifest(target) if target else None
    if target is None or manifest is None:
        raise SeparateError("esta canción ya no tiene sus pistas separadas")
    stamp = manifest.get("updated") or manifest.get("created")
    have = {t["source"]: t["file"] for t in manifest.get("tracks") or []}
    if manifest.get("quality") != QUICK or not all((target / f).is_file() for f in have.values()):
        raise SeparateError("sus pistas no son las de la pasada rápida: sepárala otra vez")
    better = [s for s in REFINE if s in have]
    _fetch(better, progress)
    _sweep(target.parent)
    work = Path(tempfile.mkdtemp(prefix=".separando-", dir=target.parent))
    try:
        # las que no se tocan van tal cual a la carpeta nueva
        carry = {}
        for source, name in have.items():
            if source not in better and source != REST:
                shutil.copy2(target / name, work / name)
                carry[source] = str(work / name)
        nets = [_net(s, [s]) for s in better]
        job = _job(song, work, nets, carry=carry, keep=list(have), format=stems_format())
        result = _run_child(job, progress)
        # por si mientras tanto se borraron o se separo otra vez
        now = read_manifest(target)
        if now is None or (now.get("updated") or now.get("created")) != stamp:
            raise SeparateError("sus pistas cambiaron mientras se mejoraban")
        levels = {**(manifest.get("levels") or {}), **(result.get("levels") or {})}
        result = {**result, "levels": levels}
        _write_manifest(work, song, result, FINAL, manifest.get("created") or time.time())
        old = _publish(work, target, ".rapidas")
    except BaseException:
        shutil.rmtree(work, ignore_errors=True)
        raise
    if old is not None:
        shutil.rmtree(old, ignore_errors=True)  # si algo las tiene abiertas, `_sweep`
    rel = os.path.relpath(target, song["root"])
    model = f"{FAST}+htdemucs_ft"
    library.update(cid, stems=json.dumps({"folder": rel, "model": model, "quality": FINAL}))
    return {"id": cid, "title": _title(song), "folder": str(target), **result}


def _stage_for(song: Mapping[str, Any]) -> str:
    """Lo que hay que hacer para que la cancion tenga las mejores pistas: si
    ya tiene las rapidas enteras, mejorarlas; si no, separarla."""
    data = info(song)
    if data and data["complete"] and data["quality"] == QUICK:
        return "refine"
    return "separate"


def request(cid: int) -> dict:
    """Pide separar una cancion: a la cola, y la cola a andar si no lo esta.

    Si ya tiene las pistas rapidas, solo se mejoran. Devuelve el estado
    (`status`). Pedir otra vez una que ya espera o se esta separando no la
    repite.
    """
    from . import library

    song = library.by_id(cid)
    if not song:
        raise SeparateError("esa canción ya no está en la biblioteca")
    reason = missing()
    if reason:
        raise SeparateError(f"no se puede separar: {reason}")
    _enqueue({"id": cid, "title": _title(song), "stage": _stage_for(song)})
    return status()


def _enqueue(item: dict) -> None:
    global _draining
    with _lock:
        cid = item["id"]
        busy = (_current and _current["id"] == cid) or any(q["id"] == cid for q in _queue)
        if not busy:
            _queue.append(item)
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


def _note(done, total, step) -> None:
    """Como va la que se esta separando, para `status`."""
    with _lock:
        if _current is not None:
            _current.update(done=done or 0, total=total or 0, step=step or "separate")


def _event(item: dict, ok: bool, error: str = "") -> None:
    global _seq
    with _lock:
        _seq += 1
        _events.append({"seq": _seq, **item, "ok": ok, "error": error})
        del _events[:-EVENTS]


def _drain(progress) -> dict:
    """El trabajo de la cola: separa lo que haya, una detras de otra."""
    global _draining
    try:
        return _drain_queue(progress)
    finally:
        with _lock:
            _draining = False


def _next() -> dict | None:
    """La siguiente: cualquier pasada rapida antes que cualquier mejora."""
    for i, q in enumerate(_queue):
        if q["stage"] == "separate":
            return _queue.pop(i)
    return _queue.pop(0) if _queue else None


MESSAGES = {
    "download": "Bajando el separador (una sola vez)…",
    "load": "Preparando «{title}»…",
    "decode": "Preparando «{title}»…",
    "separate": "{verb} «{title}»…",
    "compose": "Terminando «{title}»…",
    "encode": "Guardando «{title}» en Opus…",
}


def _drain_queue(progress) -> dict:
    global _current, _draining
    done, failed = [], []
    while True:
        with _lock:
            item = _next() if not _cancel.is_set() else None
            if item is None:
                _current = None
                _queue.clear()
                _draining = False
                break
            _current = dict(item)
            left = len(_queue)
        count = len(done) + len(failed) + 1
        total = count + left
        where = f" ({count} de {total})" if total > 1 else ""
        verb = "Mejorando" if item["stage"] == "refine" else "Separando"

        def report(done_=None, total_=None, step=None, seconds=None, _i=item, _w=where, _v=verb):
            if step in MESSAGES:
                text = MESSAGES[step].format(title=_i["title"], verb=_v) + _w
                progress(done_ or 0, total_ or 0, text)
            elif done_ is not None:
                progress(done_, total_, f"{_v} «{_i['title']}»{_w}")
            _note(done_, total_, step)

        try:
            if item["stage"] == "refine":
                r = refine_song(item["id"], report)
            else:
                r = separate_song(item["id"], report)
                if r.get("refine"):
                    # las rapidas ya estan: la mejora, a la cola (detras de
                    # las rapidas que esperen)
                    with _lock:
                        _queue.append({**item, "stage": "refine"})
            done.append(r)
            _event(item, True)
        except SeparateError as e:
            if _cancel.is_set():
                break
            failed.append({**item, "error": str(e)})
            _event(item, False, str(e))
        except Exception as e:
            log.warning("no se pudo separar %s", item, exc_info=True)
            failed.append({**item, "error": str(e) or e.__class__.__name__})
            _event(item, False, str(e) or e.__class__.__name__)
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

    unqueue(song["id"])  # si esperaba para mejorarlas, ya no hay nada que mejorar
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
            data = {
                "folder": rel,
                "model": manifest.get("model", ""),
                "quality": manifest.get("quality", ""),
            }
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
