"""Las etiquetas con mutagen: lo que hacia `tags` antes de lofty.

Solo se usa si el crate de Rust (`danplay_core`) no esta, en desarrollo o en
las pruebas sin Rust; los paquetes llevan el crate y no llevan mutagen, que es
GPL (ver docs/DEPENDENCIAS.md). Por eso mutagen esta entre las dependencias de
las pruebas y no entre las de la app: aqui se importa al usarse.

Las mismas tres familias y el mismo vocabulario que `biblioteca/src/
etiquetas.rs`, y las pruebas comparan las dos (tests/test_formats.py). Lo de
conservar el pulso y la onda al reescribir (`tags._retagged`) lo hace `tags`.
"""

import base64
import logging
from pathlib import Path

import mutagen
from mutagen.flac import FLAC, Picture
from mutagen.id3 import (
    APIC,
    COMM,
    ID3,
    POPM,
    TALB,
    TBPM,
    TCON,
    TDRC,
    TIT2,
    TKEY,
    TPE1,
    TPE2,
    TXXX,
    USLT,
    ID3NoHeaderError,
)
from mutagen.mp3 import MP3
from mutagen.mp4 import MP4, MP4Cover, MP4FreeForm
from mutagen.oggopus import OggOpus
from mutagen.oggvorbis import OggVorbis

from .tags import POPM_EMAIL, POPM_STARS, _empty, _popm_to_stars, _split_list, _truthy

log = logging.getLogger(__name__)

_NONE = (None, None, None)


def _text(frame) -> str:
    """Texto de un frame ID3. `str(frame)` pegaba los valores multiples con
    un NUL, y un NUL dentro de un artista rompia `shutil.move` («embedded
    null byte») al renombrar."""
    try:
        return "; ".join(str(x) for x in frame.text if str(x))
    except (AttributeError, TypeError):
        return str(frame)


# ---------------------------------------------------------------- apertura


def _id3(path, create=False) -> ID3 | None:
    """El ID3 de un mp3, sin parsear el audio (es lo que tarda)."""
    try:
        return ID3(str(path))
    except ID3NoHeaderError:
        if not create:
            return None
        try:
            a = MP3(str(path))
            a.add_tags()
            a.save()
            return ID3(str(path))
        except Exception:
            log.warning("no se pudo crear el ID3 de %s", path, exc_info=True)
            return None
    except Exception:
        log.warning("no se pudo leer el ID3 de %s", path, exc_info=True)
        return None


_READERS = {
    ".wav": ("mutagen.wave", "WAVE"),
    ".mp3": ("mutagen.mp3", "MP3"),
    ".flac": ("mutagen.flac", "FLAC"),
    ".m4a": ("mutagen.mp4", "MP4"),
    ".aac": ("mutagen.aac", "AAC"),
    ".ogg": ("mutagen.oggvorbis", "OggVorbis"),
    ".opus": ("mutagen.oggopus", "OggOpus"),
    ".wma": ("mutagen.asf", "ASF"),
    ".aiff": ("mutagen.aiff", "AIFF"),
}


def _open(path):
    """mutagen.File() a veces devuelve None (algunos .wav, por ejemplo).
    En ese caso se prueba el lector concreto segun la extension."""
    try:
        a = mutagen.File(str(path))
        if a is not None:
            return a
    except Exception:
        log.warning("mutagen no pudo abrir %s", path, exc_info=True)
    ext = Path(path).suffix.lower()
    if ext not in _READERS:
        return None
    module, cls = _READERS[ext]
    try:
        return getattr(__import__(module, fromlist=[cls]), cls)(str(path))
    except Exception:
        log.warning("no se pudo abrir %s como %s", path, cls, exc_info=True)
        return None


def _load(path, create=False):
    """(familia, objeto que se guarda, etiquetas) segun el formato.

    `familia` es "id3", "vorbis" o "mp4". Con `create` se añade el bloque de
    etiquetas si el archivo no lo tiene. (None, None, None) si el formato no
    admite etiquetas o el archivo no se puede abrir.
    """
    ext = Path(path).suffix.lower()
    try:
        if ext in (".wav", ".aiff", ".aif"):
            # contenedores con un ID3 dentro: se guarda por el contenedor
            audio = _open(path)
            if audio is None or not hasattr(audio, "add_tags"):
                return _NONE
            if audio.tags is None:
                if not create:
                    return _NONE
                audio.add_tags()
            return ("id3", audio, audio.tags) if isinstance(audio.tags, ID3) else _NONE
        if ext in (".flac", ".ogg", ".oga", ".opus"):
            audio = _open(path)
            if not isinstance(audio, (FLAC, OggVorbis, OggOpus)):
                return _NONE
            if audio.tags is None:
                if not create:
                    return _NONE
                audio.add_tags()
            return "vorbis", audio, audio.tags
        if ext in (".m4a", ".mp4", ".m4b"):
            audio = _open(path)
            if not isinstance(audio, MP4):
                return _NONE
            if audio.tags is None:
                if not create:
                    return _NONE
                audio.add_tags()
            return "mp4", audio, audio.tags
        # mp3 (y cualquier otro con ID3 al principio)
        t = _id3(path, create)
        return ("id3", t, t) if t is not None else _NONE
    except Exception:
        log.warning("no se pudieron abrir las etiquetas de %s", path, exc_info=True)
        return _NONE


