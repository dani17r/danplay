"""El indice por dentro: lo que se deduce del nombre, las listas ligeras, las
cifras que no se quedan viejas, la lista del reproductor y las caches en
disco (miniaturas) que se podan con el escaneo."""

import os
import threading

import pytest
from conftest import make_mp3


@pytest.fixture
def lib(configured_library, tmp_path, monkeypatch):
    from danplay import config, library

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    (tmp_path / "datos").mkdir()
    root, songs = configured_library
    library.add_folder(root)
    library.scan()
    return root, songs


@pytest.fixture
def client(lib):
    from fastapi.testclient import TestClient

    from danplay import api

    return TestClient(api.app)


def _song(name):
    from danplay import library

    return next(s for s in library.search("", limit=500) if os.path.basename(s["path"]) == name)


# ------------------------------------------------- «Artista - Titulo» del nombre


def test_sin_etiquetas_el_nombre_da_el_artista(lib):
    """«Palisades - Personal.mp3» sin etiquetas ni carpeta de artista entraba
    con artista vacio y el titulo entero. Es la convencion de la casa."""
    from danplay import library

    root, _ = lib
    path = make_mp3(root / "Descargas" / "Palisades - Personal (Official Music Video).mp3")
    before = os.stat(path).st_mtime_ns
    library.scan()
    c = _song("Palisades - Personal (Official Music Video).mp3")
    assert (c["artist"], c["title"]) == ("Palisades", "Personal")
    assert os.stat(path).st_mtime_ns == before, "el archivo no se toca"
    assert os.path.exists(path), "ni se mueve"


def test_varios_guiones_y_feat(lib):
    from danplay import library

    root, _ = lib
    make_mp3(root / "Sueltas" / "Palisades ft. Bryan - Personal - Live.mp3")
    make_mp3(root / "Sueltas" / "Sin guion aqui.mp3")
    library.scan()
    c = _song("Palisades ft. Bryan - Personal - Live.mp3")
    assert (c["artist"], c["title"], c["feat"]) == ("Palisades", "Personal (Live)", "Bryan")
    c = _song("Sin guion aqui.mp3")
    assert c["artist"] == "" and c["title"] == "Sin guion aqui"


def test_la_etiqueta_y_la_carpeta_mandan_sobre_el_nombre(lib):
    from danplay import library

    root, _ = lib
    make_mp3(root / "Sueltas" / "Otro - Nombre.mp3", artist="Etiqueta", title="Titulo")
    make_mp3(root / "Artistas" / "Barak" / "Cualquiera - Cosa.mp3")
    library.scan()
    assert _song("Otro - Nombre.mp3")["artist"] == "Etiqueta"
    assert _song("Cualquiera - Cosa.mp3")["artist"] == "Barak"


# ------------------------------------------------------------ listas ligeras


def test_la_busqueda_de_la_api_es_ligera_y_cuenta(client, lib):
    """Las listas no traen la letra ni los acordes (kilobytes por cancion):
    traen si los tienen. Y `count` dice cuantas cumplen la busqueda."""
    from danplay import library

    c = library.search("", limit=1)[0]
    library.update(c["id"], lyrics="una letra larga", chords='{"likely_key": "G"}')
    d = client.get("/api/search", params={"limit": 1}).json()
    assert d["count"] == library.stats_of()["total"] and d["total"] == 1
    song = d["songs"][0]
    for heavy in ("lyrics", "lyrics_synced", "chords", "study"):
        assert heavy not in song
    assert set(song) >= {"has_lyrics", "has_synced_lyrics", "has_chords", "has_study"}
    row = next(
        s
        for s in client.get("/api/search", params={"limit": 50}).json()["songs"]
        if s["id"] == c["id"]
    )
    assert row["has_lyrics"] is True and row["has_chords"] is True
    assert row["has_study"] is False
    # la ficha completa sigue en /api/song/{id}
    assert client.get(f"/api/song/{c['id']}").json()["lyrics"] == "una letra larga"
    # el asistente sigue viendo la fila entera
    assert library.search("", limit=50)[0].get("lyrics") is not None
    # el conteo respeta los filtros y no el limite
    d = client.get("/api/search", params={"q": "artista:barak", "limit": 1}).json()
    assert d["count"] == 2 and len(d["songs"]) == 1
    assert client.get("/api/search", params={"limit": 20000}).status_code == 200


def test_las_listas_y_el_reproductor_tambien_son_ligeros(client, lib):
    from danplay import library, playlists

    ids = [s["id"] for s in library.search("", limit=3)]
    library.update(ids[0], lyrics="letra")
    lid = playlists.create("Ligera")["id"]
    playlists.add(lid, ids)
    songs = client.get(f"/api/playlists/{lid}/songs").json()["songs"]
    assert [s["id"] for s in songs] == ids
    assert "lyrics" not in songs[0] and songs[0]["has_lyrics"] is True
    client.post("/api/external/play", json={"path": library.by_id(ids[1])["path"]})
    songs = client.get("/api/external").json()["songs"]
    assert songs and "lyrics" not in songs[0] and "has_lyrics" in songs[0]
    assert client.get("/api/playlists").json()["favorites"] == 0


def test_un_numero_raro_en_la_busqueda_no_rompe_la_consulta(lib):
    """`bpm>abc` dejaba un «?» sin valor y la busqueda entera fallaba."""
    from danplay import library

    assert library.search("bpm>abc") == library.search("")


# ---------------------------------------------------------- las cifras


