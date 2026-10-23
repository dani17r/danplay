"""«Transformar»: cada cancion en su sitio y con su nombre, una a una.

Los casos salen de una biblioteca de verdad: un disco metido en su
subcarpeta con los numeros de pista, una suelta en la raiz con «(Official
Video)», una Drum Cam entre las canciones del artista, las carpetas propias
de quien escucha (que no se tocan) y una con solo etiquetas (que se propone
para revisar, no como segura)."""

import json
import os
from pathlib import Path

import pytest
from conftest import make_mp3


def _song(found):
    """La ficha, que tiene que estar."""
    assert found is not None
    return found


@pytest.fixture
def music_library(configured_library, tmp_path, monkeypatch):
    from danplay import config, library

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    root, _ = configured_library
    artists = root / "Artistas"
    made = {
        "suelta": make_mp3(
            root / "Barak - Nueva (Official Video).mp3", artist="Barak", title="Nueva"
        ),
        "disco": make_mp3(
            artists / "Barak" / "Generacion Radical (2018)" / "06 - Hasta Ver Tu Gloria.mp3",
            artist="Barak",
            title="Hasta Ver Tu Gloria (Live)",
        ),
        "cam": make_mp3(artists / "Barak" / "Barak - Danzar (Drum Cam).mp3", artist="Barak"),
        "propia": make_mp3(root / "Pistas" / "cualquier cosa.mp3", title="x"),
        # solo cambiaria un espacio: no merece moverla
        "pegada": make_mp3(artists / "Barak" / "Barak - Intro #EternoLive.mp3", artist="Barak"),
        "etiquetas": make_mp3(root / "sin nombre.mp3", artist="Un Canal De YouTube", title="Algo"),
    }
    files = {k: Path(p) for k, p in made.items()}
    library.add_folder(root)
    library.scan()
    ids = {k: _song(library.by_path(str(p)))["id"] for k, p in files.items()}
    return root, files, ids


def _by_from(items):
    return {p["from"]: p for p in items}


def test_it_proposes_only_what_is_out_of_place_and_never_your_own_folders(music_library):
    from danplay.library import tidy

    _, _, ids = music_library
    found = tidy.proposals()
    items = _by_from(found["items"])
    album = items[
        os.path.join(
            "Artistas", "Barak", "Generacion Radical (2018)", "06 - Hasta Ver Tu Gloria.mp3"
        )
    ]
    assert album["to"] == os.path.join("Artistas", "Barak", "Barak - Hasta Ver Tu Gloria.mp3")
    assert album["sure"] and "dentro de una subcarpeta" in album["why"]
    loose = items["Barak - Nueva (Official Video).mp3"]
    assert loose["to"] == os.path.join("Artistas", "Barak", "Barak - Nueva.mp3")
    assert loose["sure"] and loose["why"][0] == "suelta en la raíz de tu música"
    cam = items[os.path.join("Artistas", "Barak", "Barak - Danzar (Drum Cam).mp3")]
    assert cam["to"] == os.path.join("Pistas", "Barak - Danzar (Drum Cam).mp3"), (
        "a su carpeta, con su nombre"
    )
    assert not cam["sure"]
    solo = items["sin nombre.mp3"]
    assert not solo["sure"], "lo que solo dicen las etiquetas, para revisar"
    assert solo["to"].startswith(os.path.join("Artistas", "Un Canal De YouTube"))
    # las bien puestas y las carpetas propias, nada
    assert not any("Pistas" in p["from"] for p in found["items"])
    assert not any(p["from"].endswith("Barak - Mi Gozo.mp3") for p in found["items"])
    assert len(found["items"]) == 4
    # las seguras primero
    assert [p["sure"] for p in found["items"]] == sorted(
        (p["sure"] for p in found["items"]), reverse=True
    )
    assert {p["id"] for p in found["items"]} == {
        ids["disco"],
        ids["suelta"],
        ids["cam"],
        ids["etiquetas"],
    }


