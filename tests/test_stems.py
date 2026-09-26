"""Separar canciones en pistas (danplay/stems.py y sus rutas), sin la red.

El proceso que separa se cambia por uno de mentira que hace lo mismo hacia
fuera: dice como va por la salida estandar y deja una pista por fuente (un
tono corto hecho con ffmpeg) y sus ondas. Todo lo demas es de verdad: la
cola, la carpeta, el manifiesto, el indice, el escaneo, la papelera y la
mezcla con ffmpeg.
"""

import json
import os
import sys
import textwrap
import time
from pathlib import Path

import pytest
from conftest import run_job

FAKE_ENGINE = textwrap.dedent(
    """
    import json, subprocess, sys
    from pathlib import Path
    job = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    say = lambda **m: print(json.dumps(m), flush=True)
    if Path(job["input"]).name.startswith("rota"):
        say(error="ffmpeg no pudo leer la cancion")
        sys.exit(1)
    say(step="load"); say(step="decode"); say(step="separate", seconds=1.0)
    out = Path(job["output"]); out.mkdir(parents=True, exist_ok=True)
    freqs = {"drums": 110, "vocals": 440, "bass": 55, "guitar": 330, "piano": 660, "other": 880}
    for i, (source, name) in enumerate(job["files"].items()):
        subprocess.run([job["ffmpeg"], "-v", "error", "-y", "-f", "lavfi", "-i",
                        f"sine=frequency={freqs[source]}:sample_rate=44100:duration=1",
                        "-ac", "2", "-c:a", "flac", str(out / name)], check=True)
        say(done=i + 1, total=len(job["files"]))
    waves = {"buckets": 2, "tracks": {s: {"peaks": [0.5, 1.0], "rms": [0.2, 0.4]} for s in job["files"]}}
    Path(job["waves"]).write_text(json.dumps(waves))
    say(ok=True, seconds=1.0, took=0.1)
    """
)


@pytest.fixture
def lib(configured_library, tmp_path, monkeypatch):
    from danplay import config, library, stems

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    (tmp_path / "datos").mkdir()
    engine = tmp_path / "motor.py"
    engine.write_text(FAKE_ENGINE, encoding="utf-8")
    monkeypatch.setattr(stems, "_command", lambda job_file: [sys.executable, str(engine), job_file])
    monkeypatch.setattr(stems, "installed", lambda model: True)
    root, songs = configured_library
    library.add_folder(root)
    library.scan()
    yield root, songs
    # que ninguna prueba deje la cola andando para la siguiente
    from danplay.api import jobs

    stems.cancel()
    jobs.wait(stems.JOB, timeout=20)


def _song(cid: int) -> dict:
    from danplay import library

    song = library.by_id(cid)
    assert song is not None
    return dict(song)


def _info(cid: int) -> dict:
    from danplay import stems

    data = stems.info(_song(cid))
    assert data is not None
    return data


def _id(name: str) -> int:
    from danplay import library

    return next(s["id"] for s in library.search("", limit=100) if s["file"] == name)


def _separate(cid: int, model: str = "6") -> dict:
    from danplay import stems
    from danplay.api import jobs

    stems.request(cid, model)
    job = jobs.wait(stems.JOB, timeout=60)
    assert job is not None and not job["active"]
    return job


def test_a_song_is_separated_into_its_own_folder(lib):

    root, _ = lib
    cid = _id("Barak - Mi Gozo.mp3")
    job = _separate(cid)
    assert not job["error"], job["error"]
    assert [s["id"] for s in job["result"]["separated"]] == [cid]
    folder = root / "Separadas" / "Barak - Mi Gozo"
    names = sorted(p.name for p in folder.iterdir())
    assert names == sorted(
        [
            ".danplay-ondas.json",
            ".danplay-pistas.json",
            "Bajo.flac",
            "Bateria.flac",
            "Guitarra.flac",
            "Otros.flac",
            "Piano.flac",
            "Voces.flac",
        ]
    )
    manifest = json.loads((folder / ".danplay-pistas.json").read_text(encoding="utf-8"))
    assert manifest["model"] == "htdemucs_6s"
    assert manifest["source"]["file"] == "Barak - Mi Gozo.mp3"
    assert manifest["source"]["folder"] == os.path.join("Artistas", "Barak")
    # en el orden del mezclador, con su nombre en castellano
    assert [t["name"] for t in manifest["tracks"]] == [
        "Batería",
        "Voces",
        "Bajo",
        "Guitarra",
        "Piano",
        "Otros",
    ]
    info = _info(cid)
    assert info["complete"] and info["model"] == "htdemucs_6s"
    assert info["tracks"][0]["wave"] == {"peaks": [0.5, 1.0], "rms": [0.2, 0.4]}
    # no quedan carpetas a medio hacer
    assert not [p for p in (root / "Separadas").iterdir() if p.name.startswith(".")]