def _save(kind, audio) -> bool:
    try:
        if kind == "id3":
            audio.save(v2_version=3)
        else:
            audio.save()
        return True
    except Exception:
        log.warning("no se pudieron guardar las etiquetas", exc_info=True)
        return False


# ---------------------------------------------------------------- lectura


def read(path) -> dict:
    """Etiquetas basicas (compatible con la version anterior)."""
    try:
        a = mutagen.File(str(path), easy=True)
        if a is None or not a.tags:
            return {}
        return {
            c: a.tags[c][0]
            for c in ("artist", "title", "album", "date", "albumartist", "genre")
            if a.tags.get(c)
        }
    except Exception:
        log.warning("no se pudieron leer las etiquetas de %s", path, exc_info=True)
        return {}


def _read_id3(t, d: dict) -> None:
    def g(k):
        return _text(t.get(k)) if t.get(k) else ""

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
            val = _text(t[k])
            if desc == "FAVORITO":
                d["favorite"] = _truthy(val)
            elif desc == "PORTADA_BORROSA":
                d["blur"] = _truthy(val)
            elif desc == "ETIQUETAS":
                d["tags"] = _split_list(val)
            elif desc == "LISTAS":
                d["playlists"] = _split_list(val)
            elif desc == "ESTUDIO":
                d["study"] = val


_VORBIS = {
    "artist": "ARTIST",
    "title": "TITLE",
    "album": "ALBUM",
    "year": "DATE",
    "genre": "GENRE",
    "album_artist": "ALBUMARTIST",
    "comment": "COMMENT",
    "lyrics": "LYRICS",
    "key": "INITIALKEY",
    "bpm": "BPM",
    "favorite": "DANPLAY_FAVORITE",
    "blur": "DANPLAY_BLUR",
    "tags": "DANPLAY_LABELS",
    "playlists": "DANPLAY_PLAYLISTS",
    "study": "DANPLAY_STUDY",
}


def _read_vorbis(audio, t, d: dict) -> None:
    def g(k):
        return "; ".join(str(x) for x in (t.get(k) or []) if str(x))

    for field in ("artist", "title", "album", "year", "genre", "album_artist", "comment", "key"):
        d[field] = g(_VORBIS[field])
    d["lyrics"] = g("LYRICS") or g("UNSYNCEDLYRICS")
    d["key"] = d["key"] or g("KEY")
    try:
        d["bpm"] = float(g("BPM") or 0)
    except ValueError:
        d["bpm"] = 0.0
    try:
        d["stars"] = max(0, min(5, round(float(g("RATING") or 0) / 20)))
    except ValueError:
        d["stars"] = 0
    d["favorite"] = _truthy(g("DANPLAY_FAVORITE"))
    d["blur"] = _truthy(g("DANPLAY_BLUR"))
    d["tags"] = _split_list(g("DANPLAY_LABELS"))
    d["playlists"] = _split_list(g("DANPLAY_PLAYLISTS"))
    # el modo estudio: se escribia en flac/ogg/opus y nunca se leia, asi que
    # perdida la base no volvia (en mp3, m4a y wav si)
    d["study"] = g("DANPLAY_STUDY")
    if isinstance(audio, FLAC):
        d["cover"] = bool(audio.pictures)
    else:
        d["cover"] = bool(t.get("METADATA_BLOCK_PICTURE"))


_MP4 = {
    "artist": "\xa9ART",
    "title": "\xa9nam",
    "album": "\xa9alb",
    "year": "\xa9day",
    "genre": "\xa9gen",
    "album_artist": "aART",
    "comment": "\xa9cmt",
    "lyrics": "\xa9lyr",
}


def _ff(name: str) -> str:
    return f"----:com.apple.iTunes:{name}"


