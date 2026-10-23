"""El pulso con Beat This! (danplay/pulse.py y danplay/beatgrid.py), sin la red.

Lo de alrededor de la red —el espectrograma, los trozos que se solapan, los
maximos, la rejilla sin huecos— se prueba con una «red» de mentira que dice
pulso donde se le manda. Que el espectrograma y el grafo dan lo mismo que
Beat This! lo comprueba scripts/export-pulse.py al generarlo.
"""

import json
import math
import sys
import textwrap
import time
from collections.abc import Sequence

import api_core
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


def _net(beats, downs, seconds=60.0):
    """Lo que diria la red: un pico de pulso en cada `beats` (en segundos,
    con la fraccion de trama que haga falta) y uno de «1» en cada `downs`."""
    frames = int(seconds * pulse.FPS)
    t = np.arange(frames) / pulse.FPS
    beat = np.full(frames, -6.0)
    down = np.full(frames, -6.0)
    for b in beats:
        beat = np.maximum(beat, 4.0 - 0.5 * ((t - b) * pulse.FPS) ** 2)
    for d in downs:
        down = np.maximum(down, 4.0 - 0.5 * ((t - d) * pulse.FPS) ** 2)
    return beat, down


def test_the_grid_keeps_one_level_from_start_to_end():
    """Lo que paso en canciones de verdad: un tramo en que la red oye el pulso
    al doble (el «y» de cada tiempo), un golpe suelto de mas y un pulso que
    no oye. El clic sigue a un solo tempo, entero, y sin el golpe de mas."""
    period = 0.6
    beats = [1.0 + period * i for i in range(60)]
    heard = [b for i, b in enumerate(beats) if i != 20]  # uno que no oye
    heard += [b + period / 2 for b in beats[30:42]]  # al doble en un tramo
    heard.append(beats[50] + 0.17)  # un golpe de mas
    g = pulse.grid(*_net(sorted(heard), beats[::4]))
    got = np.array(g["beats"])
    np.testing.assert_allclose(got, beats, atol=0.004)
    assert g["bpm"] == pytest.approx(100.0, abs=0.2)
    assert g["meter"] == 4 and g["bars"] == list(range(0, 60, 4))
    assert g["closed"] and g["first_downbeat"] == 0


def test_each_beat_falls_between_frames_where_it_is():
    """La red mira cada 20 ms; el clic no baila con ella."""
    period = 60 / 97  # no cae en tramas enteras
    beats = [0.5 + period * i for i in range(80)]
    got = np.array(pulse.grid(*_net(beats, beats[::4]))["beats"])
    assert np.abs(got - beats).max() < 0.002


def test_a_free_part_has_no_click_and_a_hole_in_tempo_is_filled():
    period = 0.5
    first = [1.0 + period * i for i in range(24)]  # hasta 12,5
    hole = first[-1] + 4 * period  # cuatro pulsos sin nada, a tempo
    second = [hole + period * i for i in range(24)]  # hasta 25,5
    free = second[-1] + 3.3  # una parte libre: no es un numero entero de pulsos
    third = [free + period * i for i in range(24)]
    g = pulse.grid(*_net(first + second + third, (first + second + third)[::4]))
    got = np.array(g["beats"])
    # el hueco a tempo, relleno; la parte libre, sin nada
    assert sum(1 for b in got if first[-1] < b < second[0]) == 3
    assert not any(second[-1] < b < third[0] for b in got)
    # sin clic antes del primero ni despues del ultimo
    assert got[0] == pytest.approx(first[0], abs=0.004)
    assert got[-1] == pytest.approx(third[-1], abs=0.004)
    with pytest.raises(ValueError):
        pulse.grid(*_net([1.0, 2.0], []))


