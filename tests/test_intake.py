"""El lector de listas pegadas (`danplay.chat.intake`).

La parte gorda son las muestras de `tests/fixtures/intake_samples.json`: listas de
WhatsApp de verdad con nombres, siglas e ids de video inventados, variantes de
formato (numeraciones, vinetas, negritas, tachados, todo en una linea, `\\r\\n`,
tabuladores, invisibles), medleys con sus enlaces emparejados en orden, secciones
que no son canciones y mensajes que NO son listas. Cada muestra trae la lectura
esperada; se compara con las tolerancias del corpus (personas por prefijo, notas
como conjunto de palabras, extras por contenido) y exacto donde importa (`n`,
`query`, `url`, `kind`, `truncated`). El fixture lo genera, anonimizado, un
script fuera del repo a partir del corpus privado: no se edita a mano.

Despues: lo que la persona pide (`read_intent`), los enlaces (`clean_youtube_url`,
`youtube_video_id`, tambien los hostiles), la ultima lista de una conversacion
(`last_setlist`), entradas hostiles con tope de tiempo y propiedades.

El parametro de rastreo de YouTube se escribe por partes (`SI`): escrito entero, el
comprobador de privacidad del repo lo tomaria por uno de verdad. Los caracteres
invisibles van con `chr()`.
"""

import dataclasses
import json
import re
import time
from pathlib import Path

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from danplay import names
from danplay.chat import intake
from danplay.chat.intake import (
    MAX_NOTE,
    MAX_SETLIST_ITEMS,
    MAX_TITLE,
    MAX_URL,
    Intent,
    SetItem,
    SetlistParse,
    clean_youtube_url,
    last_setlist,
    parse_setlist,
    read_intent,
    youtube_video_id,
)

FIXTURE = Path(__file__).parent / "fixtures" / "intake_samples.json"
SAMPLES: list[dict] = json.loads(FIXTURE.read_text(encoding="utf-8"))

WATCH = "https://www.youtube.com/watch?v="
PLAYLIST = "https://www.youtube.com/playlist?list="
SI = "s" + "i"  # el parametro de rastreo, por partes
ID = "Zx9_Qw-Er7t"  # ids inventados (11 caracteres, con _ y -)
ID2 = "-Kp3Lm8Nq2_"
PL = "PLq7Wd_Xr2-Lm9Tk4Ys6Vb1Nc3Hj8Gf5Ae"
ZWSP, NBSP, RLO = chr(0x200B), chr(0xA0), chr(0x202E)
KEYCAP_1 = "1" + chr(0xFE0F) + chr(0x20E3)
NOTES = chr(0x1F3B6)


# --------------------------------------------------------------- comparador


def rnorm(s: str) -> str:
    return names.search_form(s)


def _same_person(a: str, b: str) -> bool:
    """«Mónica Núñez» vale por «Mónica Núñez Esposa Guitarrista Pablo» (por prefijo)."""
    x, y = rnorm(a).split(), rnorm(b).split()
    n = min(len(x), len(y))
    return n > 0 and x[:n] == y[:n]


def _same_people(got: list[str], exp: list[str]) -> bool:
    return len(got) == len(exp) and all(_same_person(g, e) for g, e in zip(got, exp, strict=True))


