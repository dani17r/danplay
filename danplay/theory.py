"""Teoria musical: transposicion de acordes y tonalidades.

Deterministico a proposito: transponer es matematica, no se le pregunta a una IA.

Lo hace el crate de Rust (library/src/theory.rs, con la misma expresion
para reconocer un acorde); esto es la puerta de Python, con los mismos nombres.
"""

import danplay_core as _rust

_SHARPS, _FLATS, _INDEX, _FLAT_KEYS, _LATIN = _rust.theory_tables()

SHARPS = list(_SHARPS)
FLATS = list(_FLATS)
INDEX_SQL = dict(_INDEX)

# tonalidades que se escriben con bemoles
FLAT_KEYS = set(_FLAT_KEYS)

LATIN = dict(_LATIN)

EASY_SHAPES = ("G", "C", "D", "A", "E", "Em", "Am", "Dm")


def _txt(s) -> str:
    """Un texto que se pueda pasar a Rust: los «surrogates» de un texto que
    no es UTF-8 se cambian por «�»."""
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        return s.encode("utf-8", "surrogateescape").decode("utf-8", "replace")
    except AttributeError:
        pass
    return s


def _flats(use_flats):
    return None if use_flats is None else bool(use_flats)


def transpose_note(note, semitones, use_flats=None):
    if note not in INDEX_SQL:
        return note
    return _rust.theory_transpose_note(note, semitones % 12, _flats(use_flats))


def transpose(text, semitones, use_flats=None):
    """Transpone todos los acordes de un texto conservando el formato."""
    if not semitones:
        return text
    # doce semitonos no son cero: la nota se reescribe («Cb» es «B»)
    return _rust.theory_transpose(_txt(text), semitones % 12 or 12, _flats(use_flats))


def distance(from_key, to_key):
    """Semitonos entre dos tonalidades. None si no se reconocen."""
    return _rust.theory_distance(_txt(from_key or ""), _txt(to_key or ""))


def transpose_to(text, current_key, to_key):
    if distance(current_key, to_key) is None:
        return text
    return _rust.theory_transpose_to(_txt(text), _txt(current_key or ""), _txt(to_key or ""))


def to_latin(text):
    """Cifrado americano -> latino (Do Re Mi)."""
    return _rust.theory_to_latin(_txt(text))


def available_keys():
    return _rust.theory_available_keys()


def suggested_capo(current_key, easy_shapes=EASY_SHAPES):
    """Devuelve [(traste, forma)] para tocar un tono dificil con acordes abiertos."""
    if current_key not in INDEX_SQL:
        return []
    return _rust.theory_suggested_capo(current_key, [_txt(s) for s in easy_shapes])


def _split_key(key):
    """(indice, es_menor) de un tono escrito como «Bb», «F#m», «Am», o None."""
    return _rust.theory_split_key(_txt(key or ""))


def related_keys(key) -> dict | None:
    """Los tonos vecinos de uno, para armar un set sin saltos bruscos.

    En el circulo de quintas los vecinos son la dominante, la subdominante y
    el relativo (y los relativos de aquellos): pasar entre ellos suena
    natural; saltar a un tono lejano corta. Devuelve tambien la cejilla con
    la que ese tono se toca con acordes abiertos.
    """
    return _rust.theory_related_keys(_txt(key or ""))
