# -*- coding: utf-8 -*-
"""Etiquetas completas: metadata, estrellas, letra, portada, tono, BPM y
campos propios (favorito, listas, portada difuminada, etiquetas libres).

Todo se guarda DENTRO del archivo, para que la biblioteca sea portatil.

Tres familias de formato, con el mismo vocabulario de campos:

  - ID3 (mp3, y tambien wav/aiff, que llevan un ID3 dentro): POPM, USLT,
    APIC, TKEY, TBPM y TXXX con las descripciones de siempre (FAVORITO,
    LISTAS, PORTADA_BORROSA, ETIQUETAS).
  - Vorbis comments (flac, ogg, opus): ARTIST, TITLE..., RATING 0-100,
    LYRICS, INITIALKEY, BPM y campos DANPLAY_* para lo propio. La portada va
    como bloque Picture (en ogg, en METADATA_BLOCK_PICTURE).
  - MP4 (m4a): atomos ©ART, ©nam, ©alb..., covr, tmpo, y los campos
    libres ----:com.apple.iTunes:RATING / initialkey / DANPLAY_*.

Antes solo se sabia escribir ID3: en un flac o un m4a poner una estrella
«funcionaba» en la app, cambiaba el indice y el archivo se quedaba igual.
"""
import base64
import logging
import os
import mutagen
from collections import OrderedDict as _OrderedDict
from pathlib import Path
from threading import Lock as _Lock
from mutagen.id3 import (ID3, ID3NoHeaderError, APIC, USLT, POPM, TXXX, TKEY,
                         TBPM, TIT2, TPE1, TPE2, TALB, TDRC, TCON, COMM)
from mutagen.mp3 import MP3
from mutagen.flac import FLAC, Picture
from mutagen.mp4 import MP4, MP4Cover, MP4FreeForm
from mutagen.oggvorbis import OggVorbis
from mutagen.oggopus import OggOpus

log = logging.getLogger("danplay")

# 0-5 estrellas <-> valor POPM (convencion de Windows Media Player / Kodi)
POPM_STARS = {0: 0, 1: 1, 2: 64, 3: 128, 4: 196, 5: 255}
POPM_EMAIL = "danplay@local"

MIME_TYPES = {".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".png": "image/png",
        ".webp": "image/webp", ".gif": "image/gif"}

_NONE = (None, None, None)


def _popm_to_stars(v: int) -> int:
    if v <= 0:   return 0
    if v <= 31:  return 1
    if v <= 95:  return 2
    if v <= 159: return 3
    if v <= 221: return 4
    return 5


def _truthy(v: str) -> bool:
    return str(v).strip().lower() in ("1", "true", "si", "yes")


# Listas y etiquetas van en UN solo valor de texto. ID3v2.3 (lo que escribimos,
# por compatibilidad con Windows) no guarda valores multiples de verdad: los
# pega con «/», asi que un valor por lista no serviria. Se separan con «;»,
# que en un nombre de lista es rarisimo; los archivos viejos venian con «, »
# y se siguen leyendo. Un solo nombre que lleve coma se escribe con «;» al
# final para que al leer no se parta por la coma.
def _join_list(items) -> str:
    items = [str(x).strip() for x in items if str(x).strip()]
    value = "; ".join(items)
    if len(items) == 1 and "," in items[0]:
        value += ";"
    return value


def _split_list(value: str) -> list[str]:
    value = str(value or "")
    sep = ";" if ";" in value else ","
    return [x.strip() for x in value.split(sep) if x.strip()]


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
            a = MP3(str(path)); a.add_tags(); a.save()
            return ID3(str(path))
        except Exception:                                   # noqa: BLE001
            log.warning("no se pudo crear el ID3 de %s", path, exc_info=True)
            return None
    except Exception:                                       # noqa: BLE001
        log.warning("no se pudo leer el ID3 de %s", path, exc_info=True)
        return None