def test_the_one_stays_put_unless_the_song_says_otherwise():
    """Un «1» que la red se salta no rompe el compas (antes salian compases
    de 8); un compas de dos que la red oye una y otra vez, si."""
    period = 0.5
    beats = [1.0 + period * i for i in range(64)]
    downs = beats[::4]
    missing = downs[:5] + downs[6:]
    g = pulse.grid(*_net(beats, missing))
    assert g["bars"] == list(range(0, 64, 4))
    # un compas de dos (el 2/4 antes del coro): los «1» de despues se oyen
    # dos tiempos antes de donde tocaban, una y otra vez
    shifted = list(beats[:32:4]) + [beats[34 + 4 * k] for k in range(8)]
    bars = pulse.grid(*_net(beats, shifted))["bars"]
    assert bars[bars.index(32) + 1] == 34 and bars[-1] == 62


# ------------------------------------------------------------ el nucleo

FAKE = textwrap.dedent(
    """
    import json, os, sys
    import numpy as np
    graph, ffmpeg, path = sys.argv[1:4]
    # cada vez que se le llama, una linea (la prueba cuenta las llamadas, la
    # lance el nucleo de Python o el de Rust)
    with open(os.environ["DANPLAY_PULSE_CALLS"], "a", encoding="utf-8") as f:
        f.write(path + "\\n")
    # como el de verdad: el avance, una linea cada vez, antes de la respuesta
    print(json.dumps({"progress": 0.5}), flush=True)
    if path.endswith("sin-pulso.mp3"):
        print(json.dumps({"error": "la cancion no tiene pulso claro"}))
        sys.exit(1)
    # lo que oiria la red en 30 s a 120 (en la bateria, 4 ms despues)
    late = 0.004 if "bateria" in path else 0.0
    fps, seconds = 50, 30.0
    t = np.arange(int(seconds * fps)) / fps
    beat = np.full(t.size, -6.0)
    down = np.full(t.size, -6.0)
    for k in range(59):
        b = 0.5 + 0.5 * k + late
        beat = np.maximum(beat, 4.0 - 0.5 * ((t - b) * fps) ** 2)
        if k % 4 == 0:
            down = np.maximum(down, 4.0 - 0.5 * ((t - b) * fps) ** 2)
    if len(sys.argv) > 4:
        np.savez_compressed(sys.argv[4], beat=beat.astype(np.float16),
                            down=down.astype(np.float16),
                            low=np.zeros(int(seconds * 100), np.float16),
                            low_fps=np.float64(100.0), seconds=np.float64(seconds))
    print(json.dumps({"ok": True, "bpm": 120.0, "meter": 4, "beats": [0.5, 1.0, 1.5, 2.0],
                      "first_downbeat": 0, "phase3": 0, "phase4": 0, "confidence": 0.9,
                      "bars": [0]}))
    """
)


class _Calls(Sequence):
    """Las canciones con que se llamo al detector de mentira, en orden: las
    apunta el propio detector en un archivo, porque con el nucleo de Rust lo
    lanza otro proceso."""

    def __init__(self, path):
        self.path = path

    def _lines(self):
        if not self.path.exists():
            return []
        return self.path.read_text(encoding="utf-8").splitlines()

    def __len__(self):
        return len(self._lines())

    def __getitem__(self, i):
        return self._lines()[i]

    def __eq__(self, other):
        return self._lines() == list(other)

    __hash__ = None


@pytest.fixture
def detector(tmp_path, monkeypatch):
    from danplay import beatgrid, config

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    fake = tmp_path / "detector.py"
    fake.write_text(FAKE, encoding="utf-8")
    calls = tmp_path / "llamadas.txt"
    monkeypatch.setenv("DANPLAY_PULSE_CALLS", str(calls))

    def command(path, raw=None):
        return [sys.executable, str(fake), "grafo", "ffmpeg", path, *([str(raw)] if raw else [])]

    monkeypatch.setattr(beatgrid, "_command", command)
    monkeypatch.setattr(beatgrid, "available", lambda: "")
    # el nucleo de Rust, que corre aparte, lo recibe por el entorno (le añade
    # grafo, ffmpeg, la cancion y lo crudo, como `_command`)
    monkeypatch.setenv("DANPLAY_PULSE_DETECTOR", json.dumps([sys.executable, str(fake)]))
    beatgrid._reset()
    yield _Calls(calls)
    assert beatgrid.idle(), "la cola de rejillas se quedo trabajando"
    beatgrid._reset()


