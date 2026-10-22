"""Abrir una secuencia descargada (danplay/sequence.py): la carpeta de pistas
con su proyecto de Ableton, como las de MultiTracks.com."""

import gzip
import json
import wave
from pathlib import Path

import numpy as np
import pytest

ALS = """<?xml version="1.0" encoding="UTF-8"?>
<Ableton MajorVersion="5">
  <LiveSet>
    <Tracks>
      <AudioTrack>
        <DeviceChain><MainSequencer><Sample><ArrangerAutomation><Events>
          <AudioClip Time="0">
            <IsWarped Value="false" />
            <SampleRef><FileRef><Name Value="Bass.wav" /></FileRef></SampleRef>
          </AudioClip>
          <AudioClip Time="4">
            <IsWarped Value="false" />
            <SampleRef><FileRef><Name Value="Pad.wav" /></FileRef></SampleRef>
          </AudioClip>
        </Events></ArrangerAutomation></Sample></MainSequencer></DeviceChain>
      </AudioTrack>
    </Tracks>
    <MasterTrack>
      <DeviceChain><Mixer>
        <Tempo><ArrangerAutomation><Events>
          <FloatEvent Time="-63072000" Value="120" />
        </Events></ArrangerAutomation></Tempo>
        <TimeSignature><ArrangerAutomation><Events>
          <EnumEvent Time="-63072000" Value="201" />
          <EnumEvent Time="16" Value="200" />
          <EnumEvent Time="19" Value="201" />
        </Events></ArrangerAutomation></TimeSignature>
      </Mixer></DeviceChain>
    </MasterTrack>
    <Locators><Locators>
      <Locator><Time Value="0" /><Name Value="Count Off" /></Locator>
      <Locator><Time Value="8" /><Name Value="Intro" /></Locator>
      <Locator><Time Value="16" /><Name Value="Chorus" /></Locator>
      <Locator><Time Value="24" /><Name Value="" /></Locator>
    </Locators></Locators>
  </LiveSet>
</Ableton>
"""


def _wav(path, x, rate=44100):
    path.parent.mkdir(parents=True, exist_ok=True)
    stereo = np.repeat(np.clip(x, -1, 1)[:, None], 2, axis=1)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes((stereo * 32767).astype("<i2").tobytes())


@pytest.fixture
def downloaded(tmp_path, synthetic_ok):
    """Una secuencia como las que se bajan: 16 s a 120 (32 pulsos)."""
    rate, seconds = 44100, 16.0
    n = int(rate * seconds)
    t = np.arange(n) / rate
    folder = tmp_path / "Descargas" / "Mi Cancion-Grupo-G-120.00bpm"
    tracks = folder / "MultiTracks"
    click = np.zeros(n)
    for k in range(32):
        i = int(k * 0.5 * rate)
        click[i : i + 400] = 0.8 * np.sin(2 * np.pi * 1500 * np.arange(400) / rate)
    _wav(tracks / "Click Track.wav", click)
    _wav(tracks / "Guide.wav", 0.2 * np.sin(2 * np.pi * 300 * t) * (t < 2))
    _wav(tracks / "Bass.wav", 0.4 * np.sin(2 * np.pi * 55 * t))
    _wav(tracks / "Pad.wav", 0.2 * np.sin(2 * np.pi * 440 * t)[: n - 2 * rate])
    with gzip.GzipFile(folder / "MultiTrack.als", "wb") as f:
        f.write(ALS.encode())
    (folder / "Album.jpg").write_bytes(b"\xff\xd8\xff\xe0" + b"\0" * 64)
    return folder


def test_what_the_folder_says(downloaded):
    from danplay import sequence

    s = sequence.summary(downloaded / "MultiTracks" / "Bass.wav")
    assert (s["title"], s["artist"], s["key"], s["bpm"]) == ("Mi Cancion", "Grupo", "G", 120.0)
    assert s["project"] and s["cover"]
    assert s["sections"] == ["Count Off", "Intro", "Chorus", "Parte 4"]
    assert [t["name"] for t in s["tracks"]][:2] == ["Click Track", "Guide"]
    assert [t["role"] for t in s["tracks"]] == ["click", "guide", "", ""]
    project = sequence.read_als(downloaded / "MultiTrack.als")
    assert project["meters"] == [(0.0, 4, 4), (16.0, 3, 4), (19.0, 4, 4)]
    assert project["clips"]["Pad.wav"] == {"beat": 4.0, "warped": False}
    empty = downloaded.parent / "vacia"
    empty.mkdir()
    with pytest.raises(sequence.SequenceError, match="no tiene pistas"):
        sequence.find(empty)


def test_the_names_of_downloaded_sequences():
    from danplay import sequence

    assert sequence.parse_name("This Is The Day-Shout-F-124.00bpm") == {
        "title": "This Is The Day",
        "artist": "Shout",
        "key": "F",
        "bpm": 124.0,
    }
    got = sequence.parse_name("Way Maker - Leeland - Bb - 68 bpm")
    assert got == {"title": "Way Maker", "artist": "Leeland", "key": "Bb", "bpm": 68.0}
    assert sequence.role_of("Click") == "click" and sequence.role_of("Guia") == "guide"
    assert sequence.role_of("Vox Chop") == "" and sequence.role_of("Keys 2") == ""
    taken: set = set()
    assert [sequence.key_of(n, taken) for n in ("Drums (Live)", "1 Bajo", "Drums Live")] == [
        "drums_live",
        "p_1_bajo",
        "drums_live_2",
    ]