_READERS = {
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


def _open(path):
    """mutagen.File() a veces devuelve None (algunos .wav, por ejemplo).
    En ese caso se prueba el lector concreto segun la extension."""
    try:
        a = mutagen.File(str(path))
        if a is not None:
            return a
    except Exception:                                       # noqa: BLE001
        log.warning("mutagen no pudo abrir %s", path, exc_info=True)
    ext = Path(path).suffix.lower()
    if ext not in _READERS:
        return None
    module, cls = _READERS[ext]
    try:
        return getattr(__import__(module, fromlist=[cls]), cls)(str(path))
    except Exception:                                       # noqa: BLE001
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
    except Exception:                                       # noqa: BLE001
        log.warning("no se pudieron abrir las etiquetas de %s", path, exc_info=True)
        return _NONE


def _save(kind, audio) -> bool:
    try:
        if kind == "id3":
            audio.save(v2_version=3)
        else:
            audio.save()
        return True
    except Exception:                                       # noqa: BLE001
        log.warning("no se pudieron guardar las etiquetas", exc_info=True)
        return False


# ---------------------------------------------------------------- lectura

def read(path) -> dict:
    """Etiquetas basicas (compatible con la version anterior)."""
    try:
        a = mutagen.File(str(path), easy=True)
        if a is None or not a.tags:
            return {}
        return {c: a.tags[c][0] for c in
                ("artist","title","album","date","albumartist","genre") if a.tags.get(c)}
    except Exception:                                       # noqa: BLE001
        log.warning("no se pudieron leer las etiquetas de %s", path, exc_info=True)
        return {}


def _empty() -> dict:
    return {"artist":"", "title":"", "album":"", "year":"", "genre":"", "album_artist":"",
            "stars":0, "play_count":0, "favorite":False, "lyrics":"", "key":"",
            "bpm":0.0, "cover":False, "comment":"", "duration":0.0, "bitrate":0,
            "tags":[], "playlists":[], "blur":False, "study":""}


def _read_id3(t, d: dict) -> None:
    g = lambda k: (_text(t.get(k)) if t.get(k) else "")
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


_VORBIS = {"artist": "ARTIST", "title": "TITLE", "album": "ALBUM", "year": "DATE",
           "genre": "GENRE", "album_artist": "ALBUMARTIST", "comment": "COMMENT",
           "lyrics": "LYRICS", "key": "INITIALKEY", "bpm": "BPM",
           "favorite": "DANPLAY_FAVORITE", "blur": "DANPLAY_BLUR",
           "tags": "DANPLAY_LABELS", "playlists": "DANPLAY_PLAYLISTS",
           "study": "DANPLAY_STUDY"}


def _read_vorbis(audio, t, d: dict) -> None:
    g = lambda k: "; ".join(str(x) for x in (t.get(k) or []) if str(x))
    for field in ("artist", "title", "album", "year", "genre", "album_artist",
                  "comment", "key"):
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
    if isinstance(audio, FLAC):
        d["cover"] = bool(audio.pictures)
    else:
        d["cover"] = bool(t.get("METADATA_BLOCK_PICTURE"))


_MP4 = {"artist": "\xa9ART", "title": "\xa9nam", "album": "\xa9alb", "year": "\xa9day",
        "genre": "\xa9gen", "album_artist": "aART", "comment": "\xa9cmt",
        "lyrics": "\xa9lyr"}


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
    # UNA sola lectura del archivo. Antes se abria y parseaba dos veces: una
    # con mutagen.File() para la duracion y el bitrate, y otra con ID3() para
    # las etiquetas. En un escaneo eso es el doble de trabajo de disco por
    # cancion, y el escaneo es lo mas largo que hace la app.
    base = _open(path)
    try:
        if base is not None and base.info:
            d["duration"] = float(getattr(base.info, "length", 0) or 0)
            d["bitrate"] = int(getattr(base.info, "bitrate", 0) or 0)
    except Exception:                                       # noqa: BLE001
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
                d.update({k: v for k, v in read(path).items()})
    except Exception:                                       # noqa: BLE001
        log.warning("etiquetas ilegibles en %s", path, exc_info=True)
    return d


def duration(path) -> float:
    a = _open(path)
    try:
        return float(a.info.length) if a is not None and a.info else 0.0
    except Exception:                                       # noqa: BLE001
        log.warning("sin duracion para %s", path, exc_info=True)
        return 0.0


def bitrate(path) -> int:
    a = _open(path)
    try:
        return int(getattr(a.info, "bitrate", 0)) if a is not None and a.info else 0
    except Exception:                                       # noqa: BLE001
        log.warning("sin bitrate para %s", path, exc_info=True)
        return 0


# ---------------------------------------------------------------- escritura

_ID3_TEXT = {"artist": TPE1, "title": TIT2, "album": TALB, "year": TDRC,
             "genre": TCON, "album_artist": TPE2, "key": TKEY}
_ID3_TXXX = {"favorite": "FAVORITO", "blur": "PORTADA_BORROSA",
             "tags": "ETIQUETAS", "playlists": "LISTAS", "study": "ESTUDIO"}


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
        t.add(USLT(encoding=3, lang=fields.get("language", "spa"), desc="",
                   text=str(fields["lyrics"])))
    if "bpm" in fields:
        t.setall("TBPM", [TBPM(encoding=3, text=str(int(round(float(fields["bpm"])))))])
    if "stars" in fields:
        previous = 0
        for k in list(t.keys()):
            if k.startswith("POPM"):
                previous = int(getattr(t[k], "count", 0) or 0)
                del t[k]
        count = fields.get("play_count")
        t.add(POPM(email=POPM_EMAIL, rating=POPM_STARS[int(fields["stars"])],
                   count=previous if count is None else int(count)))
    for field, desc in _ID3_TXXX.items():
        if field in fields:
            t.setall(f"TXXX:{desc}",
                     [TXXX(encoding=3, desc=desc, text=str(fields[field]))])


def _apply_vorbis(t, fields: dict) -> None:
    for field, key in _VORBIS.items():
        if field in fields:
            value = fields[field]
            if field == "bpm":
                value = int(round(float(value)))
            t[key] = [str(value)]
    if "stars" in fields:
        t["RATING"] = [str(int(fields["stars"]) * 20)]


def _apply_mp4(t, fields: dict) -> None:
    for field, atom in _MP4.items():
        if field in fields:
            t[atom] = [str(fields[field])]
    if "bpm" in fields:
        t["tmpo"] = [int(round(float(fields["bpm"])))]
    freeform = {"key": "initialkey", "favorite": "DANPLAY_FAVORITE",
                "blur": "DANPLAY_BLUR", "tags": "DANPLAY_LABELS",
                "playlists": "DANPLAY_PLAYLISTS", "study": "DANPLAY_STUDY"}
    for field, name in freeform.items():
        if field in fields:
            t[_ff(name)] = [MP4FreeForm(str(fields[field]).encode("utf-8"))]
    if "stars" in fields:
        t[_ff("RATING")] = [MP4FreeForm(str(int(fields["stars"]) * 20).encode("utf-8"))]


def _write_fields(path, fields: dict) -> bool:
    """Escribe campos con nombre comun a todos los formatos. False si no se pudo."""
    kind, audio, t = _load(path, create=True)
    if kind is None:
        log.info("formato sin etiquetas editables: %s", path)
        return False
    try:
        {"id3": _apply_id3, "vorbis": _apply_vorbis, "mp4": _apply_mp4}[kind](t, fields)
    except Exception:                                       # noqa: BLE001
        log.warning("no se pudieron preparar las etiquetas de %s", path, exc_info=True)
        return False
    return _save(kind, audio)


def write(path, artist="", title="", album="", year="", genre="",
          album_artist="", comment="") -> bool:
    """Metadata basica. Solo se tocan los campos con valor: vaciar uno no lo borra."""
    fields = {k: v for k, v in (("artist", artist), ("title", title), ("album", album),
                                ("year", year), ("genre", genre),
                                ("album_artist", album_artist), ("comment", comment)) if v}
    return _write_fields(path, fields)


def rate(path, stars: int, play_count: int | None = None) -> bool:
    """Guarda 0-5 estrellas: POPM en ID3, RATING 0-100 en los demas."""
    fields = {"stars": max(0, min(5, int(stars)))}
    if play_count is not None:
        fields["play_count"] = int(play_count)
    return _write_fields(path, fields)


def set_favorite(path, value=True) -> bool:
    return _write_fields(path, {"favorite": "1" if value else "0"})


def set_blurred_cover(path, value=True) -> bool:
    """Marca la portada como «no la quiero ver bien».

    Se guarda dentro del archivo, igual que el favorito, para que la decision
    sobreviva a perder la base de datos y viaje con la cancion. La imagen NO
    se toca: se sigue guardando entera y el difuminado es solo al pintarla,
    asi que quitarlo la devuelve intacta.
    """
    return _write_fields(path, {"blur": "1" if value else "0"})


def set_labels(path, tags: list[str]) -> bool:
    return _write_fields(path, {"tags": _join_list(tags)})


def set_playlists(path, names: list[str]) -> bool:
    """A que listas pertenece la cancion. Es lo que permite recrearlas si se
    pierde la base de datos."""
    return _write_fields(path, {"playlists": _join_list(names)})


def _write_txxx(path, description, value) -> bool:
    """Un TXXX cualquiera. Solo tiene sentido en ID3; se conserva por si
    alguien lo usa a mano."""
    t = _id3(path, create=True)
    if t is None:
        return False
    try:
        t.setall(f"TXXX:{description}",
                 [TXXX(encoding=3, desc=description, text=str(value))])
        t.save(v2_version=3)
        return True
    except Exception:                                       # noqa: BLE001
        log.warning("no se pudo escribir TXXX:%s en %s", description, path, exc_info=True)
        return False


def write_lyrics(path, lyrics: str, language="spa") -> bool:
    return _write_fields(path, {"lyrics": lyrics or "", "language": language})


def write_study(path, study: str) -> bool:
    """Lo del modo estudio (bucle, velocidad, marcadores, notas), como JSON
    en una etiqueta propia: viaja con el archivo, como las estrellas."""
    return _write_fields(path, {"study": study or ""})


def write_analysis(path, key="", bpm=0.0) -> bool:
    fields = {}
    if key:
        fields["key"] = str(key)
    if bpm:
        fields["bpm"] = float(bpm)
    return _write_fields(path, fields) if fields else True


def _picture(data: bytes, mime: str) -> Picture:
    pic = Picture()
    pic.data = data
    pic.type = 3                         # portada delantera
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
                base64.b64encode(_picture(data, mime).write()).decode("ascii")]
        else:
            fmt = MP4Cover.FORMAT_PNG if mime == "image/png" else MP4Cover.FORMAT_JPEG
            t["covr"] = [MP4Cover(data, imageformat=fmt)]
    except Exception:                                       # noqa: BLE001
        log.warning("no se pudo preparar la portada de %s", path, exc_info=True)
        return False
    if not _save(kind, audio):
        return False
    # Se olvida lo cacheado AQUI, que es el unico sitio donde cambia una
    # caratula. No basta con mirar la fecha del archivo: la etiqueta cabe
    # en el hueco que ya habia, asi que el tamaño no cambia, y dos
    # escrituras seguidas pueden caer en el mismo tic del reloj del
    # sistema de archivos y quedarse con la MISMA fecha. Entonces la
    # entrada vieja seguiria pareciendo buena.
    forget_cover(path)
    return True


