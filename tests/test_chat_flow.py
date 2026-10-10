"""El asistente con una lista pegada: biblioteca primero, sin vueltas de mas.

Lo que pidio el usuario, probado de punta a punta con una biblioteca
SINTETICA (artistas inventados, titulos de canciones publicas) y un modelo de
mentira que contesta por guion:

  - la lista de WhatsApp la lee la app (no el modelo): las @menciones no son
    artistas y el orden numerado manda;
  - lo que ya esta en la biblioteca entra en la lista al momento, en el orden
    de la lista pegada; lo que falta se ofrece en UNA tarjeta de permiso (que
    sabe meter cada descarga en su sitio al terminar);
  - «verifica» no cambia nada; «dejala exacta» solo con esas palabras; una
    lista nueva solo si se pidio;
  - una lista pegada y un «agregala a domingo» cuestan UNA llamada al modelo;
  - lo que el modelo escribe como texto nunca borra, baja ni crea nada.
"""

import json
import time

import pytest

from danplay import chat as CH
from danplay import config, library, playlists, youtube
from danplay.chat import intake, loop, resolve, setlist
from danplay.chat import tools as T

INSERT_SONG = (
    "INSERT INTO songs (id, path, root, folder, file, artist, title, album, feat, "
    "match_key, duration, stars, favorite) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)"
)

# La lista del domingo tal como llega por WhatsApp (nombres inventados)
DOMINGO = (
    "*Domingo*✅\n"
    "*1.- Digno De Adorar* @Marta Rojas @Noemi (En Su Presencia) Voz\n"
    "*2.- Te Doy Gloria* https://youtu.be/AAAAAAAAAA1 @Marta Rojas\n"
    "*3.-De Gloria en Gloria* @Luis Perez\n"
    "*4.- Cena Del Señor*\n"
    "*5.- Rey Infinito*"
)


class Lib:
    def __init__(self) -> None:
        self.next_id = 1
        self.ids: dict[str, int] = {}

    def add(self, artist: str, title: str) -> int:
        sid = self.next_id
        self.next_id += 1
        folder = f"Artistas/{artist}"
        file = f"{artist} - {title}.mp3"
        with library.connect() as conn:
            conn.execute(
                INSERT_SONG,
                (
                    sid,
                    f"/musica/{folder}/{file}",
                    "/musica",
                    folder,
                    file,
                    artist,
                    title,
                    "",
                    "",
                    "",
                    240.0,
                    0,
                    0,
                ),
            )
        resolve.invalidate()
        return sid


@pytest.fixture
def lib(tmp_path, monkeypatch):
    """Una biblioteca sintetica en una base temporal, sin escribir etiquetas."""
    monkeypatch.setattr(config, "DATABASE", tmp_path / "danplay.db")
    monkeypatch.setattr(playlists, "_stamp_playlists_into_files", lambda ids: None)
    resolve.invalidate()
    out = Lib()
    out.ids = {
        "digno": out.add("Coro Aurora", "Digno De Adorar (En Vivo)"),
        "gloria": out.add("Banda Horizonte", "Te Doy Gloria"),
        "degloria": out.add("Banda Horizonte", "De Gloria En Gloria"),
        "rey": out.add("Coro Aurora", "Rey Infinito"),
        "otra": out.add("Trio Faro", "Sublime Gracia"),
        "marta": out.add("Marta Rojas", "Canto Nuevo"),
    }
    yield out
    resolve.invalidate()


@pytest.fixture
def asked(monkeypatch):
    """Lo que se registraria para pedir permiso (sin hilos ni YouTube)."""
    from danplay.chat import plan

    seen: list[dict] = []

    def register(tool, args, *, plan=None, after=None, chat_id=None):
        seen.append({"tool": tool, "args": args, "plan": plan, "after": after})
        return {
            "id": f"conf{len(seen)}",
            "tool": tool,
            "summary": f"Descargar {len(args.get('items') or [])} temas de YouTube",
            "effect": "Al terminar: …",
        }

    monkeypatch.setattr(plan, "register", register)
    monkeypatch.setattr(youtube, "available", lambda: True)
    return seen