def test_the_four_track_model_has_no_guitar_nor_piano(lib):

    cid = _id("Barak - Mi Gozo.mp3")
    assert not _separate(cid, "4")["error"]
    info = _info(cid)
    assert [t["source"] for t in info["tracks"]] == ["drums", "vocals", "bass", "other"]


def test_the_stems_are_not_songs_of_the_library(lib):
    from danplay import library

    before = library.stats_of()["total"]
    _separate(_id("Barak - Mi Gozo.mp3"))
    library.scan()
    assert library.stats_of()["total"] == before
    assert not [s for s in library.search("", limit=100) if "Separadas" in s["path"]]


def test_the_lists_say_which_songs_have_stems(lib):
    from danplay import library

    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid)
    rows = {r["id"]: r for r in library.search("", limit=100, light=True)}
    assert rows[cid]["has_stems"] is True
    assert "stems" not in rows[cid]
    assert not any(r["has_stems"] for i, r in rows.items() if i != cid)


def test_a_lost_index_finds_the_stems_again(lib):
    """Como todo lo demas, no depende de la base: la carpeta dice de quien es."""
    from danplay import library

    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid)
    library.update(cid, stems="")
    library.scan()
    info = _info(cid)
    assert info is not None and info["complete"]


def test_stems_deleted_by_hand_are_forgotten(lib):
    import shutil

    from danplay import library

    root, _ = lib
    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid)
    shutil.rmtree(root / "Separadas" / "Barak - Mi Gozo")
    library.scan()
    assert _song(cid)["stems"] == ""


def test_separating_again_replaces_the_old_stems(lib, monkeypatch):
    from danplay import library

    root, _ = lib
    trashed = []
    monkeypatch.setattr(library, "trash_path", lambda p: trashed.append(str(p)) or {"ok": True})
    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid, "6")
    _separate(cid, "4")
    folder = root / "Separadas" / "Barak - Mi Gozo"
    assert sorted(p.name for p in folder.glob("*.flac")) == [
        "Bajo.flac",
        "Bateria.flac",
        "Otros.flac",
        "Voces.flac",
    ]
    assert len(trashed) == 1 and ".antes-Barak - Mi Gozo" in trashed[0]


def test_two_songs_with_the_same_name_do_not_share_a_folder(lib):
    from conftest import make_mp3

    from danplay import library

    root, _ = lib
    make_mp3(root / "Otra" / "Barak - Mi Gozo.mp3", artist="Barak", title="Mi Gozo (en vivo)")
    library.scan()
    ids = [s["id"] for s in library.search("", limit=100) if s["file"] == "Barak - Mi Gozo.mp3"]
    assert len(ids) == 2
    for cid in ids:
        _separate(cid)
    folders = {_info(cid)["folder"] for cid in ids}
    assert {Path(f).name for f in folders} == {"Barak - Mi Gozo", "Barak - Mi Gozo (2)"}


def test_the_queue_separates_one_after_another(lib, monkeypatch):
    from danplay import stems
    from danplay.api import jobs

    a, b = _id("Barak - Mi Gozo.mp3"), _id("New Wine - Shekinah.mp3")
    stems.request(a)
    state = stems.request(b)
    stems.request(b)  # pedirla otra vez no la repite
    assert state["current"] is not None or state["queue"]
    job = jobs.wait(stems.JOB, timeout=60)
    assert job is not None
    assert {s["id"] for s in job["result"]["separated"]} == {a, b}
    assert stems.status()["queue"] == [] and stems.status()["current"] is None


