"""Abrir una secuencia descargada: la carpeta de pistas de una cancion (como
las de MultiTracks.com o Loop Community) con su proyecto de Ableton.

Viene asi: `Titulo-Artista-Tono-124.00bpm/`, con `MultiTracks/*.wav` (cada
instrumento, el «Click Track» y la «Guide» que canta las partes), la portada
y `MultiTrack.als` (el proyecto de Ableton: el tempo, los cambios de compas y
los localizadores de cada parte —Intro, Verse 1, Chorus…— en pulsos).

DanPlay la deja lista para tocar como una secuencia en directo:

- la cancion, mezclada sin el clic ni la guia, en `Secuencias/` (para la
  biblioteca, las listas y el modo estudio);
- las pistas, en FLAC, como sus pistas separadas (`Separadas/…`), con su
  nombre y su papel: el clic y la guia salen por la salida del clic (a los
  cascos) y lo demas por la de la musica;
- los localizadores, como marcadores del estudio; y la rejilla del clic,
  exacta (la del tempo del proyecto, no la que se adivina oyendo).

Sin el `.als` tambien vale: el tempo sale del nombre de la carpeta, y si no,
del analisis de siempre.
"""

import contextlib
import gzip
import itertools
import json
import logging
import os
import re
import shutil
import subprocess
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from . import config, convert, names

log = logging.getLogger(__name__)

AUDIO = {".wav", ".wave", ".aif", ".aiff", ".flac", ".mp3", ".m4a", ".ogg", ".opus"}
COVERS = ("album", "cover", "folder", "front", "artwork", "portada")
# Lo que dice el nombre de una pista: el clic y la guia hablada.
ROLES = {
    "click": re.compile(r"\b(click|clic|clik|metronom[eo]|metr[oó]nomo|claqueta)\b", re.I),
    "guide": re.compile(r"\b(guide|gu[ií]as?|cues?|voice\s*cues?|count\s*in|cuenta)\b", re.I),
}
# Un .als que descomprimido pesa mas que esto no es un proyecto normal.
MAX_ALS = 64 * 1024 * 1024
JOB = "secuencia"
# Las que el motor hace sonar a la vez, ademas de la cancion (render::MAX_PARTS).
MAX_TRACKS = 31
WAVE_BUCKETS = 800


class SequenceError(Exception):
    """No se pudo abrir la secuencia (por que, en castellano)."""


# ------------------------------------------------------------ que hay


def _audio_in(folder: Path) -> list[Path]:
    with contextlib.suppress(OSError):
        return sorted(
            p
            for p in folder.iterdir()
            if p.is_file() and p.suffix.lower() in AUDIO and not p.name.startswith(".")
        )
    return []


def _tracks_dir(folder: Path) -> Path | None:
    """La carpeta con las pistas: esta misma, o una de dentro (MultiTracks,
    Stems, Tracks…), hasta dos niveles. La que mas pistas tenga."""
    best, most = None, 0
    queue = [(folder, 0)]
    while queue:
        here, depth = queue.pop(0)
        n = len(_audio_in(here))
        if n > most:
            best, most = here, n
        if depth < 2:
            with contextlib.suppress(OSError):
                queue.extend(
                    (d, depth + 1)
                    for d in sorted(here.iterdir())
                    if d.is_dir()
                    and not d.name.startswith(".")
                    and d.name != "Ableton Project Info"
                )
    return best if most >= 3 else None


def role_of(name: str) -> str:
    """El papel de una pista por su nombre: 'click', 'guide' o ''."""
    for role, pattern in ROLES.items():
        if pattern.search(name):
            return role
    return ""


def key_of(name: str, taken: set[str]) -> str:
    """Una clave para la pista (la del mezclador): sin tildes ni espacios,
    que empiece por letra, unica."""
    base = re.sub(r"[^a-z0-9]+", "_", names.strip_accents(name).lower()).strip("_")[:20]
    if not base or not base[0].isalpha():
        base = "p_" + base
    key, n = base, 2
    while key in taken:
        key, n = f"{base[:17]}_{n}", n + 1
    taken.add(key)
    return key


