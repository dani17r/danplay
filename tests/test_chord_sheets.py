"""Acordes de cifrados publicados: se leen tal cual y nunca se inventan.

Las paginas de aqui estan hechas a mano con la misma forma que las de verdad
(Ultimate Guitar guarda sus datos en JSON dentro de la pagina; LaCuerda marca
los acordes con <A>). La red esta cortada en las pruebas: nada sale fuera.
"""

import html
import json

import pytest

from danplay import chord_sheets, enrich


def _ug_page(data: dict) -> str:
    store = {"store": {"page": {"data": data}}}
    return f'<html><div class="js-store" data-content="{html.escape(json.dumps(store))}"></div>'


UG_TAB = """[Intro]
[ch]Bm[/ch] [ch]G[/ch] [ch]D[/ch] [ch]A[/ch]

[Verso 1]
[tab]  [ch]Bm[/ch]          [ch]G[/ch]
Se abren los cielos[/tab]
[tab]   [ch]D[/ch]      [ch]A[/ch]   [ch]A[/ch]
Un sonido celestial[/tab]

[Coro]
[ch]Bm[/ch]   [ch]G[/ch]
La tierra canta"""


def _result(artist, song, votes=0, key="", kind="Chords", access="public", n=1):
    return {
        "artist_name": artist,
        "song_name": song,
        "votes": votes,
        "rating": 4.5,
        "tonality_name": key,
        "type": kind,
        "tab_access_type": access,
        "tab_url": f"https://tabs.example/{n}",
    }


# ---------------------------------------------------------------- leer


def test_ultimate_guitar_parsed_by_sections_and_marked_chords():
    sheet = chord_sheets.parse_ug(UG_TAB)
    names = [s["name"] for s in sheet["sections"]]
    assert names == ["Intro", "Verso 1", "Coro"]
    verse = sheet["sections"][1]
    assert verse["chords"] == ["Bm", "G", "D", "A"], "sin repetir el mismo dos veces seguidas"
    assert [ln["c"] for ln in verse["lines"]] == [True, False, True, False]
    assert verse["lines"][1]["t"] == "Se abren los cielos", "sin las marcas [tab]"
    assert sheet["key"] == "" and sheet["capo"] == 0, "el texto no dice ni tono ni cejilla"


def test_sections_written_as_plain_text_also_count():
    sheet = chord_sheets.parse_ug(
        "[Verse]\n[ch]D[/ch]\nQuien podra\n\nPrecoro:\n[ch]A[/ch]\nNi lo alto\n"
        "CORO x2\n[ch]G[/ch]\nCoro de angeles cantan\nVerso 2\n[ch]Bm[/ch]"
    )
    assert [s["name"] for s in sheet["sections"]] == ["Verse", "Precoro", "Coro x2", "Verso 2"]
    assert sheet["sections"][2]["lines"][1]["t"] == "Coro de angeles cantan", "eso es letra"


def test_key_and_capo_stated_in_the_text_itself():
    sheet = chord_sheets.parse_ug("BPM: 57  4/4   TONO: A\nCapo 2\n\n[Coro]\n[ch]A[/ch]\nletra")
    assert sheet["key"] == "A" and sheet["capo"] == 2
    # «key» dentro de la letra, lejos de la cabecera, no es un tono
    far_key = "\n".join(["[ch]C[/ch]", *["la la"] * 20, "my key: E"])
    assert chord_sheets.parse_ug(far_key)["key"] == ""


def test_lacuerda_is_parsed_by_its_markup():
    page = (
        "<html><H1>La tierra canta <br><A href='./'>Barak</A></H1>"
        "<div id=t_body><PRE><div></div><A>Bm</A>        <A>G</A>\n"
        "Se abren los cielos\n\nCoro:\n<A>D</A>   <A>A</A>\nLa tierra canta, el cielo adora\n"
        "</PRE></div>"
    )
    sheet = chord_sheets.parse_lacuerda(page)
    assert [s["name"] for s in sheet["sections"]] == ["", "Coro"]
    assert sheet["sections"][0]["chords"] == ["Bm", "G"]
    assert sheet["sections"][1]["lines"][0] == {"t": "D   A", "c": True}
    assert chord_sheets.parse_lacuerda("<html>sin cifrado</html>")["sections"] == []