def diff(got: SetlistParse, exp: dict) -> list[str]:
    """Lo que no cuadra entre una lectura y la esperada (vacio = vale)."""
    if got.is_setlist != exp["is_setlist"]:
        return [f"is_setlist={got.is_setlist} (esperado {exp['is_setlist']})"]
    out: list[str] = []
    if not exp["is_setlist"]:
        return out
    if rnorm(got.name_hint) != rnorm(exp["name_hint"]):
        out.append(f"name_hint={got.name_hint!r} (esperado {exp['name_hint']!r})")
    if got.truncated != exp["truncated"]:
        out.append(f"truncated={got.truncated} (esperado {exp['truncated']})")
    if len(got.items) != len(exp["items"]):
        out.append(
            f"{len(got.items)} items (esperados {len(exp['items'])}): {[i.title for i in got.items]}"
        )
    for g, e in zip(got.items, exp["items"], strict=False):
        w = f"item {e['n']} «{e['title'] or e['url']}»"
        bad = [f for f in ("n", "query", "url", "kind") if getattr(g, f) != e[f]]
        bad += [f for f in ("title", "artist") if rnorm(getattr(g, f)) != rnorm(e[f])]
        out += [f"{w}: {f}={getattr(g, f)!r} (esperado {e[f]!r})" for f in bad]
        if set(rnorm(g.note).split()) != set(rnorm(e["note"]).split()):
            out.append(f"{w}: note={g.note!r} (esperado {e['note']!r})")
        if not _same_people(g.people, e["people"]):
            out.append(f"{w}: people={g.people!r} (esperado {e['people']!r})")
    if not _same_people(got.people, exp["people"]):
        out.append(f"people={got.people!r} (esperado {exp['people']!r})")
    # extras: cada esperado esta dentro de lo devuelto, y nada devuelto es una cancion esperada
    blob = " | ".join(rnorm(x) for x in got.extras)
    out += [f"falta el extra «{e}»" for e in exp["extras"] if rnorm(e) not in blob]
    wanted = [rnorm(e) for e in exp["extras"]]
    for x in got.extras:
        nx = rnorm(x)
        if any(nx in e or e in nx for e in wanted):
            continue
        out += [
            f"el extra «{x}» es la cancion «{it['title']}»"
            for it in exp["items"]
            if len(it["query"]) >= 4 and it["query"] in nx
        ][:1]
    return out


def check_contract(p: SetlistParse) -> None:
    """Lo que `parse_setlist` promete de cualquier lectura (diseño §2)."""
    assert isinstance(p, SetlistParse)
    if not p.is_setlist:
        assert p == SetlistParse(), "sin lista no se devuelve nada a medias"
        return
    assert p.items, "una lista tiene al menos una cancion"
    assert len(p.items) >= 2 or p.items[0].url, "una lista de una sola necesita su enlace"
    assert len(p.items) <= MAX_SETLIST_ITEMS
    assert p.truncated >= 0
    if p.truncated:
        assert len(p.items) == MAX_SETLIST_ITEMS
    assert len(p.extras) <= intake._MAX_EXTRAS
    for k, it in enumerate(p.items, 1):
        assert isinstance(it, SetItem)
        assert it.n == k, "n es la posicion de lectura"
        assert it.title or it.url
        assert len(it.title) <= MAX_TITLE and len(it.url) <= MAX_URL and len(it.note) <= MAX_NOTE
        assert it.query == names.search_form(it.title), "query es R-NORM del titulo"
        assert not set(it.title) & set("\n\r\t*~`"), it.title
        assert it.kind in ("", "video", "playlist")
        if it.kind == "video":
            assert it.url.startswith(WATCH) and len(it.url) == len(WATCH) + 11
        elif it.kind == "playlist":
            assert it.url.startswith(PLAYLIST)
        else:
            assert it.url == ""
        for text in (it.artist, it.note, *it.people):
            assert "\n" not in text and "\r" not in text
        assert all(p and not p.startswith("@") for p in it.people)
    for text in (p.name_hint, *p.extras, *p.people):
        assert "\n" not in text and "\r" not in text
    assert all(len(x) <= MAX_NOTE for x in p.extras)


def without_raw(p: SetlistParse) -> SetlistParse:
    return dataclasses.replace(p, items=[dataclasses.replace(i, raw="") for i in p.items])


# ------------------------------------------------------------------ contrato


def test_el_contrato_de_los_datos():
    assert (MAX_SETLIST_ITEMS, MAX_TITLE, MAX_URL, MAX_NOTE) == (60, 200, 300, 200)
    assert [f.name for f in dataclasses.fields(SetItem)] == [
        "n", "title", "query", "artist", "url", "kind", "people", "note", "raw",
    ]  # fmt: skip
    assert [f.name for f in dataclasses.fields(SetlistParse)] == [
        "is_setlist", "name_hint", "items", "people", "extras", "truncated",
    ]  # fmt: skip
    assert [f.name for f in dataclasses.fields(Intent)] == [
        "check", "no_download", "replace", "create", "strict_version",
    ]  # fmt: skip
    assert SetlistParse() == SetlistParse(False, "", [], [], [], 0)
    assert Intent() == Intent(False, False, False, False, False)