def _read_mp4(t, d: dict) -> None:
    def g(k):
        vals = t.get(k) or []
        out = []
        for x in vals:
            if isinstance(x, (bytes, MP4FreeForm)):
                x = bytes(x).decode("utf-8", "replace")
            if str(x):
                out.append(str(x))
        return "; ".join(out)

    for field, atom in _MP4.items():
        d[field] = g(atom)
    d["key"] = g(_ff("initialkey"))
    try:
        d["bpm"] = float((t.get("tmpo") or [0])[0] or 0)
    except (ValueError, TypeError):
        d["bpm"] = 0.0
    try:
        d["stars"] = max(0, min(5, round(float(g(_ff("RATING")) or 0) / 20)))
    except ValueError:
        d["stars"] = 0
    d["favorite"] = _truthy(g(_ff("DANPLAY_FAVORITE")))
    d["blur"] = _truthy(g(_ff("DANPLAY_BLUR")))
    d["tags"] = _split_list(g(_ff("DANPLAY_LABELS")))
    d["playlists"] = _split_list(g(_ff("DANPLAY_PLAYLISTS")))
    d["study"] = g(_ff("DANPLAY_STUDY"))
    d["cover"] = bool(t.get("covr"))


def read_all(path) -> dict:
    """Todo: metadata + estrellas + letra + portada + tono + bpm + favorito + listas."""
    d = _empty()
    # UNA sola lectura del archivo: la duracion, el bitrate y las etiquetas
    base = _open(path)
    try:
        if base is not None and base.info:
            d["duration"] = float(getattr(base.info, "length", 0) or 0)
            d["bitrate"] = int(getattr(base.info, "bitrate", 0) or 0)
    except Exception:
        log.warning("sin duracion para %s", path, exc_info=True)
    t = getattr(base, "tags", None)
    try:
        if isinstance(t, ID3):
            _read_id3(t, d)
        elif isinstance(base, (FLAC, OggVorbis, OggOpus)) and t is not None:
            _read_vorbis(base, t, d)
        elif isinstance(base, MP4) and t is not None:
            _read_mp4(t, d)
        else:
            # un mp3 sin ID3 (o algo raro): lo poco que sepa el lector facil
            t = _id3(path) if not isinstance(base, MP3) else None
            if t is not None:
                _read_id3(t, d)
            else:
                d.update(dict(read(path).items()))
    except Exception:
        log.warning("etiquetas ilegibles en %s", path, exc_info=True)
    return d


def properties(path) -> tuple[float, int]:
    """(segundos, bits por segundo)."""
    a = _open(path)
    try:
        if a is None or not a.info:
            return 0.0, 0
        return float(a.info.length or 0), int(getattr(a.info, "bitrate", 0) or 0)
    except Exception:
        log.warning("sin duracion para %s", path, exc_info=True)
        return 0.0, 0


# ---------------------------------------------------------------- escritura

_ID3_TEXT = {
    "artist": TPE1,
    "title": TIT2,
    "album": TALB,
    "year": TDRC,
    "genre": TCON,
    "album_artist": TPE2,
    "key": TKEY,
}
_ID3_TXXX = {
    "favorite": "FAVORITO",
    "blur": "PORTADA_BORROSA",
    "tags": "ETIQUETAS",
    "playlists": "LISTAS",
    "study": "ESTUDIO",
}


def _apply_id3(t, fields: dict) -> None:
    for field, cls in _ID3_TEXT.items():
        if field in fields:
            t.setall(cls.__name__, [cls(encoding=3, text=str(fields[field]))])
    if "comment" in fields:
        t.setall("COMM", [COMM(encoding=3, lang="spa", desc="", text=str(fields["comment"]))])
    if "lyrics" in fields:
        for k in list(t.keys()):
            if k.startswith("USLT"):
                del t[k]
        t.add(
            USLT(
                encoding=3, lang=fields.get("language", "spa"), desc="", text=str(fields["lyrics"])
            )
        )
    if "bpm" in fields:
        t.setall("TBPM", [TBPM(encoding=3, text=str(round(float(fields["bpm"]))))])
    if "stars" in fields:
        previous = 0
        for k in list(t.keys()):
            if k.startswith("POPM"):
                previous = int(getattr(t[k], "count", 0) or 0)
                del t[k]
        count = fields.get("play_count")
        t.add(
            POPM(
                email=POPM_EMAIL,
                rating=POPM_STARS[int(fields["stars"])],
                count=previous if count is None else int(count),
            )
        )
    for field, desc in _ID3_TXXX.items():
        if field in fields:
            t.setall(f"TXXX:{desc}", [TXXX(encoding=3, desc=desc, text=str(fields[field]))])