# ---------------------------------------------------------------- comparar


@pytest.mark.parametrize(
    ("lib", "sheet"),
    [
        (("Hillsong", "en Español - Hermoso Nombre"), ("Hillsong Worship", "Hermoso Nombre")),
        (
            ("Christine D'Clario", "Christine D'Clario - Admirable"),
            ("Christine D'Clario", "Admirable"),
        ),
        (
            ("SovereignGraceMusic", "El Dios que Adoramos (Video Oficial)"),
            ("Sovereign Grace", "El Dios Que Adoramos"),
        ),
        (("La Mejor Musica Cristiana", "Marcos Witt - Renuevame"), ("Marcos Witt", "Renuévame")),
        (
            ("Grupo Grace", "Espiritu Santo Fluye (Altar Live)"),
            ("Grupo Grace", "Espíritu Santo Fluye"),
        ),
    ],
)
def test_same_song_even_if_it_comes_from_youtube(lib, sheet):
    assert chord_sheets.same_song(*lib, *sheet)


@pytest.mark.parametrize(
    ("lib", "sheet"),
    [
        # otra cancion que empieza igual
        (("Art Aguilera", "Hay Libertad"), ("Art Aguilera", "Hay Libertad En La Casa De Dios")),
        # el mismo titulo, de otro artista: es otra cancion
        (("Art Aguilera", "Hay Libertad"), ("Miel San Marcos", "Hay Libertad")),
        # una version de otro: no se sabe de quien es la original
        (("Matty Martinez", "A Sus Pies (cover)"), ("Miel San Marcos", "A Sus Pies")),
        (("Barak", ""), ("Barak", "")),
    ],
)
def test_another_song_is_not_accepted(lib, sheet):
    assert not chord_sheets.same_song(*lib, *sheet)


def test_search_title_drops_noise_and_the_artist():
    assert chord_sheets.search_title("Hillsong", "en Español - Hermoso Nombre") == "Hermoso Nombre"
    assert chord_sheets.search_title("Barak", "Barak - Mi Gozo (En Vivo)") == "Mi Gozo"
    assert chord_sheets.search_title("X", "Cristo Vive - Live Version") == "Cristo Vive"


# ---------------------------------------------------------------- fuentes


def _fake_get(monkeypatch, pages: dict):
    asked = []

    def get(url):
        asked.append(url)
        for start, page in pages.items():
            if url.startswith(start):
                return page
        raise OSError(f"sin red en las pruebas: {url}")

    monkeypatch.setattr(chord_sheets, "_get", get)
    return asked


def test_ultimate_guitar_prefers_the_tab_that_states_its_key(monkeypatch):
    results = [
        _result("Art Aguilera", "Hay Libertad", votes=151, n=1),
        _result("Art Aguilera", "Hay Libertad", votes=5, key="Dm", n=2),
        _result("Art Aguilera", "Hay Libertad", votes=900, kind="Tabs", n=3),
        _result("Art Aguilera", "Hay Libertad", votes=900, key="E", access="pro", n=4),
        _result("Miel San Marcos", "Hay Libertad", votes=999, key="G", n=5),
    ]
    tab = {
        "tab": {"tonality_name": "Dm"},
        "tab_view": {"meta": {"capo": 0}, "wiki_tab": {"content": UG_TAB}},
    }
    asked = _fake_get(
        monkeypatch,
        {
            chord_sheets.UG_SEARCH: _ug_page({"results": results}),
            "https://tabs.example/2": _ug_page(tab),
        },
    )
    s = chord_sheets.ultimate_guitar("Art Aguilera", "Hay Libertad")
    assert s is not None
    assert s["url"] == "https://tabs.example/2" and s["key"] == "Dm" and s["key_from"] == "datos"
    assert s["source"] == "Ultimate Guitar" and s["votes"] == 5 and s["versions"] == 2
    assert asked[-1] == "https://tabs.example/2"


