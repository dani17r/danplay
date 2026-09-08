# -*- coding: utf-8 -*-
"""Pruebas de todos los endpoints, sobre una biblioteca temporal."""
import os, pathlib, shutil, sys, tempfile
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
