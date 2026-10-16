"""Aprovechar lo hecho de una cancion que vuelve en otro archivo
(danplay/reuse.py): mover la carpeta de musica y volver a bajar la cancion no
obliga a separar otra vez, ni a sacar el clic, ni pierde sus marcadores."""

import json
import subprocess
import wave
from pathlib import Path

import numpy as np
import pytest
from conftest import FFMPEG

RATE = 22050


def _song(seed: int, seconds: float = 30.0):
    """Algo con golpes que se distinguen: un bombo irregular y ruido de
    platos, como una cancion para quien mide el desfase."""
    rng = np.random.default_rng(seed)
    n = int(seconds * RATE)
    kick = np.zeros(n, np.float32)
    hats = np.zeros(n, np.float32)
    t = 0.3
    while t < seconds - 0.5:
        i = int(t * RATE)
        k = np.arange(int(0.12 * RATE))
        kick[i : i + k.size] += (np.sin(2 * np.pi * 60 * k / RATE) * np.exp(-k / 900))[: n - i]
        j = int((t + 0.25) * RATE)
        h = rng.standard_normal(int(0.03 * RATE)) * np.exp(-np.arange(int(0.03 * RATE)) / 150)
        hats[j : j + h.size] += 0.3 * h[: max(0, n - j)]
        t += rng.uniform(0.35, 0.65)
    return 0.6 * kick, hats.astype(np.float32)


def _write(path, x, lag=0.0, codec=("libmp3lame", "-b:a", "128k"), artist="", title=""):
    """Un archivo con `x`, corrido `lag` segundos (silencio delante, o sin lo
    de delante), y sus etiquetas."""
    if lag > 0:
        x = np.concatenate([np.zeros(int(lag * RATE), np.float32), x])
    elif lag < 0:
        x = x[int(-lag * RATE) :]
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp.wav")
    with wave.open(str(tmp), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype("<i2").tobytes())
    assert FFMPEG
    subprocess.run(
        [FFMPEG, "-v", "error", "-y", "-i", str(tmp), "-c:a", codec[0], *codec[1:], str(path)],
        check=True,
    )
    tmp.unlink()
    if artist:
        from mutagen.id3 import ID3, TIT2, TPE1, ID3NoHeaderError

        try:
            id3 = ID3(path)
        except ID3NoHeaderError:
            id3 = ID3()
        id3.add(TPE1(encoding=3, text=artist))
        id3.add(TIT2(encoding=3, text=title))
        id3.save(path)
    return path


def test_the_offset_between_two_files_to_the_millisecond():
    from danplay import reuse

    kick, hats = _song(1)
    x = kick + hats
    for lag in (0.0, 0.3217, -1.25, 4.0):
        new = (
            np.concatenate([np.zeros(int(lag * RATE), np.float32), x])
            if lag > 0
            else x[int(-lag * RATE) :]
        )
        measured = reuse.offset(new, x, RATE)
        assert measured is not None
        got, score = measured
        assert abs(got - lag) < 0.0005, (lag, got)
        assert score > 0.8
    other = sum(_song(7))
    measured = reuse.offset(other, x, RATE)
    assert measured is not None
    _, score = measured
    assert score < reuse.MIN_SCORE, "otra cancion no se parece"


def test_the_study_moves_with_the_offset():
    from danplay import reuse

    study = {
        "loop": [5, 8],
        "markers": [{"t": 0.1, "label": "fuera"}, {"t": 10, "end": 20, "label": "coro"}],
        "cues": [{"t": 9.5, "file": "/x.wav"}],
        "metronome": {"bpm": 120, "own": {"beats": [0.2, 0.7, 1.2], "bars": [0, 2], "meter": 4}},
        "speed": 0.9,
    }
    out = reuse.shift_study(study, -0.5)
    assert out["loop"] == [4.5, 7.5]
    assert out["markers"] == [{"t": 9.5, "end": 19.5, "label": "coro"}]
    assert out["cues"][0]["t"] == 9.0 and out["speed"] == 0.9
    assert out["metronome"]["own"] == {"beats": [0.2, 0.7], "bars": [1], "meter": 4}
    assert out["metronome"]["bpm"] == 120
    assert reuse.shift_study(study, 0.001) is study, "lo que no se oye no se toca"


