"""Propiedades que tienen que cumplirse con CUALQUIER texto, no solo con los
ejemplos de siempre: nombres de archivo, teoria musical y TOON.

Hypothesis genera miles de casos raros (controles, tildes compuestas y
descompuestas, emojis, comillas) y reduce lo que falle al caso minimo. Asi
salieron los dos fallos de `names`: `sanitize` dejaba pasar saltos de linea,
y `strip_accents` convertia en ñ los caracteres NUL.
"""

import json
import unicodedata

from hypothesis import given, settings
from hypothesis import strategies as st

from danplay import names as N
from danplay import theory as M
from danplay import toon

# texto de verdad raro: cualquier cosa de Unicode salvo sustitutos sueltos
TEXT = st.text(st.characters(exclude_categories=("Cs",)), max_size=80)
FORBIDDEN = set('\\/:*?"<>|')


def _is_control(c: str) -> bool:
    return unicodedata.category(c) == "Cc" or N.CONTROL_CHARS.match(c) is not None


# ------------------------------------------------------------------ nombres


@given(TEXT)
def test_sanitize_always_gives_a_valid_name(s):
    out = N.sanitize(s)
    assert not (set(out) & FORBIDDEN)
    assert not any(_is_control(c) for c in out)
    assert out == out.rstrip(" ."), "Windows recorta puntos y espacios del final"
    assert "  " not in out
    stem = out.rsplit(".", 1)[0] if "." in out else out
    assert stem.upper() not in N.WINDOWS_RESERVED


@given(TEXT, TEXT, TEXT)
def test_the_final_name_is_a_valid_file_name(artist, title, feat):
    out = N.final_name(artist, title, feat)
    assert out.endswith(".mp3")
    assert not (set(out) & FORBIDDEN)
    assert not any(_is_control(c) for c in out)


@given(TEXT)
def test_strip_accents_only_removes_marks(s):
    out = N.strip_accents(s)
    assert N.strip_accents(out) == out, "idempotente"
    # quedan solo las virgulillas de la ñ
    for i, c in enumerate(unicodedata.normalize("NFD", out)):
        if unicodedata.category(c) == "Mn":
            assert c == "̃" and unicodedata.normalize("NFD", out)[i - 1] in "nN"
    # nunca aparece una ñ que no estuviera (antes salia de un NUL)
    had = unicodedata.normalize("NFD", s).count("̃")
    assert out.count("ñ") + out.count("Ñ") <= had


@given(st.text(st.characters(max_codepoint=127, exclude_categories=("Cs",)), max_size=60))
def test_strip_accents_leaves_ascii_alone(s):
    assert N.strip_accents(s) == s


@given(TEXT)
def test_clean_leaves_no_junk_at_the_edges_nor_controls(s):
    out = N.clean(s)
    assert out == out.strip(" .-–—,")
    assert not any(_is_control(c) for c in out)
    assert not (set(out) & FORBIDDEN)


@given(TEXT)
def test_the_match_key_is_stable(s):
    k = N.match_key(s + ".mp3")
    words = k.split()
    assert words == sorted(set(words)), "ordenada y sin repetir"
    assert all(len(w) > 2 for w in words)
    assert k == k.lower()


@given(TEXT)
def test_splitting_artist_and_title_never_crashes(s):
    r = N.split_artist_title(s + ".mp3")
    if r is not None:
        assert r["artist"] and r["title"]
        assert " - " not in r["artist"]


# ------------------------------------------------------------------ teoria

NOTES = st.sampled_from(sorted(M.INDEX_SQL))
SEMITONES = st.integers(min_value=-36, max_value=36)


@given(NOTES, SEMITONES)
def test_transposing_a_note_moves_exactly_those_semitones(note, n):
    out = M.transpose_note(note, n)
    assert (M.INDEX_SQL[out] - M.INDEX_SQL[note]) % 12 == n % 12


@given(NOTES, NOTES)
def test_the_round_trip_distance_is_an_octave(a, b):
    there, back = M.distance(a, b), M.distance(b, a)
    assert (there + back) % 12 == 0


@given(st.lists(NOTES, min_size=1, max_size=8), SEMITONES)
def test_transposing_and_undoing_gives_the_same_pitches(chords, n):
    text = " | ".join(chords)
    back = M.transpose(M.transpose(text, n), -n)
    got = [M.INDEX_SQL[c] for c in back.split(" | ")]
    assert got == [M.INDEX_SQL[c] for c in chords]


@given(TEXT, NOTES)
def test_transposing_to_the_same_key_changes_nothing(text, key):
    assert M.transpose_to(text, key, key) == text


@given(NOTES)
def test_the_capo_is_always_between_1_and_7(key):
    for fret, _shape in M.suggested_capo(key):
        assert 1 <= fret <= 7


# -------------------------------------------------------------------- TOON


@given(st.text(max_size=60))
def test_a_text_round_trips_through_toon(s):
    """Cuando va entre comillas, el escape es el de JSON: se puede leer igual."""
    out = toon._string(s, ",")
    if out.startswith('"'):
        assert json.loads(out) == s
    else:
        assert out == s and s == s.strip() and s not in ("true", "false", "null")


JSONISH = st.recursive(
    st.none() | st.booleans() | st.integers() | st.floats(allow_nan=False) | st.text(max_size=20),
    lambda children: (
        st.lists(children, max_size=4) | st.dictionaries(st.text(max_size=10), children, max_size=4)
    ),
    max_leaves=20,
)


@settings(max_examples=200)
@given(JSONISH)
def test_toon_encodes_anything(value):
    out = toon.encode(value)
    assert isinstance(out, str)


@given(
    st.lists(
        st.fixed_dictionaries({"id": st.integers(), "title": st.text(max_size=20)}),
        min_size=1,
        max_size=10,
    )
)
def test_a_list_of_uniform_rows_is_a_table(rows):
    out = toon.encode({"songs": rows})
    head, *lines = out.split("\n")
    # las columnas, en el orden de la primera fila
    assert head == f"songs[{len(rows)}]{{{','.join(rows[0])}}}:"
    assert len(lines) == len(rows), "una linea por fila, aunque un titulo lleve saltos"
