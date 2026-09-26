"""El pulso con Beat This! (danplay/pulse.py y danplay/beatgrid.py), sin la red.

Lo de alrededor de la red —el espectrograma, los trozos que se solapan, los
maximos, la rejilla sin huecos— se prueba con una «red» de mentira que dice
pulso donde se le manda. Que el espectrograma y el grafo dan lo mismo que
Beat This! lo comprueba scripts/exportar-pulso.py al generarlo.
"""

import json
import math
import sys
import textwrap

import numpy as np
import pytest

from danplay import pulse


def test_the_spectrogram_has_fifty_frames_a_second_and_hears_the_pitch():
    t = np.arange(2 * pulse.RATE) / pulse.RATE
    low = pulse.spectrogram((0.5 * np.sin(2 * math.pi * 110 * t)).astype(np.float32))
    high = pulse.spectrogram((0.5 * np.sin(2 * math.pi * 4000 * t)).astype(np.float32))
    assert low.shape == (1 + 2 * pulse.RATE // pulse.HOP, pulse.MELS) == high.shape
    # un tono grave en las bandas de abajo, uno agudo en las de arriba
    assert low[50].argmax() < 30 < high[50].argmax()
    # algo cortisimo (menos que media ventana) no revienta
    assert pulse.spectrogram(np.zeros(100, dtype=np.float32)).shape[1] == pulse.MELS


def test_the_mel_filters_cover_30_hz_to_11_khz():
    f = pulse.mel_filters()
    assert f.shape == (pulse.NFFT // 2 + 1, pulse.MELS)
    hz = np.linspace(0, pulse.RATE / 2, pulse.NFFT // 2 + 1)
    used = hz[f.sum(axis=1) > 0]
    assert 30 <= used.min() < 60 and 10500 < used.max() <= 11000


class Net:
    """Una «red» que dice pulso cada `every` tramas (y «1» cada cuatro pulsos),
    contando desde el principio de la cancion: asi se ve si los trozos se
    juntan donde deben."""

    def __init__(self, every=25, offset=0):
        self.every = every
        self.offset = offset
        self.calls = []

    def run(self, _outputs, feeds):
        spect = feeds["spect"]
        assert spect.ndim == 3 and spect.shape[2] == pulse.MELS
        self.calls.append(spect.shape[1])
        # la posicion en la cancion viaja en la primera banda (ver `song`)
        pos = spect[0, :, 0].round().astype(int)
        beat = np.where((pos - self.offset) % self.every == 0, 5.0, -5.0).astype(np.float32)
        down = np.where((pos - self.offset) % (4 * self.every) == 0, 5.0, -5.0).astype(np.float32)
        return beat[None], down[None]


def song(frames):
    """Un espectrograma de mentira con el numero de trama en la primera banda
    (los rellenos de los bordes, a -1: ahi no hay cancion)."""
    s = np.zeros((frames, pulse.MELS), dtype=np.float32)
    s[:, 0] = np.arange(frames)
    return s


@pytest.mark.parametrize("frames", [300, 1500, 4000, 9999])
def test_the_chunks_are_joined_back_frame_by_frame(frames):
    net = Net()
    beat, down = pulse.logits(net, song(frames))
    assert beat.shape == (frames,)
    want = (np.arange(frames) % 25) == 0
    np.testing.assert_array_equal(beat > 0, want)
    np.testing.assert_array_equal(down > 0, (np.arange(frames) % 100) == 0)
    assert all(n <= pulse.CHUNK for n in net.calls)


def test_the_beats_are_the_peaks_and_each_one_goes_to_its_beat():
    beat = np.full(500, -3.0)
    beat[[50, 100, 101, 150, 200]] = [2.0, 1.0, 1.0, 3.0, -0.5]  # 100 y 101 pegados; 200 no llega
    down = np.full(500, -3.0)
    down[[52, 149]] = [1.0, 2.0]  # un poco movidos: van al pulso de al lado
    beats, downs = pulse.beats_of(beat, down)
    np.testing.assert_allclose(beats, [1.0, 2.01, 3.0])
    np.testing.assert_allclose(downs, [1.0, 3.0])


def test_the_grid_has_no_holes_and_keeps_every_bar_as_heard():
    period = 0.5
    beats = np.array([1.0 + period * i for i in range(20) if i not in (8, 9, 10)])  # un hueco
    downs = beats[[0, 4, 7]]  # un compas de 4, uno de 3... (se lo dice la red)
    g = pulse.grid(beats, downs, duration=12.0)
    full = np.array(g["beats"])
    np.testing.assert_allclose(np.diff(full), period, atol=1e-9)
    assert full[0] == pytest.approx(0.0) and full[-1] <= 12.0 < full[-1] + period
    assert g["bpm"] == pytest.approx(120.0)
    # el compas es el mas comun; los «1», donde los oye la red
    assert g["meter"] == 4 or g["meter"] == 3
    assert [full[i] for i in g["bars"]] == pytest.approx(list(downs))
    assert g["first_downbeat"] == g["bars"][0]
    with pytest.raises(ValueError):
        pulse.grid(np.array([1.0, 2.0]), np.array([]), 3.0)


# ------------------------------------------------------------ el nucleo

FAKE = textwrap.dedent(
    """
    import json, sys
    graph, ffmpeg, path = sys.argv[1:]
    if path.endswith("sin-pulso.mp3"):
        print(json.dumps({"error": "la cancion no tiene pulso claro"}))
        sys.exit(1)
    print(json.dumps({"ok": True, "bpm": 120.0, "meter": 4, "beats": [0.5, 1.0, 1.5, 2.0],
                      "first_downbeat": 0, "phase3": 0, "phase4": 0, "confidence": 0.9,
                      "bars": [0]}))
    """
)


@pytest.fixture
def detector(tmp_path, monkeypatch):
    from danplay import beatgrid, config

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    fake = tmp_path / "detector.py"
    fake.write_text(FAKE, encoding="utf-8")
    calls = []

    def command(path):
        calls.append(path)
        return [sys.executable, str(fake), "grafo", "ffmpeg", path]

    monkeypatch.setattr(beatgrid, "_command", command)
    monkeypatch.setattr(beatgrid, "available", lambda: "")
    return calls


def test_the_grid_is_worked_out_once_and_kept(detector, tmp_path):
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    song_file.write_bytes(b"x")
    g = beatgrid.grid_of(str(song_file))
    assert g["bpm"] == 120.0 and g["bars"] == [0] and "ok" not in g
    assert beatgrid.grid_of(str(song_file)) == g
    assert len(detector) == 1, "la segunda vez tenia que salir de lo guardado"
    assert beatgrid.cached(str(song_file))
    # el archivo cambia: se calcula otra vez
    song_file.write_bytes(b"otra cosa")
    beatgrid.grid_of(str(song_file))
    assert len(detector) == 2


def test_a_song_without_pulse_says_why(detector, tmp_path):
    from danplay import beatgrid

    bad = tmp_path / "sin-pulso.mp3"
    bad.write_bytes(b"x")
    with pytest.raises(beatgrid.BeatsError, match="no tiene pulso"):
        beatgrid.grid_of(str(bad))
    with pytest.raises(beatgrid.BeatsError, match="no encuentro"):
        beatgrid.grid_of(str(tmp_path / "no-esta.mp3"))


def test_the_api_gives_the_grid_or_says_it_cannot(configured_library, detector, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api, beatgrid, library

    root, _ = configured_library
    library.add_folder(root)
    library.scan()
    cliente = TestClient(api.app)
    song_row = library.search("", limit=1)[0]
    r = cliente.get(f"/api/song/{song_row['id']}/beats")
    assert r.status_code == 200, r.text
    assert r.json()["meter"] == 4
    assert cliente.get("/api/song/999999/beats").status_code == 404
    # sin detector aqui, y sin nada guardado para otra cancion: 503
    other = library.search("", limit=2)[1]
    monkeypatch.setattr(beatgrid, "available", lambda: "falta el detector de pulso")
    assert cliente.get(f"/api/song/{other['id']}/beats").status_code == 503
    # lo ya guardado sigue saliendo
    assert cliente.get(f"/api/song/{song_row['id']}/beats").status_code == 200


def test_the_graph_travels_with_the_app():
    from danplay import beatgrid

    assert beatgrid.GRAPH.is_file()
    info = json.loads(beatgrid.GRAPH.with_suffix(".json").read_text(encoding="utf-8"))
    assert info["fps"] == pulse.FPS and info["chunk"] == pulse.CHUNK
    assert len(info["sha256"]) == 64
