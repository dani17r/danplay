# -*- coding: utf-8 -*-
"""Etiquetas ID3 completas: metadata, estrellas (POPM), letra (USLT),
portada (APIC), tono (TKEY), BPM (TBPM) y campos propios (TXXX).

Todo se guarda DENTRO del archivo, para que la biblioteca sea portatil.
"""
import mutagen
from pathlib import Path
from mutagen.id3 import (ID3, ID3NoHeaderError, APIC, USLT, POPM, TXXX, TKEY,
                         TBPM, TIT2, TPE1, TPE2, TALB, TDRC, TCON, COMM)
from mutagen.mp3 import MP3

# 0-5 estrellas <-> valor POPM (convencion de Windows Media Player / Kodi)
POPM_STARS = {0: 0, 1: 1, 2: 64, 3: 128, 4: 196, 5: 255}
POPM_EMAIL = "danplay@local"

MIME_TYPES = {".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".png": "image/png",
        ".webp": "image/webp", ".gif": "image/gif"}


def _popm_to_stars(v: int) -> int:
    if v <= 0:   return 0
    if v <= 31:  return 1
    if v <= 95:  return 2
    if v <= 159: return 3
    if v <= 221: return 4
    return 5


def _id3(path, create=False) -> ID3 | None:
    try:
        return ID3(str(path))
    except ID3NoHeaderError:
        if not create:
            return None
        try:
            a = MP3(str(path)); a.add_tags(); a.save()
            return ID3(str(path))
        except Exception:
            return None
    except Exception:
        return None


# ---------------------------------------------------------------- lectura

def read(path) -> dict:
    """Etiquetas basicas (compatible con la version anterior)."""
    try:
        a = mutagen.File(str(path), easy=True)
        if a is None or not a.tags:
            return {}
        return {c: a.tags[c][0] for c in
                ("artist","title","album","date","albumartist","genre") if a.tags.get(c)}
    except Exception:
        return {}


def read_all(path) -> dict:
    """Todo: metadata + estrellas + letra + portada + tono + bpm + favorito."""
    d = {"artist":"", "title":"", "album":"", "year":"", "genre":"", "album_artist":"",
         "stars":0, "play_count":0, "favorite":False, "lyrics":"", "key":"",
         "bpm":0.0, "cover":False, "comment":"", "duration":0.0, "bitrate":0,
         "tags":[]}
    # UNA sola lectura del archivo. Antes se abria y parseaba dos veces: una
    # con mutagen.File() para la duracion y el bitrate, y otra con ID3() para
    # las etiquetas. En un escaneo eso es el doble de trabajo de disco por
    # cancion, y el escaneo es lo mas largo que hace la app.
    base = None
    try:
        base = mutagen.File(str(path))
        if base is not None and base.info:
            d["duration"] = float(getattr(base.info, "length", 0) or 0)
            d["bitrate"] = int(getattr(base.info, "bitrate", 0) or 0)
    except Exception:
        pass
    # en un mp3 las etiquetas que acaba de leer YA son el ID3 que hace falta
    t = getattr(base, "tags", None)
    if not isinstance(t, ID3):
        t = _id3(path)
    if t is None:
        d.update({k: v for k, v in read(path).items()})
        return d
    g = lambda k: (str(t.get(k)) if t.get(k) else "")
    d["title"], d["artist"], d["album"] = g("TIT2"), g("TPE1"), g("TALB")
    d["album_artist"], d["year"], d["genre"] = g("TPE2"), g("TDRC"), g("TCON")
    d["key"] = g("TKEY")
    try:
        d["bpm"] = float(g("TBPM") or 0)
    except ValueError:
        d["bpm"] = 0.0
    for k in t:
        if k.startswith("POPM"):
            p = t[k]
            d["stars"] = _popm_to_stars(getattr(p, "rating", 0))
            d["play_count"] = int(getattr(p, "count", 0) or 0)
        elif k.startswith("USLT"):
            d["lyrics"] = str(t[k].text)
        elif k.startswith("APIC"):
            d["cover"] = True
        elif k.startswith("COMM"):
            d["comment"] = str(t[k].text[0]) if t[k].text else ""
        elif k.startswith("TXXX:"):
            desc = k.split(":", 1)[1].upper()
            val = str(t[k].text[0]) if t[k].text else ""
            if desc == "FAVORITO":
                d["favorite"] = val in ("1", "true", "si", "yes")
            elif desc == "ETIQUETAS":
                d["tags"] = [x.strip() for x in val.split(",") if x.strip()]
    return d


