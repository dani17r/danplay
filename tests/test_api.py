# -*- coding: utf-8 -*-
"""Pruebas de todos los endpoints, sobre una biblioteca temporal."""
import json, os, pathlib, shutil, sys, tempfile
import pytest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

# La biblioteca de prueba se GENERA: mp3 de verdad, de un segundo de
# silencio, hechos con ffmpeg (ver tests/conftest.py). Antes hacia falta la
# musica del que ejecutara las pruebas, asi que en cualquier otra maquina —y
# en integracion continua— casi todas se saltaban solas y no comprobaban nada.
#
# Con DANPLAY_TEST_MUSIC apuntando a una biblioteca de verdad se usa esa, por
# si se quiere probar con archivos reales.
SOURCE = os.environ.get("DANPLAY_TEST_MUSIC")

ARTISTS = {
    "Barak": ["Mi Gozo", "Sera Llena La Tierra", "Baruch Hashem"],
    "New Wine": ["Shekinah", "A Una Voz", "Aceite Fresco"],
}


def _build_library(lib):
    """Los mp3 sinteticos, con sus etiquetas puestas."""
    from conftest import make_mp3
    for artist, titles in ARTISTS.items():
        for title in titles:
            make_mp3(os.path.join(lib, "Artistas", artist, f"{artist} - {title}.mp3"),
                     artist=artist, title=title, album="Pruebas")


def _copy_library(lib):
    copied = 0
    for artist in ARTISTS:
        d = os.path.join(SOURCE, artist)
        if not os.path.isdir(d):
            continue
        for f in sorted(os.listdir(d))[:3]:
            if f.endswith(".mp3"):
                shutil.copy(os.path.join(d, f), os.path.join(lib, "Artistas", artist, f))
                copied += 1
    return copied


@pytest.fixture(scope="module")
def cliente():
    from fastapi.testclient import TestClient
    from danplay import config
    if not shutil.which("ffmpeg") and not SOURCE:
        pytest.skip("hace falta ffmpeg para generar la biblioteca de prueba")
    tmp = tempfile.mkdtemp(prefix="danplay-pruebas-")
    lib = os.path.join(tmp, "Musica")
    for sub in ("Artistas/Barak", "Artistas/New Wine", "Entrada", "Secuencias"):
        os.makedirs(os.path.join(lib, sub), exist_ok=True)
    if not (SOURCE and os.path.isdir(SOURCE) and _copy_library(lib)):
        _build_library(lib)

    config.LIBRARY = __import__("pathlib").Path(lib)
    config.INBOX = config.LIBRARY / "Entrada"
    config.ARTISTS_DIR = config.LIBRARY / "Artistas"
    config.REVIEW_DIR = config.LIBRARY / "Revisar"
    config.DATABASE = __import__("pathlib").Path(tmp) / "prueba.db"

    from danplay import library as B, api as A
    B.add_folder(lib, "prueba")
    B.scan()
    c = TestClient(A.app)
    yield c
    shutil.rmtree(tmp, ignore_errors=True)


def test_status(cliente):
    d = cliente.get("/api/status").json()
    assert d["stats"]["total"] > 0
    assert set(["rust", "ai", "ffmpeg", "fingerprint"]) <= set(d)


def test_search_everything(cliente):
    d = cliente.get("/api/search", params={"limit": 100}).json()
    assert d["total"] > 0 and "songs" in d


def test_search_by_artist(cliente):
    d = cliente.get("/api/search", params={"q": "artista:barak"}).json()
    assert d["total"] > 0
    assert all("barak" in c["artist"].lower() for c in d["songs"])


def test_search_duration_comparator(cliente):
    d = cliente.get("/api/search", params={"q": "duracion>1"}).json()
    assert all(c["duration"] > 1 for c in d["songs"])


def test_search_with_no_results(cliente):
    d = cliente.get("/api/search", params={"q": "zzzznoexistezzz"}).json()
    assert d["total"] == 0


def test_facets(cliente):
    d = cliente.get("/api/facets").json()
    assert "artists" in d and len(d["artists"]) > 0


def test_song_and_edit(cliente):
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    c = cliente.get(f"/api/song/{cid}").json()
    assert c["id"] == cid and c["existe"] is True
    r = cliente.patch(f"/api/song/{cid}", json={"genre": "Adoracion"}).json()
    assert r["genre"] == "Adoracion"


def test_missing_song(cliente):
    assert cliente.get("/api/cancion/999999").status_code == 404


def test_stars_and_favorite(cliente):
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    assert cliente.post(f"/api/song/{cid}/stars", json={"stars": 5}).json()["stars"] == 5
    assert cliente.post(f"/api/song/{cid}/favorite", json={"favorite": True}).json()["favorite"] == 1
    d = cliente.get("/api/search", params={"only_favorites": True}).json()
    assert any(c["id"] == cid for c in d["songs"])


def test_audio_is_served(cliente):
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    r = cliente.get(f"/api/song/{cid}/audio")
    assert r.status_code == 200 and r.headers["content-type"].startswith("audio/")
    assert len(r.content) > 1000


def test_path_for_rust(cliente):
    """Rust pide la ruta en disco para servir el audio sin pasar por Python."""
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    d = cliente.get(f"/api/song/{cid}/path").json()
    assert os.path.isfile(d["path"]) and d["bytes"] > 0


def test_cover_without_artwork_gives_404(cliente):
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    assert cliente.get(f"/api/song/{cid}/cover").status_code in (200, 404)


def test_playlist_lifecycle(cliente):
    lid = cliente.post("/api/playlists", json={"name": "Prueba"}).json()["id"]
    ids = [c["id"] for c in cliente.get("/api/search", params={"limit": 3}).json()["songs"]]
    r = cliente.post(f"/api/playlists/{lid}/songs", json={"ids": ids}).json()
    assert len(r["songs"]) == len(ids)
    cliente.post(f"/api/playlists/{lid}/order", json={"ids": list(reversed(ids))})
    assert cliente.get(f"/api/playlists/{lid}/songs").json()["songs"][0]["id"] == ids[-1]
    cliente.delete(f"/api/playlists/{lid}/songs/{ids[0]}")
    assert len(cliente.get(f"/api/playlists/{lid}/songs").json()["songs"]) == len(ids) - 1
    assert cliente.post(f"/api/playlists/{lid}/export").json()["file"].endswith(".m3u8")
    cliente.delete(f"/api/playlists/{lid}")
    assert all(l["id"] != lid for l in cliente.get("/api/playlists").json()["playlists"])


def test_folders_and_exclusions(cliente):
    d = cliente.get("/api/folders").json()
    assert len(d["folders"]) >= 1
    d = cliente.post("/api/exclusions", json={"pattern": "Secuencias"}).json()
    assert any(e["pattern"] == "Secuencias" for e in d["exclusions"])
    d = cliente.delete("/api/exclusions", params={"pattern": "Secuencias"}).json()
    assert all(e["pattern"] != "Secuencias" for e in d["exclusions"])


def test_adding_missing_folder_gives_400(cliente):
    assert cliente.post("/api/folders", json={"path": "/no/existe/xyz"}).status_code == 400


def test_transpose_endpoint(cliente):
    d = cliente.post("/api/transpose",
                     json={"text": "| Bb | Gm7 |", "from_key": "Bb", "to_key": "G"}).json()
    assert d["text"] == "| G | Em7 |"
    assert d["latin"] == "| Sol | Mim7 |"


def test_inbox_and_convertible(cliente):
    assert "files" in cliente.get("/api/inbox").json()
    d = cliente.get("/api/convertible").json()
    assert "Secuencias" in d["protected"]


def test_dry_run_import_moves_nothing(cliente):
    from danplay import config
    source_path = next(iter(cliente.get("/api/search", params={"limit": 1}).json()["songs"]))
    shutil.copy(source_path["path"], config.INBOX / "prueba_import.mp3")
    before = len(os.listdir(config.INBOX))
    cliente.post("/api/import", json={"dry_run": True})
    assert len(os.listdir(config.INBOX)) == before
    os.remove(config.INBOX / "prueba_import.mp3")


def test_duplicates(cliente):
    d = cliente.get("/api/duplicates").json()
    assert "identical" in d and "similar" in d


def test_settings_round_trip(cliente):
    original = cliente.get("/api/settings").json()["convert_mp3"]
    d = cliente.post("/api/settings", json={"convert_mp3": not original}).json()
    assert d["convert_mp3"] == (not original)
    cliente.post("/api/settings", json={"convert_mp3": original})


def test_ai_key_is_never_returned_whole(cliente):
    d = cliente.get("/api/settings").json()
    assert "…" in d["ai_key"] or d["ai_key"] == ""


@pytest.fixture
def perfiles_ia(tmp_path, monkeypatch):
    """Los perfiles de IA en un archivo temporal: nada toca los del usuario."""
    from danplay import ai, providers
    monkeypatch.setattr(providers, "PROFILES_FILE", tmp_path / "ai.json")
    for v in ("DANPLAY_AI_PROVIDER", "DANPLAY_AI_KEY", "DANPLAY_AI_MODEL",
              "DANPLAY_AI_CHAT_MODEL", "DEEPINFRA_API_KEY"):
        monkeypatch.delenv(v, raising=False)
    providers.reload(); ai.reset_client()
    yield
    providers.reload(); ai.reset_client()


def test_ai_providers_overview(cliente, perfiles_ia):
    d = cliente.get("/api/ai/providers").json()
    ids = {p["id"] for p in d["catalog"]}
    assert {"openai", "anthropic", "google", "deepinfra", "openrouter", "ollama", "custom"} <= ids
    assert [g["id"] for g in d["groups"]] == ["lab", "platform", "cloud", "asia", "local", "custom"]
    assert d["active"] == "" and d["active_profile"] is None and not d["ai_ready"]
    assert d["catalog_status"]["models"] > 100, "la foto de models.dev viaja con la app"
    # el catalogo no lleva ningun secreto: son datos publicos
    assert "key" not in json.dumps(d["profiles"])


def test_ai_profile_save_activate_and_delete(cliente, perfiles_ia):
    r = cliente.post("/api/ai/profile", json={"provider": "openrouter", "key": "sk-or-v1-abcdefghijklmnop",
                                              "model": "google/gemini-3.5-flash-lite",
                                              "chat_model": "google/gemini-3.8-flash"})
    assert r.status_code == 200, r.text
    d = r.json()
    assert d["saved"] == "openrouter" and d["active"] == "openrouter" and d["ai_ready"]
    assert d["active_profile"]["chat_model"] == "google/gemini-3.8-flash"
    assert "abcdefghijklmnop" not in r.text, "la clave nunca vuelve entera"
    assert d["profiles"]["openrouter"]["has_key"]
    s = cliente.get("/api/settings").json()
    assert s["provider"] == "openrouter" and s["provider_name"] == "OpenRouter"
    assert s["model"] == "google/gemini-3.5-flash-lite" and s["has_ai_key"]
    assert cliente.get("/api/status").json()["provider"] == "OpenRouter"
    assert cliente.get("/api/chat/tools").json()["provider"] == "OpenRouter"

    # un segundo proveedor, sin activarlo, no quita el activo
    d = cliente.post("/api/ai/profile", json={"provider": "ollama", "model": "qwen3:8b",
                                              "activate": False}).json()
    assert d["active"] == "openrouter" and "ollama" in d["profiles"]
    d = cliente.post("/api/ai/activate", json={"id": "ollama"}).json()
    assert d["active"] == "ollama" and d["active_profile"]["local"]
    d = cliente.delete("/api/ai/profile/ollama").json()
    assert d["active"] == "openrouter" and "ollama" not in d["profiles"]

    assert cliente.post("/api/ai/profile", json={"provider": "inventado"}).status_code == 400
    assert cliente.post("/api/ai/activate", json={"id": "inventado"}).status_code == 400
    assert cliente.post("/api/ai/profile", json={"provider": "openai", "raro": 1}).status_code == 422