def cover_from_file(audio_path, image_path) -> bool:
    p = Path(image_path)
    if not p.is_file():
        return False
    return write_cover(audio_path, p.read_bytes(),
                            MIME_TYPES.get(p.suffix.lower(), "image/jpeg"))


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
                mime = "image/png" if getattr(c, "imageformat", None) == MP4Cover.FORMAT_PNG \
                    else "image/jpeg"
                return bytes(c), mime
    except Exception:                                       # noqa: BLE001
        log.warning("portada ilegible en %s", path, exc_info=True)
    return None


# Sacar una caratula significa abrir el archivo y parsear sus etiquetas. La
# cuadricula pide muchas y las vuelve a pedir cada vez que se desplaza, asi
# que se recuerdan las ultimas. La clave lleva la fecha y el tamaño del
# archivo: si cambia cualquiera de las dos —al incrustar otra caratula, por
# ejemplo— la entrada vieja deja de valer sola, sin nada que invalidar a mano.
#
# Dos topes: en bytes, porque una portada puede ocupar 20 KB o medio mega y
# en un equipo justo de memoria la diferencia importa; y en numero de
# entradas, porque las canciones SIN portada se guardan como None (pesan
# cero bytes) y sin ese tope nunca se desalojaban: con una biblioteca grande
# la cache crecia sin fin a base de nadas.
_COVER_CACHE: "OrderedDict[tuple, tuple[bytes, str] | None]" = _OrderedDict()
_COVER_CACHE_MAX_BYTES = 12 * 1024 * 1024
_COVER_CACHE_MAX_ENTRIES = 512
_cover_bytes = 0
_cover_lock = _Lock()


