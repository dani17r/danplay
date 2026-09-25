"""La biblioteca sigue al disco: canciones movidas, borradas o renombradas por
fuera de la app, carpetas que se mueven enteras o desaparecen (un disco sin
montar) y vuelven, y el vigilante que lo hace sin pulsar nada.

Cada prueba tiene su biblioteca y su base de datos temporales: aqui se mueven
y se borran carpetas enteras, y eso no puede tocar la musica de nadie ni la
biblioteca que comparten las pruebas de la API.
"""

import os
import shutil
import sys
import time

import pytest
from conftest import make_mp3, run_job


@pytest.fixture
def lib(configured_library, tmp_path, monkeypatch):
    """La biblioteca sintetica, ya añadida y escaneada, con los datos de la app
    (formas de onda) tambien en el temporal."""
    from danplay import config, library

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    (tmp_path / "datos").mkdir()
    root, songs = configured_library
    library.add_folder(root)
    library.scan()
    return root, songs


def _ids():
    from danplay import library

    return {os.path.basename(s["path"]): s["id"] for s in library.search("", limit=100)}


def _row(name):
    from danplay import library

    return next(s for s in library.search("", limit=100) if os.path.basename(s["path"]) == name)


@pytest.fixture
def client(lib):
    from fastapi.testclient import TestClient

    from danplay import api

    return TestClient(api.app)


# ------------------------------------------------------------- canciones sueltas


def test_a_song_moved_to_another_folder_keeps_its_id_and_its_lists(lib):
    from danplay import library, playlists

    root, songs = lib
    before = _ids()
    sunday = playlists.create("Domingo")
    playlists.add(sunday["id"], list(before.values()))
    library.update(before["Barak - Mi Gozo.mp3"], chords='{"key": "G"}')  # solo en la base

    moved = root / "Artistas" / "Otros" / "Barak - Mi Gozo.mp3"
    moved.parent.mkdir(parents=True)
    shutil.move(songs["gozo"], moved)
    r = library.scan()

    assert r["back"] == 1 and r["added_count"] == 0
    assert _ids() == before, "la cancion movida entro como otra"
    song = _row("Barak - Mi Gozo.mp3")
    assert song["path"] == str(moved)
    assert song["chords"] == '{"key": "G"}', "se perdio lo que solo guarda la base"
    assert [s["id"] for s in playlists.songs(sunday["id"])] == list(before.values())


def test_a_song_renamed_in_place_keeps_its_id(lib):
    """Renombrar no cambia ni el tamaño ni la fecha: con eso basta."""
    from danplay import library

    _root, songs = lib
    old_id = _ids()["New Wine - Shekinah.mp3"]
    renamed = os.path.join(os.path.dirname(songs["shekinah"]), "Shekinah (en vivo).mp3")
    os.rename(songs["shekinah"], renamed)
    library.scan()
    assert _ids()["Shekinah (en vivo).mp3"] == old_id


def test_a_deleted_song_leaves_the_views_and_its_lists(lib):
    from danplay import library, playlists

    _root, songs = lib
    ids = _ids()
    sunday = playlists.create("Domingo")
    playlists.add(sunday["id"], list(ids.values()))
    os.remove(songs["tierra"])
    r = library.scan()
    assert r["removed"] == 1
    assert "Barak - Sera Llena La Tierra.mp3" not in _ids()
    assert library.stats_of()["total"] == 2
    lists = {p["name"]: p for p in playlists.list_all()}
    assert lists["Domingo"]["n"] == 2, "la lista cuenta una cancion que ya no enseña"
    assert len(playlists.songs(sunday["id"])) == 2


def test_a_song_that_comes_back_returns_to_its_place_in_the_list(lib):
    """Borrada por fuera y restaurada (de la papelera, de una copia): vuelve
    con su id y en su puesto de la lista, sin releer el archivo."""
    from danplay import library, playlists

    root, songs = lib
    ids = _ids()
    order = [
        ids["New Wine - Shekinah.mp3"],
        ids["Barak - Mi Gozo.mp3"],
        ids["Barak - Sera Llena La Tierra.mp3"],
    ]
    sunday = playlists.create("Domingo")
    playlists.add(sunday["id"], order)
    keep = root.parent / "guardada.mp3"
    shutil.move(songs["gozo"], keep)
    library.scan()
    assert [s["id"] for s in playlists.songs(sunday["id"])] == [order[0], order[2]]

    shutil.move(keep, songs["gozo"])
    r = library.scan()
    assert r["back"] == 1
    assert [s["id"] for s in playlists.songs(sunday["id"])] == order