def parse_name(name: str) -> dict:
    """Lo que dice el nombre de la carpeta: «This Is The Day-Shout-F-124.00bpm»,
    «Titulo - Artista - Tono - 124 bpm»..."""
    out: dict = {"title": "", "artist": "", "key": "", "bpm": 0.0}
    text = name.strip()
    m = re.search(r"[-_ ]*(\d{2,3}(?:[.,]\d+)?)\s*bpm\s*$", text, re.I)
    if m:
        out["bpm"] = float(m[1].replace(",", "."))
        text = text[: m.start()]
    parts = [p.strip() for p in re.split(r"\s+-\s+|-", text) if p.strip()]
    if parts and re.fullmatch(r"[A-G](?:#|b|♯|♭)?m?", parts[-1]):
        out["key"] = parts.pop()
    if len(parts) >= 2:
        out["title"], out["artist"] = parts[0], " - ".join(parts[1:])
    elif parts:
        out["title"] = parts[0]
    return out


def _safe_xml(data: bytes) -> bytes:
    """Un proyecto de Ableton no trae DTD: si trae, no se lee (una
    «bomba» de entidades colgaria el nucleo)."""
    head = data[:4096].upper()
    if b"<!DOCTYPE" in head or b"<!ENTITY" in data[:65536].upper():
        raise SequenceError("el proyecto de Ableton trae algo raro: no lo leo")
    return data


