# -*- coding: utf-8 -*-
"""Pruebas del nucleo: nombres, teoria musical, duplicados y etiquetas."""
import os, pathlib, shutil, sys, tempfile
import pytest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
from danplay import names as N, theory as M, duplicates as D, tags as E


# Biblioteca de referencia para las pruebas que necesitan audio de verdad.
# No se incrusta una ruta personal: se apunta a la tuya con la variable
# DANPLAY_TEST_MUSIC, y si no existe esas pruebas se saltan solas.
MUSIC = pathlib.Path(os.environ.get("DANPLAY_TEST_MUSIC")
                     or pathlib.Path.home() / "Musica")


def _muestra(relativa: str) -> str:
    return str(MUSIC / relativa)


# ------------------------------------------------------------------ nombres

@pytest.mark.parametrize("value,expected", [
    ("Espanol", "Espanol"),
    ("Español", "Español"),          # la ñ se conserva
    ("Adoración", "Adoracion"),
    ("Señor", "Señor"),
    ("Muñóz", "Muñoz"),
    ("José Luis", "Jose Luis"),
])
def test_strip_accents_keeps_enye(value, expected):
    assert N.strip_accents(value) == expected


@pytest.mark.parametrize("value,expected", [
    ("UPPERROOM", "Upperroom"),
    ("MIEL SAN MARCOS", "Miel San Marcos"),
    ("BJ Putnam", "Bj Putnam"),
    ("Christine D'Clario", "Christine D'Clario"),   # no toca lo mixto
    ("CeCe Winans", "CeCe Winans"),
])
def test_capitalize_words(value, expected):
    assert N.capitalize_words(value) == expected


@pytest.mark.parametrize("dirty,clean_name", [
    ("Barak  Mi Gozo VIDEO OFICIAL.mp3", "Barak Mi Gozo"),
    ("y2mate.com - Es Mi Rey.mp3", "Es Mi Rey"),
    ("C0080 ERES FIEL (Letras).mp3", "Eres Fiel"),
    ("Paramore_ Decode [OFFICIAL VIDEO].mp3", "Paramore Decode"),
])
def test_clean_strips_noise(dirty, clean_name):
    assert N.clean(os.path.splitext(dirty)[0]) == clean_name


def test_feat_does_not_eat_titles_starting_with_con():
    """Regresion: 'Con Poder' se convertia en un feat y vaciaba el titulo."""
    base, feat = N.extract_feat("Con Poder (Live)")
    assert base == "Con Poder (Live)" and feat == ""


@pytest.mark.parametrize("value,title,feat", [
    ("Tu Eres Rey (feat. Christine D'Clario)", "Tu Eres Rey", "Christine D'Clario"),
    ("Mi Pastor (con Julissa)", "Mi Pastor", "Julissa"),
    ("Vamos A Cantar", "Vamos A Cantar", ""),
    ("Contigo", "Contigo", ""),
])
def test_extract_feat(value, title, feat):
    b, f = N.extract_feat(value)
    assert (b, f) == (title, feat)


def test_final_name():
    assert N.final_name("Barak", "Mi Gozo") == "Barak - Mi Gozo.mp3"
    assert N.final_name("Barak", "Shekinah", "Miel San Marcos") == \
        "Barak - Shekinah (feat. Miel San Marcos).mp3"


def test_name_without_invalid_chars():
    n = N.final_name("AC/DC", 'Back: in "Black"?')
    for c in '\\/:*?"<>|':
        assert c not in n


def test_duplicate_suffix():
    with tempfile.TemporaryDirectory() as d:
        open(os.path.join(d, "A - B.mp3"), "w").close()
        assert N.free_name(d, "A - B.mp3") == "A - B - r.mp3"
        open(os.path.join(d, "A - B - r.mp3"), "w").close()
        assert N.free_name(d, "A - B.mp3") == "A - B - r2.mp3"


def test_match_key_groups_variants():
    a = N.match_key("Christine D'Clario - Rey.mp3")
    b = N.match_key("CHRISTINE DCLARIO REY (Letra) VIDEO OFICIAL.mp3")
    assert a == b