def test_applying_keeps_the_same_song_and_tidies_the_empty_folder(music_library):
    from danplay import library, tags
    from danplay.library import tidy

    root, files, ids = music_library
    cid = ids["disco"]
    library.set_study(cid, {"speed": 0.8})
    done = tidy.apply(cid, artist="Barak", title="Hasta Ver Tu Gloria")
    new = root / "Artistas" / "Barak" / "Barak - Hasta Ver Tu Gloria.mp3"
    assert done["to"] == str(new) and new.is_file()
    assert not files["disco"].exists()
    assert not files["disco"].parent.exists(), "la carpeta del disco, vacia, no se queda"
    song = _song(library.by_id(cid))
    assert song["path"] == str(new), "la misma cancion, en su sitio nuevo"
    assert json.loads(song["study"])["speed"] == 0.8, "con todo lo suyo"
    # las etiquetas no se tocan si no se pide: la suya dice mas («(Live)»)
    assert tags.read(str(new))["title"] == "Hasta Ver Tu Gloria (Live)"
    assert not any(p["id"] == cid for p in tidy.proposals()["items"]), "ya esta en su sitio"
    # y se puede deshacer
    back = tidy.apply(
        cid,
        to=os.path.join(
            "Artistas", "Barak", "Generacion Radical (2018)", "06 - Hasta Ver Tu Gloria.mp3"
        ),
    )
    assert back["to"] == str(files["disco"]) and files["disco"].is_file()


def test_corrected_by_hand_it_goes_to_the_tags_too(music_library):
    from danplay import library, tags
    from danplay.library import tidy

    root, _, ids = music_library
    cid = ids["etiquetas"]
    done = tidy.apply(cid, artist="Marcos Witt", title="Exaltate", write_tags=True)
    new = root / "Artistas" / "Marcos Witt" / "Marcos Witt - Exaltate.mp3"
    assert done["to"] == str(new) and new.is_file()
    read = tags.read(str(new))
    assert (read["artist"], read["title"]) == ("Marcos Witt", "Exaltate")
    assert _song(library.by_id(cid))["artist"] == "Marcos Witt"


def test_what_was_worked_out_from_the_sound_follows_the_song(music_library):
    """La rejilla del clic se reconoce por la ruta: al moverla, se iba (y el
    clic volvia a tardar 15-20 s)."""
    from danplay import beatgrid, config
    from danplay.library import tidy
    from danplay.pulse import key_of

    _, files, ids = music_library
    src = str(files["suelta"])
    cache = config.DATA_DIR / "beats" / f"{key_of(src)}.json"
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text(json.dumps({"version": beatgrid.VERSION, "path": src, "grid": {"bpm": 90}}))
    done = tidy.apply(ids["suelta"], artist="Barak", title="Nueva")
    assert beatgrid.cached(done["to"])
    assert beatgrid.grid_of(done["to"]) == {"bpm": 90}
    assert not cache.exists()


def test_the_api_lists_applies_and_refuses_paths_outside(music_library, connect_api):
    _, _, ids = music_library
    client = connect_api()
    r = client.get("/api/tidy")
    assert r.status_code == 200 and len(r.json()["items"]) == 4
    assert r.json()["looked"] >= 7
    r = client.post(f"/api/song/{ids['suelta']}/tidy", json={"artist": "Barak", "title": "Nueva"})
    assert r.status_code == 200, r.text
    assert r.json()["song"]["id"] == ids["suelta"]
    assert r.json()["to"].endswith(os.path.join("Artistas", "Barak", "Barak - Nueva.mp3"))
    r = client.post(f"/api/song/{ids['cam']}/tidy", json={"to": "../fuera.mp3"})
    assert r.status_code == 409
    assert client.post("/api/song/999999/tidy", json={"to": "x.mp3"}).status_code == 404
    assert client.post(f"/api/song/{ids['cam']}/tidy", json={}).status_code == 409