def test_a_song_that_fails_does_not_stop_the_queue(lib):
    from conftest import make_mp3

    from danplay import library, stems
    from danplay.api import jobs

    root, _ = lib
    make_mp3(root / "Artistas" / "Barak" / "rota.mp3", artist="Barak", title="Rota")
    library.scan()
    stems.request(_id("rota.mp3"))
    stems.request(_id("Barak - Mi Gozo.mp3"))
    job = jobs.wait(stems.JOB, timeout=60)
    assert job is not None
    assert [f["error"] for f in job["result"]["failed"]] == ["ffmpeg no pudo leer la cancion"]
    assert len(job["result"]["separated"]) == 1


def test_cancelling_stops_and_empties_the_queue(lib, tmp_path, monkeypatch):
    from danplay import stems
    from danplay.api import jobs

    slow = tmp_path / "lento.py"
    slow.write_text(
        "import json, time\nprint(json.dumps({'step': 'load'}), flush=True)\ntime.sleep(30)\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(stems, "_command", lambda job_file: [sys.executable, str(slow), job_file])
    stems.request(_id("Barak - Mi Gozo.mp3"))
    stems.request(_id("New Wine - Shekinah.mp3"))
    limit = time.monotonic() + 10
    while stems.status()["current"] is None or stems._child is None:
        assert time.monotonic() < limit
        time.sleep(0.02)
    stems.cancel()
    job = jobs.wait(stems.JOB, timeout=15)
    assert job is not None and job["error"] == "cancelado"
    assert stems.status()["queue"] == []


def test_the_weights_are_checked_before_being_used(tmp_path, monkeypatch):
    """Un archivo cambiado (o cortado) no se usa, y no queda a medias."""
    import hashlib
    import io

    from danplay import config, stems

    monkeypatch.setattr(config, "DATA_DIR", tmp_path)
    good = b"pesos de verdad" * 1000

    class Answer(io.BytesIO):
        def __enter__(self):
            return self

        def __exit__(self, *a):
            return None

    manifest = {
        "weights": {
            "file": "w.safetensors",
            "url": "https://x/w",
            "bytes": len(good),
            "sha256": hashlib.sha256(good).hexdigest(),
        }
    }
    monkeypatch.setattr(stems, "_manifest_of", lambda model: manifest)
    monkeypatch.setattr(stems.urllib.request, "urlopen", lambda *a, **k: Answer(b"otra cosa" * 10))
    with pytest.raises(stems.SeparateError, match="sha256"):
        stems.download("6")
    assert not list((tmp_path / "separador").iterdir())
    seen = []
    monkeypatch.setattr(stems.urllib.request, "urlopen", lambda *a, **k: Answer(good))
    path = stems.download("6", lambda got, total: seen.append((got, total)))
    assert path.read_bytes() == good and stems.installed("6")
    assert seen[-1] == (len(good), len(good))


def test_trashing_a_song_takes_its_stems_along(lib, monkeypatch):
    from danplay import library

    root, _ = lib
    trashed = []
    monkeypatch.setattr(
        library.songs, "trash_path", lambda p: trashed.append(str(p)) or {"ok": True}
    )
    monkeypatch.setattr(library, "trash_path", lambda p: trashed.append(str(p)) or {"ok": True})
    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid)
    r = library.trash(cid)
    assert r["ok"] and r["stems_trashed"]
    assert str(root / "Separadas" / "Barak - Mi Gozo") in trashed


def test_the_mixer_is_saved_with_the_study(lib):
    from danplay import library

    cid = _id("Barak - Mi Gozo.mp3")
    study = {
        "notes": "entrar tras el redoble",
        "mixer": {
            "on": True,
            "tracks": {
                "drums": {"mute": True, "gain": 1.0, "pan": 0},
                "vocals": {"gain": 1.4, "pan": -0.5},
                "bass": {"gain": 1.0},
                "../x": {"mute": True},
            },
        },
    }
    song = library.set_study(cid, study)
    assert song is not None
    saved = json.loads(song["study"])
    assert saved["mixer"] == {
        "on": True,
        "tracks": {"drums": {"mute": True}, "vocals": {"gain": 1.4, "pan": -0.5}},
    }