def _meter(value: int) -> tuple[int, int]:
    """Ableton guarda el compas como un numero: 99·log2(denominador) +
    (numerador − 1). 201 es 4/4; 200, 3/4; 302, 6/8."""
    den = 2 ** (value // 99)
    return value % 99 + 1, den


def read_als(path: Path) -> dict:
    """Lo que sirve del proyecto de Ableton: el tempo, los compases (con su
    pulso de inicio), los localizadores y donde empieza cada pista."""
    import xml.etree.ElementTree as ET

    try:
        with gzip.open(path, "rb") as f:
            data = f.read(MAX_ALS + 1)
    except (OSError, EOFError) as e:
        raise SequenceError("no se pudo leer el proyecto de Ableton") from e
    if len(data) > MAX_ALS:
        raise SequenceError("el proyecto de Ableton es demasiado grande")
    try:
        root = ET.fromstring(_safe_xml(data))  # noqa: S314 - ver _safe_xml
    except ET.ParseError as e:
        raise SequenceError("el proyecto de Ableton está dañado") from e

    def value(el, default=None):
        return el.get("Value", default) if el is not None else default

    tempo = 0.0
    master = root.find(".//MasterTrack")
    if master is None:
        master = root
    for ev in master.iterfind(".//Tempo/ArrangerAutomation/Events/FloatEvent"):
        with contextlib.suppress(TypeError, ValueError):
            tempo = float(ev.get("Value") or 0)
            break
    if not tempo:
        with contextlib.suppress(TypeError, ValueError):
            tempo = float(value(master.find(".//Tempo/Manual"), 0) or 0)
    meters = []
    for ev in master.iterfind(".//TimeSignature/ArrangerAutomation/Events/EnumEvent"):
        with contextlib.suppress(TypeError, ValueError):
            beat = max(0.0, float(ev.get("Time") or 0))
            num, den = _meter(int(ev.get("Value") or 201))
            meters.append((beat, num, den))
    meters.sort()
    locators = []
    for loc in root.iterfind(".//Locators/Locators/Locator"):
        with contextlib.suppress(TypeError, ValueError):
            locators.append(
                (float(value(loc.find("Time"), 0)), str(value(loc.find("Name"), "") or "").strip())
            )
    locators.sort()
    clips = {}
    for clip in root.iterfind(".//AudioClip"):
        ref = clip.find(".//SampleRef/FileRef/Name")
        name = value(ref)
        if not name:
            continue
        with contextlib.suppress(TypeError, ValueError):
            clips[name] = {
                "beat": float(clip.get("Time") or 0),
                "warped": value(clip.find("IsWarped")) == "true",
            }
    return {"tempo": tempo, "meters": meters, "locators": locators, "clips": clips}


def find(folder) -> dict:
    """La secuencia de esa carpeta (o de la carpeta de ese archivo): sus
    pistas con su papel, el proyecto, la portada y lo que dice el nombre.
    SequenceError si no lo parece."""
    folder = Path(folder)
    if folder.is_file():
        folder = folder.parent
    if not folder.is_dir():
        raise SequenceError("no encuentro esa carpeta")
    tracks_dir = _tracks_dir(folder)
    # desde dentro de MultiTracks: la carpeta de la cancion es la de arriba
    if (
        tracks_dir == folder
        and not list(folder.glob("*.als"))
        and list(folder.parent.glob("*.als"))
    ):
        folder = folder.parent
    if tracks_dir is None:
        raise SequenceError("esa carpeta no tiene pistas (hacen falta al menos tres audios)")
    als = next(iter(sorted(folder.glob("*.als"))), None) or next(
        iter(sorted(tracks_dir.glob("*.als"))), None
    )
    project = None
    if als is not None:
        with contextlib.suppress(SequenceError):
            project = read_als(als)
    cover = None
    for p in sorted(folder.iterdir()):
        if p.suffix.lower() in (".jpg", ".jpeg", ".png") and p.is_file():
            if p.stem.lower() in COVERS:
                cover = p
                break
            cover = cover or p
    info = parse_name(folder.name)
    taken: set[str] = set()
    tracks = []
    for p in _audio_in(tracks_dir):
        name = p.stem.strip()
        tracks.append(
            {"path": str(p), "name": name, "role": role_of(name), "key": key_of(name, taken)}
        )
    # el clic y la guia, delante (como en las apps de secuencias)
    order = {"click": 0, "guide": 1, "": 2}
    tracks.sort(key=lambda t: order[t["role"]])
    tempo = (project or {}).get("tempo") or info["bpm"]
    return {
        "folder": str(folder),
        "tracks_dir": str(tracks_dir),
        "title": info["title"] or folder.name,
        "artist": info["artist"],
        "key": info["key"],
        "bpm": round(float(tempo or 0), 3),
        "project": str(als) if als else "",
        "cover": str(cover) if cover else "",
        "tracks": tracks,
        "sections": [{"beat": b, "name": n} for b, n in ((project or {}).get("locators") or [])],
        "meters": (project or {}).get("meters") or [],
        "clips": (project or {}).get("clips") or {},
    }


def summary(folder) -> dict:
    """Lo que se enseña antes de abrirla."""
    s = find(folder)
    return {
        "folder": s["folder"],
        "title": s["title"],
        "artist": s["artist"],
        "key": s["key"],
        "bpm": s["bpm"],
        "project": bool(s["project"]),
        "cover": bool(s["cover"]),
        "sections": [x["name"] or f"Parte {i + 1}" for i, x in enumerate(s["sections"])],
        "tracks": [{"name": t["name"], "role": t["role"]} for t in s["tracks"]],
    }


# ------------------------------------------------------------ el tiempo


def beat_seconds(beat: float, tempo: float) -> float:
    return beat * 60.0 / tempo


def grid(tempo: float, meters: list, duration: float) -> dict:
    """La rejilla exacta: un pulso cada 60/tempo desde el principio, y el «1»
    de cada compas segun los cambios de compas del proyecto."""
    period = 60.0 / tempo
    n = int(duration / period) + 1
    beats = [round(k * period, 4) for k in range(n)]
    changes = sorted(meters) or [(0.0, 4, 4)]
    bars, k = [], 0
    while k < n:
        num = 4
        for beat, m_num, _ in changes:
            if beat <= k + 1e-6:
                num = m_num
        bars.append(k)
        # un compas no se pasa del siguiente cambio
        nxt = next((b for b, _, _ in changes if b > k + 1e-6), None)
        step = max(1, num)
        if nxt is not None and k + step > nxt:
            step = max(1, round(nxt - k))
        k += step
    meter = changes[0][1]
    return {
        "bpm": round(tempo, 3),
        "meter": meter if meter in (2, 3, 4, 5, 6, 7, 8, 9, 12) else 4,
        "beats": beats,
        "bars": bars,
        "first_downbeat": 0,
        "phase3": 0,
        "phase4": 0,
        "confidence": 1.0,
        "closed": True,
        "report": {"mode": "project", "exact": True},
    }


def markers(sections: list, tempo: float, duration: float) -> list[dict]:
    """Los localizadores como marcadores: cada parte hasta la siguiente."""
    out = []
    times = [(beat_seconds(s["beat"], tempo), s["name"]) for s in sections]
    for i, (t, name) in enumerate(times):
        end = times[i + 1][0] if i + 1 < len(times) else duration
        if t >= duration:
            break
        label = name[:60] or f"Parte {i + 1}"
        out.append({"t": round(t, 2), "end": round(min(end, duration), 2), "label": label})
    return out


# ------------------------------------------------------------ abrirla


def _duration(ffmpeg: str, path: str) -> float:
    probe = convert.tool("ffprobe")
    if probe:
        r = subprocess.run(
            [probe, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path],
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
        with contextlib.suppress(ValueError):
            return float(r.stdout.strip())
    r = subprocess.run(
        [ffmpeg, "-v", "error", "-nostdin", "-i", path, "-f", "null", "-"],
        capture_output=True,
        text=True,
        timeout=600,
        check=False,
    )
    found = re.findall(r"time=(\d+):(\d+):([\d.]+)", r.stderr)
    if found:
        h, m, s = found[-1]
        return int(h) * 3600 + int(m) * 60 + float(s)
    return 0.0


def _wave(ffmpeg: str, path: str, duration: float) -> tuple[list[float], list[float]]:
    """Pico y RMS de una pista (a la escala de 1), para su carril."""
    import numpy as np

    rate = 4000
    r = subprocess.run(
        [
            ffmpeg,
            "-v",
            "error",
            "-nostdin",
            "-i",
            path,
            "-ac",
            "1",
            "-ar",
            str(rate),
            "-f",
            "f32le",
            "-",
        ],
        capture_output=True,
        timeout=600,
        check=False,
    )
    x = (
        np.abs(np.frombuffer(r.stdout, np.float32))
        if r.returncode == 0
        else np.zeros(1, np.float32)
    )
    n = max(1, int(duration * rate))
    x = np.pad(x[:n], (0, max(0, n - x.size)))
    edges = (np.arange(WAVE_BUCKETS + 1) * n) // WAVE_BUCKETS
    peaks, rms = [], []
    for a, b in itertools.pairwise(edges):
        chunk = x[a : max(b, a + 1)]
        peaks.append(float(chunk.max(initial=0.0)))
        rms.append(float(np.sqrt(np.mean(chunk.astype(np.float64) ** 2))))
    return peaks, rms


def _run(cmd: list[str], what: str) -> None:
    r = subprocess.run(cmd, capture_output=True, timeout=3600, check=False)
    if r.returncode != 0:
        log.warning("%s fallo: %s", what, r.stderr.decode("utf-8", "replace")[-800:])
        raise SequenceError(f"no se pudo {what}")


def _offset_filter(seconds: float) -> list[str]:
    """Una pista que en el proyecto empieza mas tarde: silencio delante."""
    if seconds <= 0.0005:
        return []
    return ["-af", f"adelay=delays={seconds * 1000:.3f}:all=1"]


def target_name(seq: dict) -> str:
    artist, title = seq["artist"].strip(), seq["title"].strip()
    base = names.sanitize(f"{artist} - {title}" if artist else title) or "Secuencia"
    return base[:150]


def import_sequence(folder, progress=None) -> dict:
    """Abre la secuencia: la mezcla en Secuencias/, sus pistas como pistas
    separadas, sus partes como marcadores y su clic exacto. Devuelve
    {id, title, tracks, folder}."""
    from . import beatgrid, library, stems, tags

    progress = progress or (lambda *a, **k: None)
    seq = find(folder)
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise SequenceError("hace falta ffmpeg para abrir secuencias")
    tracks = seq["tracks"]
    if len(tracks) > MAX_TRACKS:
        raise SequenceError(
            f"tiene {len(tracks)} pistas: DanPlay suena hasta {MAX_TRACKS} a la vez"
        )
    music = [t for t in tracks if not t["role"]]
    if not music:
        raise SequenceError("la secuencia no tiene pistas de música (solo clic o guía)")
    tempo = float(seq["bpm"] or 0)
    # donde empieza cada pista (si el proyecto dice que alguna entra tarde)
    for t in tracks:
        clip = seq["clips"].get(Path(t["path"]).name)
        t["start"] = beat_seconds(clip["beat"], tempo) if clip and tempo else 0.0
    base = target_name(seq)
    library_root = Path(config.LIBRARY)
    song_dir = library_root / "Secuencias"
    song_dir.mkdir(parents=True, exist_ok=True)
    mix_path = song_dir / names.free_name(str(song_dir), base + ".flac")
    stems_parent = library_root / stems.FOLDER
    stems_parent.mkdir(parents=True, exist_ok=True)
    stems_dir = stems_parent / mix_path.stem
    total = len(tracks) + 3
    # se prepara en una carpeta oculta (el escaneo no la ve) y se pone en su
    # sitio de golpe al final
    work = Path(tempfile.mkdtemp(prefix=".secuencia-", dir=stems_parent))
    try:
        # 1. las pistas, en FLAC (con su silencio delante si entran tarde)
        progress(done=0, total=total, message=f"Preparando «{seq['title']}»…")
        files: dict[str, str] = {}

        def flac(t: dict) -> None:
            name = names.sanitize(t["name"])[:80] or t["key"]
            dest = work / f"{name}.flac"
            n = 2
            while dest.exists():
                dest = work / f"{name} ({n}).flac"
                n += 1
            dest.touch()
            _run(
                [
                    ffmpeg,
                    "-v",
                    "error",
                    "-nostdin",
                    "-y",
                    "-i",
                    t["path"],
                    *_offset_filter(t["start"]),
                    "-c:a",
                    "flac",
                    "-compression_level",
                    "5",
                    str(dest),
                ],
                f"pasar «{t['name']}» a FLAC",
            )
            files[t["key"]] = dest.name

        done = 0
        with ThreadPoolExecutor(max_workers=max(1, min(4, (os.cpu_count() or 2) // 2))) as pool:
            for _ in pool.map(flac, tracks):
                done += 1
                progress(
                    done=done,
                    total=total,
                    message=f"Pistas de «{seq['title']}»: {done}/{len(tracks)}",
                )
        duration = max(_duration(ffmpeg, str(work / files[t["key"]])) for t in tracks)
        # 2. la cancion: todo menos el clic y la guia, sumado (con un limitador
        # para que no sature)
        progress(done=done, total=total, message=f"Mezclando «{seq['title']}»…")
        inputs = []
        for t in music:
            inputs += ["-i", str(work / files[t["key"]])]
        tmp_mix = work / "mezcla.flac"
        _run(
            [
                ffmpeg,
                "-v",
                "error",
                "-nostdin",
                "-y",
                *inputs,
                "-filter_complex",
                f"amix=inputs={len(music)}:normalize=0:dropout_transition=0,alimiter=limit=0.97:level=false",
                "-c:a",
                "flac",
                str(tmp_mix),
            ],
            "mezclar la canción",
        )
        done += 1
        # 3. la onda de cada pista, todas a la misma escala
        progress(done=done, total=total, message=f"Dibujando las pistas de «{seq['title']}»…")
        waves = {t["key"]: _wave(ffmpeg, str(work / files[t["key"]]), duration) for t in tracks}
        top = max((max(p) for p, _ in waves.values() if p), default=1.0) or 1.0
        (work / stems.WAVES).write_text(
            json.dumps(
                {
                    "buckets": WAVE_BUCKETS,
                    "tracks": {
                        k: {
                            "peaks": [round(min(1.0, v / top), 3) for v in p],
                            "rms": [round(min(1.0, v / top), 3) for v in r],
                        }
                        for k, (p, r) in waves.items()
                    },
                }
            ),
            encoding="utf-8",
        )
        done += 1
        # 4. a su sitio: la cancion a Secuencias/ y las pistas a Separadas/
        shutil.move(str(tmp_mix), str(mix_path))
        tags.write(str(mix_path), artist=seq["artist"], title=seq["title"], album="Secuencias")
        if seq.get("key") or tempo:
            tags.write_analysis(str(mix_path), key=seq.get("key") or "", bpm=tempo or 0.0)
        if seq["cover"]:
            with contextlib.suppress(OSError):
                data = Path(seq["cover"]).read_bytes()
                mime = "image/png" if seq["cover"].lower().endswith(".png") else "image/jpeg"
                tags.write_cover(str(mix_path), data, mime)
        st = os.stat(mix_path)
        song_info = {
            "file": mix_path.name,
            "folder": os.path.relpath(mix_path.parent, library_root),
            "size": st.st_size,
            "duration": round(duration, 2),
            "artist": seq["artist"],
            "title": seq["title"],
        }
        manifest = {
            "version": 2,
            "app": "DanPlay",
            "model": "secuencia",
            "quality": stems.FINAL,
            "created": round(time.time(), 1),
            "updated": round(time.time(), 1),
            "source": song_info,
            "sequence": {
                "from": seq["folder"],
                "tempo": tempo,
                "key": seq.get("key") or "",
                "sections": seq["sections"],
            },
            "tracks": [
                {"source": t["key"], "file": files[t["key"]], "name": t["name"], "role": t["role"]}
                for t in tracks
            ],
        }
        (work / stems.MANIFEST).write_text(
            json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
        )
        if stems_dir.exists():
            stems_dir = stems_parent / names.free_name(str(stems_parent), stems_dir.name)
        os.replace(work, stems_dir)
    except BaseException:
        shutil.rmtree(work, ignore_errors=True)
        raise
    # 5. al indice, con sus pistas, sus partes y su clic
    row = library.index_file(str(mix_path))
    if not row:
        raise SequenceError("la canción no entró en la biblioteca (¿está la carpeta de música?)")
    rel = os.path.relpath(stems_dir, row["root"])
    library.update(
        row["id"], stems=json.dumps({"folder": rel, "model": "secuencia", "quality": stems.FINAL})
    )
    study: dict = {"mixer": {"on": True}}
    if tempo:
        study["markers"] = markers(seq["sections"], tempo, duration)
        beatgrid.save_exact(str(mix_path), grid(tempo, seq["meters"], duration))
    library.set_study(row["id"], study)
    progress(done=total, total=total, message=f"«{seq['title']}» lista")
    return {
        "id": row["id"],
        "title": seq["title"],
        "artist": seq["artist"],
        "tracks": len(tracks),
        "folder": str(stems_dir),
        "path": str(mix_path),
    }