def cached_cover(path) -> tuple[bytes, str] | None:
    """Como `extract_cover`, pero sin releer el archivo si no ha cambiado."""
    global _cover_bytes
    try:
        st = os.stat(path)
    except OSError:
        return None
    key = (str(path), st.st_mtime, st.st_size)
    with _cover_lock:
        if key in _COVER_CACHE:
            _COVER_CACHE.move_to_end(key)
            return _COVER_CACHE[key]
    r = extract_cover(path)
    size = len(r[0]) if r else 0
    with _cover_lock:
        if key not in _COVER_CACHE:
            _COVER_CACHE[key] = r
            _cover_bytes += size
            while (len(_COVER_CACHE) > 1 and
                   (_cover_bytes > _COVER_CACHE_MAX_BYTES
                    or len(_COVER_CACHE) > _COVER_CACHE_MAX_ENTRIES)):
                _, old = _COVER_CACHE.popitem(last=False)
                _cover_bytes -= len(old[0]) if old else 0
    return r


def forget_cover(path) -> None:
    """Olvida lo cacheado de un archivo. Por si se toca sin cambiar la fecha."""
    global _cover_bytes
    with _cover_lock:
        for k in [k for k in _COVER_CACHE if k[0] == str(path)]:
            old = _COVER_CACHE.pop(k)
            _cover_bytes -= len(old[0]) if old else 0