def test_ai_check_and_models_with_a_fake_provider(cliente, perfiles_ia, monkeypatch):
    from danplay import ai
    calls = []

    class _Call:
        id = "1"
        function = type("f", (), {"name": "saluda", "arguments": "{}"})()

    class _Msg:
        def __init__(self, content, tool_calls=None):
            self.content, self.tool_calls = content, tool_calls

    class _Fake:
        class chat:                                          # noqa: N801
            class completions:
                @staticmethod
                def create(**kw):
                    calls.append(kw)
                    msg = _Msg("", [_Call()]) if kw.get("tools") else _Msg("ok")
                    return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()

        class models:                                        # noqa: N801
            @staticmethod
            def list():
                return type("p", (), {"data": [type("m", (), {"id": "gpt-6-astra"})(),
                                               type("m", (), {"id": "gpt-5.6-luna"})()]})()
    monkeypatch.setattr(ai, "_build_client", lambda p: _Fake())

    draft = {"provider": "openai", "key": "sk-prueba", "model": "gpt-5.6-luna", "chat_model": "gpt-6-astra"}
    d = cliente.post("/api/ai/check", json=draft).json()
    assert d["ok"] and d["tools_ok"] and d["provider"] == "OpenAI", d
    assert [c["model"] for c in calls] == ["gpt-5.6-luna", "gpt-6-astra", "gpt-6-astra"]

    d = cliente.post("/api/ai/models", json=draft).json()
    assert d["ok"] and [m["id"] for m in d["models"]]
    astra = next(m for m in d["models"] if m["id"] == "gpt-6-astra")
    assert astra["known"] and astra["tools"] and astra["cost_out"]
    assert d["catalog"], "y la lista del catalogo, por si el proveedor no lista todo"
    assert d["suggest"]["chat"] and d["suggest"]["fast"]

    # sin proveedor en el borrador, error claro, no un 500
    assert cliente.post("/api/ai/models", json={"provider": "custom"}).json()["ok"] is False


def test_indexes_nothing_without_folders(tmp_path, monkeypatch):
    """Regresion: la app mostraba canciones de ~/Musica sin que nadie lo pidiera."""
    import importlib, pathlib
    from danplay import config, library as B
    bd_previa, bib_previa = config.DATABASE, config.LIBRARY
    config.DATABASE = pathlib.Path(tmp_path) / "vacia.db"
    config.LIBRARY = pathlib.Path(tmp_path) / "no-usar"
    (config.LIBRARY / "Artistas").mkdir(parents=True)
    try:
        assert B._roots() == [], "sin carpetas configuradas no debe haber raices"
        assert B.scan()["total"] == 0
        assert B.stats_of()["total"] == 0
    finally:
        config.DATABASE, config.LIBRARY = bd_previa, bib_previa


def test_status_warns_when_no_folders(cliente):
    d = cliente.get("/api/status").json()
    assert "configured" in d and isinstance(d["configured"], bool)


def test_warns_if_folder_is_inside_another(cliente):
    from danplay import config
    sub = str(config.ARTISTS_DIR)
    d = cliente.post("/api/check-folder", json={"path": sub}).json()
    assert d["notice"] and d["notice"]["kind"] == "inside"


def test_warns_if_folder_was_already_there(cliente):
    from danplay import config
    d = cliente.post("/api/check-folder", json={"path": str(config.LIBRARY)}).json()
    assert d["notice"] and d["notice"]["kind"] == "same"


def test_finds_a_copy_of_the_same_music(cliente, tmp_path):
    """El caso real: el original y su respaldo en rutas distintas."""
    import shutil
    from danplay import config, library as B
    copy = tmp_path / "respaldo"
    shutil.copytree(config.ARTISTS_DIR, copy)
    d = cliente.post("/api/check-folder", json={"path": str(copy)}).json()
    assert d["notice"], "deberia avisar de que es la misma musica"
    assert d["notice"]["kind"] == "copy"
    assert d["notice"]["percent"] >= 60


def test_unrelated_folder_does_not_warn(cliente, tmp_path):
    empty = tmp_path / "otra-cosa"
    empty.mkdir()
    d = cliente.post("/api/check-folder", json={"path": str(empty)}).json()
    assert d["notice"] is None


def test_adding_the_same_folder_twice_does_not_duplicate(cliente):
    """Repetir la misma carpeta debe avisar y dejarlo todo como estaba."""
    from danplay import config
    path = str(config.LIBRARY)
    before = len(cliente.get("/api/folders").json()["folders"])
    total_antes = cliente.get("/api/status").json()["stats"]["total"]
    for _ in range(3):
        r = cliente.post("/api/folders", json={"path": path}).json()
        assert r["action"] == "already_there"
        assert r["notice"]["kind"] == "same"
    assert len(cliente.get("/api/folders").json()["folders"]) == before
    cliente.post("/api/scan")
    assert cliente.get("/api/status").json()["stats"]["total"] == total_antes


def test_subfolder_of_indexed_is_not_added(cliente):
    from danplay import config
    r = cliente.post("/api/folders", json={"path": str(config.ARTISTS_DIR)}).json()
    assert r["action"] == "already_there" and r["notice"]["kind"] == "inside"
    assert all(c["path"] != str(config.ARTISTS_DIR) for c in r["folders"])


def test_parent_folder_replaces_inner_one(cliente, tmp_path):
    import shutil
    from danplay import config
    parent = tmp_path / "coleccion"
    child = parent / "discos"
    child.mkdir(parents=True)
    for f in list(config.ARTISTS_DIR.rglob("*.mp3"))[:3]:
        shutil.copy(f, child / f.name)
    cliente.post("/api/folders", json={"path": str(child)})
    r = cliente.post("/api/folders", json={"path": str(parent)}).json()
    assert r["action"] == "replaced"
    rutas = [c["path"] for c in r["folders"]]
    assert str(parent) in rutas and str(child) not in rutas
    cliente.delete("/api/folders", params={"path": str(parent)})


def test_a_copy_asks_for_confirmation_and_can_be_forced(cliente, tmp_path):
    import shutil
    from danplay import config
    copy = tmp_path / "respaldo2"
    shutil.copytree(config.ARTISTS_DIR, copy)
    r = cliente.post("/api/folders", json={"path": str(copy)}).json()
    assert r["action"] == "confirm" and r["notice"]["kind"] == "copy"
    assert all(c["path"] != str(copy) for c in r["folders"])
    r = cliente.post("/api/folders", json={"path": str(copy), "force": True}).json()
    assert r["action"] == "added"
    cliente.delete("/api/folders", params={"path": str(copy)})


def test_exclusions_ignore_case(cliente):
    """Escribir «secuencias» debe excluir la carpeta «Secuencias»."""
    from danplay import config
    (config.LIBRARY / "Secuencias").mkdir(exist_ok=True)
    import shutil
    source_path = next(config.ARTISTS_DIR.rglob("*.mp3"))
    shutil.copy(source_path, config.LIBRARY / "Secuencias" / "pista.mp3")
    cliente.post("/api/scan")
    before = cliente.get("/api/status").json()["stats"]["total"]

    cliente.post("/api/exclusions", json={"pattern": "secuencias"})   # en minusculas
    cliente.post("/api/scan")
    after = cliente.get("/api/status").json()["stats"]["total"]
    cliente.delete("/api/exclusions", params={"pattern": "secuencias"})
    cliente.post("/api/scan")
    assert after < before, "la exclusion en minusculas no surtio efecto"


def test_chat_tools_are_declared(cliente):
    d = cliente.get("/api/chat/tools").json()
    names = [h["name"] for h in d["tools"]]
    # las de siempre
    assert "search_songs" in names and "create_playlist" in names
    # y las nuevas: el chat ya descarga, busca en YouTube y comprueba en la web
    for n in ("search_youtube", "download_music", "download_status",
              "search_web", "lyrics_by_name"):
        assert n in names, f"falta la herramienta {n}"
    assert all(h["description"] for h in d["tools"]), "todas necesitan descripcion"


def test_chat_requires_messages(cliente):
    assert cliente.post("/api/chat", json={"messages": []}).status_code == 400


def test_chat_search_tool(cliente):
    from danplay import chat as CH
    r = CH.run_tool("search_songs", {"query": "artista:barak", "limit": 5})
    assert r["total"] > 0
    assert all("id" in c and "title" in c for c in r["songs"])


def test_chat_create_playlist_tool(cliente):
    from danplay import chat as CH, playlists as L
    ids = [c["id"] for c in CH.run_tool("search_songs", {"query": "", "limit": 3})["songs"]]
    r = CH.run_tool("create_playlist", {"name": "Prueba del chat", "ids": ids})
    assert r["added"] == len(ids)
    assert any(l["name"] == "Prueba del chat" for l in L.list_all())
    L.remove(r["playlist_id"])


def test_chat_transpose_tool(cliente):
    from danplay import chat as CH
    r = CH.run_tool("transpose_chords", {"chords": "| Bb | Gm7 |", "from_key": "Bb", "to_key": "G"})
    assert r["chords"] == "| G | Em7 |"


def test_unknown_tool_does_not_blow_up(cliente):
    from danplay import chat as CH
    assert "error" in CH.run_tool("herramienta_que_no_existe", {"url": "x"})


def test_download_without_items_skips_network(cliente):
    """El caso vacio se corta antes de salir a internet."""
    from danplay import chat as CH, youtube as YT
    r = CH.run_tool("download_music", {"items": []})
    assert "error" in r
    assert not YT.STATE["active"], "no deberia haber arrancado ninguna descarga"


def test_system_prompt_only_talks_music(cliente):
    """Lo que no es musica se rechaza; es el limite que pidio el usuario."""
    from danplay import chat as CH
    s = CH.SYSTEM_PROMPT.lower()
    assert "solo de musica" in s
    assert "no lo respondas" in s, "debe decirle que NO conteste lo de fuera"
    # y tiene que nombrar ejemplos de lo que queda fuera
    assert any(x in s for x in ("politica", "guerras", "deportes"))


def test_system_prompt_never_downloads_alone(cliente):
    """Puede descargar, pero solo si se lo piden: 'que no haga mas de la cuenta'."""
    from danplay import chat as CH
    s = CH.SYSTEM_PROMPT.lower()
    assert "solo descargas si te lo piden" in s
    assert "nunca" in s and "iniciativa propia" in s
    assert "haz lo que te piden y nada mas" in s


# ---------------------------------------------------------- el asistente actua
# El chat no solo consulta: pone musica, puntua, corrige y borra. Reproducir no
# lo puede hacer el nucleo (el audio es de Rust y la cola vive en la interfaz),
# asi que esas herramientas devuelven una `action` que ejecuta la app.

def test_las_herramientas_de_control_estan_declaradas(cliente):
    d = cliente.get("/api/chat/tools").json()
    names = [h["nombre" if "nombre" in h else "name"] for h in d["tools"]]
    for n in ("play_song", "play_playlist", "player_control", "set_stars",
              "set_favorite", "edit_song", "find_lyrics_and_cover",
              "delete_song", "remove_from_playlist", "delete_playlist"):
        assert n in names, f"al asistente le falta {n}"


def test_reproducir_devuelve_una_orden_para_la_app(cliente):
    from danplay import chat, library
    cid = library.search("", limit=1)[0]["id"]
    r = chat.run_tool("play_song", {"id": cid})
    assert r["ok"] and r["action"] == {"kind": "play_song", "song_id": cid}


def test_reproducir_algo_que_no_existe_no_revienta(cliente):
    from danplay import chat
    assert "error" in chat.run_tool("play_song", {"id": 999999})


def test_control_del_reproductor_valida_la_orden(cliente):
    from danplay import chat
    assert chat.run_tool("player_control", {"command": "next"})["action"] == \
        {"kind": "player", "command": "next"}
    assert "error" in chat.run_tool("player_control", {"command": "bailar"})


def test_el_asistente_puntua_y_marca_favorito(cliente):
    from danplay import chat, library
    cid = library.search("", limit=1)[0]["id"]
    chat.run_tool("set_stars", {"id": cid, "stars": 4})
    chat.run_tool("set_favorite", {"id": cid, "favorite": True})
    c = library.by_id(cid)
    assert c["stars"] == 4 and c["favorite"]
    # fuera de rango se recorta en vez de guardar cualquier cosa
    chat.run_tool("set_stars", {"id": cid, "stars": 99})
    assert library.by_id(cid)["stars"] == 5


