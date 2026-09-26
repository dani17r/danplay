"""Separar canciones en pistas (danplay/stems.py y sus rutas), sin la red.

El proceso que separa se cambia por uno de mentira que hace lo mismo hacia
fuera: dice como va por la salida estandar y deja una pista por fuente (un
tono corto hecho con ffmpeg) y sus ondas. Las que la cancion «no tiene» se
dicen en un archivo junto a ella (`<cancion>.callar`, con comas): esas no
llegan a pista, como hace el motor de verdad con lo que casi no suena. Lo que
decide el motor de verdad (que suena, lo que queda) se prueba en
test_separation.py. Todo lo demas es de verdad: la cola y sus dos pasadas,
la carpeta, el manifiesto, el indice, el escaneo, la papelera y la mezcla
con ffmpeg.
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
    import json, subprocess, sys, time
    from pathlib import Path
    job = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    say = lambda **m: print(json.dumps(m), flush=True)
    if Path(job["input"]).name.startswith("rota"):
        say(error="ffmpeg no pudo leer la cancion")
        sys.exit(1)
    # la pasada buena espera, si se le pide, a que la prueba la deje seguir
    hold = Path(job["input"] + ".esperar")
    if job.get("keep") is not None:
        while hold.exists():
            time.sleep(0.02)
    say(step="decode"); say(step="separate", seconds=1.0)
    out = Path(job["output"]); out.mkdir(parents=True, exist_ok=True)
    side = Path(job["input"] + ".callar")
    silent = set(side.read_text().split(",")) if side.exists() else set()
    freqs = {"drums": 110, "vocals": 440, "bass": 55, "guitar": 330, "piano": 660, "other": 880}
    def tone(source):
        subprocess.run([job["ffmpeg"], "-v", "error", "-y", "-f", "lavfi", "-i",
                        f"sine=frequency={freqs[source]}:sample_rate=44100:duration=1",
                        "-ac", "2", "-c:a", "flac", str(out / job["files"][source])], check=True)
    rest, keep, carry = job["rest"], job.get("keep"), job.get("carry") or {}
    made = [s for net in job["nets"] for s in net["take"]]
    for i, net in enumerate(job["nets"]):
        say(step="load")
        for s in net["take"]:
            tone(s)
        say(done=i + 1, total=len(job["nets"]))
    stays = lambda s: s in keep if keep is not None else s not in silent
    parts = [s for s in job["files"] if s != rest and (s in made or s in carry) and stays(s)]
    for s in made:
        if s not in parts:
            (out / job["files"][s]).unlink()
    say(step="compose")
    tracks = [s for s in job["files"] if s in parts or (s == rest and stays(rest))]
    if stays(rest):
        tone(rest)
    names = {s: job["files"][s] for s in tracks}
    if job.get("format") == "opus":
        say(step="encode")
        for s in tracks:
            flac = out / names[s]
            subprocess.run([job["ffmpeg"], "-v", "error", "-y", "-i", str(flac),
                            "-c:a", "libopus", str(flac.with_suffix(".opus"))], check=True)
            flac.unlink()
            names[s] = flac.with_suffix(".opus").name
    waves = {"buckets": 2, "tracks": {s: {"peaks": [0.5, 1.0], "rms": [0.2, 0.4]} for s in tracks}}
    Path(job["waves"]).write_text(json.dumps(waves))
    levels = {s: {"db": -45.0 if s in silent else -8.0, "active": 0.0 if s in silent else 0.6}
              for s in [*made, rest]}
    say(ok=True, seconds=1.0, took=0.1, tracks=names, levels=levels)
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
    monkeypatch.setattr(stems, "installed", lambda part: True)
    monkeypatch.setattr(config, "STEMS_FORMAT", "flac")
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


def _separate(cid: int) -> dict:
    """Pide separarla y espera a que acabe la cola: las dos pasadas."""
    from danplay import stems
    from danplay.api import jobs

    stems.request(cid)
    job = jobs.wait(stems.JOB, timeout=60)
    assert job is not None and not job["active"]
    return job


def _events(since: int = 0) -> list[tuple[int, str, bool]]:
    from danplay import stems

    return [(e["id"], e["stage"], e["ok"]) for e in stems.status()["events"] if e["seq"] > since]


def _last_seq() -> int:
    from danplay import stems

    return max((e["seq"] for e in stems.status()["events"]), default=0)


def test_a_song_is_separated_into_its_own_folder(lib):
    root, _ = lib
    cid = _id("Barak - Mi Gozo.mp3")
    since = _last_seq()
    job = _separate(cid)
    assert not job["error"], job["error"]
    # dos pasadas: la rapida y la buena, que la mejora
    assert [s["id"] for s in job["result"]["separated"]] == [cid, cid]
    assert _events(since) == [(cid, "separate", True), (cid, "refine", True)]
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
    assert manifest["quality"] == "mejor" and manifest["model"] == "htdemucs_6s+htdemucs_ft"
    assert manifest["source"]["file"] == "Barak - Mi Gozo.mp3"
    assert manifest["source"]["folder"] == os.path.join("Artistas", "Barak")
    assert manifest["dropped"] == []
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
    assert info["complete"] and info["best"] and info["quality"] == "mejor"
    assert info["tracks"][0]["wave"] == {"peaks": [0.5, 1.0], "rms": [0.2, 0.4]}
    # no quedan carpetas a medio hacer, ni las rapidas de antes
    assert not [p for p in (root / "Separadas").iterdir() if p.name.startswith(".")]


def test_the_quick_stems_can_be_used_while_they_are_improved(lib):
    """La pasada rapida deja las pistas en su sitio antes de mejorarlas: el
    estudio las usa ya. La buena las cambia al final, de golpe."""
    from danplay import stems

    _, songs = lib
    cid = _id("Barak - Mi Gozo.mp3")
    hold = Path(songs["gozo"] + ".esperar")
    hold.write_text("")
    since = _last_seq()
    stems.request(cid)
    limit = time.monotonic() + 30
    while (stems.status()["current"] or {}).get("stage") != "refine":
        assert time.monotonic() < limit, stems.status()
        time.sleep(0.02)
    info = _info(cid)
    assert info["complete"] and info["quality"] == "rapida" and not info["best"]
    assert _events(since) == [(cid, "separate", True)]
    assert stems.status()["current"]["id"] == cid
    guitar = Path(info["folder"]) / "Guitarra.flac"
    before = guitar.read_bytes()
    hold.unlink()
    _separate(cid)  # ya esta en marcha: no se repite, solo se espera
    assert _info(cid)["best"]
    # la guitarra no tiene especialista: la de la rapida, tal cual (y la voz)
    assert guitar.read_bytes() == before


def test_quick_ones_go_before_any_improvement(lib):
    """Un repertorio entero: primero todas se pueden usar, luego se mejoran."""
    from danplay import stems
    from danplay.api import jobs

    a, b = _id("Barak - Mi Gozo.mp3"), _id("New Wine - Shekinah.mp3")
    since = _last_seq()
    stems.request(a)
    stems.request(b)
    job = jobs.wait(stems.JOB, timeout=60)
    assert job is not None and not job["error"]
    assert _events(since) == [
        (a, "separate", True),
        (b, "separate", True),
        (a, "refine", True),
        (b, "refine", True),
    ]


def test_what_the_song_does_not_have_is_not_a_track(lib):
    """Sin piano ni guitarra: esas no salen (lo poco suyo va a «Otros»)."""
    root, songs = lib
    Path(songs["gozo"] + ".callar").write_text("piano,guitar")
    cid = _id("Barak - Mi Gozo.mp3")
    assert not _separate(cid)["error"]
    info = _info(cid)
    assert [t["source"] for t in info["tracks"]] == ["drums", "vocals", "bass", "other"]
    assert info["dropped"] == ["guitar", "piano"]
    folder = root / "Separadas" / "Barak - Mi Gozo"
    assert not (folder / "Piano.flac").exists()
    manifest = json.loads((folder / ".danplay-pistas.json").read_text(encoding="utf-8"))
    assert manifest["levels"]["piano"]["db"] < -30


def test_quick_stems_left_halfway_are_only_improved(lib, monkeypatch):
    """Si la app se cerro a mitad de la mejora, pedirla otra vez no vuelve a
    separar: solo mejora las rapidas que ya estan."""
    from danplay import stems

    cid = _id("Barak - Mi Gozo.mp3")
    real = stems.refine_song
    monkeypatch.setattr(
        stems, "refine_song", lambda *a, **k: (_ for _ in ()).throw(stems.SeparateError("corte"))
    )
    since = _last_seq()
    _separate(cid)
    assert _events(since) == [(cid, "separate", True), (cid, "refine", False)]
    assert _info(cid)["quality"] == "rapida"
    monkeypatch.setattr(stems, "refine_song", real)
    since = _last_seq()
    _separate(cid)
    assert _events(since) == [(cid, "refine", True)]
    assert _info(cid)["best"]


def test_the_lists_say_whether_the_stems_are_the_best(lib):
    from danplay import library

    cid = _id("Barak - Mi Gozo.mp3")
    _separate(cid)
    rows = {r["id"]: r for r in library.search("", limit=100, light=True)}
    assert rows[cid]["has_stems"] and rows[cid]["stems_best"] is True
    library.update(cid, stems=json.dumps({"folder": "x", "model": "htdemucs_6s"}))
    rows = {r["id"]: r for r in library.search("", limit=100, light=True)}
    assert rows[cid]["has_stems"] and rows[cid]["stems_best"] is False
    assert library.light(dict(_song(cid)))["stems_best"] is False


def test_the_stems_can_be_kept_in_opus(lib, monkeypatch):
    from danplay import config, convert, stems

    ffmpeg = convert.tool("ffmpeg")
    if not (ffmpeg and stems._has_opus(ffmpeg)):
        pytest.skip("este ffmpeg no sabe hacer Opus")
    monkeypatch.setattr(config, "STEMS_FORMAT", "opus")
    cid = _id("Barak - Mi Gozo.mp3")
    assert not _separate(cid)["error"]
    info = _info(cid)
    assert info["complete"] and info["best"]
    assert {Path(t["path"]).suffix for t in info["tracks"]} == {".opus"}


def test_what_a_crash_left_halfway_is_swept_away(lib):
    root, _ = lib
    leftover = root / "Separadas" / ".separando-abc123"
    leftover.mkdir(parents=True)
    (leftover / "Bateria.flac").write_bytes(b"a medias")
    assert not _separate(_id("Barak - Mi Gozo.mp3"))["error"]
    assert not leftover.exists()


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
    # y siguen siendo las mejores: no se ofrece separarla otra vez mejor
    rows = {r["id"]: r for r in library.search("", limit=100, light=True)}
    assert rows[cid]["stems_best"]


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
    _separate(cid)
    Path(lib[1]["gozo"] + ".callar").write_text("piano,guitar")
    _separate(cid)
    folder = root / "Separadas" / "Barak - Mi Gozo"
    assert sorted(p.name for p in folder.glob("*.flac")) == [
        "Bajo.flac",
        "Bateria.flac",
        "Otros.flac",
        "Voces.flac",
    ]
    # las de antes, a la papelera; las rapidas de cada vez, no (son un paso)
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
    assert sorted(s["id"] for s in job["result"]["separated"]) == sorted([a, a, b, b])
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
    assert {s["id"] for s in job["result"]["separated"]} == {_id("Barak - Mi Gozo.mp3")}


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

    part = {
        "graph": "htdemucs",
        "file": "w.safetensors",
        "url": "https://x/w",
        "bytes": len(good),
        "sha256": hashlib.sha256(good).hexdigest(),
    }
    monkeypatch.setattr(stems, "parts", lambda: {"drums": part})
    monkeypatch.setattr(stems.urllib.request, "urlopen", lambda *a, **k: Answer(b"otra cosa" * 10))
    with pytest.raises(stems.SeparateError, match="sha256"):
        stems.download("drums")
    assert not list((tmp_path / "separador").iterdir())
    seen = []
    monkeypatch.setattr(stems.urllib.request, "urlopen", lambda *a, **k: Answer(good))
    path = stems.download("drums", lambda got, total: seen.append((got, total)))
    assert path.read_bytes() == good and stems.installed("drums")
    assert seen[-1] == (len(good), len(good))
    # y borrarlo lo quita todo (tambien lo de versiones de antes)
    (tmp_path / "separador" / "955717e8.safetensors").write_bytes(b"de 1.16.0")
    assert stems.remove_weights() == 2
    assert not stems.installed("drums")


def test_the_separator_is_one_download_with_its_specialists():
    """Una sola cosa que bajar: la red rapida y un especialista por cada
    fuente que se mejora, todos de su autor y con su sha256."""
    from danplay import stems

    parts = stems.parts()
    assert set(parts) == {"htdemucs_6s", "drums", "bass"}
    assert {p["graph"] for p in parts.values()} == {"htdemucs_6s", "htdemucs"}
    for p in parts.values():
        assert p["url"].startswith("https://huggingface.co/adefossez/")
        assert len(p["sha256"]) == 64 and p["bytes"] > 50_000_000
        assert (stems.GRAPHS / f"{p['graph']}.onnx").is_file()
    assert len({p["file"] for p in parts.values()}) == 3


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
    assert status["ok"] and status["bytes"] > 200_000_000 and status["format"] == "flac"
    assert "models" not in status
    cid = _id("New Wine - Shekinah.mp3")
    assert client.get(f"/api/song/{cid}/stems").status_code == 404
    r = run_job(client, f"/api/song/{cid}/separate", "separacion")
    assert [s["id"] for s in r["separated"]] == [cid, cid]
    stems = client.get(f"/api/song/{cid}/stems").json()
    assert [t["file"] for t in stems["tracks"]][:2] == ["Bateria.flac", "Voces.flac"]
    assert all(t["exists"] for t in stems["tracks"])
    assert client.delete(f"/api/song/{cid}/stems").json()["ok"]
    assert client.get(f"/api/song/{cid}/stems").status_code == 404
    assert client.post("/api/song/999999/separate").status_code == 404