def turn_for(text: str, before: list[dict] | None = None) -> setlist.Turn:
    return loop._turn_for([*(before or []), {"role": "me", "text": text}])


def songs_of(name: str) -> list[int]:
    pl = playlists.by_name(name)
    assert pl
    return [s["id"] for s in playlists.songs(pl["id"], light=True)]


# --------------------------------------------------------------- las listas


def found_id(name: str):
    pl, _ = setlist.find_playlist(name)
    return pl["id"] if pl else None


def test_el_nombre_de_la_lista_se_entiende_como_lo_dice_la_gente(lib):
    a = playlists.create("domingo")["id"]
    e = playlists.create("ensayo domingo")["id"]
    n = playlists.create("Niños")["id"]
    assert found_id("la lista de los domingo") == a
    assert found_id("Domingos") == a
    assert found_id("DOMINGO") == a
    assert found_id("ensayo") == e, "la unica que la contiene"
    assert found_id("ninos") == n, "la ñ plegada como segunda pasada"
    assert setlist.find_playlist("viernes") == (None, [])
    playlists.create("ensayo viernes")
    pl, similar = setlist.find_playlist("ensayo")
    assert pl is None and {p["name"] for p in similar} == {"ensayo domingo", "ensayo viernes"}


# --------------------------------------------------- meter la lista pegada


def test_la_lista_pegada_entra_en_su_orden_y_lo_que_falta_se_ofrece(lib, asked):
    playlists.create("domingo")
    turn = turn_for("agrégame esto a la lista de los domingo:\n" + DOMINGO)
    assert turn.fresh and turn.setlist and len(turn.setlist.items) == 5
    res = T.run_tool("fill_playlist", {"playlist": "domingo"}, turn)
    ui = res["_ui"]["setlist"]
    # las cuatro que hay, en el orden de la lista pegada
    ids = lib.ids
    assert songs_of("domingo") == [ids["digno"], ids["gloria"], ids["degloria"], ids["rey"]]
    assert res["applied"] and res["added"] == 4 and res["missing"] == [4]
    assert [r["status"] for r in ui["rows"]] == [
        "found",
        "other_version",  # trae enlace que no esta en el historial: se usa la que hay
        "found",
        "missing",
        "found",
    ]
    # las @menciones no son el artista: «Marta Rojas» tiene una cancion y no se elige
    assert ui["rows"][0]["song"]["id"] == ids["digno"]
    # UNA tarjeta: Cena Del Señor marcada y el enlace de la otra version sin marcar
    assert len(asked) == 1
    card = asked[0]
    assert card["tool"] == "download_music"
    assert card["args"]["items"] == [
        "https://www.youtube.com/watch?v=AAAAAAAAAA1",
        "cena del señor",
    ]
    assert [r["checked"] for r in card["plan"]["rows"]] == [False, True]
    # y al terminar, cada una en su sitio: los huecos van en el orden de la lista
    slots = card["after"]["slots"]
    assert [s["n"] for s in slots] == [1, 2, 3, 4, 5]
    assert slots[3] == {"n": 4, "item": "cena del señor"}
    assert card["after"]["playlist"]["name"] == "domingo"
    assert res["_ui"]["confirm"]["id"] == "conf1"
    # el informe, en pasado y con nombres, y solo promete la tarjeta que hay
    text = setlist.report(ui, res["_ui"]["confirm"])
    assert "Añadí a «domingo»" in text and "Falta 1: Cena Del Señor" in text
    assert "Abajo te pido permiso" in text