def test_new_songs_never_inherit_the_id_of_one_that_left(lib):
    """Regresion: sin AUTOINCREMENT, SQLite daba a la cancion nueva el id de
    la ultima que salio del indice, y con el sus listas."""
    from danplay import library, playlists

    root, _songs = lib
    ids = _ids()
    last = max(ids.values())
    victim = next(n for n, i in ids.items() if i == last)
    sunday = playlists.create("Domingo")
    playlists.add(sunday["id"], [last])
    # lo que hace «a la papelera», sin tocar la papelera de verdad
    os.remove(library.by_id(last)["path"])
    library.forget(last)
    fresh = make_mp3(
        root / "Artistas" / "Barak" / "Barak - Nueva.mp3",
        artist="Barak",
        title="Nueva",
        seconds=2.0,
    )
    song = library.index_file(fresh)
    assert song["id"] > last, f"{victim} dejo su id a la nueva"
    assert playlists.playlists_of(song["id"]) == []


def test_songs_that_never_come_back_are_forgotten_after_a_while(lib):
    from danplay import library

    _root, songs = lib
    gone_id = _ids()["Barak - Mi Gozo.mp3"]
    os.remove(songs["gozo"])
    library.scan()
    conn = library.connect()
    assert (
        conn.execute("SELECT COUNT(*) FROM songs_missing WHERE id=?", (gone_id,)).fetchone()[0] == 1
    )
    conn.execute("UPDATE songs_missing SET gone_at = gone_at - ?", (40 * 86400,))
    conn.commit()
    assert library._purge_missing(conn) == 1
    assert conn.execute("SELECT COUNT(*) FROM songs_missing").fetchone()[0] == 0
    conn.close()


def test_a_scan_that_finds_nothing_new_does_not_refresh_the_app(lib):
    """El vigilante escanea cada vez que se toca un archivo; si nada de lo que
    se enseña cambio, la interfaz no tiene que recargar."""
    from danplay import library, playlists

    _root, _songs = lib
    playlists.rate(_ids()["Barak - Mi Gozo.mp3"], 4)  # escribe en el archivo
    before = library.revision()
    r = library.scan()
    assert r["updated"] >= 1 and not r["changed"]
    assert library.revision() == before


@pytest.mark.skipif(
    not sys.platform.startswith("linux"), reason="solo Linux deja nombres que no son UTF-8"
)
def test_a_name_that_is_not_utf8_does_not_stop_the_scan(lib):
    from danplay import library

    root, songs = lib
    folder = os.fsencode(str(root / "Artistas" / "Barak"))
    shutil.copy(songs["gozo"], folder + b"/Barak - Canci\xf3n.mp3")
    r = library.scan()
    assert r["total"] == 3, "un nombre raro tumbo el escaneo entero"


# ------------------------------------------------------------- carpetas enteras


def test_a_folder_that_moved_hides_its_songs_and_says_so(client, lib):
    from danplay import library

    root, _songs = lib
    shutil.move(str(root), str(root.parent / "movida"))
    assert library.hide_missing_roots() == 3
    d = client.get("/api/status").json()
    assert d["stats"]["total"] == 0
    assert d["configured"] is True
    assert d["missing_folders"] == [str(root)]
    folders = client.get("/api/folders").json()["folders"]
    assert [f["exists"] for f in folders] == [False]
    assert client.get("/api/search").json()["total"] == 0


def test_choosing_the_moved_folder_brings_everything_back(client, lib):
    """Lo que hace quien movio su musica: elige la carpeta nueva como si la
    importara otra vez. Es la misma: vuelve con ids, listas y notas."""
    from danplay import library, playlists

    root, _songs = lib
    ids = _ids()
    sunday = playlists.create("Domingo")
    playlists.add(sunday["id"], list(ids.values()))
    new = root.parent / "disco" / "Musica"
    new.parent.mkdir()
    shutil.move(str(root), str(new))
    library.hide_missing_roots()

    r = client.post("/api/folders", json={"path": str(new)}).json()
    assert r["action"] == "relocated"
    assert r["notice"]["other"] == str(root)
    assert [f["path"] for f in r["folders"]] == [str(new)], "la carpeta vieja se quedo"
    run_job(client, "/api/scan", "escaneo")
    assert _ids() == ids
    assert all(s["path"].startswith(str(new)) for s in library.search("", limit=10))
    assert len(playlists.songs(sunday["id"])) == 3
    assert client.get("/api/status").json()["missing_folders"] == []