def test_el_fixture_esta_completo_y_limpio():
    raw = FIXTURE.read_text(encoding="utf-8")
    ids = [s["id"] for s in SAMPLES]
    assert len(ids) == len(set(ids)) >= 90
    assert sum(not s["expect"]["is_setlist"] for s in SAMPLES) >= 20
    assert not re.search(rf"[?&]{SI}=", raw), "ningun parametro de rastreo en el repo"
    for s in SAMPLES:
        assert sorted(s) == ["expect", "id", "que", "text"]
        for it in s["expect"]["items"]:
            assert it["query"] == names.search_form(it["title"])


# ------------------------------------------------------------------ muestras


@pytest.mark.parametrize("sample", SAMPLES, ids=[s["id"] for s in SAMPLES])
def test_muestra(sample):
    got = parse_setlist(sample["text"])
    assert diff(got, sample["expect"]) == [], sample["que"]
    check_contract(got)


def _with_tracking(text: str) -> str:
    """El texto con un `si=` (y un `pp=`) en cada enlace de YouTube, como los comparte el movil."""
    text = re.sub(
        r"(youtu\.be/[A-Za-z0-9_-]{11})(?![A-Za-z0-9_?-])", rf"\1?{SI}=Ab3dE_fG9-hIjK2l", text
    )
    return re.sub(
        r"(watch\?v=[A-Za-z0-9_-]{11})(?![A-Za-z0-9_-])", rf"\1&{SI}=Qw8_rT&pp=ygUE", text
    )


TRACKED = [s for s in SAMPLES if _with_tracking(s["text"]) != s["text"]]


@pytest.mark.parametrize("sample", TRACKED, ids=[s["id"] for s in TRACKED])
def test_el_rastreo_de_los_enlaces_no_cambia_la_lectura(sample):
    plain = parse_setlist(sample["text"])
    tracked = parse_setlist(_with_tracking(sample["text"]))
    assert without_raw(tracked) == without_raw(plain)
    assert all(SI + "=" not in it.url for it in tracked.items)


def test_la_misma_lista_con_cualquier_espaciado():
    lines = [
        "*Domingo*",
        "1.- Digno De Adorar",
        f"2.- Te Doy Gloria https://youtu.be/{ID}",
        "3.- Rey Infinito",
    ]
    base = parse_setlist("\n".join(lines))
    assert base.is_setlist and base.name_hint == "Domingo"
    assert [i.title for i in base.items] == ["Digno De Adorar", "Te Doy Gloria", "Rey Infinito"]
    variants = {
        "crlf": "\r\n".join(lines) + "\r\n",
        "cr": "\r".join(lines),
        "tabuladores": "\n".join("\t" + x.replace(" ", "\t", 1) + "\t" for x in lines),
        "espacios_duros": "\n".join(x.replace(" ", NBSP) for x in lines),
        "invisibles": "\n".join(ZWSP + x + RLO for x in lines),
        "una_linea": "  ".join(lines),
        "vacias": "\n\n\n".join(lines),
    }
    for name, text in variants.items():
        got = parse_setlist(text)
        assert got.name_hint == "Domingo", name
        assert [(i.title, i.url) for i in got.items] == [(i.title, i.url) for i in base.items], name


def test_mas_de_sesenta_se_recortan_y_se_dice_cuantas_sobran():
    text = "\n".join(f"{k}. Cancion Numero {k}" for k in range(1, 100))
    got = parse_setlist(text)
    assert got.is_setlist and got.truncated == 99 - MAX_SETLIST_ITEMS
    assert [i.n for i in got.items] == list(range(1, MAX_SETLIST_ITEMS + 1))
    assert got.items[-1].title == f"Cancion Numero {MAX_SETLIST_ITEMS}"


