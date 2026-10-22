"""Las listas no se rompen (danplay/playlists.py y danplay/journal.py): una
cancion que se va sigue en su lista como «No está», vuelve sola si aparece
en otro archivo, y lo tuyo (listas en su orden, estrellas, estudio) se guarda
en un diario junto a tu musica que rehace todo si la base se pierde o la
musica se mueve."""

import json
import os
import shutil
import time

import pytest


@pytest.fixture
def lib(configured_library, tmp_path, monkeypatch):
    from danplay import config, journal, library

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    root, songs = configured_library
    library.add_folder(root)
    library.scan()
    journal._reset()
    return root, songs


def _id(name):
    from danplay import library

    return next(s["id"] for s in library.search("", limit=50) if s["file"] == name)


def _titles(pid):
    from danplay import playlists

    return [(s["title"], bool(s.get("ghost"))) for s in playlists.songs(pid, light=True)]


def test_a_song_that_goes_stays_in_its_list_as_missing(lib):
    from danplay import library, playlists

    _, songs = lib
    pl = playlists.create("Domingo")
    a, b, c = (
        _id("Barak - Mi Gozo.mp3"),
        _id("Barak - Sera Llena La Tierra.mp3"),
        _id("New Wine - Shekinah.mp3"),
    )
    playlists.set_songs(pl["id"], [a, b, c])
    os.remove(songs["tierra"])  # se borra por fuera
    library.scan()
    assert _titles(pl["id"]) == [
        ("Mi Gozo", False),
        ("Sera Llena La Tierra", True),
        ("Shekinah", False),
    ]
    summary = next(p for p in playlists.list_all() if p["id"] == pl["id"])
    assert summary["n"] == 2 and summary["missing"] == 1
    # pasado el mes de apartada, sigue ahi con su ficha
    with library.connect() as conn:
        conn.execute("UPDATE songs_missing SET gone_at=?", (time.time() - 40 * 86400,))
        conn.commit()
    library.scan()
    with library.connect() as conn:
        assert conn.execute("SELECT COUNT(*) FROM songs_missing").fetchone()[0] == 0
    ghost = playlists.songs(pl["id"], light=True)[1]
    assert (
        ghost["ghost"]
        and ghost["artist"] == "Barak"
        and ghost["file"] == "Barak - Sera Llena La Tierra.mp3"
    )


def test_it_comes_back_in_another_file_to_its_place(lib, tmp_path):
    """Se volvio a bajar (otro nombre de archivo): la lista la recupera en
    su sitio, sin tocar nada."""
    from danplay import library, playlists

    root, songs = lib
    pl = playlists.create("Ensayo")
    a, b, c = (
        _id("Barak - Mi Gozo.mp3"),
        _id("Barak - Sera Llena La Tierra.mp3"),
        _id("New Wine - Shekinah.mp3"),
    )
    playlists.set_songs(pl["id"], [a, b, c])
    keep = tmp_path / "copia.mp3"
    shutil.copy(songs["tierra"], keep)
    os.remove(songs["tierra"])
    library.scan()
    with library.connect() as conn:  # y paso el mes
        conn.execute("UPDATE songs_missing SET gone_at=?", (time.time() - 40 * 86400,))
        conn.commit()
    library.scan()
    # vuelve con otro nombre (y otra fecha): es otro archivo
    again = root / "Entrada" / "sera llena la tierra (audio).mp3"
    shutil.copy(keep, again)
    os.utime(again, (time.time(), time.time()))
    library.scan()
    got = playlists.songs(pl["id"], light=True)
    assert [s["title"] for s in got] == ["Mi Gozo", "Sera Llena La Tierra", "Shekinah"]
    assert not any(s.get("ghost") for s in got)
    assert got[1]["id"] != b and got[1]["path"] == str(again)