def test_the_grid_is_worked_out_once_and_kept(detector, tmp_path):
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    song_file.write_bytes(b"x")
    g = beatgrid.grid_of(str(song_file))
    assert g["bpm"] == 120.0 and g["meter"] == 4 and g["bars"][0] == 0 and "ok" not in g
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


def test_the_network_says_how_far_it_has_got():
    """La barra verde de la app sale de aqui: tras leer la cancion, y luego
    trozo a trozo, hasta 1."""

    class Session:
        def run(self, _, feeds):
            n = feeds["spect"].shape[1]
            return [np.zeros((1, n), np.float32), np.zeros((1, n), np.float32)]

    seen = []
    pulse.logits(
        Session(), np.zeros((pulse.CHUNK * 3, 128), np.float32), lambda d, t: seen.append((d, t))
    )
    total = len(pulse._starts(pulse.CHUNK * 3))
    assert seen == [(k, total) for k in range(1, total + 1)]


def test_the_progress_lines_are_not_taken_for_the_answer(detector, tmp_path):
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    song_file.write_bytes(b"x")
    assert beatgrid.grid_of(str(song_file))["bpm"] == 120.0
    assert beatgrid.progress(str(song_file)) is None, "ni en cola ni calculandose"
    with beatgrid._WAKE:
        assert beatgrid.request(str(tmp_path / "cancion.mp3")) is not None, "ya guardada"
        other = tmp_path / "otra.mp3"
        other.write_bytes(b"y")
        assert beatgrid.request(str(other)) is None
        assert beatgrid._PROGRESS == {} and str(other) in beatgrid._QUEUE
    assert beatgrid.idle()
    assert beatgrid.progress(str(other)) is None


def test_without_waiting_it_answers_at_once_and_works_it_out_behind(detector, tmp_path):
    """La app no espera los 15-20 s de la red: pide sin esperar, suena con la
    rejilla rapida de Rust y vuelve a preguntar."""
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    song_file.write_bytes(b"x")
    with beatgrid._WAKE:  # el hilo, parado: las dos peticiones caen en cola
        assert beatgrid.request(str(song_file)) is None
        assert beatgrid.request(str(song_file)) is None
        assert list(beatgrid._QUEUE) == [str(song_file)], "pedirla dos veces no la duplica"
    assert beatgrid.idle()
    assert detector == [str(song_file)]
    saved = beatgrid.request(str(song_file))
    assert saved is not None and saved["bpm"] == 120.0
    assert len(detector) == 1, "ya guardada: no se vuelve a calcular"


def test_what_is_needed_now_goes_before_what_comes_later(detector, tmp_path, monkeypatch):
    from danplay import beatgrid

    files = []
    for name in ("despues.mp3", "luego.mp3", "ahora.mp3"):
        f = tmp_path / name
        f.write_bytes(name.encode())
        files.append(str(f))
    later, after, now = files
    # el hilo, parado mientras se llena la cola: se ve el orden tal cual
    with beatgrid._WAKE:
        assert beatgrid.prepare(later) and beatgrid.prepare(after)
        assert beatgrid.request(now) is None
        assert list(beatgrid._QUEUE) == [now, later, after]
    assert beatgrid.idle()
    assert detector == [now, later, after]