REGRESSIONS = [
    # (texto, titulos esperados o None si no es lista); `_parse` sin el try de parse_setlist
    (":\n1.- Digno De Adorar\n2.- Te Doy Gloria", ["Digno De Adorar", "Te Doy Gloria"]),
    ("1.- Digno De Adorar (²)\n2.- Te Doy Gloria", ["Digno De Adorar", "Te Doy Gloria"]),
    ("(²)", None),
    ("1.- https://evil.example/a\n2.- https://open.spotify.com/track/b", None),
    (f"1.- https://youtu.be/{ID}1.- https://youtu.be/{ID}", None),
    ("si ya la tenemos no la bajes\nhttps://youtu.be/" + ID, None),
]


@pytest.mark.parametrize(("text", "titles"), REGRESSIONS, ids=[t[:30] for t, _ in REGRESSIONS])
def test_rarezas_que_tumbaban_o_colaban_una_lista(text, titles):
    got = intake._parse(text)
    check_contract(got)
    assert (None if not got.is_setlist else [i.title for i in got.items]) == titles


def test_lo_que_no_es_texto_no_es_lista():
    for value in (None, 12, b"1. A\n2. B", ["1. A", "2. B"], "", "   \n\t "):
        assert parse_setlist(value) == SetlistParse()  # pyright: ignore[reportArgumentType]


# -------------------------------------------------------------- read_intent

INTENT_FIELDS = ("check", "no_download", "replace", "create", "strict_version")
INTENTS = [
    # check: verifica / comprueba / revisa / «están bien», con y sin tildes
    ("verifica estas canciones", {"check"}),
    ("verifícalas", {"check"}),
    ("comprueba si están bien", {"check"}),
    ("compruébalas por favor", {"check"}),
    ("revisa la lista del domingo", {"check"}),
    ("revísala", {"check"}),
    ("¿están bien?", {"check"}),
    ("estan bien?", {"check"}),
    ("¿está bien la lista?", {"check"}),
    ("esta bien la lista", {"check"}),
    ("no la verifiques", set()),
    ("verifica y agrégalas", set()),  # gana agregar: no lo quiere repetir
    # no_download: no bajes / sin descargar / solo agrégalas / lo que tengamos
    ("no bajes nada", {"no_download"}),
    ("no las descargues", {"no_download"}),
    ("sin descargar", {"no_download"}),
    ("Sin descargar nada", {"no_download"}),
    ("solo agrégalas", {"no_download"}),
    ("sólo agregalas", {"no_download"}),
    ("solo lo que tengamos", {"no_download"}),
    ("con lo que ya tenemos", {"no_download"}),
    ("no revises nada, solo agrégalas", {"no_download"}),
    ("la que no se consiga, descárgala", set()),
    ("si ya la tenemos no la bajes", set()),  # condicional: lo que falta si se baja
    ("descarga las que falten", set()),
    ("bájalas todas", set()),
    # replace: déjala exactamente / solo estas / reemplaza / ordénala / corrígela
    ("déjala exactamente así", {"replace"}),
    ("dejala exactamente asi", {"replace"}),
    ("déjala tal cual", {"replace"}),
    ("solo estas", {"replace"}),
    ("sólo estas", {"replace"}),
    ("reemplaza la lista", {"replace"}),
    ("reemplázala", {"replace"}),
    ("ordénala como te la pasé", {"replace"}),
    ("ordenala", {"replace"}),
    ("reordena la lista", {"replace"}),
    ("corrígela", {"replace"}),
    ("corrigela", {"replace"}),
    ("esta es la lista", {"replace"}),
    ("así debe quedar", {"replace"}),
    ("asi debe quedar", {"replace"}),
    ("agrégalas a la lista", set()),
    ("no la reemplaces", set()),
    ("ordena por duración", set()),
    # create: crea / créame / ármame una lista, nueva lista, ponle de nombre
    ("crea una lista", {"create"}),
    ("créame una lista domingo", {"create"}),
    ("creame una lista", {"create"}),
    ("ármame una lista con esto", {"create"}),
    ("armame una lista", {"create"}),
    ("hazme una lista", {"create"}),
    ("haz una nueva lista", {"create"}),
    ("nueva lista: ensayo", {"create"}),
    ("ponle de nombre ensayo", {"create"}),
    ("agrégala a la lista domingo", set()),
    ("no crees otra lista", set()),
    # strict_version: «la versión de…» / «la de este enlace» / «esa versión»
    ("la versión de Marco Barrientos", {"strict_version"}),
    ("la version de marco barrientos", {"strict_version"}),
    ("la de este enlace", {"strict_version"}),
    ("esa versión", {"strict_version"}),
    ("la del video", {"strict_version"}),
    ("otra versión", set()),
    ("la lista del domingo", set()),
    # nada
    ("hola", set()),
    ("pon la segunda", set()),
    ("gracias, me avisas", set()),
    ("¿cuántas canciones tengo?", set()),
    # una lista pegada: cuentan las palabras de la persona, no los titulos
    ("1.- Solo Estas\n2.- Rey Infinito", set()),
    ("1.- Revisa Mi Corazón\n2.- Aleluya", set()),
    (
        "agregame esto a la lista domingo:\n1.- Solo Estas\n2.- Reemplaza Mi Vida\n3.- Crea En Mí",
        set(),
    ),
    ("déjala exactamente así:\n1.- Solo Estas\n2.- Rey Infinito", {"replace"}),
    ("verifica estas canciones:\n1.- Te Doy Gloria\n2.- Aleluya", {"check"}),
    (
        (
            "agregame a la lista ensayo domingo la version de hay libertad, la de este enlace "
            f"https://youtu.be/{ID}, si ya la tenemos no la bajes"
        ),
        {"strict_version"},
    ),
]