def test_artist_detected_from_vocabulary():
    vocab = {"barak": "Barak", "miel san marcos": "Miel San Marcos"}
    r = N.detect_artist("Barak - Mi Gozo.mp3", vocab)
    assert r["artist"] == "Barak" and r["confidence"] >= 0.8


def test_ambiguous_detection_lowers_confidence():
    """Con dos artistas conocidos en el nombre, debe ceder el turno a la IA."""
    vocab = {"barak": "Barak", "miel san marcos": "Miel San Marcos"}
    # sin marcador de feat: dos artistas conocidos sueltos en el nombre
    r = N.detect_artist("No hay lugar mas alto Miel San Marcos Barak.mp3", vocab)
    assert r["confidence"] < 0.8


# ------------------------------------------------------------------ musica

@pytest.mark.parametrize("from_key,to_key,value,expected", [
    ("Bb", "G",  "| Bb | Gm7 | Eb | F/A |", "| G | Em7 | C | D/F# |"),
    ("C",  "D",  "| C | Am | F | G |",       "| D | Bm | G | A |"),
    ("B",  "G",  "| B | G#m | E | F# |",     "| G | Em | C | D |"),
])
def test_transposition(from_key, to_key, value, expected):
    assert M.transpose_to(value, from_key, to_key) == expected


def test_transposition_full_cycle():
    p = "| Bb | Gm7 | Eb | F/A |"
    assert M.transpose(M.transpose(p, 5), 7) == p


def test_distance_between_keys():
    assert M.distance("C", "G") == 7
    assert M.distance("Bb", "G") == 9
    assert M.distance("no-existe", "G") is None


def test_latin_notation():
    assert M.to_latin("| Bb | Gm7 |") == "| Sib | Solm7 |"


def test_suggested_capo():
    capos = dict((f, t) for t, f in M.suggested_capo("Bb"))
    assert capos.get("G") == 3          # Sib con formas de Sol = capo 3


# ------------------------------------------------------------------ duplicados

def test_identical_finds_exact_copies():
    with tempfile.TemporaryDirectory() as d:
        a = os.path.join(d, "a.mp3"); b = os.path.join(d, "b.mp3")
        c = os.path.join(d, "c.mp3")
        with open(a, "wb") as f: f.write(b"X" * 5000)
        shutil.copy(a, b)
        with open(c, "wb") as f: f.write(b"Y" * 5000)
        groups = D.identical([a, b, c])
        assert len(groups) == 1 and set(groups[0]) == {a, b}


def test_rust_is_active():
    assert D.RUST, "el crate de Rust deberia estar compilado"


def test_rust_and_python_hashes_match():
    with tempfile.TemporaryDirectory() as d:
        f = os.path.join(d, "x.mp3")
        with open(f, "wb") as h: h.write(os.urandom(200000))
        assert D.hashes([f])[f] == D.partial_hash(f)


# ------------------------------------------------------------------ etiquetas

@pytest.fixture
def mp3(tmp_path):
    source_path = _muestra("Artistas/Barak/Barak - Mi Gozo.mp3")
    if not os.path.exists(source_path):
        pytest.skip("no hay biblioteca de prueba")
    target = tmp_path / "prueba.mp3"
    shutil.copy(source_path, target)
    return str(target)


def test_tags_round_trip(mp3):
    E.write(mp3, artist="Barak", title="Mi Gozo", album="Radical", year="2018")
    E.rate(mp3, 4)
    E.set_favorite(mp3, True)
    E.write_analysis(mp3, key="Bb", bpm=72)
    E.write_lyrics(mp3, "una letra\nde dos lineas")
    E.write_cover(mp3, b"\xff\xd8\xff\xe0" + b"\x00" * 400, "image/jpeg")
    E.set_labels(mp3, ["domingo", "rapida"])

    d = E.read_all(mp3)
    assert d["artist"] == "Barak" and d["title"] == "Mi Gozo"
    assert d["album"] == "Radical" and d["stars"] == 4
    assert d["favorite"] is True and d["key"] == "Bb" and d["bpm"] == 72
    assert "dos lineas" in d["lyrics"] and d["cover"] is True
    assert d["tags"] == ["domingo", "rapida"]


