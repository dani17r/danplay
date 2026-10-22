"""Enriquecimiento: letra, portada, acordes, metadata y artistas implicados.

Fuentes, en orden de preferencia:
  letra    -> LRCLIB (abierto, gratis, con letra sincronizada) -> IA
  portada  -> iTunes Search (sin clave) -> Cover Art Archive
  acordes  -> un cifrado publicado (Ultimate Guitar, LaCuerda: ver chord_sheets.py),
              nunca la IA; el tono, solo si el cifrado lo dice
  metadata -> MusicBrainz -> IA

Lo hace el crate de Rust (library/src/enrich.rs, con el puente en
core/src/enrich.rs), paso a paso como antes. Aqui quedan la red (`_get`, con
urllib) y la puerta de cada funcion, con los mismos nombres. Rust busca en
este modulo, en el momento de usarlo, lo de fuera (`_get`, `log`, `ai`,
`library`, `tags`, `convert`, `chord_sheets`, `date`) y los pasos
(`lyrics_lrclib`, `lyrics`, `cover`, `details`, `chords`): lo que las pruebas
cambian se sigue usando.
"""

import json
import logging
import re
import sys
import urllib.request
from collections.abc import Mapping
from datetime import date  # noqa: F401  (Rust lo busca aqui: el dia de un cifrado)
from typing import Any

import danplay_core as _rust

# los busca Rust aqui, en el momento de usarlos (las pruebas los cambian)
from . import ai, chord_sheets, convert, library, tags  # noqa: F401

log = logging.getLogger(__name__)

(
    USER_AGENT,
    TIMEOUT,
    # Tope de lo que se descarga de una caratula. Sin el, una respuesta grande
    # (o una pagina de error) se incrustaba entera dentro del mp3.
    MAX_IMAGE_BYTES,
    _magic,
    _uuid,
    _guessed,
    _sheet,
    _fillable,
    _unknown,
) = _rust.enrich_constants()
# Los primeros bytes de cada formato. Una pagina HTML de error devuelta con
# `Content-Type: image/jpeg` no pasa de aqui.
MAGIC = tuple(_magic)
UUID = re.compile(_uuid, re.I)

# este modulo: lo de fuera que usa Rust se busca en el cada vez
_SELF = sys.modules[__name__]


def _get(url, headers=None, binary=False):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        if not binary:
            return json.loads(r.read().decode("utf-8"))
        kind = (r.headers.get("content-type") or "").split(";")[0].strip().lower()
        if not kind.startswith("image/"):
            raise ValueError(f"eso no es una imagen, es {kind or 'algo sin tipo'}")
        # se lee uno de mas para saber si se paso del tope, y no el archivo entero
        data = r.read(MAX_IMAGE_BYTES + 1)
        if len(data) > MAX_IMAGE_BYTES:
            raise ValueError("la imagen pesa demasiado")
        return data


def image_type(data: bytes) -> str:
    """El tipo real segun los primeros bytes, o cadena vacia si no es imagen."""
    return _rust.enrich_image_type(data)


# ---------------------------------------------------------------- letra

# La letra limpia, sin las marcas de un LRC (vive en `tags`, que es donde se
# decide que va en el USLT).
lrc_to_plain = tags.lrc_to_plain


def _lrclib_result(d) -> dict | None:
    """Lo que interesa de una respuesta de LRCLIB: la letra limpia y la LRC."""
    return _rust.enrich_lrclib_result(d)


def lyrics_lrclib(artist, title, album="", duration: float = 0) -> dict | None:
    """LRCLIB devuelve letra plana y sincronizada (.lrc). Sin clave de API."""
    return _rust.enrich_lyrics_lrclib(_SELF, artist, title, album, duration)


def lyrics(artist, title, album="", duration: float = 0, allow_ai=True) -> dict | None:
    return _rust.enrich_lyrics(_SELF, artist, title, album, duration, allow_ai)


# ---------------------------------------------------------------- portada


def _cover_queries(artist, title, album) -> list[str]:
    """Consultas a probar, de la mas precisa a la mas suelta.

    Los titulos que vienen de descargas llevan ruido («- r», «(En Vivo)»,
    «(cover X)») y con eso iTunes no encuentra nada. Se prueba tambien una
    version limpia, y sin artista cuando no lo hay.
    """
    return _rust.enrich_cover_queries(artist, title, album)


def _fetch_image(url: str) -> tuple[bytes, str] | None:
    """Descarga una imagen y la deja lista para incrustar: que de verdad sea
    una imagen (no la pagina de error de un servicio caido), sin pasar de
    5 MB, y encogida (una portada de 1000x1000 engordaba cada mp3 en dos
    megas)."""
    return _rust.enrich_fetch_image(_SELF, url)


