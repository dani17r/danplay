# -*- coding: utf-8 -*-
"""Tuberia de importacion.

Orden de resolucion, de mas fiable a menos:
  1. etiquetas ID3 ya presentes
  2. huella acustica (AcoustID -> MusicBrainz)
  3. heuristico contra los artistas que ya existen en la biblioteca
  4. IA (DeepInfra)
  5. si nada funciona -> carpeta Revisar/
"""
import os, shutil
from dataclasses import dataclass, field
from pathlib import Path
from . import config, convert, tags, fingerprint, ai, names

HEURISTIC_THRESHOLD = 0.80
AI_THRESHOLD         = 0.55

CATEGORY_FOLDER = {
    "track":     "Pistas",
    "sequence": "Secuencias",
    "tutorial":  "Tutoriales y Play Along",
}


@dataclass
class Result:
    source_path: Path
    target: Path | None = None
    artist: str = ""
    title: str = ""
    feat: str = ""
    extra: str = ""
    source: str = ""
    confidence: float = 0.0
    action: str = ""            # movido | simulado | revisar | error
    note: str = ""
    warnings: list = field(default_factory=list)


def _from_tags(path) -> dict | None:
    t = tags.read(path)
    if t.get("artist") and t.get("title"):
        return {"artist": t["artist"], "title": t["title"], "feat": "", "extra": "",
                "category": "song", "confidence": 0.95, "source": "tags"}
    return None


def _from_fingerprint(path) -> dict | None:
    r = fingerprint.identify(path)
    if r:
        return {"artist": r["artist"], "title": r["title"], "feat": "", "extra": "",
                "category": "song", "confidence": r["score"], "source": "fingerprint"}
    return None


def _from_heuristic(path, vocab) -> dict | None:
    d = names.detect_artist(os.path.basename(path), vocab)
    if d["artist"] and d["confidence"] >= HEURISTIC_THRESHOLD:
        return {**d, "extra": "", "category": "song", "source": "heuristic"}
    return None


def _from_ai(path, vocab) -> dict | None:
    seconds = tags.duration(path)
    pista = f"Duracion: {int(seconds//60)}:{int(seconds%60):02d}\n" if seconds else ""
    r = ai.resolve(os.path.basename(path), set(vocab.values()), pista)
    if not r or r.get("error"):
        return r
    if r["artist"] and r["confidence"] >= AI_THRESHOLD:
        return r
    return None


def _safe(step, *args):
    """Un escalon que falla no debe tumbar la cascada entera.

    Paso esto de verdad: AcoustID devolvia un error de servicio, la excepcion
    subia hasta arriba y NADA se podia importar. Un escalon roto solo significa
    que decide el siguiente.
    """
    try:
        return step(*args)
    except Exception:                                       # noqa: BLE001
        return None


def resolve(path, vocab) -> dict:
    """Aplica la cascada y devuelve el mejor dato disponible."""
    for step in (_from_tags, _from_fingerprint):
        d = _safe(step, path)
        if d:
            return d
    d = _safe(_from_heuristic, path, vocab)
    if d:
        return d
    d = _safe(_from_ai, path, vocab)
    if d and not d.get("error"):
        return d
    return {"artist": "", "title": names.clean(Path(path).stem), "feat": "",
            "extra": "", "category": "unknown", "confidence": 0.0,
            "source": d.get("error", "none") if d else "none"}


def process(path, vocab=None, dry_run=False, escribir_tags=None,
             convert_mp3=None) -> Result:
    path = Path(path)
    convert_mp3 = config.CONVERT_TO_MP3 if convert_mp3 is None else convert_mp3
    vocab = vocab if vocab is not None else names.vocabulary(config.ARTISTS_DIR)
    escribir_tags = config.WRITE_TAGS if escribir_tags is None else escribir_tags
    res = Result(source_path=path)

    # conversion opcional a mp3 (la casilla de la app)
    if convert_mp3 and not dry_run and convert.needs_convert(path):
        c = convert.convert(path, config.MP3_QUALITY, config.KEEP_ORIGINAL)
        if c.get("ok"):
            pct = c['saved_percent']
            res.warnings.append("convertido a mp3 (" +
                (f"{pct}% mas ligero" if pct > 0 else f"{abs(pct)}% mas pesado") + ")")
            path = Path(c["target"])
            res.source_path = path
        elif not c.get("skipped"):
            res.warnings.append(f"no se pudo convertir: {c.get('reason','')}")
    elif convert_mp3 and dry_run and convert.needs_convert(path):
        res.warnings.append("se convertiria a mp3")

    d = resolve(path, vocab)
    res.artist, res.title = d["artist"], d["title"]
    res.feat, res.extra = d.get("feat", ""), d.get("extra", "")
    res.source, res.confidence = d.get("source", ""), float(d.get("confidence", 0))

    # limpieza final segun las reglas de la casa
    res.artist = names.clean(res.artist) if res.artist else ""
    res.title  = names.clean(res.title) or path.stem
    ext = path.suffix.lower()

    category = d.get("category", "song")
    if category in CATEGORY_FOLDER:
        folder = config.LIBRARY / CATEGORY_FOLDER[category]
        name = names.final_name("", res.title, res.feat, res.extra, ext)
    elif res.artist:
        # reutiliza la carpeta existente si ya hay una equivalente
        real_name = vocab.get(names._flat(res.artist), res.artist)
        res.artist = real_name
        folder = config.ARTISTS_DIR / names.sanitize(real_name)
        name = names.final_name(real_name, res.title, res.feat, res.extra, ext)
    else:
        folder = config.REVIEW_DIR
        name = names.final_name("", res.title, res.feat, res.extra, ext)
        res.action = "review"

    folder.mkdir(parents=True, exist_ok=True)
    name = names.free_name(str(folder), name)
    if name != names.final_name(res.artist, res.title, res.feat, res.extra, ext):
        if " - r" in Path(name).stem:
            res.warnings.append("posible duplicado: se guardo con sufijo ' - r'")
    res.target = folder / name

    if dry_run:
        res.action = res.action or "dry_run"
        return res
    try:
        shutil.move(str(path), str(res.target))
        if escribir_tags and res.artist:
            if not tags.write(res.target, res.artist, res.title):
                res.warnings.append("no se pudieron escribir las etiquetas")
        if res.action != "review":
            res.action = "moved"
        if res.artist and names._flat(res.artist) not in vocab:
            vocab[names._flat(res.artist)] = res.artist
    except Exception as e:
        res.action, res.note = "error", str(e)
    return res


def process_inbox(dry_run=False, limit=None, convert_mp3=None) -> list[Result]:
    """Procesa todo lo que haya en la carpeta Entrada/."""
    config.INBOX.mkdir(parents=True, exist_ok=True)
    vocab = names.vocabulary(config.ARTISTS_DIR)
    pendientes = sorted(p for p in config.INBOX.iterdir()
                        if p.is_file() and p.suffix.lower() in config.EXTENSIONS)
    if limit:
        pendientes = pendientes[:limit]
    return [process(p, vocab, dry_run, convert_mp3=convert_mp3) for p in pendientes]