def test_a_failure_is_remembered_until_the_file_changes(detector, tmp_path):
    from danplay import beatgrid

    bad = tmp_path / "sin-pulso.mp3"
    bad.write_bytes(b"x")
    assert beatgrid.request(str(bad)) is None
    assert beatgrid.idle()
    with pytest.raises(beatgrid.BeatsError, match="no tiene pulso"):
        beatgrid.request(str(bad))
    assert len(detector) == 1, "lo que fallo no se vuelve a intentar sin mas"
    assert not beatgrid.prepare(str(bad))
    with pytest.raises(beatgrid.BeatsError, match="no encuentro"):
        beatgrid.request(str(tmp_path / "no-esta.mp3"))


def test_writing_a_tag_keeps_what_was_worked_out_from_the_sound(synthetic_library, detector):
    """Guardar algo del modo estudio reescribe la etiqueta (fecha y tamaño
    nuevos) pero no el sonido: la rejilla del clic y la onda siguen valiendo.
    Antes se perdian cada vez y el clic volvia a tardar 15-20 s."""
    import os

    from danplay import beatgrid, config, tags, waveform

    _, songs = synthetic_library
    path = str(songs["gozo"])
    # un dia antes: la escritura cae seguro en otro segundo
    old = os.stat(path).st_mtime - 86400
    os.utime(path, (old, old))
    beatgrid.grid_of(path)
    wave = waveform._cache_file(path)
    wave.write_text(
        json.dumps({"path": path, "mtime": os.path.getmtime(path), "peaks": [1], "rms": [1]}),
        encoding="utf-8",
    )
    before = os.stat(path)
    assert tags.write_study(path, json.dumps({"speed": 0.9}))
    after = os.stat(path)
    assert (after.st_size, after.st_mtime) != (before.st_size, before.st_mtime)
    assert beatgrid.cached(path), "la rejilla tenia que seguir valiendo"
    beatgrid.grid_of(path)
    assert len(detector) == 1, "y no volver a calcularse"
    assert len(list((config.DATA_DIR / "beats").glob("*.json"))) == 1, "sin dejar la vieja"
    assert json.loads(wave.read_text(encoding="utf-8"))["mtime"] == os.path.getmtime(path)
    # otra cancion cambiada por fuera (el sonido puede ser otro): no se toca
    assert not beatgrid.retag(str(songs["tierra"]), before, after)


def test_the_api_gives_the_grid_or_says_it_cannot(
    configured_library, detector, monkeypatch, connect_api, tmp_path
):
    from danplay import beatgrid, library

    root, _ = configured_library
    library.add_folder(root)
    library.scan()
    client = connect_api()
    song_row = library.search("", limit=1)[0]
    r = client.get(f"/api/song/{song_row['id']}/beats")
    assert r.status_code == 200, r.text
    assert r.json()["meter"] == 4
    assert client.get("/api/song/999999/beats").status_code == 404
    # sin detector aqui, y sin nada guardado para otra cancion: 503
    other = library.search("", limit=2)[1]
    monkeypatch.setattr(beatgrid, "available", lambda: "falta el detector de pulso")
    # (al de Rust, sin los datos que viajan con la app: sin el grafo)
    monkeypatch.setenv("DANPLAY_RESOURCES", str(tmp_path / "sin-datos"))
    assert client.get(f"/api/song/{other['id']}/beats").status_code == 503
    # lo ya guardado sigue saliendo
    assert client.get(f"/api/song/{song_row['id']}/beats").status_code == 200


def _queue_done(client, ids):
    """Que la cola de rejillas acabe: la de Python se mira por dentro; la del
    nucleo de Rust, preguntando hasta que ninguna siga pendiente."""
    from danplay import beatgrid

    if not api_core.RUST:
        return beatgrid.idle()
    limit = time.monotonic() + 30
    while time.monotonic() < limit:
        if all(
            client.get(f"/api/song/{cid}/beats", params={"wait": "false"}).status_code != 202
            for cid in ids
        ):
            return True
        time.sleep(0.1)
    return False