def test_verifica_no_cambia_nada(lib, asked):
    pl = playlists.create("domingo")["id"]
    playlists.add(pl, [lib.ids["rey"], lib.ids["otra"]])
    turn = turn_for("verifica que esté bien la lista del domingo:\n" + DOMINGO)
    assert turn.intent.check
    # aunque el modelo pida dejarla exacta, «verifica» manda
    res = T.run_tool("fill_playlist", {"playlist": "domingo", "mode": "set"}, turn)
    assert res["mode"] == "check" and not res["applied"]
    assert songs_of("domingo") == [lib.ids["rey"], lib.ids["otra"]], "intacta"
    assert asked == [], "comprobar no ofrece descargas"
    text = setlist.report(res["_ui"]["setlist"], None)
    assert text.startswith("Comprobé «domingo»") and "No cambié nada." in text
    assert "sobra Sublime Gracia" in text


def test_dejarla_exacta_solo_con_sus_palabras(lib, asked):
    pl = playlists.create("domingo")["id"]
    playlists.add(pl, [lib.ids["otra"], lib.ids["rey"]])
    # el modelo pide «set» pero la persona no lo dijo: solo se añade
    res = T.run_tool("fill_playlist", {"playlist": "domingo", "mode": "set"}, turn_for(DOMINGO))
    assert res["mode"] == "add" and "note" in res
    assert lib.ids["otra"] in songs_of("domingo"), "no se quito nada"
    # con sus palabras, si: exacta, en su orden, y con lo de antes para deshacer
    turn = turn_for("déjala exactamente así:\n" + DOMINGO)
    assert turn.intent.replace
    res = T.run_tool("fill_playlist", {"playlist": "domingo"}, turn)
    ui = res["_ui"]["setlist"]
    assert res["mode"] == "set" and res["applied"]
    ids = lib.ids
    assert songs_of("domingo") == [ids["digno"], ids["gloria"], ids["degloria"], ids["rey"]]
    assert ui["previous"][0] == ids["otra"]
    assert [x["title"] for x in ui["removed"]] == ["Sublime Gracia"]


def test_una_lista_nueva_solo_si_la_pidio(lib, asked):
    res = T.run_tool("fill_playlist", {"playlist": "jueves", "create": True}, turn_for(DOMINGO))
    assert "error" in res and "no existe la lista" in res["error"]
    assert playlists.by_name("jueves") is None
    turn = turn_for("créame una lista nueva que se llame jueves con estas:\n" + DOMINGO)
    assert turn.intent.create
    res = T.run_tool("fill_playlist", {"playlist": "jueves", "create": True}, turn)
    assert res["created"] and res["added"] == 4


def test_sin_descargar_si_lo_dijo(lib, asked):
    playlists.create("domingo")
    turn = turn_for("agrégalas a domingo, no bajes nada:\n" + DOMINGO)
    assert turn.intent.no_download
    res = T.run_tool("fill_playlist", {"playlist": "domingo"}, turn)
    assert asked == [] and res["_ui"]["confirm"] is None
    text = setlist.report(res["_ui"]["setlist"], None)
    assert "No la bajo, como dijiste." in text


def test_una_lista_vieja_no_pisa_lo_que_pide_ahora(lib, asked):
    playlists.create("domingo")
    before = [{"role": "me", "text": DOMINGO}, {"role": "ai", "text": "Listo."}]
    turn = turn_for("añade Sublime Gracia a domingo", before)
    assert turn.setlist is not None and not turn.fresh
    res = T.run_tool("fill_playlist", {"playlist": "domingo", "items": ["Sublime Gracia"]}, turn)
    assert songs_of("domingo") == [lib.ids["otra"]], "lo que mando el modelo, no la lista de antes"
    # sin canciones del modelo, la lista de antes vale («verifica la lista»)
    res = T.run_tool("fill_playlist", {"playlist": "domingo", "mode": "check"}, turn)
    assert len(res["_ui"]["setlist"]["rows"]) == 5


def test_el_modelo_no_puede_colar_items_con_la_lista_pegada(lib, asked):
    playlists.create("domingo")
    res = T.run_tool(
        "fill_playlist",
        {"playlist": "domingo", "items": ["Sublime Gracia"]},
        turn_for(DOMINGO),
    )
    assert lib.ids["otra"] not in songs_of("domingo")
    assert res["added"] == 4