def _open(path):
    """mutagen.File() a veces devuelve None (algunos .wav, por ejemplo).
    En ese caso se prueba el lector concreto segun la extension."""
    try:
        a = mutagen.File(str(path))
        if a is not None:
            return a
    except Exception:
        pass
    ext = Path(path).suffix.lower()
    lectores = {
        ".wav":  ("mutagen.wave", "WAVE"),
        ".mp3":  ("mutagen.mp3", "MP3"),
        ".flac": ("mutagen.flac", "FLAC"),
        ".m4a":  ("mutagen.mp4", "MP4"),
        ".aac":  ("mutagen.aac", "AAC"),
        ".ogg":  ("mutagen.oggvorbis", "OggVorbis"),
        ".opus": ("mutagen.oggopus", "OggOpus"),
        ".wma":  ("mutagen.asf", "ASF"),
        ".aiff": ("mutagen.aiff", "AIFF"),
    }
    if ext not in lectores:
        return None
    modulo, clase = lectores[ext]
    try:
        return getattr(__import__(modulo, fromlist=[clase]), clase)(str(path))
    except Exception:
        return None


def duration(path) -> float:
    a = _open(path)
    try:
        return float(a.info.length) if a is not None and a.info else 0.0
    except Exception:
        return 0.0


def bitrate(path) -> int:
    a = _open(path)
    try:
        return int(getattr(a.info, "bitrate", 0)) if a is not None and a.info else 0
    except Exception:
        return 0


# ---------------------------------------------------------------- escritura

def write(path, artist="", title="", album="", year="", genre="",
             albumartista="", comentario="") -> bool:
    t = _id3(path, create=True)
    if t is None:
        return False
    pairs = [(TIT2, title), (TPE1, artist), (TALB, album), (TDRC, year),
             (TCON, genre), (TPE2, albumartista)]
    try:
        for clase, value in pairs:
            if value:
                t.setall(clase.__name__, [clase(encoding=3, text=str(value))])
        if comentario:
            t.setall("COMM", [COMM(encoding=3, lang="spa", desc="", text=comentario)])
        t.save(v2_version=3)
        return True
    except Exception:
        return False


def rate(path, stars: int, play_count: int | None = None) -> bool:
    """Guarda 0-5 estrellas en el frame POPM (lo leen casi todos los reproductores)."""
    t = _id3(path, create=True)
    if t is None:
        return False
    stars = max(0, min(5, int(stars)))
    previous = 0
    for k in list(t.keys()):
        if k.startswith("POPM"):
            previous = int(getattr(t[k], "count", 0) or 0)
            del t[k]
    try:
        t.add(POPM(email=POPM_EMAIL, rating=POPM_STARS[stars],
                   count=previous if play_count is None else int(play_count)))
        t.save(v2_version=3)
        return True
    except Exception:
        return False


def set_favorite(path, value=True) -> bool:
    return _write_txxx(path, "FAVORITO", "1" if value else "0")


def set_labels(path, tags: list[str]) -> bool:
    return _write_txxx(path, "ETIQUETAS", ", ".join(tags))


def _write_txxx(path, description, value) -> bool:
    t = _id3(path, create=True)
    if t is None:
        return False
    try:
        t.setall(f"TXXX:{description}",
                 [TXXX(encoding=3, desc=description, text=str(value))])
        t.save(v2_version=3)
        return True
    except Exception:
        return False


def write_lyrics(path, lyrics: str, language="spa") -> bool:
    t = _id3(path, create=True)
    if t is None:
        return False
    try:
        for k in list(t.keys()):
            if k.startswith("USLT"):
                del t[k]
        t.add(USLT(encoding=3, lang=language, desc="", text=lyrics))
        t.save(v2_version=3)
        return True
    except Exception:
        return False


def write_analysis(path, key="", bpm=0.0) -> bool:
    t = _id3(path, create=True)
    if t is None:
        return False
    try:
        if key:
            t.setall("TKEY", [TKEY(encoding=3, text=str(key))])
        if bpm:
            t.setall("TBPM", [TBPM(encoding=3, text=str(int(round(float(bpm)))))])
        t.save(v2_version=3)
        return True
    except Exception:
        return False


def write_cover(path, data: bytes, mime="image/jpeg") -> bool:
    t = _id3(path, create=True)
    if t is None:
        return False
    try:
        for k in list(t.keys()):
            if k.startswith("APIC"):
                del t[k]
        t.add(APIC(encoding=3, mime=mime, type=3, desc="Cover", data=data))
        t.save(v2_version=3)
        return True
    except Exception:
        return False


def cover_from_file(audio_path, image_path) -> bool:
    p = Path(image_path)
    if not p.is_file():
        return False
    return write_cover(audio_path, p.read_bytes(),
                            MIME_TYPES.get(p.suffix.lower(), "image/jpeg"))


def extract_cover(path) -> tuple[bytes, str] | None:
    """Devuelve (datos, mime) de la caratula incrustada, o None."""
    t = _id3(path)
    if t is None:
        return None
    for k in t:
        if k.startswith("APIC"):
            return t[k].data, t[k].mime
    return None
