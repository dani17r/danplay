# -*- coding: utf-8 -*-
"""Teoria musical: transposicion de acordes y tonalidades.

Deterministico a proposito: transponer es matematica, no se le pregunta a una IA.
"""
import re

SHARPS = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"]
FLATS    = ["C","Db","D","Eb","E","F","Gb","G","Ab","A","Bb","B"]
INDEX_SQL = {}
for i, n in enumerate(SHARPS): INDEX_SQL[n] = i
for i, n in enumerate(FLATS):    INDEX_SQL[n] = i
INDEX_SQL.update({"E#":5, "B#":0, "Fb":4, "Cb":11})

# tonalidades que se escriben con bemoles
FLAT_KEYS = {"F","Bb","Eb","Ab","Db","Gb","Cb","Dm","Gm","Cm","Fm","Bbm","Ebm","Abm"}

LATIN = {"C":"Do","C#":"Do#","Db":"Reb","D":"Re","D#":"Re#","Eb":"Mib","E":"Mi",
          "F":"Fa","F#":"Fa#","Gb":"Solb","G":"Sol","G#":"Sol#","Ab":"Lab","A":"La",
          "A#":"La#","Bb":"Sib","B":"Si"}

CHORD = re.compile(r"\b([A-G][#b]?)((?:maj|min|m|dim|aug|sus|add|M)?\d*(?:[#b]\d+)*"
                    r"(?:sus\d?|add\d+|dim7?|aug|maj7?|m7b5)?)"
                    r"(/([A-G][#b]?))?", re.NOFLAG)


def _nombre(index, use_flats):
    return (FLATS if use_flats else SHARPS)[index % 12]


def transpose_note(note, semitones, use_flats=None):
    if note not in INDEX_SQL:
        return note
    target = (INDEX_SQL[note] + semitones) % 12
    if use_flats is None:
        use_flats = "b" in note
    return _nombre(target, use_flats)


def transpose(text, semitones, use_flats=None):
    """Transpone todos los acordes de un texto conservando el formato."""
    if not semitones:
        return text
    def _t(m):
        root, sufijo, _, bajo = m.group(1), m.group(2) or "", m.group(3), m.group(4)
        new_note = transpose_note(root, semitones, use_flats)
        out = new_note + sufijo
        if bajo:
            out += "/" + transpose_note(bajo, semitones, use_flats)
        return out
    return CHORD.sub(_t, text)


def distance(from_key, to_key):
    """Semitonos entre dos tonalidades. None si no se reconocen."""
    d = re.sub(r"m(in)?$", "", (from_key or "").strip())
    h = re.sub(r"m(in)?$", "", (to_key or "").strip())
    if d not in INDEX_SQL or h not in INDEX_SQL:
        return None
    return (INDEX_SQL[h] - INDEX_SQL[d]) % 12


def transpose_to(text, tono_actual, to_key):
    n = distance(tono_actual, to_key)
    if n is None:
        return text
    return transpose(text, n, use_flats=(to_key in FLAT_KEYS))


def to_latin(text):
    """Cifrado americano -> latino (Do Re Mi)."""
    return CHORD.sub(lambda m: LATIN.get(m.group(1), m.group(1)) + (m.group(2) or "")
                      + ("/" + LATIN.get(m.group(4), m.group(4)) if m.group(4) else ""), text)


def available_keys():
    return SHARPS + [b for b in FLATS if b not in SHARPS]


def suggested_capo(tono_actual, easy_shapes=("G","C","D","A","E","Em","Am","Dm")):
    """Devuelve [(traste, forma)] para tocar un tono dificil con acordes abiertos."""
    if tono_actual not in INDEX_SQL:
        return []
    outs = []
    for shape in easy_shapes:
        base = re.sub(r"m$", "", shape)
        if base not in INDEX_SQL:
            continue
        fret = (INDEX_SQL[tono_actual] - INDEX_SQL[base]) % 12
        if 1 <= fret <= 7:
            outs.append((fret, shape))
    return sorted(outs)


def _split_key(key):
    """(indice, es_menor) de un tono escrito como «Bb», «F#m», «Am», o None."""
    k = (key or "").strip()
    minor = bool(re.search(r"(m|min|-)$", k)) and not k.endswith("dim")
    root = re.sub(r"(m|min|-)$", "", k)
    if root not in INDEX_SQL:
        return None
    return INDEX_SQL[root], minor


def _key_name(index, minor, use_flats):
    return _nombre(index, use_flats) + ("m" if minor else "")


def related_keys(key) -> dict | None:
    """Los tonos vecinos de uno, para armar un set sin saltos bruscos.

    En el circulo de quintas los vecinos son la dominante, la subdominante y
    el relativo (y los relativos de aquellos): pasar entre ellos suena
    natural; saltar a un tono lejano corta. Devuelve tambien la cejilla con
    la que ese tono se toca con acordes abiertos.
    """
    parsed = _split_key(key)
    if not parsed:
        return None
    i, minor = parsed
    root = re.sub(r"(m|min|-)$", "", key.strip())
    # se escribe con bemoles si el tono lo lleva («Bb») o si es de los que
    # van con bemoles en la armadura («F», «Dm»); F#m no es F
    use_flats = "b" in root[1:] or (root + ("m" if minor else "")) in FLAT_KEYS
    if minor:
        relative = _key_name((i + 3) % 12, False, use_flats)
        neighbors = [_key_name((i + 7) % 12, True, use_flats), _key_name((i + 5) % 12, True, use_flats)]
        cousins = [_key_name((i + 10) % 12, False, use_flats), _key_name((i + 8) % 12, False, use_flats)]
    else:
        relative = _key_name((i + 9) % 12, True, use_flats)
        neighbors = [_key_name((i + 7) % 12, False, use_flats), _key_name((i + 5) % 12, False, use_flats)]
        cousins = [_key_name((i + 4) % 12, True, use_flats), _key_name((i + 2) % 12, True, use_flats)]
    name = _key_name(i, minor, use_flats)
    return {"key": name, "relative": relative, "neighbors": neighbors,
            "also_close": cousins, "latin": to_latin(name),
            "capo": [{"fret": f, "shape": sh} for f, sh in suggested_capo(_nombre(i, use_flats))],
            "note": "vecinos = misma armadura o una alteracion de diferencia: pasar de uno a "
                    "otro no suena a salto"}