def test_el_asistente_corrige_datos(cliente):
    from danplay import chat, library
    cid = library.search("", limit=1)[0]["id"]
    r = chat.run_tool("edit_song", {"id": cid, "genre": "Adoracion"})
    assert r["ok"] and "genre" in r["changed"]
    assert library.by_id(cid)["genre"] == "Adoracion"
    # sin nada que cambiar, lo dice en vez de hacer una escritura vacia
    assert "error" in chat.run_tool("edit_song", {"id": cid})


def test_lo_descargado_entra_en_el_indice_sin_reescanear(cliente):
    """El archivo llegaba a Artistas/ pero no al indice: no salia en la app."""
    import shutil
    from danplay import config, library
    origen = library.search("", limit=1)[0]["path"]
    destino = config.ARTISTS_DIR / "Barak" / "Barak - Copia De Prueba.mp3"
    shutil.copy(origen, destino)
    antes = library.stats_of()["total"]
    nueva = library.index_file(str(destino))
    assert nueva and nueva["id"]
    assert library.stats_of()["total"] == antes + 1
    library.forget(nueva["id"]); destino.unlink()


def test_indexar_algo_fuera_de_las_carpetas_no_hace_nada(cliente):
    from danplay import library
    assert library.index_file("/tmp/no-existe-esto.mp3") is None


# ------------------------------------------------------- historial de descargas
# Toda descarga queda apuntada, venga del boton o del asistente. No es
# metadata de una cancion, asi que vive en la base y no en el mp3; el escaneo
# no la toca.

def test_el_historial_empieza_vacio_y_acepta_entradas(cliente):
    from danplay import library
    library.clear_download_history()
    assert library.download_count() == 0
    library.log_download({"source": "manual", "query": "x", "title": "Una", "ok": True})
    assert library.download_count() == 1


def test_distingue_quien_pidio_la_descarga(cliente):
    from danplay import library
    library.clear_download_history()
    library.log_download({"source": "manual", "title": "A", "ok": True})
    library.log_download({"source": "assistant", "title": "B", "ok": True})
    fuentes = {h["source"] for h in library.download_history()}
    assert fuentes == {"manual", "assistant"}


def test_guarda_los_tres_estados(cliente):
    from danplay import library
    library.clear_download_history()
    library.log_download({"title": "ok", "ok": True})
    library.log_download({"title": "repe", "ok": False, "already": True})
    library.log_download({"title": "mal", "ok": False, "reason": "no disponible"})
    h = {x["title"]: x for x in library.download_history()}
    assert h["ok"]["ok"] == 1 and h["ok"]["already"] == 0
    assert h["repe"]["ok"] == 0 and h["repe"]["already"] == 1
    assert h["mal"]["ok"] == 0 and h["mal"]["reason"] == "no disponible"


def test_el_historial_sale_de_lo_mas_nuevo_a_lo_mas_viejo(cliente):
    from danplay import library
    library.clear_download_history()
    for i, t in enumerate(("vieja", "media", "nueva")):
        library.log_download({"title": t, "ok": True, "at": 1000 + i})
    assert [h["title"] for h in library.download_history()] == ["nueva", "media", "vieja"]


def test_el_escaneo_no_borra_el_historial(cliente):
    """El indice se reconstruye desde los archivos; el historial no es del mp3."""
    from danplay import library
    library.clear_download_history()
    library.log_download({"title": "sobrevive", "ok": True})
    library.scan()
    assert library.download_count() == 1


def test_el_endpoint_devuelve_el_historial(cliente):
    from danplay import library
    library.clear_download_history()
    library.log_download({"source": "assistant", "title": "Del chat", "ok": True})
    d = cliente.get("/api/downloads/history").json()
    assert d["total"] == 1 and d["items"][0]["title"] == "Del chat"
    assert cliente.delete("/api/downloads/history").json()["removed"] == 1
    assert cliente.get("/api/downloads/history").json()["total"] == 0


# ---------------------------------------------------------------------------
# Lo que se toco al afinar el rendimiento. Cubre el comportamiento, no la
# implementacion: da igual como se ponga al dia el indice mientras acabe bien.

def test_favorites_filter_is_not_cut_off_by_the_limit(cliente):
    """«Favoritos» debe enseñar los favoritos de TODA la biblioteca.

    El filtro se aplicaba despues de recortar por `limit`, asi que solo salian
    los favoritos que hubiera entre los primeros N resultados. Con un limit de
    1 y el favorito al final de la lista, no salia ninguno.
    """
    todas = cliente.get("/api/search", params={"limit": 500}).json()["songs"]
    assert len(todas) >= 2
    ultima = todas[-1]
    cliente.post(f"/api/song/{ultima['id']}/favorite", json={"favorite": True})
    try:
        cuantos = cliente.get("/api/search",
                              params={"only_favorites": True, "limit": 500}).json()
        assert ultima["id"] in [c["id"] for c in cuantos["songs"]]
        # pedir exactamente los que hay tiene que devolverlos todos: el limite
        # se aplica a los favoritos, no a la lista entera antes de filtrar
        n = cuantos["total"]
        ajustado = cliente.get("/api/search",
                               params={"only_favorites": True, "limit": n}).json()
        assert ajustado["total"] == n
    finally:
        cliente.post(f"/api/song/{ultima['id']}/favorite", json={"favorite": False})


def test_min_stars_filter_is_not_cut_off_by_the_limit(cliente):
    todas = cliente.get("/api/search", params={"limit": 500}).json()["songs"]
    ultima = todas[-1]
    cliente.post(f"/api/song/{ultima['id']}/stars", json={"stars": 5})
    try:
        todos = cliente.get("/api/search",
                            params={"min_stars": 4, "limit": 500}).json()
        assert ultima["id"] in [c["id"] for c in todos["songs"]]
        n = todos["total"]
        ajustado = cliente.get("/api/search",
                               params={"min_stars": 4, "limit": n}).json()
        assert ajustado["total"] == n
    finally:
        cliente.post(f"/api/song/{ultima['id']}/stars", json={"stars": 0})


def test_search_survives_a_quote_in_the_query(cliente):
    """Una comilla rompia la sintaxis de FTS5 y reventaba la busqueda entera."""
    for q in ['rock"n roll', 'a"', '"']:
        r = cliente.get("/api/search", params={"q": q})
        assert r.status_code == 200, f"{q!r} -> {r.status_code}"


def test_resolving_a_duplicate_updates_the_index(cliente):
    """Tras conservar una copia, la borrada sale del indice y la que queda esta.

    Antes esto se resolvia relanzando un escaneo completo; ahora se actualiza
    solo lo que cambio, y el resultado tiene que ser el mismo.
    """
    from danplay import config
    original = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    copia = pathlib.Path(original["path"]).with_name("copia duplicada - r.mp3")
    shutil.copy(original["path"], copia)
    indexada = cliente.get("/api/search", params={"limit": 500}).json()
    from danplay import library as B
    B.index_file(str(copia))

    antes = {c["path"] for c in
             cliente.get("/api/search", params={"limit": 500}).json()["songs"]}
    assert str(copia) in antes

    # `dry_run` es True por defecto: borrar hay que pedirlo expresamente
    prueba = cliente.post("/api/duplicates/resolve",
                          json={"keep": str(copia), "remove": [original["path"]]}).json()
    assert prueba["dry_run"] and os.path.exists(original["path"]), \
        "sin pedirlo, no deberia borrar nada"

    r = cliente.post("/api/duplicates/resolve",
                     json={"keep": str(copia), "remove": [original["path"]],
                           "dry_run": False}).json()
    assert r["ok"], r

    despues = {c["path"] for c in
               cliente.get("/api/search", params={"limit": 500}).json()["songs"]}
    assert original["path"] not in despues, "la copia borrada sigue en el indice"
    assert r["kept"] in despues, "la que se conserva no quedo indexada"
    assert not os.path.exists(original["path"])


def test_untagged_files_still_report_duration(cliente):
    """Un archivo sin etiquetas tiene que enseñar su duracion igual.

    `mutagen.File()` devuelve un objeto cuyo len() es el NUMERO DE ETIQUETAS,
    asi que comprobarlo con `if base` daba falso en los archivos sin etiquetar
    y se saltaba la duracion y el bitrate: salian a 0 en la lista.
    """
    from mutagen.id3 import ID3
    from danplay import tags as T
    original = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    desnudo = pathlib.Path(original["path"]).with_name("sin etiquetas.mp3")
    shutil.copy(original["path"], desnudo)
    try:
        t = ID3(str(desnudo))
        t.delete()
        t.save()
        d = T.read_all(str(desnudo))
        assert d["duration"] > 0, "un archivo sin etiquetas se quedo sin duracion"
    finally:
        desnudo.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# El puente con la interfaz.
#
# El JS y Python se hablan por nombres de parametro, y no hay nada que los ate:
# al pasar el codigo a ingles se quedaron cuatro en castellano (`ruta`,
# `patron`, `forzar`, `keepOne`) y las funciones correspondientes de la app
# dejaron de funcionar en silencio —un 422 que nadie miraba— hasta que alguien
# intentaba quitar una carpeta. Esto los compara solos.

def _api_js() -> str:
    return (pathlib.Path(__file__).resolve().parent.parent
            / "desktop/src/api.js").read_text(encoding="utf-8")


def test_the_interface_uses_the_query_parameters_the_api_expects():
    """Todo parametro de consulta OBLIGATORIO tiene que aparecer en api.js.

    Solo los obligatorios: si falta uno, la peticion se rechaza entera con un
    422 y la funcion deja de existir para el usuario. Los que tienen valor por
    defecto se pueden omitir sin consecuencias, y ademas varios se construyen
    con URLSearchParams, asi que su nombre no esta escrito en el JS.
    """
    import inspect
    from fastapi import params as fastapi_params
    from pydantic_core import PydanticUndefined
    from danplay import api as A
    js = _api_js()
    faltan = []
    for route in A.app.routes:
        fn = getattr(route, "endpoint", None)
        if fn is None:
            continue
        for name, p in inspect.signature(fn).parameters.items():
            if not isinstance(p.default, fastapi_params.Query):
                continue
            obligatorio = p.default.default in (PydanticUndefined, Ellipsis)
            if obligatorio and f"{name}=" not in js:
                faltan.append(f"{sorted(route.methods)} {route.path} -> «{name}»")
    assert not faltan, ("la interfaz no manda estos parametros de consulta:\n  "
                        + "\n  ".join(faltan))


def test_the_interface_sends_the_body_fields_the_api_reads(cliente):
    """Los campos del cuerpo que la app manda tienen que existir de verdad.

    Se comprueba con llamadas reales, que es lo unico que demuestra que el
    nombre coincide: un campo mal escrito no da error, simplemente se ignora
    y la accion no hace nada.
    """
    # quitar una carpeta: la app manda ?path=
    d = cliente.get("/api/folders").json()
    assert d["folders"], "la biblioteca de prueba deberia tener una carpeta"
    ruta = d["folders"][0]["path"]
    r = cliente.delete(f"/api/folders?path={ruta}")
    assert r.status_code == 200, r.text
    cliente.post("/api/folders", json={"path": ruta, "label": "prueba"})

    # exclusiones: la app manda ?pattern=
    cliente.post("/api/exclusions", json={"pattern": "Prueba", "kind": "glob"})
    r = cliente.delete("/api/exclusions?pattern=Prueba")
    assert r.status_code == 200, r.text
    assert "Prueba" not in [e["pattern"] for e in r.json()["exclusions"]]