def test_a_missing_one_keeps_its_place_when_the_list_is_reordered(lib):
    """La interfaz enseña las «No está»: al reordenar (o fijar la lista
    entera) se quedan donde se dejaron; quitarlas, las quita del todo."""
    from danplay import library, playlists

    _, songs = lib
    pl = playlists.create("Orden")
    a, b, c = (
        _id("Barak - Mi Gozo.mp3"),
        _id("Barak - Sera Llena La Tierra.mp3"),
        _id("New Wine - Shekinah.mp3"),
    )
    playlists.set_songs(pl["id"], [a, b, c])
    os.remove(songs["tierra"])
    library.scan()
    playlists.set_songs(pl["id"], [b, c, a])
    assert _titles(pl["id"]) == [
        ("Sera Llena La Tierra", True),
        ("Shekinah", False),
        ("Mi Gozo", False),
    ]
    # sin nombrarla, no se borra: se queda detras
    playlists.set_songs(pl["id"], [a, c])
    assert _titles(pl["id"])[-1] == ("Sera Llena La Tierra", True)
    playlists.reorder(pl["id"], [b, a, c])
    assert _titles(pl["id"])[0] == ("Sera Llena La Tierra", True)
    playlists.remove_song(pl["id"], b)
    assert [t for t, _ in _titles(pl["id"])] == ["Mi Gozo", "Shekinah"]


def test_deleting_it_from_danplay_can_take_it_out_of_its_lists(lib, monkeypatch):
    from danplay import library, playlists

    monkeypatch.setattr(library.songs, "trash_path", lambda p: {"ok": True})
    pl = playlists.create("Uno")
    a, b = _id("Barak - Mi Gozo.mp3"), _id("New Wine - Shekinah.mp3")
    playlists.set_songs(pl["id"], [a, b])
    # sin quitarla de las listas: queda como «No está»
    assert library.trash(a)["lists"] == 0
    assert _titles(pl["id"]) == [("Mi Gozo", True), ("Shekinah", False)]
    # quitandola: no queda nada
    r = library.trash(b, from_lists=True)
    assert r["lists"] == 1
    assert _titles(pl["id"]) == [("Mi Gozo", True)]


