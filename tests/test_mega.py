"""La separacion de maxima calidad (danplay/mega.py y separation.run_max),
con una red de mentira: el espectrograma, el reparto y que todo siga
sumando la cancion."""

import json
import subprocess
import wave

import numpy as np
import pytest
from conftest import FFMPEG

HEADS = [
    "drums",
    "percussion",
    "congas",
    "tambourine",
    "bass",
    "vocal",
    "lead-vocal",
    "back-vocal",
    "guitar",
    "electric-guitar",
    "acoustic-guitar",
    "piano",
    "digital-piano",
    "keys",
    "organ",
    "synth",
    "strings",
    "bowed_strings",
    "brass",
    "wind",
]


def test_the_spectrogram_goes_and_comes_back():
    from danplay import mega

    x = np.random.default_rng(0).standard_normal((2, 50000)).astype(np.float32)
    z = mega.stft(x)
    assert z.shape == (2, mega.FREQS, 50000 // mega.HOP + 1)
    np.testing.assert_allclose(mega.istft(z, x.shape[-1]), x, atol=2e-5)
    # el orden de la red: por frecuencia, cada canal, real e imaginaria
    net = mega.to_net(z)
    assert net.shape == (1, z.shape[-1], mega.FREQS * 4)
    assert net[0, 3, 4 * 10 + 2] == pytest.approx(z[1, 10, 3].real)
    assert net[0, 3, 4 * 10 + 1] == pytest.approx(z[0, 10, 3].imag)
    # una mascara con el mismo orden vuelve a su sitio
    back = mega.from_net(np.stack([net, 2 * net], axis=1))
    np.testing.assert_allclose(back[0], z, rtol=1e-6)
    np.testing.assert_allclose(back[1], 2 * z, rtol=1e-6)


class Net:
    """Una red de mentira: cada cabeza oye una franja de frecuencias."""

    def __init__(self, bands: dict[str, tuple[int, int]]):
        self.bands = bands

    def run(self, _, feeds):
        from danplay import mega

        t = feeds["spec"].shape[1]
        out = np.zeros((1, len(HEADS), t, mega.FREQS * 4), np.float32)
        for i, h in enumerate(HEADS):
            a, b = self.bands.get(h, (0, 0))
            out[0, i, :, 4 * a : 4 * b : 2] = 1.0  # la parte real a 1
        return [out]


def test_the_split_adds_up_and_follows_what_the_net_hears():
    from danplay import mega

    rate = mega.RATE
    t = np.arange(rate * 3) / rate
    low = 0.3 * np.sin(2 * np.pi * 200 * t)  # «congas»
    high = 0.3 * np.sin(2 * np.pi * 3000 * t)  # «el kit»
    drums = np.stack([low + high, low + high]).astype(np.float32)
    guitar = np.stack([0.2 * np.sin(2 * np.pi * 700 * t)] * 2).astype(np.float32)
    hz = mega.FREQS / (rate / 2)
    bands = {
        "congas": (int(150 * hz), int(260 * hz)),
        "drums": (int(2500 * hz), int(3500 * hz)),
        "electric-guitar": (int(600 * hz), int(800 * hz)),
    }
    got: dict = {}

    def emit(parts):
        for k, v in parts.items():
            got.setdefault(k, []).append(v)

    mix = drums + guitar
    mega.separate(Net(bands), HEADS, 44100, mix, {"drums": drums, "melodic": guitar}, emit)
    out = {k: np.concatenate(v, axis=1) for k, v in got.items()}
    assert out["drums"].shape == drums.shape
    np.testing.assert_allclose(out["drums"] + out["percussion"], drums, atol=1e-4)
    core = slice(rate // 2, -rate // 2)  # lejos de los bordes del espectrograma
    # las congas, a su pista; el kit, en la bateria
    assert np.abs(out["percussion"][0, core] - low[core]).max() < 0.02
    assert np.abs(out["drums"][0, core] - high[core]).max() < 0.02
    np.testing.assert_allclose(out["eguitar"][0, core], guitar[0, core], atol=0.02)
    assert np.abs(out["piano"]).max() < 1e-3, "lo que la red no oye no se reparte"


@pytest.fixture
def song(tmp_path, synthetic_ok):
    """Una cancion de 4 s y sus pistas buenas (bateria, bajo y voces)."""
    rate = 44100
    t = np.arange(rate * 4) / rate
    parts = {
        "drums": 0.2 * np.sin(2 * np.pi * 200 * t) + 0.2 * np.sin(2 * np.pi * 3000 * t),
        "bass": 0.3 * np.sin(2 * np.pi * 60 * t),
        "vocals": 0.2 * np.sin(2 * np.pi * 440 * t),
        "guitar": 0.2 * np.sin(2 * np.pi * 700 * t),
    }
    paths = {}
    for name, x in parts.items():
        p = tmp_path / f"{name}.wav"
        with wave.open(str(p), "wb") as w:
            w.setnchannels(2)
            w.setsampwidth(2)
            w.setframerate(rate)
            w.writeframes((np.repeat(x[:, None], 2, 1) * 32767).astype("<i2").tobytes())
        paths[name] = p
    mix = tmp_path / "cancion.wav"
    total = np.sum(list(parts.values()), axis=0)
    with wave.open(str(mix), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes((np.repeat(total[:, None], 2, 1) * 32767).astype("<i2").tobytes())
    return mix, paths


def test_the_max_pass_keeps_what_sounds_and_the_rest_is_other(song, tmp_path, monkeypatch):
    from danplay import mega, separation, stems

    mix, paths = song
    hz = mega.FREQS / (mega.RATE / 2)
    bands = {
        "congas": (int(150 * hz), int(260 * hz)),
        "drums": (int(2500 * hz), int(3500 * hz)),
        "electric-guitar": (int(600 * hz), int(800 * hz)),
        "lead-vocal": (int(380 * hz), int(500 * hz)),
    }
    manifest = tmp_path / "mega53.json"
    manifest.write_text(json.dumps({"heads": HEADS, "chunk": 44100 * 2}), encoding="utf-8")
    out = tmp_path / "pistas"
    job = {
        "input": str(mix),
        "ffmpeg": FFMPEG,
        "output": str(out),
        "files": {s: f"{stems.SOURCES[s][0]}.flac" for s in stems.SOURCES},
        "rest": "other",
        "carry": {k: str(paths[k]) for k in ("drums", "bass", "vocals")},
        "mega": {"graph": "x", "manifest": str(manifest), "weights": "y"},
        "waves": str(out / "ondas.json"),
    }
    said = []
    monkeypatch.setattr(separation, "_say", lambda **m: said.append(m))
    r = separation.run_max(job, open_net=lambda *a: Net(bands))
    tracks = r["tracks"]
    # la percusion sale; sin coros, las voces quedan como estaban
    assert {"drums", "percussion", "bass", "vocals", "eguitar", "other"} <= set(tracks)
    assert "lead" not in tracks and "backing" not in tracks
    assert "piano" not in tracks and "strings" not in tracks, "lo que no suena no es pista"
    # todo junto suena como la cancion
    assert FFMPEG
    raw = {}
    for k, name in tracks.items():
        data = subprocess.run(
            [FFMPEG, "-v", "error", "-i", str(out / name), "-f", "f32le", "-"],
            capture_output=True,
            check=True,
        ).stdout
        raw[k] = np.frombuffer(data, np.float32)
    whole = subprocess.run(
        [FFMPEG, "-v", "error", "-i", str(mix), "-f", "f32le", "-"], capture_output=True, check=True
    ).stdout
    total = sum(raw.values())
    np.testing.assert_allclose(total, np.frombuffer(whole, np.float32), atol=2e-3)
    assert r["levels"]["percussion"]["db"] > -20
    assert any(m.get("step") == "compose" for m in said)
    assert json.loads((out / "ondas.json").read_text())["tracks"].keys() == set(tracks)