def test_rescanning_reuses_files_that_have_not_changed(cliente):
    """Un reescaneo no vuelve a abrir los archivos que siguen igual.

    Y tiene que dejar el indice EXACTAMENTE igual que un escaneo completo:
    el atajo es solo para no releer, no para guardar cosas distintas.
    """
    from danplay import library as B
    def foto():
        conn = B.connect()
        d = {r["path"]: tuple(r[k] for k in B.COLUMNS)
             for r in conn.execute("SELECT * FROM songs")}
        conn.close()
        return d

    B.scan()
    antes = foto()
    r = B.scan()
    assert antes == foto(), "el reescaneo cambio el indice"
    assert r["reused"] > 0, "no reaprovecho ninguno"
    assert r["total"] == len(antes)

    # Tocar un archivo obliga a releerlo, y el resto se sigue reaprovechando.
    # Se elige uno CON artista a proposito: los que no lo tienen se releen
    # siempre (pueden resolverse ahora que hay mas carpetas de artista), asi
    # que tocar uno de esos no cambiaria la cuenta.
    artista_en = B.COLUMNS.index("artist")
    alguna = next(p for p, fila in antes.items()
                  if fila[artista_en] and os.path.exists(p))
    os.utime(alguna, None)
    r2 = B.scan()
    assert r2["total"] == r["total"]
    assert r2["reused"] == r["reused"] - 1, "deberia releer justo el que se toco"


def test_a_lost_database_is_rebuilt_from_the_files(cliente):
    """Sin base de datos no hay atajo que valga: se relee todo del disco.

    Es la promesa de la app: las estrellas, el favorito y la letra viven
    dentro del mp3, asi que perder el indice no pierde nada.
    """
    from danplay import config, library as B
    songs = cliente.get("/api/search", params={"limit": 500}).json()["songs"]
    elegida = songs[0]
    cliente.post(f"/api/song/{elegida['id']}/stars", json={"stars": 4})
    cliente.post(f"/api/song/{elegida['id']}/favorite", json={"favorite": True})

    os.remove(config.DATABASE)
    B._prepared.clear()
    B.add_folder(str(config.LIBRARY), "prueba")
    r = B.scan()
    assert r["reused"] == 0, "sin base no hay nada que reaprovechar"

    rehecha = {c["path"]: c for c in
               cliente.get("/api/search", params={"limit": 500}).json()["songs"]}
    assert len(rehecha) == r["total"]
    vuelta = rehecha[elegida["path"]]
    assert vuelta["stars"] == 4, "las estrellas no volvieron del archivo"
    assert vuelta["favorite"], "el favorito no volvio del archivo"
    cliente.post(f"/api/song/{vuelta['id']}/stars", json={"stars": 0})
    cliente.post(f"/api/song/{vuelta['id']}/favorite", json={"favorite": False})


def test_the_cover_cache_notices_when_the_file_changes(cliente):
    """Se recuerda la caratula para no reabrir el mp3, pero sin quedarse vieja.

    La clave lleva la fecha y el tamaño del archivo, asi que incrustar otra
    portada invalida la entrada sola.
    """
    from danplay import tags as T
    original = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    copia = pathlib.Path(original["path"]).with_name("con caratula.mp3")
    shutil.copy(original["path"], copia)
    try:
        uno = b"\xff\xd8\xff\xe0" + b"1" * 400
        otro = b"\xff\xd8\xff\xe0" + b"2" * 900
        assert T.write_cover(str(copia), uno, "image/jpeg")
        assert T.cached_cover(str(copia))[0] == uno
        assert T.cached_cover(str(copia))[0] == uno        # ahora desde la cache
        assert T.write_cover(str(copia), otro, "image/jpeg")
        assert T.cached_cover(str(copia))[0] == otro, "devolvio la caratula vieja"
    finally:
        copia.unlink(missing_ok=True)


def test_the_cover_cache_stays_within_its_memory_budget(cliente):
    """No puede crecer sin freno: en un equipo justo eso se nota."""
    from danplay import tags as T
    original = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    copias = []
    try:
        grande = b"\xff\xd8\xff\xe0" + b"x" * (700 * 1024)
        for i in range(24):                       # 24 x 700 KB = ~16 MB > tope
            c = pathlib.Path(original["path"]).with_name(f"pesada {i}.mp3")
            shutil.copy(original["path"], c)
            copias.append(c)
            T.write_cover(str(c), grande, "image/jpeg")
            T.cached_cover(str(c))
        assert T._cover_bytes <= T._COVER_CACHE_MAX_BYTES, "se paso del tope"
    finally:
        for c in copias:
            c.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# Portadas difuminadas: para las que uno no quiere tener delante. La imagen no
# se toca; solo se marca como «pintala borrosa».

def test_blurring_a_cover_does_not_touch_the_image(cliente):
    """Difuminar es solo como se pinta: el archivo no cambia.

    La portada se pone aqui mismo en vez de buscar una cancion que ya la
    tenga, para que la prueba no dependa de con que musica se ejecute.
    """
    from danplay import tags as T
    elegida = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    imagen = b"\xff\xd8\xff\xe0" + b"portada de prueba" * 20
    assert T.write_cover(elegida["path"], imagen, "image/jpeg")
    antes = T.extract_cover(elegida["path"])
    assert antes and antes[0] == imagen
    try:
        r = cliente.post(f"/api/song/{elegida['id']}/blur", json={"blur": True}).json()
        assert r["blur"] == 1
        despues = T.extract_cover(elegida["path"])
        assert despues[0] == antes[0], "la imagen NO deberia cambiar al difuminarla"
        assert despues[1] == antes[1]
        # y al quitarlo sigue igual
        cliente.post(f"/api/song/{elegida['id']}/blur", json={"blur": False})
        assert T.extract_cover(elegida["path"])[0] == imagen
    finally:
        cliente.post(f"/api/song/{elegida['id']}/blur", json={"blur": False})


def test_blur_survives_a_lost_database(cliente):
    """Como las estrellas: la marca vive dentro del mp3, no en el indice."""
    from danplay import config, library as B
    elegida = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    cliente.post(f"/api/song/{elegida['id']}/blur", json={"blur": True})

    os.remove(config.DATABASE)
    B._prepared.clear()
    B.add_folder(str(config.LIBRARY), "prueba")
    B.scan()

    vuelta = next(x for x in cliente.get("/api/search", params={"limit": 500}).json()["songs"]
                  if x["path"] == elegida["path"])
    assert vuelta["blur"] == 1, "el difuminado no volvio del archivo"
    cliente.post(f"/api/song/{vuelta['id']}/blur", json={"blur": False})


def test_blurring_an_unknown_song_is_a_404(cliente):
    assert cliente.post("/api/song/999999/blur", json={"blur": True}).status_code == 404


def test_the_blur_column_is_added_to_an_older_database(tmp_path):
    """Quien ya tenia su base no deberia notar nada: la columna se añade sola."""
    import sqlite3
    from danplay import config, library as B
    vieja = tmp_path / "vieja.db"
    conn = sqlite3.connect(vieja)
    # una tabla `songs` como la de antes, sin la columna nueva
    sin_blur = [c for c in B.COLUMNS if c != "blur"]
    conn.execute(f"CREATE TABLE songs (id INTEGER PRIMARY KEY, "
                 + ",".join(f"{c} TEXT" for c in sin_blur) + ")")
    conn.execute(f"INSERT INTO songs (path) VALUES ('/x/uno.mp3')")
    conn.commit(); conn.close()

    antes = config.DATABASE
    try:
        config.DATABASE = vieja
        B._prepared.clear()
        c = B.connect()
        columnas = {r[1] for r in c.execute("PRAGMA table_info(songs)")}
        filas = c.execute("SELECT COUNT(*) FROM songs").fetchone()[0]
        c.close()
        assert "blur" in columnas, "no se añadio la columna"
        assert filas == 1, "se perdieron filas al migrar"
    finally:
        config.DATABASE = antes
        B._prepared.clear()


def test_the_duplicate_report_uses_the_keys_the_interface_reads():
    """El informe de duplicados y la interfaz tienen que hablar el mismo idioma.

    Devolvia `sugerida`, `relativa` y `tiene_sufijo` mientras la plantilla leia
    `suggested`, `relative` y `has_suffix`: la insignia de «mejor calidad» no
    salia nunca y la ruta quedaba vacia. El remedo de la prueba del frontend
    tenia las claves en ingles, asi que pasaba en verde contra un contrato que
    no existia. Esto lo comprueba contra el codigo de verdad.
    """
    import inspect
    import re
    from danplay import api as A

    fuente = inspect.getsource(A.duplicates_report)
    claves_grupo = set(re.findall(r'"(\w+)":', fuente))
    for k in ("items", "suggested", "relative", "has_suffix"):
        assert k in claves_grupo, f"el informe ya no devuelve «{k}»"

    vue = (pathlib.Path(__file__).resolve().parent.parent
           / "desktop/src/components/DuplicateGroup.vue").read_text(encoding="utf-8")
    for k in ("suggested", "relative", "has_suffix"):
        assert k in vue, f"la interfaz ya no lee «{k}»"
    # y que no queden nombres del esquema anterior en ninguno de los dos lados
    for viejo in ("sugerida", "relativa", "tiene_sufijo"):
        assert viejo not in fuente, f"quedo «{viejo}» en la API"
        assert viejo not in vue, f"quedo «{viejo}» en la interfaz"


# --------------------------------------------------------------- seguridad
# El transporte: por socket Unix no hace falta nada, pero en Windows la app
# habla por loopback y ahi el token es lo unico que separa a DanPlay de
# cualquier otro programa del equipo.

def test_with_a_token_nothing_passes_without_it(cliente):
    from danplay import api as A
    A._TOKEN = "secreto-de-prueba"
    try:
        assert cliente.get("/api/status").status_code == 401
        assert cliente.get("/api/status", headers={"Authorization": "Bearer otro"}
                           ).status_code == 401
        ok = cliente.get("/api/status", headers={"Authorization": "Bearer secreto-de-prueba"})
        assert ok.status_code == 200
    finally:
        A._TOKEN = ""


def test_in_browser_mode_a_plain_post_is_refused(cliente):
    """Sin la cabecera propia, un POST desde otra pagina no dispara nada.

    CORS impide LEER la respuesta, pero no evita el efecto: un POST sin cuerpo
    a /api/scan desde cualquier pestaña abierta arrancaba un escaneo.
    """
    from danplay import api as A
    A._ENFORCE_HOST = True
    local = {"Host": "localhost"}       # el TestClient manda «testserver»
    try:
        assert cliente.post("/api/scan", headers=local).status_code == 403
        r = cliente.post("/api/scan", headers={**local, "X-DanPlay": "1"})
        assert r.status_code in (200, 409)
        # leer nunca hace nada, asi que no se pide
        assert cliente.get("/api/status", headers=local).status_code == 200
    finally:
        A._ENFORCE_HOST = False


def test_an_unknown_host_is_refused(cliente):
    from danplay import api as A
    A._ENFORCE_HOST = True
    try:
        r = cliente.get("/api/status", headers={"Host": "malo.example"})
        assert r.status_code == 421
    finally:
        A._ENFORCE_HOST = False


def test_editing_only_accepts_the_fields_a_person_can_fix(cliente):
    """Poner estrellas por aqui dejaba el indice y el archivo diciendo cosas
    distintas: las estrellas van al POPM del mp3, no a la base a secas."""
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    assert cliente.patch(f"/api/song/{cid}", json={"stars": 5}).status_code == 422
    assert cliente.patch(f"/api/song/{cid}", json={"cid": 1}).status_code == 422
    assert cliente.patch(f"/api/song/{cid}", json={"title": "Nuevo"}).status_code == 200


def test_stars_out_of_range_are_refused(cliente):
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    assert cliente.post(f"/api/song/{cid}/stars", json={"stars": 9}).status_code == 422
    assert cliente.post(f"/api/song/{cid}/stars", json={"stars": 3}).status_code == 200


def test_resolving_refuses_paths_outside_the_library(cliente, tmp_path):
    fuera = tmp_path / "fuera.mp3"
    fuera.write_bytes(b"x" * 100)
    dentro = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["path"]
    r = cliente.post("/api/duplicates/resolve",
                     json={"keep": dentro, "remove": [str(fuera)], "dry_run": False})
    assert r.status_code == 400
    assert fuera.exists(), "no deberia haber borrado nada de fuera"