def test_a_project_with_entities_is_not_read(tmp_path):
    from danplay import sequence

    bomb = tmp_path / "x.als"
    with gzip.GzipFile(bomb, "wb") as f:
        f.write(b'<?xml version="1.0"?><!DOCTYPE a [<!ENTITY b "c">]><a>&b;</a>')
    with pytest.raises(sequence.SequenceError, match="raro"):
        sequence.read_als(bomb)


def test_the_exact_grid_follows_the_meter_changes():
    from danplay import sequence

    g = sequence.grid(120.0, [(0.0, 4, 4), (16.0, 3, 4), (19.0, 4, 4)], 16.0)
    assert len(g["beats"]) == 33 and g["beats"][1] == 0.5
    assert g["bars"][:7] == [0, 4, 8, 12, 16, 19, 23]
    assert g["report"]["exact"] is True


def test_opening_it_leaves_it_ready_to_play(configured_library, downloaded, tmp_path, monkeypatch):
    from danplay import beatgrid, config, library, sequence, stems

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    lib, _ = configured_library
    library.add_folder(lib)
    seen = []
    r = sequence.import_sequence(downloaded, progress=lambda **k: seen.append(k))
    assert seen[-1]["done"] == seen[-1]["total"]
    song = library.by_id(r["id"])
    assert song and Path(song["path"]).as_posix().endswith("Secuencias/Grupo - Mi Cancion.flac")
    assert (song["artist"], song["title"], song["key"]) == ("Grupo", "Mi Cancion", "G")
    assert abs(song["duration"] - 16.0) < 0.05
    info = stems.info(song)
    assert info and info["complete"] and info["sequence"]
    assert [(t["name"], t["role"]) for t in info["tracks"]][:2] == [
        ("Click Track", "click"),
        ("Guide", "guide"),
    ]
    assert all(
        t["wave"] and len(t["wave"]["peaks"]) == sequence.WAVE_BUCKETS for t in info["tracks"]
    )
    study = json.loads(song["study"])
    assert study["mixer"] == {"on": True}
    assert [m["label"] for m in study["markers"]] == ["Count Off", "Intro", "Chorus", "Parte 4"]
    assert study["markers"][2] == {"t": 8.0, "end": 12.0, "label": "Chorus"}
    # la ultima parte llega hasta el final: la duracion de las pistas, que
    # se lee de su cabecera (sin ffprobe)
    assert study["markers"][-1]["end"] == pytest.approx(16.0, abs=0.05)
    # el clic exacto, y sigue valiendo aunque cambie la forma de sacar las demas
    monkeypatch.setattr(beatgrid, "VERSION", beatgrid.VERSION + 1)
    assert beatgrid.cached(song["path"])
    g = beatgrid.grid_of(song["path"])
    assert g["bpm"] == 120.0 and g["bars"][:6] == [0, 4, 8, 12, 16, 19]
    # el pad entra al pulso 4: dos segundos de silencio delante
    pad = next(t for t in info["tracks"] if t["name"] == "Pad")
    import subprocess

    from danplay import convert

    raw = subprocess.run(
        [
            convert.tool("ffmpeg") or "ffmpeg",
            "-v",
            "error",
            "-i",
            pad["path"],
            "-ac",
            "1",
            "-f",
            "f32le",
            "-",
        ],
        capture_output=True,
        check=True,
    ).stdout
    x = np.abs(np.frombuffer(raw, np.float32))
    assert x[: int(1.9 * 44100)].max() < 1e-3 and x[int(2.1 * 44100) :].max() > 0.1
    # la mezcla no lleva ni el clic ni la guia: se oye el bajo, no el clic
    mix = subprocess.run(
        [
            convert.tool("ffmpeg") or "ffmpeg",
            "-v",
            "error",
            "-i",
            song["path"],
            "-ac",
            "1",
            "-f",
            "f32le",
            "-",
        ],
        capture_output=True,
        check=True,
    ).stdout
    m = np.frombuffer(mix, np.float32)
    spectrum = np.abs(np.fft.rfft(m[: 44100 * 4]))
    freqs = np.fft.rfftfreq(44100 * 4, 1 / 44100)
    assert (
        spectrum[np.argmin(np.abs(freqs - 1500))] < 0.05 * spectrum[np.argmin(np.abs(freqs - 55))]
    )


def test_the_api_opens_it(configured_library, downloaded, tmp_path, monkeypatch):
    from conftest import run_job
    from fastapi.testclient import TestClient

    from danplay import api, config, library

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    lib, _ = configured_library
    library.add_folder(lib)
    cliente = TestClient(api.app)
    r = cliente.get("/api/sequence", params={"path": str(downloaded)})
    assert r.status_code == 200 and r.json()["bpm"] == 120.0
    assert cliente.get("/api/sequence", params={"path": str(tmp_path)}).status_code == 422
    result = run_job(cliente, "/api/sequence/import", "secuencia", json={"path": str(downloaded)})
    assert result["tracks"] == 4 and library.by_id(result["id"])
    bad = cliente.post("/api/sequence/import", json={"path": str(tmp_path / "nada")})
    assert bad.status_code == 422