@pytest.mark.parametrize(("text", "on"), INTENTS, ids=[t[:40] for t, _ in INTENTS])
def test_read_intent(text, on):
    got = read_intent(text)
    assert {f for f in INTENT_FIELDS if getattr(got, f)} == on


def test_read_intent_en_mayusculas_y_sin_texto():
    assert read_intent("VERIFICA LA LISTA").check
    assert read_intent("NO BAJES NADA").no_download
    for value in (None, 5, ""):
        assert read_intent(value) == Intent()  # pyright: ignore[reportArgumentType]


# ------------------------------------------------------------------ enlaces

URLS = [
    (f"https://youtu.be/{ID}?{SI}=x9_Kd&list=RD{ID}", WATCH + ID),
    (f"https://youtu.be/{ID}?{SI}=x9_Kd&t=42", WATCH + ID),
    (f"https://www.youtube.com/watch?v={ID}&list={PL}&index=3", WATCH + ID),
    (f"https://www.youtube.com/watch?list={PL}&v={ID}", WATCH + ID),
    (
        f"https://www.youtube.com/watch?v={ID}&{SI}=a&pp=ygUE&start_radio=1&feature=share",
        WATCH + ID,
    ),
    (f"https://www.youtube.com/playlist?list={PL}", PLAYLIST + PL),
    (f"https://www.youtube.com/watch?list={PL}", PLAYLIST + PL),
    (f"https://music.youtube.com/watch?v={ID}&feature=share", WATCH + ID),
    (f"https://m.youtube.com/watch?v={ID}&t=42s", WATCH + ID),
    (f"youtu.be/{ID}", WATCH + ID),
    (f"www.youtube.com/watch?v={ID}", WATCH + ID),
    (f"//youtu.be/{ID2}", WATCH + ID2),
    (f"http://youtube.com/watch?v={ID2}", WATCH + ID2),
    (f"https://YOUTU.BE/{ID}", WATCH + ID),
    (f"https://www.youtube.com/shorts/{ID}", WATCH + ID),
    (f"https://www.youtube.com/live/{ID}?{SI}=a", WATCH + ID),
    (f"https://www.youtube.com/embed/{ID}", WATCH + ID),
    (f"https://www.youtube.com/watch?v={ID}#t=3", WATCH + ID),
    (f"  https://youtu.be/{ID}  ", WATCH + ID),
]
HOSTILE_URLS = [
    f"https://youtube.com.evil.example/watch?v={ID}",
    f"https://youtube.com@evil.example/watch?v={ID}",
    f"https://evil.example@youtube.com/watch?v={ID}",
    f"https://user:pass@www.youtube.com/watch?v={ID}",
    f"http://127.0.0.1/watch?v={ID}",
    f"https://[::1]/watch?v={ID}",
    f"javascript:alert(1)//youtube.com/watch?v={ID}",
    f"javascript://youtube.com/watch?v={ID}",
    f"file:///etc/passwd?v={ID}",
    f"file://youtube.com/watch?v={ID}",
    f"ftp://youtube.com/watch?v={ID}",
    "ytsearch50:foo",
    f"ytsearch50:https://youtu.be/{ID}",
    f"https://evil.example/watch?v={ID}",
    f"evil.example/watch?v={ID}",
    f"https://notyoutube.com/watch?v={ID}",
    f"https://youtube.com\\@evil.example/watch?v={ID}",
    f"https://evil.example\\@youtube.com/watch?v={ID}",
    f"https://www.youtube.com:8080/watch?v={ID}",
    f"https://www.youtube.com./watch?v={ID}",
    f"https://www.you tube.com/watch?v={ID}",
    f"https://www.youtube.com/watch?v={ID}\nhttps://evil.example",
    f"https://www.youtube.com\t/watch?v={ID}",
    "https://www.youtube.com/watch?v=corto",
    f"https://www.youtube.com/watch?v={ID}x",
    "https://www.youtube.com/@CanalDeEjemplo",
    "https://www.youtube.com/playlist?list=",
    "https://www.youtube.com/playlist?list=a/b",
    "https://youtu.be/",
    "https://" + "a" * 3000,
    "",
]