def _apply_vorbis(t, fields: dict) -> None:
    for field, key in _VORBIS.items():
        if field in fields:
            value = fields[field]
            if field == "bpm":
                value = round(float(value))
            t[key] = [str(value)]
    if "stars" in fields:
        t["RATING"] = [str(int(fields["stars"]) * 20)]


def _apply_mp4(t, fields: dict) -> None:
    for field, atom in _MP4.items():
        if field in fields:
            t[atom] = [str(fields[field])]
    if "bpm" in fields:
        t["tmpo"] = [round(float(fields["bpm"]))]
    freeform = {
        "key": "initialkey",
        "favorite": "DANPLAY_FAVORITE",
        "blur": "DANPLAY_BLUR",
        "tags": "DANPLAY_LABELS",
        "playlists": "DANPLAY_PLAYLISTS",
        "study": "DANPLAY_STUDY",
    }
    for field, name in freeform.items():
        if field in fields:
            t[_ff(name)] = [MP4FreeForm(str(fields[field]).encode("utf-8"))]
    if "stars" in fields:
        t[_ff("RATING")] = [MP4FreeForm(str(int(fields["stars"]) * 20).encode("utf-8"))]


def write_fields(path, fields: dict) -> bool:
    """Escribe campos con nombre comun a todos los formatos. False si no se pudo."""
    kind, audio, t = _load(path, create=True)
    if kind is None:
        log.info("formato sin etiquetas editables: %s", path)
        return False
    try:
        {"id3": _apply_id3, "vorbis": _apply_vorbis, "mp4": _apply_mp4}[kind](t, fields)
    except Exception:
        log.warning("no se pudieron preparar las etiquetas de %s", path, exc_info=True)
        return False
    return _save(kind, audio)


def write_txxx(path, description, value) -> bool:
    t = _id3(path, create=True)
    if t is None:
        return False
    try:
        t.setall(f"TXXX:{description}", [TXXX(encoding=3, desc=description, text=str(value))])
        t.save(v2_version=3)
        return True
    except Exception:
        log.warning("no se pudo escribir TXXX:%s en %s", description, path, exc_info=True)
        return False


def _picture(data: bytes, mime: str) -> Picture:
    pic = Picture()
    pic.data = data
    pic.type = 3  # portada delantera
    pic.mime = mime
    pic.desc = "Cover"
    return pic


def write_cover(path, data: bytes, mime="image/jpeg") -> bool:
    kind, audio, t = _load(path, create=True)
    if kind is None:
        log.info("formato sin portada editable: %s", path)
        return False
    try:
        if kind == "id3":
            for k in list(t.keys()):
                if k.startswith("APIC"):
                    del t[k]
            t.add(APIC(encoding=3, mime=mime, type=3, desc="Cover", data=data))
        elif isinstance(audio, FLAC):
            audio.clear_pictures()
            audio.add_picture(_picture(data, mime))
        elif kind == "vorbis":
            t["METADATA_BLOCK_PICTURE"] = [
                base64.b64encode(_picture(data, mime).write()).decode("ascii")
            ]
        else:
            fmt = MP4Cover.FORMAT_PNG if mime == "image/png" else MP4Cover.FORMAT_JPEG
            t["covr"] = [MP4Cover(data, imageformat=fmt)]
    except Exception:
        log.warning("no se pudo preparar la portada de %s", path, exc_info=True)
        return False
    return _save(kind, audio)


def extract_cover(path) -> tuple[bytes, str] | None:
    """Devuelve (datos, mime) de la caratula incrustada, o None."""
    kind, audio, t = _load(path)
    if kind is None:
        return None
    try:
        if kind == "id3":
            for k in t:
                if k.startswith("APIC"):
                    return t[k].data, t[k].mime
        elif isinstance(audio, FLAC):
            if audio.pictures:
                p = audio.pictures[0]
                return p.data, p.mime or "image/jpeg"
        elif kind == "vorbis":
            raw = (t.get("METADATA_BLOCK_PICTURE") or [None])[0]
            if raw:
                p = Picture(base64.b64decode(raw))
                return p.data, p.mime or "image/jpeg"
        else:
            covers = t.get("covr") or []
            if covers:
                c = covers[0]
                mime = (
                    "image/png"
                    if getattr(c, "imageformat", None) == MP4Cover.FORMAT_PNG
                    else "image/jpeg"
                )
                return bytes(c), mime
    except Exception:
        log.warning("portada ilegible en %s", path, exc_info=True)
    return None