def test_relocating_by_hand(client, lib):
    root, _songs = lib
    ids = _ids()
    new = root.parent / "Otra"
    shutil.move(str(root), str(new))
    r = client.post("/api/folders/relocate", json={"from": str(root), "to": str(new)})
    assert r.status_code == 200, r.text
    assert r.json()["back"] == 3
    assert _ids() == ids
    # una carpeta que sigue en su sitio no se «reubica»
    r = client.post("/api/folders/relocate", json={"from": str(new), "to": str(root.parent)})
    assert r.status_code == 400


def test_an_unrelated_folder_is_not_taken_for_the_moved_one(client, lib, tmp_path):
    root, _songs = lib
    shutil.move(str(root), str(tmp_path / "lejos"))
    other = tmp_path / "otra-musica"
    make_mp3(other / "Alguien - Algo.mp3", artist="Alguien", title="Algo")
    r = client.post("/api/folders", json={"path": str(other)}).json()
    assert r["action"] == "added"


def test_a_disk_that_comes_back_brings_its_songs_as_they_were(lib):
    """Un disco que no estaba montado al arrancar: al volver, cada cancion
    recupera su id sin releer ningun archivo."""
    from danplay import library

    root, _songs = lib
    ids = _ids()
    away = root.parent / "desmontado"
    shutil.move(str(root), str(away))
    library.hide_missing_roots()
    assert library.stats_of()["total"] == 0
    shutil.move(str(away), str(root))
    r = library.scan()
    assert r["back"] == 3 and r["updated"] == 0 and r["added_count"] == 0
    assert _ids() == ids


def test_removing_a_folder_and_adding_it_again_keeps_the_ids(client, lib):
    root, _songs = lib
    ids = _ids()
    client.delete("/api/folders", params={"path": str(root)})
    assert client.get("/api/status").json()["stats"]["total"] == 0
    client.post("/api/folders", json={"path": str(root)})
    run_job(client, "/api/scan", "escaneo")
    assert _ids() == ids


def test_locate_says_where_each_song_is_now(client, lib):
    """Lo que usa la cola de Rust para ponerse al dia de una vez."""
    from danplay import library

    root, songs = lib
    ids = _ids()
    moved = root / "Movidas" / "Barak - Mi Gozo.mp3"
    moved.parent.mkdir()
    shutil.move(songs["gozo"], moved)
    os.remove(songs["tierra"])
    library.scan()
    paths = client.post("/api/songs/locate", json={"ids": [*list(ids.values()), 999999]}).json()[
        "paths"
    ]
    assert paths[str(ids["Barak - Mi Gozo.mp3"])] == str(moved)
    assert paths[str(ids["Barak - Sera Llena La Tierra.mp3"])] is None
    assert paths[str(ids["New Wine - Shekinah.mp3"])] == songs["shekinah"]
    assert paths["999999"] is None


# ------------------------------------------------------------- el vigilante


def _until(check, timeout=15.0):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if check():
            return True
        time.sleep(0.1)
    return False


def test_the_watcher_follows_what_happens_on_disk(lib):
    """Sin pulsar nada: una cancion que llega, se mueve y se borra desde fuera
    de la app, y la carpeta entera que se va y vuelve."""
    pytest.importorskip("watchdog")
    from danplay import library, watcher

    root, _songs = lib
    w = watcher.Watcher(quiet=0.2, check_every=0.2, rescan_every=0)
    w.start()
    try:
        assert _until(lambda: w._watches), "no llego a vigilar la carpeta"
        new = make_mp3(
            root / "Artistas" / "Barak" / "Barak - Llega.mp3",
            artist="Barak",
            title="Llega",
            seconds=2.0,
        )
        assert _until(lambda: "Barak - Llega.mp3" in _ids()), "no vio la cancion nueva"
        new_id = _ids()["Barak - Llega.mp3"]

        moved = root / "Artistas" / "Otra" / "Barak - Llega.mp3"
        moved.parent.mkdir()
        shutil.move(new, moved)
        assert _until(lambda: _row("Barak - Llega.mp3")["path"] == str(moved))
        assert _ids()["Barak - Llega.mp3"] == new_id

        os.remove(moved)
        assert _until(lambda: "Barak - Llega.mp3" not in _ids()), "no vio que se borro"

        away = root.parent / "fuera"
        shutil.move(str(root), str(away))
        assert _until(lambda: library.stats_of()["total"] == 0), "no vio irse la carpeta"
        shutil.move(str(away), str(root))
        assert _until(lambda: library.stats_of()["total"] == 3), "no vio volver la carpeta"
    finally:
        w.stop()