@pytest.mark.parametrize(("url", "want"), URLS, ids=[u.strip()[:48] for u, _ in URLS])
def test_clean_youtube_url(url, want):
    assert clean_youtube_url(url) == want
    assert clean_youtube_url(want) == want, "idempotente"
    vid = want.removeprefix(WATCH) if want.startswith(WATCH) else ""
    assert youtube_video_id(url) == vid


@pytest.mark.parametrize("url", HOSTILE_URLS, ids=[u[:48] for u in HOSTILE_URLS])
def test_lo_que_no_es_de_youtube_da_vacio(url):
    assert clean_youtube_url(url) == ""
    assert youtube_video_id(url) == ""


def test_lo_que_no_es_texto_da_vacio():
    for value in (None, 12, [f"https://youtu.be/{ID}"], b"https://youtu.be/x"):
        assert clean_youtube_url(value) == ""
        assert youtube_video_id(value) == ""


def test_una_lista_pegada_conserva_la_lista_de_youtube_y_quita_el_mix():
    text = (
        "Setlist\n"
        f"1.- Digno De Adorar https://www.youtube.com/playlist?list={PL}\n"
        f"2.- Te Doy Gloria https://youtu.be/{ID}?{SI}=x&list=RD{ID}&start_radio=1\n"
        "3.- Rey Infinito https://www.youtube.com/@CanalDeEjemplo"
    )
    got = parse_setlist(text)
    assert [(i.url, i.kind) for i in got.items] == [
        (PLAYLIST + PL, "playlist"),
        (WATCH + ID, "video"),
        ("", ""),
    ]


def test_un_enlace_hostil_en_la_lista_no_es_cancion():
    text = (
        f"1.- Digno De Adorar https://youtube.com.evil.example/watch?v={ID}\n"
        f"2.- Te Doy Gloria https://youtube.com@evil.example/watch?v={ID2}\n"
        f"3.- Rey Infinito http://127.0.0.1/watch?v={ID}\n"
        "4.- ytsearch50:aleluya"
    )
    got = parse_setlist(text)
    assert [i.title for i in got.items] == [
        "Digno De Adorar",
        "Te Doy Gloria",
        "Rey Infinito",
        "ytsearch50:aleluya",
    ]
    assert all(i.url == "" and i.kind == "" for i in got.items)
    # sin barra no es un enlace: es texto, y en la consulta los dos puntos ya no estan
    assert got.items[3].query == "ytsearch50 aleluya"


# ------------------------------------------------------------- last_setlist

LISTA_VIEJA = "1.- Digno De Adorar\n2.- Te Doy Gloria"
LISTA_NUEVA = "*Domingo*\n1.- Hay Libertad\n2.- Aleluya"


def _titles(p: SetlistParse | None) -> list[str] | None:
    return None if p is None else [i.title for i in p.items]