def test_without_a_key_in_the_data_the_text_key_counts(monkeypatch):
    tab = {"tab": {}, "tab_view": {"meta": [], "wiki_tab": {"content": "TONO: G\n" + UG_TAB}}}
    _fake_get(
        monkeypatch,
        {
            chord_sheets.UG_SEARCH: _ug_page({"results": [_result("Barak", "La Tierra Canta")]}),
            "https://tabs.example/1": _ug_page(tab),
        },
    )
    s = chord_sheets.ultimate_guitar("Barak", "La Tierra Canta")
    assert s is not None
    assert s["key"] == "G" and s["key_from"] == "texto"


def test_nothing_if_ultimate_guitar_does_not_have_it(monkeypatch):
    _fake_get(
        monkeypatch, {chord_sheets.UG_SEARCH: _ug_page({"results": [_result("Otro", "Otra")]})}
    )
    assert chord_sheets.ultimate_guitar("Barak", "La Tierra Canta") is None


def test_lacuerda_through_the_artist_page(monkeypatch):
    index = (
        "<ul id=b_main>"
        "<li id='r000' lcd='R-1'><a href=\"espiritu_santo_ven\">Espíritu Santo ven <em>acordes</em></A></li>"
        "<li id='r001' lcd='RR-21'><a href=\"la_tierra_canta\">La tierra canta <em>acordes</em></A></li>"
        "</ul>"
    )
    versions = (
        "<li id='liElm2' onclick='tOpen(2)'><div class='rtLabel'><a href='la_tierra_canta-2.shtml'>x</a>"
        "<li id='liElm1' onclick='tOpen(1)'><div class='rtLabel'><a href='la_tierra_canta.shtml'>x</a>"
    )
    page = "<H1>La tierra canta <br><A href='./'>Barak</A></H1><div id=t_body><PRE><A>Bm</A>\nletra</PRE>"
    base = chord_sheets.LC_BASE + "barak/"
    _fake_get(
        monkeypatch,
        {base + "la_tierra_canta-2.shtml": page, base + "la_tierra_canta": versions, base: index},
    )
    s = chord_sheets.lacuerda("Barak", "La Tierra Canta")
    assert s is not None
    assert s["url"] == base + "la_tierra_canta-2.shtml", "la version mejor valorada, la primera"
    assert (
        s["title"] == "La tierra canta" and s["key"] == "" and s["sections"][0]["chords"] == ["Bm"]
    )
    assert chord_sheets.lacuerda("Barak", "Otra Cancion") is None


def test_find_merges_sources_and_reports_the_failed_ones(monkeypatch):
    def offline_source(artist, title):
        raise OSError("sin red")

    with_key = {"source": "B", "key": "G"}
    monkeypatch.setattr(
        chord_sheets,
        "SOURCES",
        (
            ("A", lambda a, t: {"source": "A", "key": ""}),
            ("B", lambda a, t: with_key),
            ("C", offline_source),
        ),
    )
    r = chord_sheets.find("Barak", "Mi Gozo")
    assert r["sheet"] is with_key, "gana el que dice el tono"
    assert r["tried"] == ["A", "B", "C"] and r["failed"] == ["C"]
    assert chord_sheets.find("Barak", "")["sheet"] is None


def test_the_key_that_can_be_asserted():
    assert chord_sheets.sheet_key({"key": "G", "capo": 0}) == "G"
    assert chord_sheets.sheet_key({"key": "G", "capo": 2}) == "", "con cejilla, lo escrito no suena"
    assert chord_sheets.sheet_key(None) == ""


# ---------------------------------------------------------------- guardarlo


def _song(chords=""):
    return {"id": 3, "artist": "Barak", "title": "Mi Gozo", "chords": chords}