# ------------------------------------------------------------------ la mezcla


def test_the_mix_filter_pans_and_mutes_like_the_player():
    from danplay import stems

    assert stems.gains(1.0, 0.0) == (1.0, 1.0)
    assert stems.gains(0.5, -1.0) == (0.5, 0.0)
    assert stems.gains(1.2, 0.5) == pytest.approx((0.6, 1.2))
    f = stems.mix_filter([(1.0, 1.0), (0.6, 1.2)], speed=0.8, pitch=2, rubberband=True)
    assert "[1:a]aformat=channel_layouts=stereo,pan=stereo|c0=0.6000*c0|c1=1.2000*c1[a1]" in f
    assert "amix=inputs=2:normalize=0" in f
    assert "rubberband=tempo=0.8000:pitch=1.12246" in f and f.endswith("[out]")
    assert "atempo=0.5,atempo=0.5000" in stems.mix_filter([(1, 1)], speed=0.25)


def test_the_mix_without_drums_is_saved_and_joins_the_library(lib):
    import subprocess

    root, _ = lib
    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid)
    from fastapi.testclient import TestClient

    from danplay import api

    client = TestClient(api.app)
    tracks = [
        {"source": s, "gain": 0 if s == "drums" else 1}
        for s in ["drums", "vocals", "bass", "guitar", "piano", "other"]
    ]
    target = root / "Pistas" / "Barak - Mi Gozo (sin bateria).mp3"
    target.parent.mkdir()
    r = run_job(
        client, f"/api/song/{cid}/stems/mix", "mezcla", json={"tracks": tracks, "path": str(target)}
    )
    assert r["path"] == str(target) and target.is_file()
    assert r["title"] == "Mi Gozo (sin batería)"
    row = _song(r["id"])
    assert row["title"] == "Mi Gozo (sin batería)" and row["artist"] == "Barak"
    length = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            str(target),
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    assert float(length) == pytest.approx(1.0, abs=0.1)


def test_a_mix_cannot_go_outside_the_library_nor_over_a_song(lib, tmp_path):
    from fastapi.testclient import TestClient

    from danplay import api, stems

    root, songs = lib
    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid)
    tracks = [{"source": "vocals"}]
    with pytest.raises(stems.SeparateError, match="dentro de tu biblioteca"):
        stems.export_mix(cid, tracks, str(tmp_path / "fuera.mp3"))
    with pytest.raises(stems.SeparateError, match="ya hay una canción"):
        stems.export_mix(cid, tracks, songs["tierra"])
    with pytest.raises(stems.SeparateError, match="calladas"):
        stems.export_mix(cid, [{"source": "vocals", "gain": 0}], str(root / "x.mp3"))
    client = TestClient(api.app)
    r = client.post(
        f"/api/song/{cid}/stems/mix", json={"tracks": tracks, "path": "x", "format": "ogg"}
    )
    assert r.status_code == 422


# ------------------------------------------------------------------ la API


def test_the_api_separates_lists_and_deletes_stems(lib, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api, library

    monkeypatch.setattr(library, "trash_path", lambda p: {"ok": True})
    client = TestClient(api.app)
    status = client.get("/api/separate").json()
    assert status["ok"] and status["default"] == "6"
    assert [m["id"] for m in status["models"]] == ["6", "4"]
    cid = _id("New Wine - Shekinah.mp3")
    assert client.get(f"/api/song/{cid}/stems").status_code == 404
    r = run_job(client, f"/api/song/{cid}/separate", "separacion", json={"model": "6"})
    assert r["separated"][0]["id"] == cid
    stems = client.get(f"/api/song/{cid}/stems").json()
    assert [t["file"] for t in stems["tracks"]][:2] == ["Bateria.flac", "Voces.flac"]
    assert all(t["exists"] for t in stems["tracks"])
    assert client.delete(f"/api/song/{cid}/stems").json()["ok"]
    assert client.get(f"/api/song/{cid}/stems").status_code == 404
    assert client.post("/api/song/999999/separate", json={}).status_code == 404
    assert client.post(f"/api/song/{cid}/separate", json={"model": "9"}).status_code == 422