def test_last_setlist_toma_la_ultima_lista_visible_de_la_persona():
    messages = [
        {"role": "me", "text": LISTA_VIEJA},
        {"role": "ai", "text": "Hecho."},
        {"role": "me", "text": LISTA_NUEVA},
        {"role": "ai", "text": "1.- Rey Infinito\n2.- Eres Santo"},  # la del asistente no cuenta
        {"role": "me", "text": "1.- Cena Del Señor\n2.- Mañana", "hidden": True},
        {"role": "me", "text": "1.- Salmo 23\n2.- Agnus Dei", "event": "download_done"},
        {"role": "me", "text": "1.- Él Es Santo\n2.- Sin Vergüenza", "app": True},
        {"role": "me", "text": "completa lo que puedas"},
        {"role": "ai", "text": "¿Cuál?"},
    ]
    got = last_setlist(messages)
    assert _titles(got) == ["Hay Libertad", "Aleluya"]
    assert got is not None and got.name_hint == "Domingo"


def test_last_setlist_acepta_role_user_y_tolera_basura():
    messages = [
        "no es un dict",
        {"role": "user", "text": LISTA_VIEJA},
        {"role": "me", "text": None},
        {"role": "me"},
        {"role": "assistant", "text": LISTA_NUEVA},
        {"role": "system", "text": LISTA_NUEVA},
        7,
    ]
    assert _titles(last_setlist(messages)) == ["Digno De Adorar", "Te Doy Gloria"]
    assert _titles(last_setlist(tuple(messages))) == ["Digno De Adorar", "Te Doy Gloria"]


def test_last_setlist_sin_lista_da_none():
    assert last_setlist([]) is None
    assert (
        last_setlist([{"role": "me", "text": "hola"}, {"role": "ai", "text": LISTA_NUEVA}]) is None
    )
    for value in (None, "texto", {"role": "me", "text": LISTA_NUEVA}, 3):
        assert last_setlist(value) is None


def test_last_setlist_solo_mira_la_ventana():
    chat = [{"role": "me", "text": "vale"}] * (intake._MAX_SCAN - 1)
    assert last_setlist([{"role": "me", "text": LISTA_VIEJA}, *chat]) is not None
    chat.append({"role": "me", "text": "vale"})
    assert last_setlist([{"role": "me", "text": LISTA_VIEJA}, *chat]) is None


# ------------------------------------------------------- entradas hostiles