def test_the_api_without_waiting_and_preparing_the_next(configured_library, detector, connect_api):
    from danplay import library

    root, _ = configured_library
    library.add_folder(root)
    library.scan()
    client = connect_api()
    first, second = library.search("", limit=2)
    r = client.get(f"/api/song/{first['id']}/beats", params={"wait": "false"})
    assert r.status_code == 202 and r.json()["pending"] is True
    assert 0 <= r.json()["progress"] <= 1
    assert client.post(f"/api/song/{second['id']}/beats/prepare").json() == {"queued": True}
    assert _queue_done(client, [first["id"], second["id"]])
    r = client.get(f"/api/song/{first['id']}/beats", params={"wait": "false"})
    assert r.status_code == 200 and r.json()["meter"] == 4
    r = client.get(f"/api/song/{second['id']}/beats", params={"wait": "false"})
    assert r.status_code == 200, "la de despues ya estaba hecha al pasar a ella"
    assert len(detector) == 2
    assert client.post("/api/song/999999/beats/prepare").status_code == 404


def test_the_graph_travels_with_the_app():
    from danplay import beatgrid

    assert beatgrid.GRAPH.is_file()
    info = json.loads(beatgrid.GRAPH.with_suffix(".json").read_text(encoding="utf-8"))
    assert info["fps"] == pulse.FPS and info["chunk"] == pulse.CHUNK
    assert len(info["sha256"]) == 64


# ------------------------------------------------------------ re-analizar


def test_what_the_network_heard_is_kept_and_a_new_version_needs_no_network(detector, tmp_path):
    """Lo que dijo la red se guarda (`.npz`): una version nueva de la forma
    de sacar la rejilla, o cambiar de modo, sale de ahi en milisegundos."""
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    song_file.write_bytes(b"x")
    g = beatgrid.grid_of(str(song_file))
    assert beatgrid._raw_file(str(song_file)).is_file()
    # una rejilla de una version de antes: se rehace sin pasar la red
    cache = beatgrid._cache(str(song_file))
    saved = json.loads(cache.read_text(encoding="utf-8"))
    cache.write_text(json.dumps({**saved, "version": 2}), encoding="utf-8")
    assert not beatgrid.cached(str(song_file))
    assert beatgrid.grid_of(str(song_file))["beats"] == g["beats"]
    assert len(detector) == 1, "lo que oyo la red ya estaba guardado"


def test_reanalysing_a_stretch_only_touches_that_stretch(detector, tmp_path):
    """El caso de las canciones con cortes: medio minuto con el clic a
    contratiempo. Re-analizar ese tramo lo pone en su sitio y deja lo demas
    tal cual, cosido sin costura."""
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    song_file.write_bytes(b"x")
    path = str(song_file)
    good = beatgrid.grid_of(path)["beats"]
    # se estropea a mano: de 10 a 16 s, medio pulso tarde
    cache = beatgrid._cache(path)
    saved = json.loads(cache.read_text(encoding="utf-8"))
    bad = [b + 0.25 if 10 <= b <= 16 else b for b in good]
    saved["grid"]["beats"] = bad
    cache.write_text(json.dumps(saved), encoding="utf-8")
    assert beatgrid.grid_of(path)["beats"] == bad
    g = beatgrid.reanalyze(path, "repair", 9.9, 16.4)
    np.testing.assert_allclose(g["beats"], good, atol=0.01)
    assert g["report"]["fixed"] == 1 and g["report"]["joins"] == {"left": True, "right": True}
    assert len(detector) == 1, "sin pasar la red otra vez"
    # se queda guardado, con lo que se hizo
    again = json.loads(cache.read_text(encoding="utf-8"))
    assert again["grid"]["beats"] == g["beats"]
    assert again["edits"] == [{"mode": "repair", "start": 9.9, "end": 16.4}]


