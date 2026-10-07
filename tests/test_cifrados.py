"""Acordes de cifrados publicados: se leen tal cual y nunca se inventan.

Las paginas de aqui estan hechas a mano con la misma forma que las de verdad
(Ultimate Guitar guarda sus datos en JSON dentro de la pagina; LaCuerda marca
los acordes con <A>). La red esta cortada en las pruebas: nada sale fuera.
"""

import html
import json

import pytest

from danplay import cifrados, enrich


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


def test_ultimate_guitar_se_lee_por_secciones_y_acordes_marcados():
    sheet = cifrados.parse_ug(UG_TAB)
    names = [s["name"] for s in sheet["sections"]]
    assert names == ["Intro", "Verso 1", "Coro"]
    verso = sheet["sections"][1]
    assert verso["chords"] == ["Bm", "G", "D", "A"], "sin repetir el mismo dos veces seguidas"
    assert [ln["c"] for ln in verso["lines"]] == [True, False, True, False]
    assert verso["lines"][1]["t"] == "Se abren los cielos", "sin las marcas [tab]"
    assert sheet["key"] == "" and sheet["capo"] == 0, "el texto no dice ni tono ni cejilla"


def test_las_partes_escritas_como_texto_tambien_cuentan():
    sheet = cifrados.parse_ug(
        "[Verse]\n[ch]D[/ch]\nQuien podra\n\nPrecoro:\n[ch]A[/ch]\nNi lo alto\n"
        "CORO x2\n[ch]G[/ch]\nCoro de angeles cantan\nVerso 2\n[ch]Bm[/ch]"
    )
    assert [s["name"] for s in sheet["sections"]] == ["Verse", "Precoro", "Coro x2", "Verso 2"]
    assert sheet["sections"][2]["lines"][1]["t"] == "Coro de angeles cantan", "eso es letra"


def test_el_tono_y_la_cejilla_que_dice_el_propio_texto():
    sheet = cifrados.parse_ug("BPM: 57  4/4   TONO: A\nCapo 2\n\n[Coro]\n[ch]A[/ch]\nletra")
    assert sheet["key"] == "A" and sheet["capo"] == 2
    # «key» dentro de la letra, lejos de la cabecera, no es un tono
    lejos = "\n".join(["[ch]C[/ch]", *["la la"] * 20, "my key: E"])
    assert cifrados.parse_ug(lejos)["key"] == ""


def test_lacuerda_se_lee_por_sus_marcas():
    page = (
        "<html><H1>La tierra canta <br><A href='./'>Barak</A></H1>"
        "<div id=t_body><PRE><div></div><A>Bm</A>        <A>G</A>\n"
        "Se abren los cielos\n\nCoro:\n<A>D</A>   <A>A</A>\nLa tierra canta, el cielo adora\n"
        "</PRE></div>"
    )
    sheet = cifrados.parse_lacuerda(page)
    assert [s["name"] for s in sheet["sections"]] == ["", "Coro"]
    assert sheet["sections"][0]["chords"] == ["Bm", "G"]
    assert sheet["sections"][1]["lines"][0] == {"t": "D   A", "c": True}
    assert cifrados.parse_lacuerda("<html>sin cifrado</html>")["sections"] == []


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
def test_la_misma_cancion_aunque_venga_de_youtube(lib, sheet):
    assert cifrados.same_song(*lib, *sheet)


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
def test_no_se_da_por_buena_otra_cancion(lib, sheet):
    assert not cifrados.same_song(*lib, *sheet)


def test_lo_que_se_busca_va_sin_ruido_ni_artista():
    assert cifrados.search_title("Hillsong", "en Español - Hermoso Nombre") == "Hermoso Nombre"
    assert cifrados.search_title("Barak", "Barak - Mi Gozo (En Vivo)") == "Mi Gozo"
    assert cifrados.search_title("X", "Cristo Vive - Live Version") == "Cristo Vive"


# ---------------------------------------------------------------- fuentes


def _fake_get(monkeypatch, pages: dict):
    asked = []

    def get(url):
        asked.append(url)
        for start, page in pages.items():
            if url.startswith(start):
                return page
        raise OSError(f"sin red en las pruebas: {url}")

    monkeypatch.setattr(cifrados, "_get", get)
    return asked


def test_de_ultimate_guitar_gana_el_que_dice_su_tono(monkeypatch):
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
            cifrados.UG_SEARCH: _ug_page({"results": results}),
            "https://tabs.example/2": _ug_page(tab),
        },
    )
    s = cifrados.ultimate_guitar("Art Aguilera", "Hay Libertad")
    assert s is not None
    assert s["url"] == "https://tabs.example/2" and s["key"] == "Dm" and s["key_from"] == "datos"
    assert s["source"] == "Ultimate Guitar" and s["votes"] == 5 and s["versions"] == 2
    assert asked[-1] == "https://tabs.example/2"