def test_zero_stars_read_as_zero(mp3):
    E.rate(mp3, 5); E.rate(mp3, 0)
    assert E.read_all(mp3)["stars"] == 0


def test_extract_cover(mp3):
    E.write_cover(mp3, b"\xff\xd8\xff\xe0" + b"\x00" * 400, "image/jpeg")
    data, mime = E.extract_cover(mp3)
    assert mime == "image/jpeg" and data.startswith(b"\xff\xd8")


@pytest.mark.parametrize("folder", [_muestra("Secuencias")])
def test_wav_duration_is_not_zero(folder):
    """mutagen.File() devuelve None para algunos .wav; debe usarse el lector concreto."""
    if not os.path.isdir(folder):
        pytest.skip("no hay carpeta de secuencias")
    wavs = [os.path.join(folder, f) for f in os.listdir(folder) if f.lower().endswith(".wav")]
    if not wavs:
        pytest.skip("no hay wav de prueba")
    for w in wavs:
        assert E.duration(w) > 1, f"duracion 0 en {os.path.basename(w)}"


def test_mp3_duration_still_right():
    m = _muestra("Artistas/Barak/Barak - Mi Gozo.mp3")
    if not os.path.exists(m):
        pytest.skip("sin biblioteca")
    assert E.duration(m) > 60


# ------------------------------------------------------------ resolver duplicados

@pytest.mark.parametrize("value,expected", [
    ("Barak - Mi Gozo - r.mp3", "Barak - Mi Gozo.mp3"),
    ("Barak - Mi Gozo - r2.mp3", "Barak - Mi Gozo.mp3"),
    ("Barak - Mi Gozo - R3.mp3", "Barak - Mi Gozo.mp3"),
    ("Barak - Mi Gozo.mp3", "Barak - Mi Gozo.mp3"),
    ("Rey (En Vivo) - r.mp3", "Rey (En Vivo).mp3"),
])
def test_strip_duplicate_suffix(value, expected):
    assert D.name_without_suffix(value) == expected


def test_resolve_deletes_others_and_renames():
    with tempfile.TemporaryDirectory() as d:
        base = os.path.join(d, "Barak - Mi Gozo.mp3")
        copy = os.path.join(d, "Barak - Mi Gozo - r.mp3")
        for f in (base, copy):
            with open(f, "wb") as h: h.write(b"x" * 100)
        inodo_copia = os.stat(copy).st_ino
        r = D.resolve(keep=copy, remove=[base])
        assert r["ok"]
        # queda un solo archivo, con el nombre limpio, y es el que conservamos
        assert os.listdir(d) == ["Barak - Mi Gozo.mp3"]
        assert os.path.basename(r["kept"]) == "Barak - Mi Gozo.mp3"
        assert r["renamed"] is True
        assert os.stat(r["kept"]).st_ino == inodo_copia


def test_resolve_keeps_clean_one_unrenamed():
    with tempfile.TemporaryDirectory() as d:
        base = os.path.join(d, "A - B.mp3")
        copy = os.path.join(d, "A - B - r.mp3")
        for f in (base, copy):
            with open(f, "wb") as h: h.write(b"x" * 50)
        r = D.resolve(keep=base, remove=[copy])
        assert r["ok"] and r["renamed"] is False
        assert os.path.isfile(base) and not os.path.exists(copy)


def test_resolve_deletes_nothing_on_dry_run():
    with tempfile.TemporaryDirectory() as d:
        a = os.path.join(d, "X.mp3"); b = os.path.join(d, "X - r.mp3")
        for f in (a, b):
            with open(f, "wb") as h: h.write(b"y" * 20)
        r = D.resolve(keep=b, remove=[a], dry_run=True)
        assert r["dry_run"] and os.path.isfile(a) and os.path.isfile(b)


def test_resolve_warns_if_kept_is_missing():
    r = D.resolve(keep="/no/existe.mp3", remove=[])
    assert not r["ok"]


# ----------------------------------------------------------------- descargas
# Todo sin red: se comprueban las decisiones del modulo, no que YouTube conteste.
from danplay import youtube as Y