def test_otra_version_elegida_por_numero(lib, asked):
    playlists.create("domingo")
    res = T.run_tool(
        "fill_playlist",
        {"playlist": "domingo", "overrides": [{"n": 1, "id": lib.ids["marta"]}], "only": [1]},
        turn_for(DOMINGO),
    )
    assert songs_of("domingo") == [lib.ids["marta"]]
    assert res["_ui"]["setlist"]["rows"][0]["note"] == "la eligió la persona"


def test_find_songs_solo_mira(lib):
    res = T.run_tool("find_songs", {"items": ["rey infinito", "Cena del Señor"]}, setlist.Turn())
    assert res["total"] == 2 and res["found"] == 1 and res["missing"] == [2]
    assert "id" + str(lib.ids["rey"]) in res["rows"]
    assert setlist.report_found(res["_ui"]["found"]) is None, "falta una: lo cuenta el modelo"
    res = T.run_tool("find_songs", {"items": ["rey infinito"]}, setlist.Turn())
    assert setlist.report_found(res["_ui"]["found"]) == "Sí, la tienes: Coro Aurora - Rey Infinito."


# ---------------------------------------------------- descargas: biblioteca primero


def test_bajar_algo_que_ya_tenemos_no_pide_permiso(lib, asked):
    res, pending = loop._ask(
        "download_music", {"items": ["Rey Infinito", "Te doy gloria"]}, None, setlist.Turn()
    )
    assert pending is None and asked == []
    assert len(res["already_there"]) == 2 and "no hace falta" in res["note"]


def test_bajar_solo_lo_que_falta(lib, asked):
    res, pending = loop._ask(
        "download_music",
        {"items": ["Rey Infinito", "Cena del Señor"], "after": {"x": 1}, "plan": {"y": 2}},
        None,
        setlist.Turn(),
    )
    assert pending and pending["id"] == "conf1"
    assert asked[0]["args"]["items"] == ["Cena del Señor"]
    assert asked[0]["after"] is None, "un `after` del modelo no llega al registro"
    assert res["needs_confirmation"] and res["already_there"][0]["song"]["id"] == lib.ids["rey"]


def test_las_urls_no_se_pasan_a_minusculas_ni_se_repiten(lib):
    plan = T.download_plan(
        {
            "items": [
                # el parametro de quien compartio el enlace (escrito por partes)
                "https://youtu.be/AbCdEfGhIj1?s" + "i=" + "x" * 16,
                "https://www.youtube.com/watch?v=AbCdEfGhIj1&list=RDxyz",
                "https://youtu.be/abcdefghij1",
                "Rey  Infinito",
                "rey infinito",
            ]
        }
    )
    assert plan["items"] == [
        "https://www.youtube.com/watch?v=AbCdEfGhIj1",
        "https://www.youtube.com/watch?v=abcdefghij1",
        "Rey  Infinito",
    ]


def test_search_songs_prueba_sin_exigir_todas_las_palabras(lib):
    # una errata: la busqueda de siempre (todas las palabras, por prefijo) no da nada
    assert library.search("Digno De Adorrar", None, "artist", 5) == []
    r = T.run_tool("search_songs", {"query": "Digno De Adorrar"})
    assert r["relaxed"] and r["songs"][0]["id"] == lib.ids["digno"]
    r = T.run_tool("search_songs", {"query": "artista:nadie"})
    assert r == {"total": 0, "songs": []}, "con filtros no se aproxima"


def test_search_youtube_normaliza_el_texto_pero_no_los_enlaces(monkeypatch):
    seen = []
    monkeypatch.setattr(youtube, "info", lambda q, n: seen.append(q) or {"ok": False})
    T.run_tool("search_youtube", {"query": "*Cena Del Señor* ✅"})
    T.run_tool("search_youtube", {"query": "https://youtu.be/AbCdEfGhIj1"})
    assert seen == ["cena del señor", "https://youtu.be/AbCdEfGhIj1"]