def test_a_cover_has_to_be_an_image(cliente, tmp_path):
    """Se podia pasar cualquier archivo del disco y recuperarlo despues
    pidiendo la caratula de esa cancion."""
    cid = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]["id"]
    secreto = tmp_path / "secreto.txt"
    secreto.write_text("una contraseña")
    assert cliente.post(f"/api/song/{cid}/cover", json={"path": str(secreto)}).status_code == 400
    disfrazado = tmp_path / "disfrazado.jpg"
    disfrazado.write_text("tampoco soy una imagen")
    assert cliente.post(f"/api/song/{cid}/cover",
                        json={"path": str(disfrazado)}).status_code == 400


def test_the_audio_says_its_real_format(cliente):
    d = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    r = cliente.get(f"/api/song/{d['id']}/path").json()
    assert r["kind"] == "audio/mpeg"
    from danplay.api import audio_type
    assert audio_type("x.flac") == "audio/flac"
    assert audio_type("x.opus") == "audio/opus"
    assert audio_type("x.m4a") == "audio/mp4"


def test_the_assistant_does_not_delete_on_its_own():
    """Lo que no tiene vuelta atras se pregunta ANTES, aunque lo pida el modelo.

    Por la busqueda web y por los titulos de YouTube entra texto que escribe
    cualquiera; si eso pudiera disparar un borrado, bastaria con una linea
    bien puesta en una pagina.
    """
    from danplay import chat
    llamadas = []

    class _Call:
        def __init__(self, name, args):
            self.id = "1"
            self.function = type("f", (), {"name": name, "arguments": args})()

    class _FakeClient:
        class chat:                                          # noqa: N801
            class completions:
                @staticmethod
                def create(**kw):
                    llamadas.append(kw)
                    if len(llamadas) == 1:
                        msg = type("m", (), {"content": "", "tool_calls":
                                             [_Call("delete_song", '{"id": 1}')]})()
                    else:
                        msg = type("m", (), {"content": "Te lo pregunto antes.",
                                             "tool_calls": None})()
                    return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()

    original_get, original_available = chat.ai._get_client, chat.ai.available
    borradas = []
    original_trash = chat.library.trash
    chat.ai._get_client = lambda: _FakeClient()
    chat.ai.available = lambda: True
    chat.library.trash = lambda cid: borradas.append(cid) or {"ok": True}
    try:
        r = chat.reply([{"role": "user", "text": "borra la 1"}])
    finally:
        chat.ai._get_client, chat.ai.available = original_get, original_available
        chat.library.trash = original_trash

    assert borradas == [], "no puede borrar sin preguntar"
    assert r["confirm"], "deberia devolver que quiere confirmacion"
    assert r["confirm"]["tool"] == "delete_song"
    assert "papelera" in r["confirm"]["summary"]


def test_confirming_is_only_for_the_three_that_need_it():
    from danplay import chat
    assert chat.confirm("search_songs", {})["ok"] is False


def test_the_confirm_endpoint_refuses_anything_else(cliente):
    r = cliente.post("/api/chat/confirm", json={"tool": "search_songs", "args": {}})
    assert r.status_code == 400


# ------------------------------------------------- arrancar una descarga
# La descarga corre en segundo plano y la peticion vuelve enseguida. Aqui se
# comprueba el arranque, con `youtube.download` sustituido: lo que se prueba
# es que se llegue a llamar y que el turno se suelte, no que yt-dlp baje nada.
#
# El fallo que cubre: la API marcaba `STATE["active"]` a mano antes de llamar
# a `run_job`, y `run_job`, al verlo puesto, contestaba «ya hay una descarga
# en marcha» sin bajar nada. El estado se quedaba en marcha para siempre y
# cualquier descarga posterior —del boton o del asistente— recibia un 409.


@pytest.fixture
def descarga_simulada(monkeypatch):
    import time
    from danplay import youtube
    llamadas = []

    def falsa(query, **kw):
        llamadas.append({"query": query, **kw})
        time.sleep(0.02)
        return [{"ok": True, "title": query, "url": "https://youtu.be/x",
                 "forced": bool(kw.get("force"))}]

    monkeypatch.setattr(youtube, "download", falsa)
    monkeypatch.setattr(youtube, "available", lambda: True)
    youtube.STATE["active"] = False
    yield llamadas
    # que un fallo aqui no deje el turno cogido para las pruebas siguientes
    youtube.STATE["active"] = False


def _esperar_a_que_termine(timeout=3.0):
    import time
    from danplay import youtube
    limite = time.time() + timeout
    while youtube.STATE["active"] and time.time() < limite:
        time.sleep(0.02)
    assert not youtube.STATE["active"], "la descarga no ha soltado el turno"


def test_el_boton_de_descargas_llega_a_descargar_y_suelta_el_turno(cliente, descarga_simulada):
    from danplay import youtube
    r = cliente.post("/api/youtube/download", json={"query": "barak mi gozo", "results": 3})
    assert r.status_code == 200, r.text
    _esperar_a_que_termine()
    assert len(descarga_simulada) == 1, "download() no se llego a llamar"
    assert descarga_simulada[0]["results"] == 3
    assert youtube.STATE["phase"] == "done"
    assert youtube.STATE["results"] and youtube.STATE["results"][0]["ok"]
    # y la siguiente no recibe un 409 por un turno que nadie solto
    r = cliente.post("/api/youtube/download", json={"query": "otra"})
    assert r.status_code == 200, r.text
    _esperar_a_que_termine()
    assert len(descarga_simulada) == 2


def test_dos_descargas_a_la_vez_no_caben(cliente, descarga_simulada):
    from danplay import youtube
    assert youtube.claim(), "el turno deberia estar libre"
    try:
        r = cliente.post("/api/youtube/download", json={"query": "x"})
        assert r.status_code == 409
        r = cliente.post("/api/chat/confirm",
                         json={"tool": "download_music", "args": {"items": ["x"]}})
        assert r.status_code == 409
    finally:
        youtube.release()
    assert not descarga_simulada, "no deberia haber descargado nada"


def test_confirmar_una_descarga_del_asistente_arranca_con_sus_argumentos(cliente, descarga_simulada):
    """Los argumentos son los de la herramienta: `items` y `force`, no `query`.

    Con `query` la confirmacion acababa SIEMPRE en «400: hace falta algo que
    descargar», aunque la persona acabara de aceptar.
    """
    from danplay import youtube
    r = cliente.post("/api/chat/confirm", json={
        "tool": "download_music",
        "args": {"items": ["I Want Jesus Bethel", "Ruja o Leao Carol Braga"],
                 "force": True}})
    assert r.status_code == 200, r.text
    d = r.json()
    assert d["ok"] and d["result"]["active"]
    assert d["result"]["items"] == ["I Want Jesus Bethel", "Ruja o Leao Carol Braga"]
    assert d["result"]["force"] is True
    _esperar_a_que_termine()
    assert [c["query"] for c in descarga_simulada] == ["I Want Jesus Bethel",
                                                       "Ruja o Leao Carol Braga"]
    for c in descarga_simulada:
        assert c["force"] is True, "el «bajala igual» tiene que llegar a la descarga"
        assert c["results"] == 1, "del chat se coge el primer resultado, no cinco"
        assert c["source"] == "assistant"
    # el resultado queda donde lo mira el chat y la pagina de Descargas
    assert len(youtube.STATE["results"]) == 2
    assert all(x["forced"] for x in youtube.STATE["results"])


def test_confirmar_sin_nada_que_bajar_es_un_400_claro(cliente, descarga_simulada):
    r = cliente.post("/api/chat/confirm",
                     json={"tool": "download_music", "args": {"items": []}})
    assert r.status_code == 400
    assert "descargar" in r.json()["detail"]
    assert not descarga_simulada


def test_confirmar_acepta_tambien_query_suelto(cliente, descarga_simulada):
    """Por si el modelo manda `query` en vez de `items`: se baja igual."""
    r = cliente.post("/api/chat/confirm",
                     json={"tool": "download_music", "args": {"query": "algo"}})
    assert r.status_code == 200, r.text
    _esperar_a_que_termine()
    assert [c["query"] for c in descarga_simulada] == ["algo"]


def test_el_plan_de_descarga_sanea_los_argumentos():
    from danplay import chat, config
    plan = chat.download_plan({"items": [" a ", "A", "", "b"], "quality": "rara",
                               "file_it": False, "force": "si"})
    assert plan["items"] == ["a", "b"], "sin vacios ni repetidos"
    assert plan["quality"] == config.MP3_QUALITY, "una calidad desconocida cae a la de la app"
    assert plan["file_it"] is False and plan["force"] is True
    largo = chat.download_plan({"items": [str(i) for i in range(chat.MAX_DOWNLOADS + 5)]})
    assert len(largo["items"]) == chat.MAX_DOWNLOADS and largo["trimmed"]
    assert chat.download_plan({})["items"] == []
    assert chat.download_plan({"items": "solo una"})["items"] == ["solo una"]


def test_el_dialogo_de_confirmacion_dice_que_se_va_a_bajar():
    from danplay import chat
    texto = chat._describe("download_music", {"items": ["Mi Gozo", "Shekinah"]})
    assert "«Mi Gozo»" in texto and "«Shekinah»" in texto
    assert "otra version" not in texto
    con_force = chat._describe("download_music", {"items": ["Mi Gozo"], "force": True})
    assert "otra version" in con_force, "hay que avisar de que se guarda repetida"


# ------------------------------------------- ids inventados y repertorios
# Paso de verdad: el modelo creo «Herlin» con los ids 102, 103 y 104 (otras
# canciones), la «corrigio» con 123, 456 y 789 (uno era Kabed y dos no
# existian: la lista decia «6 temas» con cuatro), y pidio borrar el
# repertorio id 1 —«domingo»— creyendo que era «Herlin».


def _ids(cliente, n=3):
    from danplay import library
    return [c["id"] for c in library.search("", limit=n)]


def _limpiar_listas(*nombres):
    from danplay import playlists
    for l in playlists.list_all():
        if l["name"] in nombres:
            playlists.remove(l["id"])


def test_un_id_inventado_no_entra_en_ninguna_lista(cliente):
    from danplay import chat, playlists
    _limpiar_listas("Inventada")
    ids = _ids(cliente, 2)
    r = chat.run_tool("create_playlist", {"name": "Inventada", "ids": ids + [999999]})
    assert "error" in r and "999999" in r["error"]
    assert "search_songs" in r["error"], "tiene que decirle de donde salen los ids"
    assert not any(l["name"] == "Inventada" for l in playlists.list_all()), \
        "con un id falso no se crea nada: los demas tampoco son de fiar"
    # y la base tampoco admite huerfanos aunque se cuele por otro camino
    made = playlists.create("Inventada")
    assert playlists.add(made["id"], [999999, 888888]) == 0
    assert playlists.by_id(made["id"])["n"] == 0
    _limpiar_listas("Inventada")


def test_crear_una_lista_devuelve_lo_que_entro_de_verdad(cliente):
    from danplay import chat
    _limpiar_listas("Con nombres")
    ids = _ids(cliente, 2)
    r = chat.run_tool("create_playlist", {"name": "Con nombres", "ids": ids})
    assert r["added"] == 2 and r["created"] is True
    assert [c["id"] for c in r["songs"]] == ids, "cuenta que canciones son, no solo cuantas"
    assert all(c["title"] for c in r["songs"])
    # la ficha del chat enseña los nombres: asi se ve si metio lo que no era
    resumen = chat._summarize("create_playlist", r)
    assert r["songs"][0]["title"] in resumen
    _limpiar_listas("Con nombres")


def test_los_repertorios_se_nombran_por_su_nombre(cliente):
    from danplay import chat, playlists
    _limpiar_listas("Herlin", "domingo")
    a = playlists.create("domingo")["id"]
    b = playlists.create("Herlin")["id"]
    ids = _ids(cliente, 3)
    playlists.add(a, ids[:2])
    # ver: por nombre, sin distinguir mayusculas ni tildes
    r = chat.run_tool("playlist_songs", {"name": "herlín"})
    assert r["playlist_id"] == b and r["songs"] == []
    # borrar por nombre borra ESA, no la del id que el modelo se imagine
    assert chat._describe("delete_playlist", {"name": "Herlin"}).startswith("Borrar el repertorio «Herlin»")
    r = chat.run_tool("delete_playlist", {"name": "Herlin", "id": a})
    assert r["ok"] and r["name"] == "Herlin"
    assert playlists.by_id(a)["name"] == "domingo", "«domingo» sigue ahi"
    # un nombre que no existe: error con la lista de las que hay
    r = chat.run_tool("playlist_songs", {"name": "No existe"})
    assert "error" in r and "«domingo»" in r["error"] and "No inventes ids" in r["error"]
    _limpiar_listas("domingo")