@pytest.mark.parametrize("value,expected", [
    ("https://www.youtube.com/watch?v=abc", True),
    ("https://youtu.be/abc", True),
    ("https://music.youtube.com/watch?v=abc", True),
    ("https://ejemplo.com/cancion.mp3", False),
    ("barak sera llena la tierra", False),
    ("", False),
])
def test_recognizes_youtube_urls(value, expected):
    assert Y.is_url(value) is expected


def test_plain_text_becomes_a_search():
    assert Y.normalize("barak sera llena la tierra", 3) == "ytsearch3:barak sera llena la tierra"


def test_a_url_is_kept_as_is():
    u = "https://youtu.be/abc?si=xyz"
    assert Y.normalize(u) == u


def test_provisional_name_is_already_clean():
    """El ruido de YouTube se quita antes de tocar el disco."""
    n = Y._provisional_name({"title": "BARAK - Sera Llena La Tierra (VIDEO OFICIAL) HD",
                               "id": "xyz"})
    assert n == "Barak - Sera Llena La Tierra"


def test_falls_back_to_id_without_title():
    assert Y._provisional_name({"title": "", "id": "abc123"}) == "abc123"


def test_qualities_match_the_app():
    assert Y.QUALITY_KBPS["high"] == "320"
    assert Y.QUALITY_KBPS["medium"] == "192"


def test_never_tags_with_the_channel_name():
    """Sin datos de YouTube Music no se escribe nada: la cascada decide.

    Si se etiquetara con el canal, `_desde_tags` lo aceptaria con 0.95 de
    confianza y archivaria la cancion bajo un artista inventado.
    """
    assert Y._tag_if_trustworthy("/no/importa.mp3",
                                     {"artist": "", "track": "", "channel": "Fulanito Music"}) is False
    assert Y._tag_if_trustworthy("/no/importa.mp3",
                                     {"artist": "Barak", "track": ""}) is False
    # "- Topic" es el canal automatico de YouTube Music, no un artista
    assert Y._tag_if_trustworthy("/no/importa.mp3",
                                     {"artist": " - Topic", "track": "Algo"}) is False


def test_download_state_starts_idle():
    assert Y.STATE["active"] is False
    assert set(Y.STATE) >= {"active", "phase", "percent", "index", "total"}


# ------------------------------------------------------- busqueda de caratula
# Los titulos que vienen de descargas llevan ruido y con eso iTunes no encuentra
# nada. Se prueban variantes cada vez mas limpias.
from danplay import enrich as EN


def test_las_consultas_de_caratula_van_de_precisa_a_suelta():
    qs = EN._cover_queries("Adoracion La Ibi", "Santo Por Siempre (En Vivo) - r", "")
    assert qs[0] == "Adoracion La Ibi Santo Por Siempre (En Vivo)"
    assert "Adoracion La Ibi Santo Por Siempre" in qs   # sin el parentesis
    assert all(" - r" not in q for q in qs), "el sufijo de duplicado no debe ir"


def test_sin_artista_se_busca_solo_por_titulo():
    qs = EN._cover_queries("", "Eres Libre (Freedom) (cover Jesus Culture)", "")
    assert qs and all(not q.startswith(" ") for q in qs)
    assert "Eres Libre" in qs


def test_el_album_manda_sobre_el_titulo():
    qs = EN._cover_queries("Barak", "Mi Gozo", "Generacion Radical")
    assert qs[0] == "Barak Generacion Radical"


def test_un_titulo_vacio_no_genera_consultas_basura():
    assert EN._cover_queries("", "", "") == []


# ------------------------------------------------- rellenar la ficha con IA
# Un modelo que no sabe algo contesta «desconocido». Guardarlo es peor que
# dejarlo vacio: el campo parece relleno y nadie vuelve a mirarlo. Paso de
# verdad y escribio «desconocido» como album y año de una cancion.

@pytest.mark.parametrize("campo,valor", [
    ("album", "desconocido"), ("album", "N/A"), ("album", "-"),
    ("genre", "varios"), ("genre", "sin datos"), ("year", "desconocido"),
    ("album", ""), ("album", None), ("genre", "   "),
])
def test_lo_que_la_ia_no_sabe_no_se_guarda(campo, valor):
    assert EN._dato_util(campo, valor) == ""