K = 200_000
HOSTILE = {
    "asteriscos": lambda: "*" * K,
    "corchetes": lambda: "[" * K,
    "parentesis_sin_cerrar": lambda: "(" * K,
    "espacios": lambda: " " * K,
    "arrobas": lambda: "@" * 50_000,
    "youtu_be_sin_id_pegado": lambda: "https://youtu.be/" * (K // 17),
    "youtu_be_sin_id_separado": lambda: "https://youtu.be/ " * (K // 18),
    "youtu_be_sin_id_lineas": lambda: "https://youtu.be/\n" * (K // 18),
    "numeracion_sola": lambda: "1.-\n" * 20_000,
    "anidamiento": lambda: "(" * 50_000 + "[" * 50_000 + ")" * 50_000 + "]" * 50_000,
    "anidamiento_de_formato": lambda: "*_~([" * 40_000,
    "negritas_sin_cerrar": lambda: "*a " * 66_000,
    "tachados_sin_cerrar": lambda: "~a " * 66_000,
    "enlaces_markdown_rotos": lambda: "[a](" * 50_000,
    "menciones_con_particulas": lambda: ("@Ana " + "de " * 10) * 5_000,
    "menciones_y_notas": lambda: "@Ana Ruiz voz " * 15_000,
    "emojis": lambda: NOTES * K,
    "keycaps": lambda: (KEYCAP_1 + " ") * 40_000,
    "etiquetas": lambda: "Banda: " * 30_000,
    "invisibles": lambda: (ZWSP + RLO + "a" + chr(0x301)) * 40_000,
    "separadores_de_medley": lambda: " & " * 66_000,
    "barras": lambda: "a/" * 100_000,
    "url_larguisima": lambda: "https://youtu.be/" + "A" * K,
    "miles_de_canciones": lambda: "".join(
        f"{k % 99 + 1}.- Te Doy Gloria @Ana Ruiz (Voz) https://youtu.be/{ID}\n"
        for k in range(3_000)
    ),
    "miles_de_enlaces": lambda: " ".join(
        f"https://youtu.be/AbCdEfGh{k % 1000:03d}" for k in range(9_000)
    ),
}


@pytest.fixture(scope="module")
def warm():
    # el primer enlace importa `youtube` (medio segundo): que no lo pague la medicion
    assert youtube_video_id(f"https://youtu.be/{ID}") == ID


@pytest.mark.parametrize("name", list(HOSTILE))
def test_entrada_hostil_en_menos_de_un_segundo(name, warm):
    text = HOSTILE[name]()
    t0 = time.perf_counter()
    got = intake._parse(text)  # sin el try de parse_setlist: un fallo no se esconde
    elapsed = time.perf_counter() - t0
    assert elapsed < 1, f"{name}: {elapsed:.2f} s"
    check_contract(got)
    t0 = time.perf_counter()
    read_intent(text)
    assert time.perf_counter() - t0 < 1


def test_mas_de_200_kb_se_corta_y_no_cuelga(warm):
    text = "1.- Digno De Adorar\n2.- Te Doy Gloria\n" + "x" * 1_000_000
    t0 = time.perf_counter()
    got = parse_setlist(text)
    assert time.perf_counter() - t0 < 1
    check_contract(got)


# ---------------------------------------------------------------- propiedades

PIECES = [
    "1.- ", "2) ", "3. ", "10.000 ", "• ", "- ", "* ", "*", "_", "~", "**", "@Ana Ruiz ", "@", "@ana ",
    "(Voz) ", "(", ")", "[", "]", "](", "Medley: ", "Contiene: ", " & ", " + ", " / ", "Banda:",
    "Guitarra: ", "\n", "\r\n", "\r", "\t", "  ", NOTES, KEYCAP_1, ZWSP, NBSP, RLO, "Setlist",
    "*Domingo* 12/10", "Digno De Adorar", "te doy gloria", "agregame esto:", "~Rey Infinito~",
    f"https://youtu.be/{ID}", f"{WATCH}{ID2}&list={PL}", f"{PLAYLIST}{PL}", "https://evil.example/x",
    f"https://youtube.com.evil.example/watch?v={ID}", "youtu.be/", "?v=", "[8:14 p. m., 1/2/2024] Ana: ",
]  # fmt: skip
LISTISH = st.lists(st.one_of(st.sampled_from(PIECES), st.text(max_size=6)), max_size=60).map(
    "".join
)


@settings(max_examples=300, deadline=None)
@given(LISTISH)
def test_el_lector_nunca_falla_y_respeta_el_contrato(text):
    got = intake._parse(text)
    check_contract(got)
    assert parse_setlist(text) == got


@settings(deadline=None)
@given(st.text(max_size=200))
def test_texto_cualquiera_tampoco(text):
    check_contract(intake._parse(text))
    read_intent(text)


URL_PIECES = st.lists(
    st.sampled_from(
        [
            "https://", "http://", "//", "javascript:", "file://", "ytsearch5:", "www.", "m.", "music.",
            "youtube.com", "youtu.be", "evil.example", "@", "\\", ":443", ":8080", ".", "/", "watch",
            "?", "&", "=", "v=", "list=", "si=", "shorts/", "playlist", ID, ID2, PL, "RD", "#", " ",
        ]
    ),
    max_size=14,
).map("".join)  # fmt: skip


@settings(max_examples=400, deadline=None)
@given(st.one_of(URL_PIECES, st.text(max_size=40)))
def test_clean_youtube_url_canonica_e_idempotente(url):
    out = clean_youtube_url(url)
    assert out == "" or re.fullmatch(
        r"https://www\.youtube\.com/(?:watch\?v=[A-Za-z0-9_-]{11}|playlist\?list=[A-Za-z0-9_-]{2,80})",
        out,
    ), out
    assert clean_youtube_url(out) == out
    vid = youtube_video_id(url)
    assert vid == (out.removeprefix(WATCH) if out.startswith(WATCH) else "")