def test_what_is_found_is_saved_and_not_searched_again(monkeypatch):
    saved, asked = {}, []
    sheet = {"source": "LaCuerda", "key": "", "capo": 0, "sections": []}
    monkeypatch.setattr(enrich.library, "update", lambda cid, **f: saved.update(f))
    monkeypatch.setattr(
        enrich.chord_sheets,
        "find",
        lambda a, t: asked.append(t) or {"sheet": sheet, "tried": ["LaCuerda"], "failed": []},
    )
    r = enrich.chords(_song(json.dumps({"album": "Gozo"})))
    doc = json.loads(saved["chords"])
    assert r["sheet"] == sheet and doc["sheet"] == sheet and doc["album"] == "Gozo"
    assert doc["sheet_tried"] == ["LaCuerda"] and doc["sheet_checked"]
    again = enrich.chords(_song(saved["chords"]))
    assert again["cached"] and asked == ["Mi Gozo"]
    enrich.chords(_song(saved["chords"]), refresh=True)
    assert asked == ["Mi Gozo", "Mi Gozo"], "«buscar otra vez» si busca"


def test_not_found_is_saved_but_a_network_failure_is_not(monkeypatch):
    saved = {}
    monkeypatch.setattr(enrich.library, "update", lambda cid, **f: saved.update(f))
    answer = {"sheet": None, "tried": ["A", "B"], "failed": ["B"]}
    monkeypatch.setattr(enrich.chord_sheets, "find", lambda a, t: answer)
    enrich.chords(_song())
    assert not saved, "con una fuente caida, no se sabe si esta"
    answer["failed"] = []
    enrich.chords(_song())
    assert json.loads(saved["chords"])["sheet"] is None
    assert enrich.chords({"id": 4, "title": "", "chords": ""})["error"]


def test_filled_key_comes_from_the_chord_sheet_not_the_ai(monkeypatch):
    song = {
        "id": 5,
        "artist": "Barak",
        "title": "Mi Gozo",
        "album": "A",
        "year": "2018",
        "genre": "X",
        "key": "",
    }
    monkeypatch.setattr(enrich.library, "by_id", lambda cid: song)
    edited = {}
    monkeypatch.setattr(enrich.library, "edit", lambda cid, **f: edited.update(f))
    monkeypatch.setattr(enrich.ai, "available", lambda: False)
    monkeypatch.setattr(
        enrich, "details", lambda c: pytest.fail("para el tono no se pregunta a la IA")
    )
    sheet = {"source": "Ultimate Guitar", "key": "D", "capo": 0}
    monkeypatch.setattr(enrich, "chords", lambda c: {"sheet": sheet, "tried": [], "failed": []})
    r = enrich.autofill(5)
    assert r["ok"] and r["filled"] == {"key": "D"} and edited == {"key": "D"}

    edited.clear()
    sheet["capo"] = 3
    r = enrich.autofill(5)
    assert not edited and "cejilla" in r["reason"]
    sheet.update(capo=0, key="")
    assert "no dice el tono" in enrich.autofill(5)["reason"]


def test_search_404_means_nothing_and_title_alone_is_tried(monkeypatch):
    import urllib.error

    tab = {"tab": {}, "tab_view": {"wiki_tab": {"content": UG_TAB}}}
    asked = []

    def get(url):
        asked.append(url)
        if url == chord_sheets.UG_SEARCH + "Barak%20Danzar":
            raise urllib.error.HTTPError(url, 404, "no hay nada", None, None)  # type: ignore[arg-type]
        if url.startswith(chord_sheets.UG_SEARCH):
            return _ug_page({"results": [_result("Barak", "Danzar")]})
        return _ug_page(tab)

    monkeypatch.setattr(chord_sheets, "_get", get)
    s = chord_sheets.ultimate_guitar("Barak", "Danzar")
    assert s is not None
    assert s["title"] == "Danzar" and asked[1] == chord_sheets.UG_SEARCH + "Danzar"


def test_channels_search_with_the_artist_name():
    assert chord_sheets.search_title(
        "MarcosWittVEVO", "Marcos Witt - Levántate y sálvame (En vivo)"
    ) == ("Levántate y sálvame")
    assert chord_sheets.search_title("X", "Hay libertad ((Cover de otros))") == "Hay libertad"
