"""La forma de buscar un nombre (`names.search_form`): la regla del usuario.

«Al buscar, hay que convertir las canciones sin caracteres extraños ni
acentos, todo en minúsculas, y la ñ debe ir.» Se prueba con lo que de verdad
llega por el chat (listas de WhatsApp con asteriscos y emojis) y con
propiedades sobre texto cualquiera.

Los caracteres invisibles o compuestos van con `chr()` y no escritos tal cual:
en el editor no se distinguirían de los normales.
"""

import unicodedata

import pytest
from hypothesis import given
from hypothesis import strategies as st

from danplay import names as N

TILDE = chr(0x0303)  # la virgulilla combinada: n + TILDE es una ñ descompuesta
ZWSP = chr(0x200B)  # espacio de ancho cero
ZWJ = chr(0x200D)  # unión de emojis
NBSP = chr(0x00A0)
VS16 = chr(0xFE0F)  # selector de emoji
SKIN = chr(0x1F3FB)  # tono de piel
PRAY = chr(0x1F64F)  # manos juntas
DOT_I = chr(0x0130)  # «İ», la i mayúscula turca con punto

TEXT = st.text(st.characters(exclude_categories=("Cs",)), max_size=80)


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        # lo que pega el usuario desde WhatsApp
        (f"*3.-De Gloria en Gloria* ✅{PRAY}{SKIN}", "3 de gloria en gloria"),
        (f"*Domingo*❤{VS16}{PRAY}{SKIN}", "domingo"),
        ("~Rey Infinito~ _(en vivo)_", "rey infinito en vivo"),
        ("• *Hay Libertad*", "hay libertad"),
        # sin tildes ni dieresis, en minusculas
        ("Cuán Grande Es Dios", "cuan grande es dios"),
        ("CUÁN GRANDE ES DIOS", "cuan grande es dios"),
        ("Güero Pingüino", "guero pinguino"),
        ("Ruja O Leão", "ruja o leao"),
        ("Café Ç", "cafe c"),
        # y la ñ se queda: es otra letra, no una tilde
        ("Cena Del Señor", "cena del señor"),
        ("NIÑO", "niño"),
        ("Ñandú", "ñandu"),
        ("Mañana", "mañana"),
        # la ñ descompuesta (n + virgulilla, de un teclado o de macOS) vuelve compuesta
        (f"Sen{TILDE}or", "señor"),
        (f"SEN{TILDE}OR", "señor"),
        # apostrofos: sin hueco
        ("Christine D'Clario", "christine dclario"),
        ("Christine D’Clario", "christine dclario"),
        # simbolos, guiones y puntuacion: espacio
        ("Barak - Mi Gozo (Video Oficial) [HD]", "barak mi gozo video oficial hd"),
        ("Hay-libertad", "hay libertad"),
        ("Rock & Roll / Pop + Jazz", "rock roll pop jazz"),
        ("10.000 Razones", "10 000 razones"),
        (f"a\tb\nc{NBSP}d{ZWSP}e", "a b c d e"),
        # nada que buscar
        ("", ""),
        ("   ", ""),
        (f"✅{PRAY}{SKIN} *** ---", ""),
        (None, ""),
    ],
)
def test_la_forma_de_buscar(value, expected):
    assert N.search_form(value) == expected


def test_los_emojis_compuestos_no_dejan_restos():
    # selector de variacion, tono de piel y union de emojis (ZWJ)
    family = f"{chr(0x1F468)}{ZWJ}{chr(0x1F469)}{ZWJ}{chr(0x1F467)}"
    assert N.search_form(f"alabanza ❤{VS16} {family} {PRAY}{SKIN}") == "alabanza"


def test_las_tildes_de_una_i_turca_no_dejan_un_punto_suelto():
    assert N.search_form(f"{DOT_I}stanbul") == "istanbul"


def test_las_palabras_son_las_de_la_forma():
    assert N.search_tokens("*Cena Del Señor* ✅") == ["cena", "del", "señor"]
    assert N.search_tokens("") == []


def test_la_segunda_pasada_pliega_solo_la_enye():
    assert N.fold_enye(N.search_form("Cena Del Señor")) == "cena del senor"
    assert N.fold_enye("mañana ñandu") == "manana nandu"
    # la forma normal NO la pliega
    assert N.search_form("Mañana") != N.search_form("Manana")


# ----------------------------------------------------------- propiedades


@given(TEXT)
def test_es_idempotente(s):
    once = N.search_form(s)
    assert N.search_form(once) == once


@given(TEXT)
def test_solo_letras_digitos_y_espacios_simples_en_minuscula(s):
    out = N.search_form(s)
    assert out == out.strip()
    assert "  " not in out
    assert all(c.isalnum() or c == " " for c in out)
    assert out == out.lower()
    # ningun diacritico suelto, salvo la virgulilla de la ñ (que sale compuesta)
    decomposed = unicodedata.normalize("NFD", out).replace(f"n{TILDE}", "n")
    assert not any(unicodedata.category(c) == "Mn" for c in decomposed)


@given(st.text(alphabet="abcñÑáéíóúü ", max_size=40))
def test_la_enye_nunca_se_pierde(s):
    # cada ñ del texto (en cualquier mayuscula) sale como ñ
    assert N.search_form(s).count("ñ") == s.lower().count("ñ")