def test_steady_tempo_in_a_stretch_fits_between_its_neighbours(detector, tmp_path):
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    song_file.write_bytes(b"x")
    path = str(song_file)
    good = beatgrid.grid_of(path)["beats"]
    cache = beatgrid._cache(path)
    saved = json.loads(cache.read_text(encoding="utf-8"))
    # un hueco de 4 s sin clic
    saved["grid"]["beats"] = [b for b in good if not 8 < b < 12]
    cache.write_text(json.dumps(saved), encoding="utf-8")
    g = beatgrid.reanalyze(path, "steady", 8.2, 11.8)
    np.testing.assert_allclose(g["beats"], good, atol=0.01)
    assert g["report"]["fixed"] == 1


def test_the_whole_song_with_the_separated_drums(detector, tmp_path, monkeypatch):
    from danplay import beatgrid

    song_file = tmp_path / "cancion.mp3"
    drums = tmp_path / "bateria.flac"
    song_file.write_bytes(b"x")
    drums.write_bytes(b"d")
    with pytest.raises(beatgrid.BeatsError, match="batería separada"):
        beatgrid.reanalyze(str(song_file), "drums")
    monkeypatch.setattr(
        beatgrid, "_drums_of", lambda p: str(drums) if p == str(song_file) else None
    )
    g = beatgrid.reanalyze(str(song_file), "drums")
    assert detector == [str(song_file), str(drums)]
    assert beatgrid._raw_file(str(drums)).is_file()
    assert g["bpm"] == pytest.approx(120, abs=0.1)
    with pytest.raises(beatgrid.BeatsError, match="no sé"):
        beatgrid.reanalyze(str(song_file), "magia")
    # mejorar toda la biblioteca: con la bateria ya oida, sin pasar la red
    assert beatgrid.improve(str(song_file))["bpm"] == pytest.approx(120, abs=0.1)
    assert len(detector) == 2


def test_changes_counts_places_not_beats():
    from danplay import beatgrid

    beats = [0.5 * k for k in range(100)]
    assert beatgrid.changes(beats, beats) == 0
    moved = [b + 0.25 if 10 <= b <= 14 else b for b in beats]
    holed = [b for b in beats if not 30 < b < 35]
    assert beatgrid.changes(moved, beats) == 1
    assert beatgrid.changes(holed, beats) == 1
    both = [b for b in moved if not 30 < b < 35]
    assert beatgrid.changes(both, beats) == 2


def test_the_api_reanalyses(configured_library, detector, connect_api):
    from danplay import library

    root, _ = configured_library
    library.add_folder(root)
    library.scan()
    client = connect_api()
    song_row = library.search("", limit=1)[0]
    url = f"/api/song/{song_row['id']}/beats/reanalyze"
    r = client.post(url, json={"mode": "steady"})
    assert r.status_code == 200, r.text
    assert r.json()["report"]["fixed"] == 0 and r.json()["grid"]["meter"] == 4
    assert client.post(url, json={"mode": "repair", "start": 5, "end": 3}).status_code == 422
    assert client.post(url, json={"mode": "magia"}).status_code == 422
    assert client.post("/api/song/999999/beats/reanalyze", json={}).status_code == 404


def test_what_the_network_heard_survives_a_cut(tmp_path):
    """Guardarlo va a un temporal: un corte a medias no deja un archivo roto.
    Y uno roto (de antes) se descarta en vez de tumbar el analisis."""
    raw = {
        "beat": np.zeros(100),
        "down": np.zeros(100),
        "low": np.zeros(200),
        "low_fps": 100.0,
        "seconds": 2.0,
    }
    path = tmp_path / "x.npz"
    pulse.save_raw(path, raw)
    loaded = pulse.load_raw(path)
    assert loaded and loaded["seconds"] == 2.0
    assert not (tmp_path / "x.npz.part").exists()
    path.write_bytes(path.read_bytes()[:40])  # cortado a medias
    assert pulse.load_raw(path) is None
    assert not path.exists(), "el roto se quita: se vuelve a calcular"
    assert pulse.load_raw(tmp_path / "no-esta.npz") is None