def test_corregir_una_lista_la_deja_exactamente_como_se_pide(cliente):
    from danplay import chat, playlists
    _limpiar_listas("Arreglame")
    ids = _ids(cliente, 4)
    lid = playlists.create("Arreglame")["id"]
    playlists.add(lid, [ids[0], ids[1]])          # dos que no van
    r = chat.run_tool("set_playlist_songs", {"name": "Arreglame", "ids": [ids[2], ids[3], ids[1]]})
    assert "error" not in r, r
    assert r["removed"] == 1 and r["added"] == 2 and r["total"] == 3
    assert [c["id"] for c in playlists.songs(lid)] == [ids[2], ids[3], ids[1]], "en ese orden"
    # con un id inventado no se toca nada
    r = chat.run_tool("set_playlist_songs", {"name": "Arreglame", "ids": [ids[0], 424242]})
    assert "error" in r
    assert [c["id"] for c in playlists.songs(lid)] == [ids[2], ids[3], ids[1]]
    _limpiar_listas("Arreglame")


def test_quitar_y_renombrar(cliente):
    from danplay import chat, playlists
    _limpiar_listas("Vieja", "Nueva")
    ids = _ids(cliente, 3)
    lid = playlists.create("Vieja")["id"]
    playlists.add(lid, ids)
    r = chat.run_tool("remove_from_playlist", {"name": "Vieja", "song_ids": [ids[0], ids[2]]})
    assert r["removed"] == 2 and [c["id"] for c in r["songs"]] == [ids[1]]
    r = chat.run_tool("rename_playlist", {"name": "Vieja", "new_name": "Nueva", "note": "para el domingo"})
    assert r["ok"] and r["name"] == "Nueva" and r["was"] == "Vieja"
    assert playlists.by_name("nueva")["note"] == "para el domingo"
    assert chat.run_tool("rename_playlist", {"name": "Nueva"})["error"]
    _limpiar_listas("Nueva")


def test_la_revision_sube_con_cada_cambio_que_se_ensena(cliente):
    """Es lo que Rust vigila para avisar a las ventanas (`danplay://changed`)."""
    from danplay import library, playlists
    antes = cliente.get("/api/status").json()["revision"]
    made = playlists.create("Revision")
    r1 = library.revision()
    assert r1 > antes, "crear una lista cuenta"
    playlists.add(made["id"], _ids(cliente, 1))
    assert library.revision() > r1, "añadir a una lista cuenta"
    r2 = library.revision()
    playlists.rate(_ids(cliente, 1)[0], 3)
    assert library.revision() > r2, "puntuar cuenta"
    r3 = library.revision()
    playlists.remove(made["id"])
    assert library.revision() > r3, "borrar la lista cuenta"
    assert cliente.get("/api/status").json()["revision"] == library.revision()


# ------------------------------------------------ narrar no es hacer
# Paso de verdad: el modelo dijo «Confirmo descarga», «Descarga pedida» y
# «Ya la cree» en tres turnos seguidos sin llamar a una sola herramienta. La
# persona no vio ni el dialogo ni la lista. Tres defensas: el estado real en
# cada turno, las marcas en el historial, y el reintento obligado.


class _Turnos:
    """Un cliente de IA de mentira que contesta por turnos y guarda lo que recibe.

    `judge` es lo que contesta cuando se le pregunta como clasificador (la
    llamada de una palabra de `_judge_claims`): esa no gasta turnos.
    """

    def __init__(self, respuestas, judge="NO"):
        self.respuestas = list(respuestas)
        self.recibido = []
        self.judge = judge
        self.judged = []

        class _Call:
            def __init__(self, name, args):
                self.id = "1"
                self.function = type("f", (), {"name": name, "arguments": args})()

        outer = self

        class chat:                                          # noqa: N801
            class completions:
                @staticmethod
                def create(**kw):
                    if kw.get("max_tokens") == 3:            # el juez
                        outer.judged.append(kw["messages"][-1]["content"])
                        msg = type("m", (), {"content": outer.judge, "tool_calls": None})()
                        return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()
                    # copia: el historial es la misma lista y sigue creciendo
                    outer.recibido.append({**kw, "messages": [dict(m) for m in kw["messages"]]})
                    r = outer.respuestas.pop(0) if outer.respuestas else ("Fin.", None)
                    text, calls = r
                    tool_calls = [_Call(n, a) for n, a in calls] if calls else None
                    msg = type("m", (), {"content": text, "tool_calls": tool_calls})()
                    return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()
        self.chat = chat


def _con_cliente(fake, fn):
    from danplay import chat
    original_get, original_available = chat.ai._get_client, chat.ai.available
    chat.ai._get_client = lambda: fake
    chat.ai.available = lambda: True
    try:
        return fn()
    finally:
        chat.ai._get_client, chat.ai.available = original_get, original_available


def test_el_detector_de_narracion_reconoce_lo_que_paso():
    from danplay import chat
    for t in ("Ya la creé: lista **Herlin** con las tres.", "Descarga pedida. La app te avisará.",
              "Confirmo descarga.", "Voy a descargar las tres canciones",
              "Las tres canciones ya están en tu biblioteca.", "Ahora sí está en tu repertorio.",
              "Aquí está tu lista **Herlin**",
              # las que se colaron la segunda vez
              "Descargando: 🎵 **\"QUE SE ABRÁ EL CIELO\"**. La app te avisará cuando esté.",
              "Ya está descargada: (id: 278). Añadida a la lista **\"Herlin\"**. Ahora tiene 5 canciones.",
              "Lista **Herlin** actualizada con las correctas.", "Añadidas las dos a «domingo».",
              "Listo: quité Kabed de la lista.", "Hecho. Ya suena Mi Gozo.",
              "Bajando la de Barak, te aviso cuando termine."):
        assert chat.claims_action(t), t
    for t in ("Miles Davis grabó Kind of Blue en 1959.", "Tienes 260 canciones.",
              "¿Quieres que la ponga a sonar?", "Te pido permiso para descargar «Mi Gozo».",
              "Eso se sale de lo mío: solo llevo temas de música.",
              "¿Quieres descargar este ritmo y añadirlo a la lista «Herlin»? Sí = lo bajo. No = lo dejo.",
              "El tono de Mi Gozo es Bb, aproximado."):
        assert not chat.claims_action(t), t


def test_una_marca_imitada_se_borra_y_cuenta_como_mentira(cliente):
    """Paso: el modelo escribio «[herramientas que usaste en este mensaje:
    download_music (1 item)]» el solo, sin llamar a nada."""
    from danplay import chat
    fake = _Turnos([
        ("Descargando. [herramientas que usaste en este mensaje: download_music (1 item)]", None),
        ("", [("library_summary", "{}")]),
        ("Tienes canciones.", None),
    ])
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "bajala"}]))
    assert fake.recibido[1]["tool_choice"] == "required", "se le obliga a usar herramientas"
    assert r["text"] == "Tienes canciones."
    assert "[herramientas" not in "".join(m["content"] for m in fake.recibido[1]["messages"]
                                        if m["role"] == "assistant"), \
        "la marca imitada no vuelve a entrar en el historial"
    assert chat.strip_markers("x [Nota de la app: y] z") == "x z"
    assert chat.has_markers("[herramientas que usaste en este mensaje: x]")


def test_si_las_frases_no_saltan_decide_el_juez(cliente):
    from danplay import chat
    # una narracion con palabras que la lista no conoce; el juez dice que SI
    fake = _Turnos([("Todo en orden con tu repertorio, quedó como pediste.", None),
                    ("", [("library_summary", "{}")]),
                    ("Vale.", None)], judge="SI")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "arregla la lista"}]))
    assert fake.judged, "se le pregunto al juez"
    assert fake.recibido[1]["tool_choice"] == "required"
    assert r["text"] == "Vale."
    # y con el juez diciendo NO, la respuesta se queda como esta
    fake = _Turnos([("Todo en orden con tu repertorio, quedó como pediste.", None)], judge="NO")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "arregla la lista"}]))
    assert r["text"].startswith("Todo en orden") and len(fake.recibido) == 1


def test_un_si_a_una_pregunta_suya_obliga_a_usar_herramientas(cliente):
    """«¿la bajo?» — «si mejor si descargala» — «Descargando…» sin llamar a
    nada. Ahora la primera vuelta de ese turno va obligada."""
    from danplay import chat
    fake = _Turnos([("", [("library_summary", "{}")]), ("Hecho.", None)])
    _con_cliente(fake, lambda: chat.reply([
        {"role": "user", "text": "baja esa"},
        {"role": "ai", "text": "¿Quieres que la descargue y la añada a «Herlin»?"},
        {"role": "user", "text": "si mejor si descargala"}]))
    assert fake.recibido[0]["tool_choice"] == "required"
    assert fake.recibido[1]["tool_choice"] == "auto"
    # la pregunta suya puede tener un mensaje de la app entre medias
    assert chat._answers_an_offer([
        {"role": "ai", "text": "¿Quieres descargar este ritmo y añadirlo a «Herlin»?"},
        {"role": "ai", "text": "Cancelado, no he tocado nada.", "app": True},
        {"role": "user", "text": "si mejor si descargala"}])
    # un «no» no obliga a nada; una pregunta de conocimiento tampoco
    assert not chat._answers_an_offer([{"role": "ai", "text": "¿La bajo?"},
                                       {"role": "user", "text": "no, dejala"}])
    assert not chat._answers_an_offer([{"role": "ai", "text": "Kind of Blue es de 1959."},
                                       {"role": "user", "text": "si"}])
    assert not chat._answers_an_offer([{"role": "ai", "text": "¿La bajo?"},
                                       {"role": "user", "text": "¿de qué año es?"}])


def test_las_notas_del_historial_son_de_sistema_no_texto_suyo(cliente):
    from danplay import chat
    fake = _Turnos([("ok", None)])
    _con_cliente(fake, lambda: chat.reply([
        {"role": "user", "text": "crea la lista"},
        {"role": "ai", "text": "Lista creada. [herramientas que usaste en este mensaje: create_playlist]",
         "tools": [{"name": "create_playlist", "summary": "lista «X» con 3 temas"}]},
        {"role": "user", "text": "gracias"}]))
    msgs = fake.recibido[0]["messages"]
    assistant = [m for m in msgs if m["role"] == "assistant"]
    assert assistant[0]["content"] == "Lista creada.", "sin marcas dentro de su texto"
    i = msgs.index(assistant[0])
    assert msgs[i + 1]["role"] == "system"
    assert "create_playlist (lista «X» con 3 temas)" in msgs[i + 1]["content"]


def test_si_dice_que_hizo_algo_sin_herramientas_se_le_obliga_a_hacerlo(cliente):
    """Primera vuelta: «Ya la cree» sin llamar a nada. Se le para y en la
    siguiente TIENE que usar herramientas; la lista se crea de verdad."""
    from danplay import chat, library, playlists
    cid = library.search("", limit=1)[0]["id"]
    fake = _Turnos([
        ("Ya la creé: lista **Prueba narrada** con tus canciones.", None),
        ("", [("create_playlist", f'{{"name": "Prueba narrada", "ids": [{cid}]}}')]),
        ("Ahora sí: lista «Prueba narrada» creada con 1 tema.", None),
    ])
    try:
        r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "creame la lista"}]))
        assert [h["name"] for h in r["tools"]] == ["create_playlist"]
        assert any(l["name"] == "Prueba narrada" for l in playlists.list_all())
        assert "Ahora sí" in r["text"]
        # la segunda peticion al modelo iba obligada a usar herramientas y con el toque
        assert fake.recibido[1]["tool_choice"] == "required"
        assert fake.recibido[1]["messages"][-1]["role"] == "system"
        assert "ninguna herramienta ha hecho nada" in fake.recibido[1]["messages"][-1]["content"]
        # y la tercera vuelve a ser libre
        assert fake.recibido[2]["tool_choice"] == "auto"
    finally:
        for l in playlists.list_all():
            if l["name"] == "Prueba narrada":
                playlists.remove(l["id"])