@pytest.mark.parametrize("valor,esperado", [
    ("2019", "2019"), ("circa 2019", "2019"), ("1998", "1998"),
    ("2019-05-01", "2019"), ("siglo XXI", ""), ("19", ""),
])
def test_el_año_son_cuatro_cifras_o_nada(valor, esperado):
    assert EN._dato_util("year", valor) == esperado


@pytest.mark.parametrize("campo,valor", [
    ("album", "Generacion Radical"), ("genre", "Adoracion"), ("key", "Bb"),
])
def test_un_dato_de_verdad_si_se_guarda(campo, valor):
    assert EN._dato_util(campo, valor) == valor


# ---------------------------------------------------------------------------
# Deteccion de duplicados: el atajo tiene que dar EXACTAMENTE los mismos
# grupos que comparar todo contra todo. Es lo unico que autoriza el atajo.

def _grupos_a_lo_bruto(paths, umbral=0.88):
    """Todos contra todos, sin atajos. La referencia."""
    import os
    from collections import defaultdict
    from rapidfuzz import fuzz
    from danplay import names
    mk = {r: names.match_key(os.path.basename(r)) for r in paths}
    por_clave = defaultdict(list)
    for r, k in mk.items():
        if k:
            por_clave[k].append(r)
    grupos = [g for g in por_clave.values() if len(g) > 1]
    ya = {r for g in grupos for r in g}
    resto = [r for r in paths if r not in ya and mk[r]]
    usados = set()
    for i, a in enumerate(resto):
        if a in usados:
            continue
        g = [a]
        for b in resto[i + 1:]:
            if b in usados:
                continue
            if fuzz.token_set_ratio(mk[a], mk[b]) / 100.0 >= umbral:
                g.append(b)
        if len(g) > 1:
            grupos.append(g)
            usados.update(g)
    return grupos


def _normalizar(grupos):
    return sorted(tuple(sorted(g)) for g in grupos)


def test_the_shortcut_finds_exactly_the_same_duplicate_groups():
    import random
    from danplay import duplicates
    casos = {
        # plurales y erratas: NO comparten ninguna palabra, asi que caen por
        # la rama de comparacion simple. Son las que romperia un atajo ingenuo.
        "plurales": ["/x/gozo mio.mp3", "/x/gozos mios.mp3",
                     "/x/santo fuego.mp3", "/x/santos fuegos.mp3",
                     "/x/alabanza nueva.mp3", "/x/alabanzas nuevas.mp3"],
        # uno contenido en otro: la puntuacion de conjuntos da 100
        "subconjuntos": ["/x/barak gozo.mp3", "/x/barak gozo vivo estudio.mp3",
                         "/x/barak.mp3", "/x/gozo.mp3", "/x/barak gozo vivo.mp3"],
        # claves vacias o de una sola letra (match_key las descarta)
        "raros": ["/x/.mp3", "/x/a.mp3", "/x/el la de.mp3", "/x/AAA.mp3", "/x/aaa.mp3"],
    }
    random.seed(4)
    palabras = ["barak", "gozo", "vivo", "santo", "fuego", "gloria", "rey",
                "cristo", "amor", "cielo", "paz", "luz"]
    casos["mezcla"] = ["/x/%s - %s.mp3" % (random.choice(palabras).title(),
                                           " ".join(random.sample(palabras, 3)))
                       for _ in range(400)]
    for nombre, paths in casos.items():
        esperado = _normalizar(_grupos_a_lo_bruto(paths))
        obtenido = _normalizar(duplicates.same_song(paths))
        assert obtenido == esperado, f"difieren en «{nombre}»"


def test_identical_files_are_grouped_byte_by_byte(tmp_path):
    from danplay import duplicates
    a = tmp_path / "a.mp3"; a.write_bytes(b"x" * 5000)
    b = tmp_path / "b.mp3"; b.write_bytes(b"x" * 5000)
    c = tmp_path / "c.mp3"; c.write_bytes(b"y" * 5000)
    grupos = duplicates.identical([str(a), str(b), str(c)])
    assert _normalizar(grupos) == [(str(a), str(b))]
