# -*- coding: utf-8 -*-
"""Conversion de formatos a mp3, conservando etiquetas y portada.

Es opcional y desactivable: en la app sera una casilla.
Las carpetas de trabajo (Secuencias, Pistas) quedan siempre fuera:
ahi los .wav son material de produccion y comprimirlos seria perder calidad.
"""
import os, shutil, subprocess, tempfile, time
from pathlib import Path
from . import config

SOURCE_FORMATS = {".m4a", ".aac", ".ogg", ".opus", ".wma", ".flac", ".wav", ".alac", ".aiff"}
QUALITIES = {"high": ["-b:a", "320k"], "medium": ["-b:a", "192k"],
             "variable": ["-q:a", "0"], "copy": ["-b:a", "256k"]}


# `/api/status` se consulta constantemente desde la app, y cada consulta
# preguntaba al sistema de archivos por ffmpeg. Se recuerda la respuesta: si
# esta, para siempre (un binario no desaparece); si no esta, se vuelve a mirar
# de vez en cuando, para que instalarlo se note sin reiniciar la app.
_binaries: dict = {}
_RECHECK_SECONDS = 5.0


def _has_binary(name: str) -> bool:
    found, checked = _binaries.get(name, (False, 0.0))
    if found:
        return True
    if time.monotonic() - checked < _RECHECK_SECONDS:
        return False
    found = shutil.which(name) is not None
    _binaries[name] = (found, time.monotonic())
    return found


def available() -> bool:
    return _has_binary("ffmpeg")


def in_protected_folder(path) -> bool:
    """True si el archivo esta en una carpeta que no se debe convertir."""
    partes = {p.lower() for p in Path(path).parts}
    return bool(partes & {p.lower() for p in config.NEVER_CONVERT})


def needs_convert(path) -> bool:
    p = Path(path)
    return (p.suffix.lower() in SOURCE_FORMATS
            and not in_protected_folder(path))


def convert(path, quality="high", conservar_original=False, target=None) -> dict:
    """Convierte a mp3 conservando metadata y caratula. No toca carpetas protegidas."""
    path = Path(path)
    if not available():
        return {"ok": False, "reason": "falta ffmpeg"}
    if in_protected_folder(path):
        return {"ok": False, "reason": "carpeta protegida", "skipped": True}
    if path.suffix.lower() == ".mp3":
        return {"ok": False, "reason": "ya es mp3", "skipped": True}

    out = Path(target) if target else path.with_suffix(".mp3")
    if out.exists():
        base = out.with_suffix("")
        i = 2
        while out.exists():
            out = Path(f"{base} ({i}).mp3"); i += 1

    cmd = (["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(path),
            "-map", "0:a", "-map", "0:v?", "-c:v", "copy", "-id3v2_version", "3",
            "-map_metadata", "0", "-codec:a", "libmp3lame"]
           + QUALITIES.get(quality, QUALITIES["high"]) + [str(out)])
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    except subprocess.TimeoutExpired:
        return {"ok": False, "reason": "tiempo agotado"}
    if r.returncode != 0 or not out.exists():
        # segundo intento sin la portada, que a veces rompe el mapeo
        cmd2 = [c for c in cmd if c not in ("-map", "0:v?", "-c:v", "copy")]
        r = subprocess.run(cmd2, capture_output=True, text=True, timeout=600)
        if r.returncode != 0 or not out.exists():
            return {"ok": False, "reason": (r.stderr or "").strip()[:160]}

    before, after = path.stat().st_size, out.stat().st_size
    if not conservar_original:
        try:
            path.unlink()
        except OSError:
            pass
    return {"ok": True, "source_path": str(path), "target": str(out),
            "before": before, "after": after,
            "saved_percent": round(100 * (1 - after / before), 1) if before else 0}


# Una caratula no necesita mas de esto. Meter un png de 8 MB dentro de cada
# mp3 hincha la biblioteca y ralentiza la app para nada: a 1000 px ya se ve
# perfecta en cualquier pantalla.
COVER_MAX_SIDE = 1000
COVER_MAX_BYTES = 600 * 1024


def image_size(path) -> tuple[int, int]:
    """Ancho y alto de una imagen, o (0, 0) si no se puede saber."""
    if not _has_binary("ffprobe"):
        return (0, 0)
    try:
        r = subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x", str(path)],
            capture_output=True, text=True, timeout=20)
        w, h = r.stdout.strip().split("x")[:2]
        return int(w), int(h)
    except Exception:                                       # noqa: BLE001
        return (0, 0)


def shrink_image(path, max_side=COVER_MAX_SIDE) -> tuple[bytes, str] | None:
    """Devuelve (datos, mime) de la imagen, encogida si hacia falta.

    Se mira el peso Y las medidas: un png de 3000x3000 puede pesar poco y aun
    asi es absurdo meterlo en cada mp3, porque hay que decodificarlo entero
    cada vez que se pinta la caratula.

    Si ya vale, se devuelve tal cual: reconvertir una jpg correcta solo le
    quita calidad sin ganar nada.
    """
    p = Path(path)
    if not p.is_file():
        return None
    raw = p.read_bytes()
    mime = {"png": "image/png", "webp": "image/webp"}.get(
        p.suffix.lower().lstrip("."), "image/jpeg")
    w, h = image_size(p)
    if len(raw) <= COVER_MAX_BYTES and max(w, h) <= max_side:
        return raw, mime
    if not available():
        return raw, mime                 # sin ffmpeg se guarda tal cual
    out = Path(tempfile.gettempdir()) / f"danplay-cover-{os.getpid()}.jpg"
    cmd = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(p),
           "-vf", f"scale='min({max_side},iw)':-2", "-q:v", "3", str(out)]
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=60)
        if r.returncode == 0 and out.is_file():
            data = out.read_bytes()
            out.unlink(missing_ok=True)
            return (data, "image/jpeg") if data else (raw, mime)
    except (subprocess.TimeoutExpired, OSError):
        pass
    out.unlink(missing_ok=True)
    return raw, mime


def candidates(root=None) -> list[str]:
    """Archivos que se convertirian (respetando carpetas protegidas)."""
    from . import library as B
    root = root or config.LIBRARY
    return [r for r in B.audio_files(root) if needs_convert(r)]


def convert_batch(paths=None, quality="high", conservar_original=False,
                   dry_run=False, progress=None) -> dict:
    paths = paths if paths is not None else candidates()
    done_items, failures, skipped = [], [], []
    for i, r in enumerate(paths, 1):
        if dry_run:
            done_items.append({"source_path": r, "target": str(Path(r).with_suffix(".mp3")),
                           "dry_run": True})
        else:
            res = convert(r, quality, conservar_original)
            (done_items if res["ok"] else (skipped if res.get("skipped") else failures)).append(res)
        if progress:
            progress(i, len(paths))
    ahorro = sum(h.get("before", 0) - h.get("after", 0) for h in done_items)
    return {"converted": len(done_items), "failures": len(failures), "skipped": len(skipped),
            "saved_bytes": ahorro, "detail": done_items, "errors": failures}