def _stems(song_row, folder, kick, hats, lag=0.0):
    """Unas pistas separadas de mentira (el bombo y lo demas) con su
    manifiesto, enlazadas a la cancion."""
    from danplay import library, stems

    folder.mkdir(parents=True, exist_ok=True)
    _write(folder / "Bateria.flac", kick, lag, codec=("flac",))
    _write(folder / "Otros.flac", hats, lag, codec=("flac",))
    manifest = {
        "version": 2,
        "quality": stems.FINAL,
        "model": "prueba",
        "source": {
            "file": song_row["file"],
            "folder": song_row["folder"],
            "size": song_row["size"],
            "duration": song_row["duration"],
            "artist": song_row["artist"],
            "title": song_row["title"],
        },
        "tracks": [
            {"source": "drums", "file": "Bateria.flac", "name": "Bateria"},
            {"source": "other", "file": "Otros.flac", "name": "Otros"},
        ],
    }
    (folder / stems.MANIFEST).write_text(json.dumps(manifest), encoding="utf-8")
    import os

    rel = os.path.relpath(folder, song_row["root"])
    library.update(song_row["id"], stems=json.dumps({"folder": rel, "model": "prueba"}))


def _row(path) -> dict:
    from danplay import library

    brief = library.by_path(str(path))
    assert brief, f"no esta en el indice: {path}"
    song = library.by_id(brief["id"])
    assert song
    return dict(song)


@pytest.fixture
def home(configured_library, tmp_path, monkeypatch):
    from danplay import config

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    return configured_library


def test_moving_the_music_and_downloading_again_keeps_everything(home, tmp_path):
    """Lo que paso: se cambio la carpeta de musica de sitio y se volvio a
    bajar una cancion que ya tenia sus pistas. Vuelve con su id (sus listas),
    sus pistas (sin separar otra vez) y sus marcadores, todo corrido lo que
    traiga de mas la descarga nueva."""
    from danplay import library, reuse, stems

    lib, _ = home
    kick, hats = _song(3)
    first = _write(
        lib / "Artistas/Grupo/Grupo - Cancion Larga.mp3",
        kick + hats,
        artist="Grupo",
        title="Cancion Larga",
    )
    library.add_folder(lib)
    library.scan()
    row = _row(first)
    _stems(row, lib / "Separadas" / "Grupo - Cancion Larga", kick, hats)
    library.set_study(row["id"], {"loop": [5, 8], "markers": [{"t": 10, "label": "coro"}]})
    # la carpeta de antes deja de ser la de musica (y el archivo ya no esta)
    first.unlink()
    library.remove_folder(lib)
    new_lib = tmp_path / "Nueva"
    again = _write(
        new_lib / "Artistas/Grupo/Grupo - Cancion Larga (Official Video).mp3",
        kick + hats,
        lag=0.25,
        codec=("libmp3lame", "-b:a", "192k"),
        artist="Grupo",
        title="Cancion Larga",
    )
    library.add_folder(new_lib)
    library.scan()
    assert reuse.idle(60)
    back = _row(again)
    assert back["id"] == row["id"], "la misma cancion: su id, sus listas"
    data = stems.info(back)
    assert data and data["complete"], "sus pistas, sin separar otra vez"
    assert str(new_lib) in data["folder"]
    manifest = stems.read_manifest(Path(data["folder"]))
    assert manifest and abs(manifest["shifted"] - 0.25) < 0.003
    study = json.loads(back["study"])
    assert abs(study["markers"][0]["t"] - 10.25) < 0.003
    assert abs(study["loop"][0] - 5.25) < 0.003
    # y las pistas, corridas, cuadran con el archivo nuevo
    measured = reuse.lag_between(str(again), [t["path"] for t in data["tracks"]])
    assert measured is not None
    lag, score = measured
    assert abs(lag) < 0.002 and score > 0.8