def test_the_api_deletes_from_the_lists_too(lib, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api, library, playlists

    monkeypatch.setattr(library.songs, "trash_path", lambda p: {"ok": True})
    pl = playlists.create("Dos")
    a = _id("Barak - Mi Gozo.mp3")
    playlists.set_songs(pl["id"], [a])
    client = TestClient(api.app)
    r = client.delete(f"/api/song/{a}", params={"from_lists": "true"})
    assert r.status_code == 200 and r.json()["lists"] == 1
    assert client.get(f"/api/playlists/{pl['id']}/songs").json()["songs"] == []


def test_the_diary_is_written_next_to_the_music(lib):
    from danplay import config, journal, library, playlists

    root, _ = lib
    pl = playlists.create("Domingo", note="la de la iglesia", color="#f80")
    a, b = _id("New Wine - Shekinah.mp3"), _id("Barak - Mi Gozo.mp3")
    playlists.set_songs(pl["id"], [a, b])
    library.update(b, stars=4, study=json.dumps({"speed": 0.8}))
    assert journal.write() == 2
    for where in (root / ".danplay" / "diario.json", config.DATA_DIR / "diario.json"):
        data = json.loads(where.read_text(encoding="utf-8"))
        assert data["format"] == "danplay-diario"
        lists = {p["name"]: p for p in data["playlists"]}
        assert lists["Domingo"]["songs"] == [a, b] and lists["Domingo"]["color"] == "#f80"
        gozo = next(s for s in data["songs"] if s["id"] == b)
        assert gozo["stars"] == 4 and gozo["rel"] == "Artistas/Barak/Barak - Mi Gozo.mp3"
    assert journal.write() == 0, "sin cambios no se reescribe"


def test_a_diary_written_on_windows_is_understood_anywhere(lib):
    """La ruta dentro de la carpeta se guarda con «/»; uno escrito con «\\»
    (en Windows) se reconoce igual."""
    from danplay import journal, library

    root, _ = lib
    with library.connect() as conn:
        rows = [dict(r) for r in conn.execute("SELECT * FROM songs")]
    index = journal._index(rows)
    gozo = next(r for r in rows if r["file"] == "Barak - Mi Gozo.mp3")
    entry = {
        "path": "C:\\Musica\\Artistas\\Barak\\Barak - Mi Gozo.mp3",
        "root": root.name,
        "rel": "Artistas\\Barak\\Barak - Mi Gozo.mp3",
        "file": "otro nombre.mp3",
        "size": -1,
    }
    found = journal._find(entry, *index)
    assert found and found["id"] == gozo["id"]


def test_a_lost_database_is_rebuilt_from_the_diary(lib, tmp_path, monkeypatch):
    """Lo peor: la base se pierde (o es otro equipo). Al escanear, el diario
    rehace las listas en su orden, con sus notas, las estrellas y el estudio;
    lo que ya no esta, como «No está»."""
    from danplay import config, journal, library, playlists

    root, songs = lib
    pl = playlists.create("Domingo", note="la de la iglesia")
    a, b, c = (
        _id("New Wine - Shekinah.mp3"),
        _id("Barak - Mi Gozo.mp3"),
        _id("Barak - Sera Llena La Tierra.mp3"),
    )
    playlists.set_songs(pl["id"], [a, b, c])
    library.update(b, stars=5, chords=json.dumps({"sheet": {"key": "G"}}))
    journal.write()
    # sin base, y una cancion que ya no esta
    os.remove(songs["tierra"])
    monkeypatch.setattr(config, "DATABASE", tmp_path / "otra.db")
    library._prepared.clear()
    library.add_folder(root)
    library.scan()
    told = journal.reconcile()
    assert told["fresh"] and told["lists"] == 1 and told["ghosts"] == 1
    new = playlists.by_name("Domingo")
    assert new and new["note"] == "la de la iglesia"
    assert _titles(new["id"]) == [
        ("Shekinah", False),
        ("Mi Gozo", False),
        ("Sera Llena La Tierra", True),
    ]
    gozo = library.by_id(
        next(s["id"] for s in playlists.songs(new["id"]) if s["title"] == "Mi Gozo")
    )
    assert gozo and gozo["stars"] == 5 and json.loads(gozo["chords"])["sheet"]["key"] == "G"
    report = journal.status()
    assert report and report["lists"] == 1
    # otra vez, nada que hacer
    assert journal.reconcile()["lists"] == 0


def test_moving_the_music_keeps_the_lists_and_does_not_undo_what_you_removed(lib, tmp_path):
    from danplay import journal, library, playlists

    root, _ = lib
    pl = playlists.create("Banda")
    a, b, c = (
        _id("Barak - Mi Gozo.mp3"),
        _id("New Wine - Shekinah.mp3"),
        _id("Barak - Sera Llena La Tierra.mp3"),
    )
    playlists.set_songs(pl["id"], [a, b, c])
    playlists.export_m3u(pl["id"])
    journal.write()
    # se quita una a proposito: no vuelve
    playlists.remove_song(pl["id"], [c])
    journal.write()
    # la musica entera se mueve a otro sitio (otra carpeta gestionada)
    new_root = tmp_path / "Musica nueva"
    shutil.copytree(root, new_root)
    shutil.rmtree(root)
    library.add_folder(new_root)
    library.scan()
    journal.reconcile()
    got = playlists.songs(pl["id"], light=True)
    assert [s["title"] for s in got] == ["Mi Gozo", "Shekinah"]
    assert all(str(new_root) in s["path"] for s in got)
    m3u = (new_root / "Listas" / "Banda.m3u8").read_text(encoding="utf-8")
    assert "Shekinah" in m3u