def test_sin_tono_en_los_datos_vale_el_del_texto(monkeypatch):
    tab = {"tab": {}, "tab_view": {"meta": [], "wiki_tab": {"content": "TONO: G\n" + UG_TAB}}}
    _fake_get(
        monkeypatch,
        {
            cifrados.UG_SEARCH: _ug_page({"results": [_result("Barak", "La Tierra Canta")]}),
            "https://tabs.example/1": _ug_page(tab),
        },
    )
    s = cifrados.ultimate_guitar("Barak", "La Tierra Canta")
    assert s is not None
    assert s["key"] == "G" and s["key_from"] == "texto"


def test_si_ultimate_guitar_no_la_tiene_no_hay_nada(monkeypatch):
    _fake_get(monkeypatch, {cifrados.UG_SEARCH: _ug_page({"results": [_result("Otro", "Otra")]})})
    assert cifrados.ultimate_guitar("Barak", "La Tierra Canta") is None


def test_lacuerda_por_la_pagina_del_artista(monkeypatch):
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
    base = cifrados.LC_BASE + "barak/"
    _fake_get(
        monkeypatch,
        {base + "la_tierra_canta-2.shtml": page, base + "la_tierra_canta": versions, base: index},
    )
    s = cifrados.lacuerda("Barak", "La Tierra Canta")
    assert s is not None
    assert s["url"] == base + "la_tierra_canta-2.shtml", "la version mejor valorada, la primera"
    assert (
        s["title"] == "La tierra canta" and s["key"] == "" and s["sections"][0]["chords"] == ["Bm"]
    )
    assert cifrados.lacuerda("Barak", "Otra Cancion") is None


def test_find_junta_las_fuentes_y_dice_las_que_fallaron(monkeypatch):
    def caida(artist, title):
        raise OSError("sin red")

    con_tono = {"source": "B", "key": "G"}
    monkeypatch.setattr(
        cifrados,
        "SOURCES",
        (
            ("A", lambda a, t: {"source": "A", "key": ""}),
            ("B", lambda a, t: con_tono),
            ("C", caida),
        ),
    )
    r = cifrados.find("Barak", "Mi Gozo")
    assert r["sheet"] is con_tono, "gana el que dice el tono"
    assert r["tried"] == ["A", "B", "C"] and r["failed"] == ["C"]
    assert cifrados.find("Barak", "")["sheet"] is None


def test_el_tono_que_se_puede_afirmar():
    assert cifrados.sheet_key({"key": "G", "capo": 0}) == "G"
    assert cifrados.sheet_key({"key": "G", "capo": 2}) == "", "con cejilla, lo escrito no suena"
    assert cifrados.sheet_key(None) == ""


# ---------------------------------------------------------------- guardarlo


def _song(chords=""):
    return {"id": 3, "artist": "Barak", "title": "Mi Gozo", "chords": chords}


def test_lo_encontrado_se_guarda_y_no_se_vuelve_a_buscar(monkeypatch):
    saved, asked = {}, []
    sheet = {"source": "LaCuerda", "key": "", "capo": 0, "sections": []}
    monkeypatch.setattr(enrich.library, "update", lambda cid, **f: saved.update(f))
    monkeypatch.setattr(
        enrich.cifrados,
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


def test_que_no_este_se_guarda_pero_que_falle_la_red_no(monkeypatch):
    saved = {}
    monkeypatch.setattr(enrich.library, "update", lambda cid, **f: saved.update(f))
    answer = {"sheet": None, "tried": ["A", "B"], "failed": ["B"]}
    monkeypatch.setattr(enrich.cifrados, "find", lambda a, t: answer)
    enrich.chords(_song())
    assert not saved, "con una fuente caida, no se sabe si esta"
    answer["failed"] = []
    enrich.chords(_song())
    assert json.loads(saved["chords"])["sheet"] is None
    assert enrich.chords({"id": 4, "title": "", "chords": ""})["error"]


def test_rellenar_el_tono_sale_del_cifrado_y_no_de_la_ia(monkeypatch):
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


def test_un_404_del_buscador_es_que_no_hay_nada_y_se_prueba_solo_el_titulo(monkeypatch):
    import urllib.error

    tab = {"tab": {}, "tab_view": {"wiki_tab": {"content": UG_TAB}}}
    asked = []

    def get(url):
        asked.append(url)
        if url == cifrados.UG_SEARCH + "Barak%20Danzar":
            raise urllib.error.HTTPError(url, 404, "no hay nada", None, None)  # type: ignore[arg-type]
        if url.startswith(cifrados.UG_SEARCH):
            return _ug_page({"results": [_result("Barak", "Danzar")]})
        return _ug_page(tab)

    monkeypatch.setattr(cifrados, "_get", get)
    s = cifrados.ultimate_guitar("Barak", "Danzar")
    assert s is not None
    assert s["title"] == "Danzar" and asked[1] == cifrados.UG_SEARCH + "Danzar"


def test_los_canales_buscan_con_el_nombre_del_artista():
    assert cifrados.search_title(
        "MarcosWittVEVO", "Marcos Witt - Levántate y sálvame (En vivo)"
    ) == ("Levántate y sálvame")
    assert cifrados.search_title("X", "Hay libertad ((Cover de otros))") == "Hay libertad"