# ------------------------------------------------------------- el bucle


class Fake:
    """Un modelo de mentira: contesta por guion y apunta lo que recibe."""

    def __init__(self, script, judge="NO"):
        self.script = list(script)
        self.got: list[dict] = []
        self.judge = judge
        outer = self

        class _Call:
            def __init__(self, name, args):
                self.id = f"c{len(outer.got)}"
                self.type = "function"
                self.function = type(
                    "f", (), {"name": name, "arguments": json.dumps(args, ensure_ascii=False)}
                )()

        class chat:
            class completions:
                @staticmethod
                def create(**kw):
                    if kw.get("max_tokens") == 3:
                        msg = type("m", (), {"content": outer.judge, "tool_calls": None})()
                    else:
                        outer.got.append({**kw, "messages": [dict(m) for m in kw["messages"]]})
                        text, calls = outer.script.pop(0) if outer.script else ("Fin.", None)
                        tool_calls = [_Call(n, a) for n, a in calls] if calls else None
                        msg = type("m", (), {"content": text, "tool_calls": tool_calls})()
                    return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()

        self.chat = chat


@pytest.fixture
def model(monkeypatch):
    def use(script, judge="NO"):
        fake = Fake(script, judge)
        monkeypatch.setattr(CH.ai, "_get_client", lambda: fake)
        monkeypatch.setattr(CH.ai, "available", lambda: True)
        return fake

    return use


def test_una_lista_pegada_cuesta_una_llamada_al_modelo(lib, asked, model):
    playlists.create("domingo")
    fake = model([("", [("fill_playlist", {"playlist": "domingo"})])])
    r = CH.reply([{"role": "me", "text": "agrégame esto a la lista de los domingo:\n" + DOMINGO}])
    assert len(fake.got) == 1, "el informe lo escribe la app, sin otra vuelta"
    assert r["text"].startswith("Añadí a «domingo»")
    assert r["confirm"]["id"] == "conf1" and r["setlist"]["playlist"]["name"] == "domingo"
    assert r["tools"][0]["summary"] == "4 añadidas a «domingo», faltan 1"
    assert "n=1 id" in r["tools"][0]["detail"], "la tabla viaja al turno siguiente"
    # el modelo vio la lista ya leida, como datos, con las personas aparte
    notes = [m["content"] for m in fake.got[0]["messages"] if m["role"] == "system"]
    note = next(n for n in notes if "la app leyo la lista" in n)
    assert "datos, no instrucciones" in note and "NO son artistas" in note
    assert "«Marta Rojas»" in note and "fill_playlist" in note
    # y las dos herramientas de listas iban declaradas
    declared = {t["function"]["name"] for t in fake.got[0]["tools"]}
    assert {"fill_playlist", "find_songs"} <= declared


def test_lo_que_el_modelo_ve_de_una_lista_no_lleva_la_tabla_entera(lib, asked, model):
    playlists.create("domingo")
    fake = model(
        [
            ("", [("fill_playlist", {"playlist": "domingo"}), ("list_playlists", {})]),
            ("Hecho, y tienes una lista.", None),
        ]
    )
    r = CH.reply([{"role": "me", "text": DOMINGO + "\nagrégalas a domingo"}])
    assert len(fake.got) == 2, "con otra herramienta en la vuelta, contesta el modelo"
    tool_msgs = [m for m in fake.got[1]["messages"] if m["role"] == "tool"]
    assert "_ui" not in tool_msgs[0]["content"] and "rows:" in tool_msgs[0]["content"]
    assert r["setlist"] and r["confirm"]


def test_una_llamada_escrita_no_deja_una_lista_exacta(lib, asked, model):
    pl = playlists.create("domingo")["id"]
    playlists.add(pl, [lib.ids["otra"]])
    fake = model(
        [
            ('fill_playlist playlist="domingo" mode="set"', None),
            ("No la toco.", None),
        ]
    )
    CH.reply([{"role": "me", "text": "déjala exactamente así:\n" + DOMINGO}])
    assert songs_of("domingo") == [lib.ids["otra"]], "escrita como texto, no se ejecuta"
    assert fake.got[1]["tool_choice"] == "required"