def test_solo_se_le_para_una_vez_y_solo_si_no_uso_nada(cliente):
    from danplay import chat
    # tras consultar, decir «ya la tienes» es un dato, no una accion: vale
    fake = _Turnos([("", [("library_summary", "{}")]),
                    ("Ya la tienes en tu biblioteca.", None)], judge="NO")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "¿la tengo?"}]))
    assert r["text"] == "Ya la tienes en tu biblioteca."
    assert len(fake.recibido) == 2
    # pero consultar y luego decir «añadida» sin añadir, no: lo pilla el juez
    fake = _Turnos([("", [("search_songs", '{"query": "gozo"}')]),
                    ("Añadida a la lista Herlin.", None),
                    ("", [("library_summary", "{}")]),
                    ("Vale.", None)], judge="SI")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "añadela a Herlin"}]))
    assert fake.recibido[2]["tool_choice"] == "required"
    assert r["text"] == "Vale."
    # y con una herramienta que HACE algo, el texto vale aunque suene a accion
    fake = _Turnos([("", [("set_stars", '{"id": 1, "stars": 4}')]),
                    ("Puntuada con 4 estrellas.", None)], judge="SI")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "ponle 4"}]))
    assert r["text"] == "Puntuada con 4 estrellas." and len(fake.recibido) == 2
    # y si tras el toque sigue narrando, no se insiste (no es un bucle), pero
    # la respuesta sale señalada
    fake = _Turnos([("Ya la creé.", None), ("Ya la creé, de verdad.", None)])
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "crea la lista"}]))
    assert r["text"].startswith("Ya la creé, de verdad.") and r["narrated"] is True
    assert len(fake.recibido) == 2


def test_una_respuesta_normal_no_se_toca(cliente):
    from danplay import chat
    fake = _Turnos([("Kind of Blue es de 1959.", None)])
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "¿de qué año es?"}]))
    assert r["text"] == "Kind of Blue es de 1959."
    assert len(fake.recibido) == 1


def test_cada_turno_lleva_el_estado_real_de_la_app(cliente):
    from danplay import chat, playlists
    made = playlists.create("Estado real")
    try:
        fake = _Turnos([("ok", None)])
        _con_cliente(fake, lambda: chat.reply([
            {"role": "user", "text": "hola"},
            {"role": "ai", "text": "Ya la creé."},         # sin herramientas
            {"role": "user", "text": "no la veo"}]))
        msgs = fake.recibido[0]["messages"]
        nota = msgs[-1]
        assert nota["role"] == "system" and nota is not msgs[0]
        assert "ESTADO REAL" in nota["content"]
        assert "«Estado real»" in nota["content"]
        assert "Descarga en marcha: no" in nota["content"]
        assert "no uso ninguna herramienta" in nota["content"]
        # el mensaje narrado va señalado en el historial, en una nota de sistema
        narrado = next(m for m in msgs if m["role"] == "assistant")
        assert narrado["content"] == "Ya la creé."
        nota = msgs[msgs.index(narrado) + 1]
        assert nota["role"] == "system" and "NO uso ninguna herramienta" in nota["content"]
    finally:
        playlists.remove(made["id"])


def test_los_mensajes_con_herramientas_van_marcados_con_lo_que_hicieron(cliente):
    from danplay import chat
    fake = _Turnos([("ok", None)])
    _con_cliente(fake, lambda: chat.reply([
        {"role": "user", "text": "crea la lista"},
        {"role": "ai", "text": "Lista creada.",
         "tools": [{"name": "create_playlist", "summary": "lista «X» con 3 temas"}]},
        {"role": "user", "text": "gracias"}]))
    msgs = fake.recibido[0]["messages"]
    hecho = next(m for m in msgs if m["role"] == "assistant")
    nota = msgs[msgs.index(hecho) + 1]
    assert nota["role"] == "system"
    assert "create_playlist (lista «X» con 3 temas)" in nota["content"]
    assert "no uso ninguna herramienta" not in msgs[-1]["content"], \
        "el ultimo mensaje del asistente SI uso herramientas"


def test_una_herramienta_que_falla_no_cuenta_como_hecho(cliente):
    """add_to_playlist con un id inventado devuelve error; si luego escribe
    «Añadida a Herlin», eso sigue siendo narracion y se le para."""
    from danplay import chat, playlists
    _limpiar_listas("Fallida")
    playlists.create("Fallida")
    fake = _Turnos([("", [("add_to_playlist", '{"name": "Fallida", "ids": [999999]}')]),
                    ("Añadida a Fallida. Ahora tiene 1 cancion.", None),
                    ("", [("playlist_songs", '{"name": "Fallida"}')]),
                    ("No, no la he podido añadir: ese id no existe.", None)], judge="SI")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "añade la 999999 a Fallida"}]))
    assert fake.recibido[2]["tool_choice"] == "required", "tras el error hay que pararle"
    assert r["text"].startswith("No, no la he podido")
    assert r["tools"][0]["summary"].startswith("error:")
    _limpiar_listas("Fallida")


def test_el_aviso_de_fin_de_descarga_no_dispara_el_detector(cliente):
    """El aviso de la app pide «responde solo: Terminado» y el modelo puede
    decir «Ya estan». Eso no es narrar: la descarga ocurrio, y el juez lo
    sabe. La primera vuelta va obligada a herramientas (el remate)."""
    from danplay import chat
    fake = _Turnos([("", [("list_playlists", "{}")]), ("Ya están en tu biblioteca.", None)], judge="NO")
    r = _con_cliente(fake, lambda: chat.reply([
        {"role": "user", "text": "baja estas dos"},
        {"role": "ai", "text": "Descargando 2 temas.", "app": True,
         "tools": [{"name": "download_music", "summary": "aceptada, en marcha"}]},
        {"role": "ai", "text": "**Descargadas (2):** …", "app": True,
         "tools": [{"name": "download_music", "summary": "2 descargadas"}]},
        {"role": "user", "text": "[aviso de la app] La descarga ha terminado…", "event": "download_done"}]))
    assert fake.recibido[0]["tool_choice"] == "required"
    assert r["text"] == "Ya están en tu biblioteca."
    assert "narrated" not in r
    # al juez se le dijo que la descarga ya ocurrio; las frases no se miran aqui
    assert fake.judged and "YA ocurrio" in fake.judged[0]
    # y el estado real no le dice que su ultimo mensaje no hizo nada (era de la app)
    nota = fake.recibido[0]["messages"][-1]["content"]
    assert "no uso ninguna herramienta" not in nota


def test_la_segunda_confirmacion_del_turno_se_rechaza_con_claridad(cliente):
    from danplay import chat, playlists
    _limpiar_listas("Una", "Otra")
    a = playlists.create("Una")["id"]; b = playlists.create("Otra")["id"]
    fake = _Turnos([("", [("delete_playlist", f'{{"id": {a}}}'), ("delete_playlist", f'{{"id": {b}}}')]),
                    ("Te he pedido confirmacion para borrar «Una»; «Otra» te la pido despues.", None)])
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "borra Una y Otra"}]))
    assert r["confirm"]["args"]["id"] == a
    assert r["tools"][0]["summary"] == "espera tu visto bueno"
    assert r["tools"][1]["summary"].startswith("error:"), "la segunda NO se pidio y se dice"
    _limpiar_listas("Una", "Otra")


def test_si_sigue_narrando_tras_el_toque_se_ve(cliente):
    from danplay import chat
    fake = _Turnos([("Ya la creé.", None), ("", [("library_summary", "{}")]), ("Ya la creé, de verdad.", None)],
                   judge="SI")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "crea la lista"}]))
    assert r["narrated"] is True
    assert "Nota de la app" in r["text"]
    assert len(fake.recibido) == 3, "no es un bucle: se le para una vez"


def test_el_juez_no_se_molesta_por_conocimiento_musical(cliente):
    from danplay import chat
    fake = _Turnos([("Kind of Blue es de 1959 y lo grabo Miles Davis.", None)], judge="SI")
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "¿de que año es?"}]))
    assert r["text"].startswith("Kind of Blue") and not fake.judged
    # con palabras de la app si se le pregunta, y ve la peticion del usuario
    fake = _Turnos([("Puedo crear la lista con esas tres si quieres.", None)], judge="NO")
    _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "haz una lista"}]))
    assert fake.judged and "haz una lista" in fake.judged[0]


def test_tras_la_descarga_el_remate_va_obligado_y_la_lista_se_hace_de_verdad(cliente):
    """Fue aqui donde «Añadida a la lista (id: 271)» paso sin comprobar: el
    turno del aviso de fin de descarga daba todo por hecho."""
    from danplay import chat, playlists, library
    _limpiar_listas("Remate")
    playlists.create("Remate")
    cid = library.search("", limit=1)[0]["id"]
    fake = _Turnos([("", [("add_to_playlist", f'{{"name": "Remate", "ids": [{cid}]}}')]),
                    ("Añadida a «Remate».", None)], judge="NO")
    r = _con_cliente(fake, lambda: chat.reply([
        {"role": "user", "text": "descarga esta y metela en Remate"},
        {"role": "ai", "text": "**Descargada:** …", "app": True,
         "tools": [{"name": "download_music", "summary": "1 descargada"}]},
        {"role": "user", "text": f"[aviso de la app] … pediste «x» → entro como «y» (id {cid}) …",
         "event": "download_done"}]))
    assert fake.recibido[0]["tool_choice"] == "required", "el remate va obligado a usar herramientas"
    assert [c["id"] for c in playlists.songs(playlists.by_name("Remate")["id"])] == [cid]
    assert "narrated" not in r
    # y si en ese turno solo narra («Añadida») tras consultar, el juez lo pilla
    fake = _Turnos([("", [("playlist_songs", '{"name": "Remate"}')]),
                    ("Añadida a «Remate», ahora tiene 2.", None),
                    ("", [("library_summary", "{}")]), ("Nada que añadir.", None)], judge="SI")
    _con_cliente(fake, lambda: chat.reply([
        {"role": "user", "text": "[aviso de la app] …", "event": "download_done"}]))
    assert fake.judged and "YA ocurrio" in fake.judged[0]
    assert fake.recibido[2]["tool_choice"] == "required"
    _limpiar_listas("Remate")


def test_un_si_a_algo_que_la_app_ya_hizo_no_obliga_a_repetirlo():
    from danplay import chat
    assert not chat._answers_an_offer([
        {"role": "ai", "text": "¿Confirmas que mande «X» a la papelera?"},
        {"role": "ai", "text": "Listo, esta en la papelera del sistema.", "app": True,
         "tools": [{"name": "delete_song", "summary": "hecho"}]},
        {"role": "user", "text": "si"}])


def test_el_estado_real_cuenta_las_ultimas_descargas_con_su_id(cliente):
    from danplay import chat, library
    cid = library.search("", limit=1)[0]["id"]
    library.clear_download_history()
    library.log_download({"source": "assistant", "query": "https://youtu.be/x", "ok": True,
                          "title": "QUE SE ABRÁ EL CIELO - ISH MELTON DRUM CAM",
                          "artist": "Ish Melton", "song": "Que Se Abra El Cielo (Drum Cam)",
                          "song_id": cid})
    nota = chat._context_note([{"role": "user", "text": "hola"}])
    assert "ISH MELTON DRUM CAM" in nota and f"id {cid}" in nota
    assert "No la vuelvas a bajar" in nota
    library.clear_download_history()