def test_las_cifras_no_guardan_un_total_de_antes_del_cambio(lib, monkeypatch):
    """Si alguien escribe mientras se cuenta, lo contado ya nace viejo y no
    se puede quedar en la cache cinco segundos."""
    from danplay import library

    real = library.db.connect
    calls = {"n": 0}

    def connect_and_write():
        calls["n"] += 1
        conn = real()
        library._touch()  # otro hilo acaba de cambiar algo
        return conn

    monkeypatch.setattr(library.db, "connect", connect_and_write)
    library.stats_of()
    library.stats_of()
    assert calls["n"] == 2, "la segunda vez tenia que volver a contar"
    monkeypatch.setattr(library.db, "connect", real)
    library.stats_of()
    n = calls["n"]
    library.stats_of()
    assert calls["n"] == n, "sin cambios, la cache vale"


def test_la_revision_no_pierde_avisos_con_varios_hilos(lib):
    from danplay import library

    start = library.revision()

    def touch():
        for _ in range(500):
            library._touch()

    threads = [threading.Thread(target=touch) for _ in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert library.revision() == start + 8 * 500


# --------------------------------------------------- la lista del reproductor


def test_solo_se_abre_audio(client, tmp_path):
    """Se aceptaba cualquier archivo y se le pasaba a Rust para reproducirlo."""
    texto = tmp_path / "notas.txt"
    texto.write_text("no soy musica")
    r = client.post("/api/external/play", json={"path": str(texto)})
    assert r.status_code == 400 and "audio" in r.json()["detail"]
    falso = tmp_path / "falso.mp3"
    assert client.post("/api/external/play", json={"path": str(falso)}).status_code == 404


def test_la_lista_del_reproductor_mezcla_las_dos_procedencias_en_orden(client, lib, tmp_path):
    from danplay import external, library

    external.clear()
    inside = library.search("", limit=2)
    outside = make_mp3(tmp_path / "fuera" / "suelta.mp3", title="Suelta")
    external.played(inside[0]["path"])
    external.played(outside)
    external.played(inside[1]["path"])
    got = external.listing()
    assert next(s["id"] for s in got) == inside[1]["id"]
    assert got[1]["title"] == "Suelta" and got[1]["id"] < 0
    assert got[2]["id"] == inside[0]["id"]


# ------------------------------------------------------ repertorios por nombre


def test_buscar_un_repertorio_no_recalcula_todos(lib, monkeypatch):
    from danplay import playlists

    for name in ("Uno", "Dos", "Herlín"):
        playlists.create(name)
    monkeypatch.setattr(playlists, "list_all", lambda: pytest.fail("no hace falta la lista entera"))
    assert playlists.by_name("herlin")["name"] == "Herlín"
    assert playlists.by_id(playlists.by_name("dos")["id"])["name"] == "Dos"
    assert playlists.by_name("no existe") is None and playlists.by_id("x") is None


def test_el_indice_de_canciones_en_listas_existe(lib):
    from danplay import library

    conn = library.connect()
    try:
        plan = " ".join(
            r[3]
            for r in conn.execute(
                "EXPLAIN QUERY PLAN SELECT playlist_id FROM playlist_songs WHERE song_id=1"
            )
        )
    finally:
        conn.close()
    assert "i_playlist_songs_song" in plan, plan


# --------------------------------------------------------------- miniaturas


def test_las_miniaturas_se_podan_con_el_escaneo(client, lib):
    """La carpeta covers/ no se podaba nunca: cada caratula cambiada y cada
    cancion borrada dejaba sus miniaturas para siempre."""
    from danplay import convert, library, tags, thumbnails

    if not convert.available():
        pytest.skip("hace falta ffmpeg")
    songs = library.search("", limit=2)
    cover = _jpeg()
    for s in songs:
        assert tags.write_cover(s["path"], cover, "image/jpeg")
    for s in songs:
        r = client.get(f"/api/song/{s['id']}/cover", params={"size": 96})
        assert r.status_code == 200 and r.headers["content-type"].startswith("image/")
    folder = thumbnails.folder()
    assert len(list(folder.iterdir())) == 2
    # un huerfano de otro formato y una cancion que se va
    (folder / ("a" * 40 + ".jpg")).write_bytes(b"viejo")
    os.remove(songs[0]["path"])
    library.scan()
    left = list(folder.iterdir())
    assert len(left) == 1, left
    # la caratula cambia: la version vieja sobra
    assert tags.write_cover(songs[1]["path"], _jpeg(color="blue"), "image/jpeg")
    client.get(f"/api/song/{songs[1]['id']}/cover", params={"size": 96})
    library.scan()
    assert len(list(folder.iterdir())) == 1
    # y con la cancion se van sus miniaturas
    library.forget(songs[1]["id"])
    assert not list(folder.iterdir())


def test_la_cache_de_miniaturas_tiene_tope(lib):
    from danplay import thumbnails

    folder = thumbnails.folder()
    for i in range(10):
        f = folder / f"{i:024d}_96_abcdef0123.jpg"
        f.write_bytes(b"x" * 1000)
        os.utime(f, (1000 + i, 1000 + i))
    assert thumbnails.trim(max_bytes=5000) >= 5
    left = sorted(p.name for p in folder.iterdir())
    assert len(left) <= 4 and left[-1].startswith("000000000000000000000009"), (
        "se van las que hace mas que no se piden"
    )


def _jpeg(color="red") -> bytes:
    """Una imagen JPEG de verdad (ffmpeg), para que se pueda encoger."""
    import subprocess
    import tempfile

    from conftest import FFMPEG

    with tempfile.TemporaryDirectory() as d:
        out = os.path.join(d, "tapa.jpg")
        subprocess.run(
            [
                FFMPEG,
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                f"color=c={color}:s=400x400",
                "-frames:v",
                "1",
                out,
            ],
            check=True,
            capture_output=True,
            timeout=60,
        )
        with open(out, "rb") as f:
            return f.read()