def test_a_folder_added_in_settings_is_indexed_without_asking(lib, tmp_path):
    """Ajustes añade la carpeta sin analizarla: el vigilante la ve y la indexa."""
    from danplay import library, watcher

    w = watcher.Watcher(quiet=0.2, check_every=0.2, rescan_every=0, observe=False)
    w.start()
    try:
        assert _until(lambda: library.stats_of()["total"] == 3)
        other = tmp_path / "Mas musica"
        make_mp3(other / "Alguien - Otra.mp3", artist="Alguien", title="Otra")
        library.add_folder(other)
        assert _until(lambda: "Alguien - Otra.mp3" in _ids()), "no indexo la carpeta nueva"
    finally:
        w.stop()


def test_the_watcher_ignores_what_does_not_matter(lib):
    """Leer una cancion (el propio reproductor), un .txt o una carpeta
    excluida no son motivo para escanear."""
    from types import SimpleNamespace as Ev

    from danplay import library, watcher

    root, songs = lib
    library.add_exclusion("Secuencias")
    w = watcher.Watcher(observe=False)
    w.refresh_roots()
    events = watcher._Events(w)

    events.dispatch(Ev(event_type="opened", src_path=songs["gozo"], is_directory=False))
    events.dispatch(Ev(event_type="closed_no_write", src_path=songs["gozo"], is_directory=False))
    events.dispatch(Ev(event_type="created", src_path=str(root / "notas.txt"), is_directory=False))
    events.dispatch(Ev(event_type="modified", src_path=str(root / "Artistas"), is_directory=True))
    events.dispatch(
        Ev(
            event_type="created",
            src_path=str(root / "Secuencias" / "click.wav"),
            is_directory=False,
        )
    )
    events.dispatch(Ev(event_type="created", src_path="/en/otro/sitio.mp3", is_directory=False))
    assert w._pending_since is None

    events.dispatch(
        Ev(
            event_type="moved",
            src_path=str(root / "a.part"),
            dest_path=str(root / "Artistas" / "Barak" / "b.mp3"),
            is_directory=False,
        )
    )
    assert w._pending_since is not None


def test_the_watcher_waits_for_calm_before_scanning():
    from danplay import watcher

    w = watcher.Watcher(quiet=2.0, observe=False)
    w.mark()
    start = w._last_event
    assert not w.due(start + 1.0), "escaneo en plena copia"
    assert w.due(start + 2.5)
    assert not w.due(start + 3.0), "dos escaneos para el mismo aviso"
    # con cambios sin parar, se escanea igualmente pasado un rato
    w.mark()
    first = w._pending_since
    w._last_event = first + watcher.MAX_WAIT
    assert w.due(first + watcher.MAX_WAIT + 0.1)


def test_prepare_hides_a_missing_folder_before_the_first_answer(lib):
    from danplay import library, watcher

    root, _songs = lib
    shutil.move(str(root), str(root.parent / "movida"))
    watcher.prepare()
    assert library.stats_of()["total"] == 0


def test_nothing_brings_a_moved_folder_back_empty(client, lib):
    """Regresion: la interfaz pregunta por la Entrada al arrancar, y crearla
    (con sus carpetas de mas arriba) hacia reaparecer, vacia, la carpeta de
    musica que se acababa de mover: ya no salia «No encuentro tu musica» y
    elegir la carpeta en su sitio nuevo no se reconocia como la misma."""
    from danplay import config, ingest, library, playlists, youtube

    root, _songs = lib
    assert root / "Entrada" == config.INBOX
    shutil.move(str(root), str(root.parent / "movida"))

    assert client.get("/api/inbox").json() == {"files": [], "total": 0}
    assert ingest.process_inbox() == []
    with pytest.raises(library.FolderGone):
        library.ensure_folder(config.INBOX)
    down = youtube.download_one("https://www.youtube.com/watch?v=x")
    assert not down["ok"]
    sunday = playlists.create("Domingo")
    assert client.post(f"/api/playlists/{sunday['id']}/export").status_code == 400

    assert not root.exists(), "la carpeta movida reaparecio vacia"
    assert client.get("/api/status").json()["missing_folders"] == [str(root)]


def test_a_new_inbox_is_still_created_where_nothing_is_missing(lib, tmp_path, monkeypatch):
    """Lo de siempre sigue igual: sin carpetas que falten, la Entrada se crea."""
    from danplay import config, library

    monkeypatch.setattr(config, "INBOX", tmp_path / "otra" / "Entrada")
    assert library.ensure_folder(config.INBOX).is_dir()