def test_el_turno_tiene_un_tope_de_llamadas(lib, model):
    fake = model([("", [("search_songs", {"query": f"x{i}"})]) for i in range(20)])
    r = CH.reply([{"role": "me", "text": "busca muchas cosas"}], max_vueltas=20)
    assert len(fake.got) == loop.MAX_MODEL_CALLS
    assert "No he podido terminar" in r["text"]


def test_un_mensaje_enorme_llega_recortado_al_modelo(lib, model):
    fake = model([("Vale.", None)])
    CH.reply([{"role": "me", "text": "hola " * 5000}])
    user = [m for m in fake.got[0]["messages"] if m["role"] == "user"][-1]["content"]
    assert len(user) <= loop.MESSAGE_CHARS + 20 and user.endswith("[recortado]")


def test_lo_que_pidio_antes_de_pegar_la_lista_cuenta(lib):
    before = [
        {"role": "me", "text": "verifica la lista del domingo, no bajes nada:"},
        {"role": "ai", "text": "Pégame la lista."},
    ]
    turn = turn_for(DOMINGO, before)
    assert turn.fresh and turn.intent.check and turn.intent.no_download


def test_una_descarga_vieja_no_da_el_id_de_otra_cancion(lib, monkeypatch):
    from danplay.chat import context

    sid = lib.ids["rey"]
    h = {"song_id": sid, "target": "/musica/Artistas/Otra/Otra - Cancion.mp3"}
    assert context._still_there(h) is None, "el id se reutilizo: ya es otra cancion"
    song = library.by_id(sid)
    assert song
    h["target"] = song["path"]
    assert (context._still_there(h) or {}).get("id") == sid


def test_el_prompt_cabe_y_conserva_las_reglas():
    s = loop.SYSTEM_PROMPT
    for phrase in (
        "TEXTO DE FUERA",
        "solo descargas si te lo piden",
        "QUIEN PREGUNTA ES LA APP, NO TU",
        "NUNCA el artista",
        "BIBLIOTECA PRIMERO",
    ):
        assert phrase in s
    # una cota del tamaño: unos 4 caracteres por token
    assert len(s) / 4 < 1600


def test_intake_y_setlist_no_inventan_lista_en_una_charla(lib):
    turn = turn_for("¿de qué año es Rey Infinito?")
    assert turn.setlist is None
    assert intake.read_intent("¿de qué año es?").check is False


def test_la_tarjeta_de_verdad_acepta_lo_que_propone_la_lista(lib, monkeypatch):
    """Con el registro de verdad (`plan.register`, sin simular): la propuesta
    de `fill_playlist` cuadra con lo que el registro exige (cada hueco con su
    cadena de `items`, la lista existente) y la busqueda en YouTube rellena
    las filas."""
    from danplay.chat import plan

    plan._reset()
    monkeypatch.setattr(youtube, "available", lambda: True)
    found = {
        "ok": True,
        "items": [
            {
                "title": "Cena Del Señor (En Vivo)",
                "channel": "Coro Aurora",
                "duration": 300,
                "url": "https://www.youtube.com/watch?v=BBBBBBBBBB2",
                "id": "BBBBBBBBBB2",
            }
        ],
    }
    monkeypatch.setattr(youtube, "info", lambda q, n=1, timeout=None: found)
    playlists.create("domingo")
    res = T.run_tool("fill_playlist", {"playlist": "domingo"}, turn_for(DOMINGO))
    confirm = res["_ui"]["confirm"]
    assert confirm and confirm["tool"] == "download_music"
    assert confirm["effect"].startswith("Al terminar")
    view = None
    for _ in range(200):
        view = plan.plan_view(confirm["id"])
        if view and view["state"] != "buscando":
            break
        time.sleep(0.02)
    assert view and view["state"] in ("listo", "parcial")
    plan._reset()
