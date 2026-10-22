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

Las lee y las escribe el crate de Rust (`danplay_core`, con lofty: ver
library/src/tags.rs). Antes era mutagen, que es GPL y chocaba con
la licencia de DanPlay en los paquetes; queda en `tags_mutagen`, solo para
cuando el crate no esta (desarrollo, las pruebas sin Rust).
"""

import logging
import os
import re
from collections import OrderedDict as _OrderedDict
from pathlib import Path
from threading import Lock as _Lock

log = logging.getLogger(__name__)

try:
    import danplay_core as _rust

    if not hasattr(_rust, "tags_read"):  # un crate de antes de las etiquetas
        _rust = None
except ImportError:
    _rust = None

# 0-5 estrellas <-> valor POPM (convencion de Windows Media Player / Kodi)
POPM_STARS = {0: 0, 1: 1, 2: 64, 3: 128, 4: 196, 5: 255}
POPM_EMAIL = "danplay@local"

MIME_TYPES = {
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".png": "image/png",
    ".webp": "image/webp",
    ".gif": "image/gif",
}


def _popm_to_stars(v: int) -> int:
    if v <= 0:
        return 0
    if v <= 31:
        return 1
    if v <= 95:
        return 2
    if v <= 159:
        return 3
    if v <= 221:
        return 4
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


# Las marcas de tiempo de un LRC («[01:23.45]») y sus lineas de cabecera
# («[ar:Barak]», «[length: 3:42]»).
_LRC_STAMP = re.compile(r"\[\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?\]")
_LRC_HEADER = re.compile(r"^\s*\[[a-z]{1,8}:[^\]]*\]\s*$", re.IGNORECASE)


def lrc_to_plain(text: str) -> str:
    """La letra sin las marcas de tiempo de un LRC.

    Lo que va en el USLT del archivo y en la hoja del atril es la letra
    limpia: con las marcas, la hoja imprimia «[00:12.34]» delante de cada
    verso. Un texto que no es LRC sale tal cual.
    """
    if not text or not _LRC_STAMP.search(text):
        return text or ""
    lines = []
    for line in str(text).splitlines():
        if _LRC_HEADER.match(line):
            continue
        lines.append(_LRC_STAMP.sub("", line).strip())
    return "\n".join(lines).strip()


# ---------------------------------------------------------------- lo calculado
# Escribir una etiqueta cambia la fecha (y a veces el tamaño) del archivo,
# pero no su sonido. Lo que se calcula del sonido y se guarda por archivo (la
# rejilla del clic, la forma de onda) se reconoce por esa fecha: sin esto, se
# perdia cada vez que DanPlay guardaba algo en la cancion (el modo estudio
# lo hace a menudo) y el clic volvia a tardar 15-20 s.


def _stamp(path):
    try:
        return os.stat(path) if path else None
    except OSError:
        return None


def _retagged(path, before) -> None:
    """Tras reescribir la etiqueta de `path`: lo calculado de su sonido
    sigue valiendo con la fecha nueva."""
    after = _stamp(path)
    if before is None or after is None:
        return
    if (before.st_size, before.st_mtime) == (after.st_size, after.st_mtime):
        return
    from . import beatgrid, waveform

    for carry in (beatgrid.retag, waveform.retag):
        try:
            carry(str(path), before, after)
        except Exception:
            log.warning("no pude conservar lo calculado de %s", path, exc_info=True)


# ---------------------------------------------------------------- lectura


def _legacy():
    """La version con mutagen, para cuando no esta el crate."""
    from . import tags_mutagen

    return tags_mutagen


def _why() -> str:
    return (_rust.last_error() if _rust else None) or "motivo desconocido"


def read(path) -> dict:
    """Etiquetas basicas (compatible con la version anterior)."""
    if _rust is None:
        return _legacy().read(path)
    d = read_all(path)
    names = {"year": "date", "album_artist": "albumartist"}
    return {
        names.get(k, k): d[k]
        for k in ("artist", "title", "album", "year", "album_artist", "genre")
        if d[k]
    }


def _empty() -> dict:
    return {
        "artist": "",
        "title": "",
        "album": "",
        "year": "",
        "genre": "",
        "album_artist": "",
        "stars": 0,
        "play_count": 0,
        "favorite": False,
        "lyrics": "",
        "key": "",
        "bpm": 0.0,
        "cover": False,
        "comment": "",
        "duration": 0.0,
        "bitrate": 0,
        "tags": [],
        "playlists": [],
        "blur": False,
        "study": "",
    }


def read_all(path) -> dict:
    """Todo: metadata + estrellas + letra + portada + tono + bpm + favorito + listas,
    y la duracion y el bitrate, en UNA lectura del archivo."""
    if _rust is None:
        return _legacy().read_all(path)
    d = _rust.tags_read(str(path))
    if _rust.last_error():
        log.warning("etiquetas ilegibles en %s", _why())
    return d


def read_many(paths) -> dict:
    """`read_all` de muchas canciones, en paralelo: {ruta: etiquetas}."""
    if _rust is None:
        return {str(p): _legacy().read_all(p) for p in paths}
    return dict(_rust.tags_read_many([str(p) for p in paths]))


def duration(path) -> float:
    if _rust is None:
        return _legacy().properties(path)[0]
    return _rust.tags_properties(str(path))[0]


def bitrate(path) -> int:
    if _rust is None:
        return _legacy().properties(path)[1]
    return _rust.tags_properties(str(path))[1]


# ---------------------------------------------------------------- escritura


def _write_fields(path, fields: dict) -> bool:
    """Escribe campos con nombre comun a todos los formatos. False si no se pudo."""
    before = _stamp(path)
    if _rust is None:
        ok = _legacy().write_fields(path, fields)
    else:
        ok = _rust.tags_write(str(path), fields)
        if not ok:
            reason = _why()
            level = logging.INFO if "sin etiquetas editables" in reason else logging.WARNING
            log.log(level, "no se pudieron guardar las etiquetas de %s", reason)
    if ok:
        _retagged(path, before)
    return ok


def write(
    path, artist="", title="", album="", year="", genre="", album_artist="", comment=""
) -> bool:
    """Metadata basica. Solo se tocan los campos con valor: vaciar uno no lo borra."""
    fields = {
        k: v
        for k, v in (
            ("artist", artist),
            ("title", title),
            ("album", album),
            ("year", year),
            ("genre", genre),
            ("album_artist", album_artist),
            ("comment", comment),
        )
        if v
    }
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
    before = _stamp(path)
    if _rust is None:
        ok = _legacy().write_txxx(path, description, value)
    else:
        ok = _rust.tags_write_txxx(str(path), str(description), str(value))
        if not ok:
            log.warning("no se pudo escribir TXXX:%s en %s", description, _why())
    if ok:
        _retagged(path, before)
    return ok


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


def write_cover(path, data: bytes, mime="image/jpeg") -> bool:
    before = _stamp(path)
    if _rust is None:
        ok = _legacy().write_cover(path, data, mime)
    else:
        ok = _rust.cover_write(str(path), bytes(data), mime)
        if not ok:
            log.info("no se pudo poner la portada de %s", _why())
    if not ok:
        return False
    _retagged(path, before)
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
    return write_cover(audio_path, p.read_bytes(), MIME_TYPES.get(p.suffix.lower(), "image/jpeg"))


def extract_cover(path) -> tuple[bytes, str] | None:
    """Devuelve (datos, mime) de la caratula incrustada, o None."""
    if _rust is None:
        return _legacy().extract_cover(path)
    found = _rust.cover_read(str(path))
    if found is None and _rust.last_error():
        log.warning("portada ilegible en %s", _why())
    return found


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
_COVER_CACHE: _OrderedDict[tuple, tuple[bytes, str] | None] = _OrderedDict()
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
            while len(_COVER_CACHE) > 1 and (
                _cover_bytes > _COVER_CACHE_MAX_BYTES
                or len(_COVER_CACHE) > _COVER_CACHE_MAX_ENTRIES
            ):
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
