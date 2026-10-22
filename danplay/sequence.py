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

Leer la carpeta y el proyecto, la rejilla, los marcadores y la onda de cada
pista lo hace Rust (library/src/sequence.rs); aqui quedan lanzar ffmpeg y
meterla en la biblioteca.
"""

import contextlib
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

import danplay_core as _rust

from . import config, convert, names, tags

log = logging.getLogger(__name__)

AUDIO = {".wav", ".wave", ".aif", ".aiff", ".flac", ".mp3", ".m4a", ".ogg", ".opus"}
COVERS = ("album", "cover", "folder", "front", "artwork", "portada")
# Un .als que descomprimido pesa mas que esto no es un proyecto normal.
MAX_ALS = 64 * 1024 * 1024
JOB = "sequence"
# Las que el motor hace sonar a la vez, ademas de la cancion (render::MAX_PARTS).
MAX_TRACKS = 31
WAVE_BUCKETS = 800


class SequenceError(Exception):
    """No se pudo abrir la secuencia (por que, en castellano)."""


# ------------------------------------------------------------ que hay


def role_of(name: str) -> str:
    """El papel de una pista por su nombre: 'click', 'guide' o ''."""
    return _rust.sequence_role_of(str(name))


def key_of(name: str, taken: set[str]) -> str:
    """Una clave para la pista (la del mezclador): sin tildes ni espacios,
    que empiece por letra, unica (y se apunta en `taken`)."""
    key = _rust.sequence_key_of(str(name), sorted(taken))
    taken.add(key)
    return key


def parse_name(name: str) -> dict:
    """Lo que dice el nombre de la carpeta: «This Is The Day-Shout-F-124.00bpm»,
    «Titulo - Artista - Tono - 124 bpm»..."""
    return _rust.sequence_parse_name(str(name))


def read_als(path: Path) -> dict:
    """Lo que sirve del proyecto de Ableton: el tempo, los compases (con su
    pulso de inicio), los localizadores y donde empieza cada pista. Un
    proyecto con DTD no se lee (una «bomba» de entidades colgaria el nucleo)."""
    try:
        return _rust.sequence_read_als(os.fspath(path))
    except ValueError as e:
        raise SequenceError(str(e)) from None


def find(folder) -> dict:
    """La secuencia de esa carpeta (o de la carpeta de ese archivo): sus
    pistas con su papel, el proyecto, la portada y lo que dice el nombre.
    SequenceError si no lo parece."""
    try:
        return _rust.sequence_find(os.fspath(folder))
    except ValueError as e:
        raise SequenceError(str(e)) from None


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
    return _rust.sequence_grid(
        float(tempo), [(float(b), int(n), int(d)) for b, n, d in meters], float(duration)
    )


def markers(sections: list, tempo: float, duration: float) -> list[dict]:
    """Los localizadores como marcadores: cada parte hasta la siguiente."""
    return _rust.sequence_markers(
        [(float(s["beat"]), str(s["name"])) for s in sections], float(tempo), float(duration)
    )


# ------------------------------------------------------------ abrirla


def _duration(ffmpeg: str, path: str) -> float:
    # Las pistas ya son FLAC (ver `flac` en `import_sequence`), y su cabecera
    # dice las muestras que tiene: es exacto y al momento. Decodificarla
    # entera con ffmpeg queda por si la cabecera no lo dijera.
    seconds = tags.duration(path)
    if seconds > 0:
        return seconds
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
    return _rust.sequence_wave(r.stdout if r.returncode == 0 else None, rate, float(duration))


def _run(cmd: list[str], what: str) -> None:
    r = subprocess.run(cmd, capture_output=True, timeout=3600, check=False)
    if r.returncode != 0:
        log.warning("%s fallo: %s", what, r.stderr.decode("utf-8", "replace")[-800:])
        raise SequenceError(f"no se pudo {what}")


def _offset_filter(seconds: float) -> list[str]:
    """Una pista que en el proyecto empieza mas tarde: silencio delante."""
    return _rust.sequence_offset_filter(float(seconds))


def target_name(seq: dict) -> str:
    return _rust.sequence_target_name(seq["artist"], seq["title"])


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
        (work / stems.WAVES).write_text(
            _rust.sequence_waves_text([(k, p, r) for k, (p, r) in waves.items()]),
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