def test_a_second_copy_takes_the_click_and_the_study_but_not_another_song(home, tmp_path):
    from danplay import beatgrid, library, reuse

    lib, _ = home
    kick, hats = _song(5)
    first = _write(
        lib / "Artistas/Grupo/Grupo - Otra Mas.mp3", kick + hats, artist="Grupo", title="Otra Mas"
    )
    library.add_folder(lib)
    library.scan()
    row = _row(first)
    library.set_study(row["id"], {"markers": [{"t": 12, "label": "puente"}]})
    beats = [round(0.5 + 0.5 * k, 4) for k in range(50)]
    grid = {
        "bpm": 120.0,
        "meter": 4,
        "beats": beats,
        "bars": list(range(0, 50, 4)),
        "first_downbeat": 0,
    }
    beatgrid._save(str(first), grid)
    # otra descarga de la misma (un poco recortada por delante), al lado
    second = _write(
        lib / "Artistas/Grupo/Grupo - Otra Mas (Audio).mp3",
        kick + hats,
        lag=-0.1,
        artist="Grupo",
        title="Otra Mas",
    )
    # y otra cancion que se llama igual pero no es ella
    impostor_kick, impostor_hats = _song(11)
    impostor = _write(
        lib / "Artistas/Grupo/Grupo - Otra Mas (En Vivo).mp3",
        impostor_kick + impostor_hats,
        artist="Grupo",
        title="Otra Mas",
    )
    library.scan()
    assert reuse.idle(60)
    copy = _row(second)
    assert copy["id"] != row["id"], "la de antes sigue: esta es otra fila"
    assert beatgrid.cached(str(second)), "el clic, sin volver a calcularlo"
    got = beatgrid.grid_of(str(second))["beats"]
    assert abs(got[0] - 0.4) < 0.003 and len(got) == 50
    assert abs(json.loads(copy["study"])["markers"][0]["t"] - 11.9) < 0.003
    other = _row(impostor)
    assert not other["study"] and not beatgrid.cached(str(impostor)), "otra version: nada"


def test_asking_to_separate_a_copy_takes_the_stems_already_made(home, monkeypatch):
    """Si se pide separar una que ya esta separada en otro archivo (y aun no
    se habia aprovechado), se aprovechan sus pistas sin separar."""
    from danplay import library, reuse, stems

    monkeypatch.setattr(reuse, "later", lambda *a, **k: None)
    lib, _ = home
    kick, hats = _song(9)
    first = _write(
        lib / "Artistas/Grupo/Grupo - Tercera.mp3", kick + hats, artist="Grupo", title="Tercera"
    )
    library.add_folder(lib)
    library.scan()
    row = _row(first)
    _stems(row, lib / "Separadas" / "Grupo - Tercera", kick, hats)
    second = _write(
        lib / "Entrada/tercera.mp3", kick + hats, lag=0.05, artist="Grupo", title="Tercera"
    )
    library.scan()
    copy = _row(second)
    assert stems.info(copy) is None
    state = stems.request(copy["id"])
    assert state["reused"] is True and not state["queue"]
    data = stems.info(_row(second))
    theirs = stems.info(_row(first))
    assert data and data["complete"] and theirs and data["folder"] != theirs["folder"]
    assert theirs["complete"], "la de antes conserva las suyas"
    assert state["events"][-1]["stage"] == "reuse"


def test_what_was_lost_before_comes_back_with_one_pass(home, tmp_path, monkeypatch):
    """Lo que ya le habia pasado al usuario: la cancion re-bajada entro como
    nueva (sin listas, sin pistas). Una pasada por la biblioteca la junta con
    la de antes: vuelve a sus listas y recupera sus pistas."""
    import importlib

    from danplay import library, playlists, reuse, stems

    scan_module = importlib.import_module("danplay.library.scan")

    lib, _ = home
    kick, hats = _song(13)
    first = _write(
        lib / "Artistas/Grupo/Grupo - Cuarta.mp3", kick + hats, artist="Grupo", title="Cuarta"
    )
    library.add_folder(lib)
    library.scan()
    row = _row(first)
    _stems(row, lib / "Separadas" / "Grupo - Cuarta", kick, hats)
    domingo = playlists.create("Domingo")
    playlists.add(domingo["id"], [row["id"]])
    first.unlink()
    library.remove_folder(lib)
    # como antes de saber juntarlas: entra como nueva
    monkeypatch.setattr(scan_module, "_find_twin", lambda conn, fresh: None)
    monkeypatch.setattr(reuse, "later", lambda *a, **k: None)
    monkeypatch.setattr(playlists, "relink_ghosts", lambda ids: 0)
    new_lib = tmp_path / "Otra"
    again = _write(
        new_lib / "Grupo - Cuarta.mp3", kick + hats, lag=0.12, artist="Grupo", title="Cuarta"
    )
    library.add_folder(new_lib)
    library.scan()
    fresh = _row(again)
    assert fresh["id"] != row["id"] and stems.info(fresh) is None
    # en su lista sigue la de antes, como «No está»
    assert [s["id"] for s in playlists.songs(domingo["id"], ghosts=False)] == []
    assert [s.get("ghost") for s in playlists.songs(domingo["id"])] == [True]
    told = reuse.sweep()
    assert told["lists"] == 1 and told["stems"] == 1
    assert [s["id"] for s in playlists.songs(domingo["id"])] == [fresh["id"]]
    data = stems.info(_row(again))
    assert data and data["complete"]
    assert reuse.sweep()["stems"] == 0, "la segunda pasada no hace nada"
