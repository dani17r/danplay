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
def test_sanitize_da_siempre_un_nombre_valido(s):
    out = N.sanitize(s)
    assert not (set(out) & FORBIDDEN)
    assert not any(_is_control(c) for c in out)
    assert out == out.rstrip(" ."), "Windows recorta puntos y espacios del final"
    assert "  " not in out
    stem = out.rsplit(".", 1)[0] if "." in out else out
    assert stem.upper() not in N.WINDOWS_RESERVED


@given(TEXT, TEXT, TEXT)
def test_el_nombre_final_es_un_archivo_valido(artist, title, feat):
    out = N.final_name(artist, title, feat)
    assert out.endswith(".mp3")
    assert not (set(out) & FORBIDDEN)
    assert not any(_is_control(c) for c in out)


@given(TEXT)
def test_strip_accents_solo_quita_marcas(s):
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
def test_strip_accents_no_toca_el_ascii(s):
    assert N.strip_accents(s) == s


@given(TEXT)
def test_clean_no_deja_basura_en_los_bordes_ni_controles(s):
    out = N.clean(s)
    assert out == out.strip(" .-–—,")
    assert not any(_is_control(c) for c in out)
    assert not (set(out) & FORBIDDEN)


@given(TEXT)
def test_la_clave_de_comparacion_es_estable(s):
    k = N.match_key(s + ".mp3")
    words = k.split()
    assert words == sorted(set(words)), "ordenada y sin repetir"
    assert all(len(w) > 2 for w in words)
    assert k == k.lower()


@given(TEXT)
def test_separar_artista_y_titulo_no_revienta(s):
    r = N.split_artist_title(s + ".mp3")
    if r is not None:
        assert r["artist"] and r["title"]
        assert " - " not in r["artist"]


# ------------------------------------------------------------------ teoria

NOTES = st.sampled_from(sorted(M.INDEX_SQL))
SEMITONES = st.integers(min_value=-36, max_value=36)


@given(NOTES, SEMITONES)
def test_transponer_una_nota_mueve_justo_esos_semitonos(note, n):
    out = M.transpose_note(note, n)
    assert (M.INDEX_SQL[out] - M.INDEX_SQL[note]) % 12 == n % 12


@given(NOTES, NOTES)
def test_la_distancia_de_ida_y_vuelta_da_una_octava(a, b):
    there, back = M.distance(a, b), M.distance(b, a)
    assert (there + back) % 12 == 0


@given(st.lists(NOTES, min_size=1, max_size=8), SEMITONES)
def test_transponer_y_deshacer_vuelve_al_mismo_sonido(chords, n):
    text = " | ".join(chords)
    back = M.transpose(M.transpose(text, n), -n)
    got = [M.INDEX_SQL[c] for c in back.split(" | ")]
    assert got == [M.INDEX_SQL[c] for c in chords]


@given(TEXT, NOTES)
def test_transponer_al_mismo_tono_no_cambia_nada(text, key):
    assert M.transpose_to(text, key, key) == text


@given(NOTES)
def test_la_cejilla_siempre_entre_1_y_7(key):
    for fret, _shape in M.suggested_capo(key):
        assert 1 <= fret <= 7


# -------------------------------------------------------------------- TOON


@given(st.text(max_size=60))
def test_un_texto_en_toon_vuelve_igual(s):
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
def test_toon_codifica_cualquier_cosa(value):
    out = toon.encode(value)
    assert isinstance(out, str)


@given(
    st.lists(
        st.fixed_dictionaries({"id": st.integers(), "title": st.text(max_size=20)}),
        min_size=1,
        max_size=10,
    )
)
def test_una_lista_de_iguales_es_una_tabla(rows):
    out = toon.encode({"songs": rows})
    head, *lines = out.split("\n")
    # las columnas, en el orden de la primera fila
    assert head == f"songs[{len(rows)}]{{{','.join(rows[0])}}}:"
    assert len(lines) == len(rows), "una linea por fila, aunque un titulo lleve saltos"