def cover(artist, title, album="") -> tuple[bytes, str] | None:
    """Busca caratula. iTunes primero (rapido, sin clave), luego Cover Art Archive."""
    return _rust.enrich_cover(_SELF, artist, title, album)


# ---------------------------------------------------------------- IA


def details(song: Mapping[str, Any]) -> dict | None:
    """Artistas implicados, album, año, genero y contexto. El tono y los
    acordes no: esos salen de un cifrado publicado (`chords`)."""
    return _rust.enrich_details(_SELF, song)


# Lo que la IA decia de memoria sobre el tono y los acordes antes de 1.21. Ya
# no se pide, y lo que quede guardado no se enseña: eran aproximaciones.
_GUESSED = tuple(_guessed)
# lo que va en la columna `chords` y no es de la IA: el cifrado de la web
_SHEET = tuple(_sheet)


def _doc(song: Mapping[str, Any]) -> dict:
    """El JSON de la columna `chords`: la ficha de la IA y el cifrado."""
    return _rust.enrich_doc(_SELF, song)


def stored_sheet(song: Mapping[str, Any]) -> dict | None:
    """El cifrado guardado de una cancion, sin buscar nada."""
    return _rust.enrich_stored_sheet(_SELF, song)


def _save_doc(song_id, doc: dict) -> None:
    _rust.enrich_save_doc(_SELF, song_id, doc)


def _save_details(song: Mapping[str, Any], d: dict) -> None:
    """Guarda lo que dijo la IA sin perder el cifrado que ya hubiera."""
    _rust.enrich_save_details(_SELF, song, d)


def cached_details(song: Mapping[str, Any]) -> dict | None:
    """La ficha de la IA guardada, si merece la pena. Una respuesta con
    confianza baja no se reutiliza: la dio un modelo que no conocia la
    cancion, y con otro mejor —o el mismo otro dia— puede salir."""
    return _rust.enrich_cached_details(_SELF, song)


def _confidence(d: dict) -> float:
    """La confianza que dio la IA, como numero (a veces llega «alta» o nada)."""
    return _rust.enrich_confidence(d)


def details_for(song: Mapping[str, Any]) -> tuple[dict | None, bool]:
    """Los detalles de una cancion: los guardados si valen, y si no, se
    piden a la IA y se guardan. Devuelve (detalles, venian_guardados)."""
    return _rust.enrich_details_for(_SELF, song)


# ---------------------------------------------------------------- acordes


def chords(song: Mapping[str, Any], refresh: bool = False) -> dict:
    """El cifrado de una cancion: el guardado, o el que se encuentre en la
    web si no se habia buscado (o si se pide otra vez con `refresh`).

    Devuelve {"sheet": cifrado o None, "cached", "checked": cuando se busco,
    "tried": fuentes consultadas, "failed": las que no respondieron}. Que no
    este en ninguna se guarda tambien (no se vuelve a buscar sola cada vez);
    que no respondieran, no: es la red, no la cancion."""
    return _rust.enrich_chords(_SELF, song, refresh)


def transpose_sheet(sheet: Mapping[str, Any], semitones: int = 0, to_key: str = "") -> dict:
    """El cifrado en otro tono: solo se tocan las lineas de acordes (en una
    de letra, «Dios» no es un Re)."""
    return _rust.enrich_transpose_sheet(sheet, semitones, to_key)


# campos de ficha que la IA puede rellenar
FILLABLE = tuple(_fillable)

# Lo que devuelve un modelo cuando no sabe algo. Guardarlo seria peor que
# dejarlo vacio: el campo parece relleno y ya nadie vuelve a mirarlo.
UNKNOWN_VALUES = set(_unknown)


def _useful_value(field: str, value) -> str:
    """El valor si sirve, o cadena vacia. Evita guardar «desconocido»."""
    return _rust.enrich_useful_value(field, value)


def autofill(song_id) -> dict:
    """Rellena album, año, genero y tono, solo lo que este vacio.

    El tono sale del cifrado publicado de la cancion (y solo si lo dice), no
    de la IA: eso no necesita IA. Album, año y genero, de la IA.

    Devuelve que se relleno, que sigue faltando y por que, para que la interfaz
    pueda decirlo en vez de dejar al usuario adivinando.
    """
    return _rust.enrich_autofill(_SELF, song_id)


# ---------------------------------------------------------------- orquestacion


def enrich(
    song_id, with_lyrics=True, with_cover=True, with_details=True, save_to_file=True
) -> dict:
    """La letra, la caratula y la ficha de una cancion (lo que falte; la
    ficha, siempre que se pida), en el indice y en el archivo."""
    return _rust.enrich_enrich(_SELF, song_id, with_lyrics, with_cover, with_details, save_to_file)