def test_el_dialogo_avisa_si_esa_direccion_se_bajo_hace_poco(cliente):
    from danplay import chat, library
    cid = library.search("", limit=1)[0]["id"]
    library.clear_download_history()
    library.log_download({"source": "assistant", "query": "https://youtu.be/x", "ok": True,
                          "title": "x", "song_id": cid})
    texto = chat._describe("download_music", {"items": ["https://youtu.be/x"], "force": True})
    assert "OJO" in texto and "se bajo hace 0 min" in texto and "duplica" in texto
    assert "OJO" not in chat._describe("download_music", {"items": ["https://youtu.be/otra"]})
    library.clear_download_history()


def test_el_prompt_deja_las_confirmaciones_a_la_app():
    from danplay import chat
    s = chat.SYSTEM_PROMPT
    assert "QUIEN PREGUNTA ES LA APP, NO TU" in s
    assert "sin pedir permiso" in s
    assert "NO el titulo de YouTube" in s


def test_lo_que_devolvio_cada_herramienta_viaja_al_turno_siguiente(cliente):
    """«Esa», «la segunda», «la que te dije»: sin los ids y nombres de lo que
    enseño antes, el modelo no podia entenderlo y volvia a preguntar o a
    inventar. Ahora cada herramienta deja un `detail` y el historial lo lleva."""
    from danplay import chat, library
    songs = library.search("", limit=2)
    fake = _Turnos([("", [("search_songs", '{"query": "", "limit": 2}')]), ("Tienes dos.", None)])
    r = _con_cliente(fake, lambda: chat.reply([{"role": "user", "text": "¿que tengo?"}]))
    detail = r["tools"][0]["detail"]
    assert f"id {songs[0]['id']}" in detail and songs[0]["title"] in detail
    # y al turno siguiente, la nota de sistema lo trae
    fake = _Turnos([("La segunda es esa.", None)])
    _con_cliente(fake, lambda: chat.reply([
        {"role": "user", "text": "¿que tengo?"},
        {"role": "ai", "text": "Tienes dos.", "tools": r["tools"]},
        {"role": "user", "text": "pon la segunda"}]))
    notas = [m["content"] for m in fake.recibido[0]["messages"] if m["role"] == "system"]
    assert any(f"id {songs[1]['id']}" in n and songs[1]["title"] in n for n in notas), notas


def test_la_ventana_de_historial_empieza_en_el_usuario_y_no_se_come_la_peticion():
    from danplay import chat
    msgs = [{"role": "user", "text": "descarga esta y metela en Herlin"}]
    for _ in range(10):
        msgs += [{"role": "ai", "text": "Descargando.", "app": True},
                 {"role": "ai", "text": "**Descargada:** …", "app": True},
                 {"role": "user", "text": "[aviso de la app] …", "hidden": True, "event": "download_done"},
                 {"role": "ai", "text": "Terminado."}]
    w = chat._window(msgs)
    assert w[0]["role"] == "user" and len(w) <= chat.HISTORY_MESSAGES
    assert len(w) > 24, "la ventana de antes se quedaba corta con los mensajes de la app"
    # el ultimo mensaje siempre entra, aunque sea enorme
    w = chat._window([{"role": "user", "text": "x" * 50_000}])
    assert len(w) == 1


def test_brief_resume_con_ids_y_nombres():
    from danplay import chat
    assert chat._brief("search_songs", {"songs": [{"id": 12, "artist": "Barak", "title": "Mi Gozo"}]}) \
        == "id 12 «Barak - Mi Gozo»"
    assert chat._brief("playlist_songs", {"playlist_id": 2, "name": "Herlin", "songs": []}) == "«Herlin» (id 2): vacia"
    assert chat._brief("list_playlists", {"playlists": [{"id": 1, "name": "domingo", "items": 2}]}) \
        == "«domingo» (id 1, 2 temas)"
    assert chat._brief("search_songs", {"error": "no"}) == ""
    largo = {"songs": [{"id": i, "artist": "A", "title": f"T{i}"} for i in range(15)]}
    assert chat._brief("search_songs", largo).endswith("y 5 mas")


def test_una_descarga_pendiente_no_se_resume_como_cero_descargadas():
    """La ficha del chat decia «descargo · 0 descargada(s)» cuando en realidad
    estaba esperando el visto bueno. Confundia: parecia que habia fallado."""
    from danplay import chat
    llamadas = []

    class _Call:
        def __init__(self, name, args):
            self.id = "1"
            self.function = type("f", (), {"name": name, "arguments": args})()

    class _FakeClient:
        class chat:                                          # noqa: N801
            class completions:
                @staticmethod
                def create(**kw):
                    llamadas.append(kw)
                    if len(llamadas) == 1:
                        msg = type("m", (), {"content": "", "tool_calls": [
                            _Call("download_music",
                                  '{"items": ["Ruja o Leao Carol Braga"], "force": true}')]})()
                    else:
                        msg = type("m", (), {"content": "Te lo pido.", "tool_calls": None})()
                    return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()

    original_get, original_available = chat.ai._get_client, chat.ai.available
    chat.ai._get_client = lambda: _FakeClient()
    chat.ai.available = lambda: True
    try:
        r = chat.reply([{"role": "user", "text": "bajala igual"}])
    finally:
        chat.ai._get_client, chat.ai.available = original_get, original_available
    assert r["confirm"]["tool"] == "download_music"
    assert r["confirm"]["args"]["items"] == ["Ruja o Leao Carol Braga"]
    assert r["confirm"]["args"]["force"] is True
    assert r["tools"][0]["summary"] == "espera tu visto bueno"
    assert "0 descargada" not in r["tools"][0]["summary"]


def test_renombrar_una_lista_por_la_api(cliente):
    from danplay import playlists
    for l in playlists.list_all():
        if l["name"] in ("Vieja", "Nueva", "Otra"):
            playlists.remove(l["id"])
    lid = playlists.create("Vieja")["id"]
    other = playlists.create("Otra")["id"]
    r = cliente.patch(f"/api/playlists/{lid}", json={"name": "Nueva"})
    assert r.status_code == 200, r.text
    assert r.json()["playlist"]["name"] == "Nueva"
    assert any(l["name"] == "Nueva" for l in r.json()["playlists"])
    # el nombre de otra lista no se puede pisar; ni dejarlo vacio
    assert cliente.patch(f"/api/playlists/{lid}", json={"name": "otra"}).status_code == 409
    assert cliente.patch(f"/api/playlists/{lid}", json={}).status_code == 400
    assert cliente.patch("/api/playlists/999999", json={"name": "x"}).status_code == 404
    playlists.remove(lid); playlists.remove(other)


# --------------------------------------------- la lista del reproductor
# Abrir una cancion desde el explorador NO la importa a la biblioteca. Lo que
# se guarda es que sono, para poder volver a ella desde el reproductor.


def _de_fuera(cliente, nombre="suelta.mp3", titulo="Suelta", artista="Nadie"):
    """Un mp3 real FUERA de la biblioteca."""
    from conftest import make_mp3
    from danplay import config
    fuera = os.path.join(os.path.dirname(str(config.LIBRARY)), "fuera")
    return make_mp3(os.path.join(fuera, nombre), artist=artista, title=titulo)


def test_an_outside_song_plays_without_entering_the_library(cliente):
    from danplay import library
    antes = library.stats_of()["total"]
    ruta = _de_fuera(cliente)

    r = cliente.post("/api/external/play", json={"path": ruta})
    assert r.status_code == 200, r.text
    song = r.json()["song"]
    assert song["id"] < 0, "las de fuera llevan id negativo"
    assert song["title"] == "Suelta" and song["artist"] == "Nadie", song
    assert song["external"] is True
    assert library.stats_of()["total"] == antes, "se ha colado en la biblioteca"
    assert library.by_id(song["id"]) is None, "la biblioteca no debe encontrarla"


def test_playing_it_again_does_not_duplicate_it_and_moves_it_up(cliente):
    cliente.delete("/api/external")
    una = _de_fuera(cliente, "una.mp3", "Una")
    otra = _de_fuera(cliente, "otra.mp3", "Otra")
    cliente.post("/api/external/play", json={"path": una})
    cliente.post("/api/external/play", json={"path": otra})
    titulos = [s["title"] for s in cliente.get("/api/external").json()["songs"]]
    assert titulos == ["Otra", "Una"], titulos

    cliente.post("/api/external/play", json={"path": una})
    songs = cliente.get("/api/external").json()["songs"]
    assert [s["title"] for s in songs] == ["Una", "Otra"], "no subio al reproducirla"
    assert len(songs) == 2, "se añadio dos veces"


def test_a_library_song_opened_from_outside_stays_the_library_one(cliente):
    cliente.delete("/api/external")
    dentro = cliente.get("/api/search", params={"limit": 1}).json()["songs"][0]
    ruta = cliente.get(f"/api/song/{dentro['id']}/path").json()["path"]

    song = cliente.post("/api/external/play", json={"path": ruta}).json()["song"]
    assert song["id"] == dentro["id"], "deberia ser la de la biblioteca, con su id"
    assert not song.get("external")
    assert [s["id"] for s in cliente.get("/api/external").json()["songs"]] == [dentro["id"]]


def test_an_outside_song_serves_its_audio_and_its_cover_route(cliente):
    ruta = _de_fuera(cliente, "sonora.mp3", "Sonora")
    song = cliente.post("/api/external/play", json={"path": ruta}).json()["song"]
    d = cliente.get(f"/api/song/{song['id']}/path").json()
    assert d["path"] == ruta and d["bytes"] > 0, d
    # sin caratula dentro, 404 limpio: lo que importa es que resuelva el id
    assert cliente.get(f"/api/song/{song['id']}/cover").status_code in (200, 404)


def test_saving_the_list_keeps_it_after_discarding(cliente):
    cliente.delete("/api/external")
    cliente.post("/api/external/play", json={"path": _de_fuera(cliente, "g1.mp3", "G1")})
    cliente.post("/api/external/play", json={"path": _de_fuera(cliente, "g2.mp3", "G2")})

    r = cliente.post("/api/external/save", json={"name": "Guardada del reproductor"})
    assert r.status_code == 200, r.text
    lid, n = r.json()["id"], r.json()["n"]
    assert n == 2

    assert cliente.delete("/api/external").json()["removed"] == 2
    assert cliente.get("/api/external").json()["songs"] == []
    # y lo guardado sigue entero
    guardada = cliente.get(f"/api/playlists/{lid}/songs").json()["songs"]
    assert sorted(s["title"] for s in guardada) == ["G1", "G2"], guardada


def test_an_empty_list_cannot_be_saved(cliente):
    cliente.delete("/api/external")
    assert cliente.post("/api/external/save", json={"name": "Vacia"}).status_code == 400


def test_one_song_can_be_dropped_from_the_list(cliente):
    cliente.delete("/api/external")
    a = _de_fuera(cliente, "q1.mp3", "Q1")
    cliente.post("/api/external/play", json={"path": a})
    cliente.post("/api/external/play", json={"path": _de_fuera(cliente, "q2.mp3", "Q2")})
    cid = [s for s in cliente.get("/api/external").json()["songs"] if s["title"] == "Q1"][0]["id"]
    assert cliente.delete(f"/api/external/{cid}").json()["removed"] is True
    assert [s["title"] for s in cliente.get("/api/external").json()["songs"]] == ["Q2"]


def test_a_file_that_is_not_there_is_not_added(cliente):
    r = cliente.post("/api/external/play", json={"path": "/no/existe/nada.mp3"})
    assert r.status_code == 404


def test_an_outside_song_can_be_looked_at_but_not_touched(cliente):
    ruta = _de_fuera(cliente, "mirar.mp3", "Mirar")
    cid = cliente.post("/api/external/play", json={"path": ruta}).json()["song"]["id"]

    # se puede VER
    d = cliente.get(f"/api/song/{cid}").json()
    assert d["title"] == "Mirar" and d["existe"] is True

    # pero no se le escribe nada dentro ni se borra desde aqui
    assert cliente.patch(f"/api/song/{cid}", json={"title": "Otro"}).status_code == 404
    assert cliente.post(f"/api/song/{cid}/stars", json={"stars": 5}).status_code in (404, 422, 500)
    assert cliente.delete(f"/api/song/{cid}").status_code == 404
    assert cliente.get(f"/api/song/{cid}").json()["title"] == "Mirar", "se le cambio el titulo"
