"""El motor del separador (danplay/separation.py), sin bajar ningun modelo.

Lo que hace numpy alrededor de la red —el espectrograma, su inversa, los
trozos que se solapan y se funden— se prueba con una red de mentira que
devuelve lo que le entra: si todo lo de alrededor esta bien, la cancion sale
tal cual. El grafo de verdad se carga con pesos inventados del tamaño de los
oficiales: asi se comprueba que la app sabe rellenar cada peso sin bajarse
los 55 MB. Que el grafo da lo mismo que Demucs lo comprueba
scripts/exportar-separador.py al generarlo.
"""

import json
import math

import numpy as np
import pytest

from danplay import separation as S


def tones(length: int) -> np.ndarray:
    """Estereo con varios tonos: sin nada en la frecuencia de Nyquist, que el
    espectrograma de Demucs tira (y con ruido blanco no volveria entero)."""
    t = np.arange(length) / S.RATE
    left = 0.3 * np.sin(2 * math.pi * 220 * t) + 0.2 * np.sin(2 * math.pi * 1375 * t)
    right = 0.25 * np.sin(2 * math.pi * 330 * t + 0.4) + 0.1 * np.sin(2 * math.pi * 5000 * t)
    return np.stack([left, right]).astype(np.float32)


def test_the_spectrogram_and_its_inverse_give_back_the_signal():
    """Como en Demucs, la vuelta es exacta salvo en los bordes del trozo: su
    espectrograma se queda sin las dos ventanas de cada extremo (las pone a
    cero al volver) y eso lo corrige la rama temporal de la red. En PyTorch
    pasa lo mismo, con el mismo error en los bordes."""
    x = tones(S.SEGMENT)
    mag = S.spec(x)
    assert mag.shape == (4, S.FREQS, math.ceil(S.SEGMENT / S.HOP))
    back = S.ispec(mag[None], S.SEGMENT)[0]
    assert back.shape == x.shape
    edge = 3 * S.NFFT
    assert np.abs(back - x)[:, edge:-edge].max() < 1e-4


class Identity:
    """Una «red» que devuelve la mezcla repartida entre sus fuentes por la
    rama temporal (la del espectrograma, a cero). Sumadas, la cancion. Sirve
    para comprobar todo lo de fuera: los trozos, el solape, la normalizacion."""

    def __init__(self, shares=(0.25, 0.75)):
        self.shares = shares
        self.calls = 0

    def run(self, _outputs, feeds):
        self.calls += 1
        mix, mag = feeds["mix"], feeds["mag"]
        assert mix.shape == (1, 2, S.SEGMENT) and mix.dtype == np.float32
        assert mag.shape == (1, 4, S.FREQS, 336)
        n = len(self.shares)
        spectra = np.zeros((1, n, 4, S.FREQS, 336), dtype=np.float32)
        waves = np.stack([mix[0] * share for share in self.shares])[None]
        return spectra, waves


@pytest.mark.parametrize("seconds", [3.0, 7.8, 21.3])
def test_the_pieces_are_joined_back_into_the_whole_song(seconds):
    x = tones(int(seconds * S.RATE))
    blocks, progress = [], []
    net = Identity()
    # se le da una copia: la normaliza en su sitio
    S.separate(net, x.copy(), blocks.append, lambda d, t: progress.append((d, t)))
    out = np.concatenate(blocks, axis=-1)
    # tantos trozos como hacen falta, y cada tramo emitido una sola vez
    assert out.shape == (2, 2, x.shape[-1])
    assert net.calls == math.ceil(x.shape[-1] / S.STRIDE)
    assert progress[-1] == (net.calls, net.calls)
    # cada fuente lleva la media de la mezcla (como en Demucs): aqui es ~0
    np.testing.assert_allclose(out[0], 0.25 * x, atol=1e-4)
    np.testing.assert_allclose(out.sum(0), x, atol=1e-4)


def test_a_song_with_an_offset_and_a_loud_mix_comes_back_the_same():
    """La mezcla se normaliza antes de la red (media y desviacion, como Demucs)
    y se deshace despues: una señal alta y descentrada vuelve igual."""
    x = tones(int(9 * S.RATE)) * 2.5 + 0.05
    blocks = []
    S.separate(Identity((1.0,)), x.copy(), blocks.append)
    out = np.concatenate(blocks, axis=-1)[0]
    np.testing.assert_allclose(out, x, atol=1e-4)


def test_it_stops_when_cancelled():
    net = Identity()
    S.separate(net, tones(int(20 * S.RATE)), lambda b: None, cancelled=lambda: net.calls >= 1)
    assert net.calls == 1


def test_a_failure_while_joining_is_not_swallowed():
    def boom(_block):
        raise OSError("disco lleno")

    with pytest.raises(OSError, match="disco lleno"):
        S.separate(Identity(), tones(int(12 * S.RATE)), boom)


def test_each_track_gets_its_wave_on_a_common_scale():
    waves = S.Waves(["drums", "vocals"], length=1000, buckets=10)
    loud = np.zeros((2, 2, 600), dtype=np.float32)
    loud[0, :, 50] = 0.8  # la bateria pega fuerte en la primera columna
    loud[1, 1, 250] = 0.2  # la voz, flojita, en la tercera
    waves.add(loud)
    rest = np.zeros((2, 2, 400), dtype=np.float32)
    rest[0, 0, 399] = -0.4  # en la ultima, un golpe en negativo
    waves.add(rest)
    r = waves.result()
    assert r["buckets"] == 10
    drums, vocals = r["tracks"]["drums"]["peaks"], r["tracks"]["vocals"]["peaks"]
    assert drums[0] == 1.0 and drums[9] == 0.5 and sum(drums[1:9]) == 0
    # la escala es la de la pista mas alta: la voz se ve pequeña
    assert vocals[2] == 0.25 and vocals[0] == 0
    assert 0 < r["tracks"]["drums"]["rms"][0] < drums[0]


def _write_safetensors(path, tensors: dict) -> None:
    header, blobs, at = {}, [], 0
    for name, value in tensors.items():
        raw = np.ascontiguousarray(value).tobytes()
        kind = {"float16": "F16", "float32": "F32"}[str(value.dtype)]
        header[name] = {
            "dtype": kind,
            "shape": list(value.shape),
            "data_offsets": [at, at + len(raw)],
        }
        blobs.append(raw)
        at += len(raw)
    head = json.dumps(header).encode()
    with open(path, "wb") as f:
        f.write(len(head).to_bytes(8, "little"))
        f.write(head)
        for raw in blobs:
            f.write(raw)


def test_safetensors_are_read_without_the_library(tmp_path):
    a = np.arange(6, dtype=np.float16).reshape(2, 3)
    b = np.array([1.5, -2.0], dtype=np.float32)
    _write_safetensors(tmp_path / "w.safetensors", {"a": a, "b": b})
    got = S.read_safetensors(tmp_path / "w.safetensors")
    np.testing.assert_array_equal(got["a"], a)
    np.testing.assert_array_equal(got["b"], b)
    (tmp_path / "roto.safetensors").write_bytes(b"\xff" * 16)
    with pytest.raises(ValueError):
        S.read_safetensors(tmp_path / "roto.safetensors")


@pytest.mark.parametrize("graph", ["htdemucs_6s", "htdemucs"])
def test_the_real_graph_loads_with_weights_laid_out_like_the_official_ones(tmp_path, graph):
    """Pesos inventados con los nombres y formas del archivo del autor: ONNX
    Runtime comprueba al cargar que cada peso casa en nombre, tipo y forma."""
    pytest.importorskip("onnxruntime")
    from danplay import stems

    folder = stems.GRAPHS
    manifest = json.loads((folder / f"{graph}.json").read_text(encoding="utf-8"))
    rng = np.random.default_rng(0)
    fake = {}
    for item in manifest["initializers"]:
        if item["source"] not in fake:
            fake[item["source"]] = (rng.standard_normal(item["shape"]) * 0.01).astype(np.float16)
    weights = tmp_path / manifest["weights"]["file"]
    _write_safetensors(weights, fake)
    session = S.open_session(folder / f"{graph}.onnx", manifest, weights, n_threads=2)
    inputs = {i.name: i.shape for i in session.get_inputs()}
    outputs = {o.name: o.shape for o in session.get_outputs()}
    assert inputs == {"mix": [1, 2, S.SEGMENT], "mag": [1, 4, S.FREQS, 336]}
    n = len(manifest["sources"])
    assert outputs == {"spec": [1, n, 4, S.FREQS, 336], "wave": [1, n, 2, S.SEGMENT]}


# ------------------------------------------------------------ el trabajo entero

SIX = ["drums", "bass", "other", "vocals", "guitar", "piano"]  # el orden de htdemucs_6s
FILES = {
    "drums": "Bateria.flac",
    "vocals": "Voces.flac",
    "bass": "Bajo.flac",
    "guitar": "Guitarra.flac",
    "piano": "Piano.flac",
    "other": "Otros.flac",
}


def _song(tmp_path, seconds=9.0):
    """Una cancion de tonos en WAV, y lo que se lee de ella (lo que usa el motor)."""
    import shutil
    import wave

    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        pytest.skip("hace falta ffmpeg")
    x = tones(int(seconds * S.RATE))
    path = tmp_path / "cancion.wav"
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(S.RATE)
        w.writeframes((np.clip(x.T, -1, 1) * 32767).astype("<i2").tobytes())
    return ffmpeg, path, S._decode(ffmpeg, str(path))


def _job(tmp_path, ffmpeg, song, nets, **extra) -> dict:
    manifest = tmp_path / "red.json"
    manifest.write_text(json.dumps({"sources": SIX}), encoding="utf-8")
    return {
        "input": str(song),
        "output": str(tmp_path / "pistas"),
        "files": FILES,
        "rest": "other",
        "nets": [
            {"graph": "g", "manifest": str(manifest), "weights": "w", "take": t} for t in nets
        ],
        "ffmpeg": ffmpeg,
        "waves": str(tmp_path / "ondas.json"),
        **extra,
    }


def test_what_is_not_in_the_song_is_not_a_track_and_all_add_up_to_the_song(tmp_path, capsys):
    """Sin piano (la red no le da nada): no sale su pista. «Otros» es lo que
    queda de la cancion, asi que todas juntas son exactamente la cancion."""
    ffmpeg, song, mix = _song(tmp_path)
    shares = dict(
        zip(SIX, (0.3, 0.2, 0.9, 0.3, 0.1, 0.0), strict=True)
    )  # «other» de la red: ni se mira
    net = Identity(tuple(shares.values()))
    job = _job(tmp_path, ffmpeg, song, [["drums", "bass", "vocals", "guitar", "piano"]])
    r = S.run(job, open_net=lambda *a: net)
    assert r["tracks"] == {s: FILES[s] for s in ["drums", "vocals", "bass", "guitar", "other"]}
    out = tmp_path / "pistas"
    assert not (out / "Piano.flac").exists()
    assert r["levels"]["piano"]["db"] < S.QUIET_DB and r["levels"]["piano"]["active"] == 0
    assert r["levels"]["drums"]["db"] == pytest.approx(20 * math.log10(0.3), abs=0.2)
    back = {s: S._decode(ffmpeg, str(out / name)) for s, name in r["tracks"].items()}
    # lo que queda: la cancion menos las demas (aqui, una decima parte)
    np.testing.assert_allclose(back["other"], 0.1 * mix, atol=2e-4)
    total = sum(back.values())
    assert np.abs(total - mix).max() < 1e-4
    waves = json.loads((tmp_path / "ondas.json").read_text())
    assert set(waves["tracks"]) == set(r["tracks"])
    # y conto como iba
    said = [json.loads(line) for line in capsys.readouterr().out.splitlines()]
    assert {m.get("step") for m in said} >= {"decode", "load", "separate", "compose"}


def test_the_improvement_keeps_what_was_decided(tmp_path):
    """La pasada buena: una red por fuente, lo que no se mejora entra tal
    cual, y salen las mismas pistas que en la rapida aunque ahora alguna suene
    poco."""
    ffmpeg, song, mix = _song(tmp_path)
    carried = tmp_path / "Guitarra.flac"
    enc = S._encoder(ffmpeg, carried)
    S._write(enc, 0.25 * mix)
    S._close({"guitar": enc}, "guardar")
    nets = iter([Identity((0.5, 0, 0, 0, 0, 0)), Identity((0, 0, 0, 0.001, 0, 0))])
    job = _job(
        tmp_path,
        ffmpeg,
        song,
        [["drums"], ["vocals"]],
        carry={"guitar": str(carried)},
        keep=["drums", "vocals", "guitar", "other"],
    )
    r = S.run(job, open_net=lambda *a: next(nets))
    # la voz casi no suena, pero la rapida la vio: se queda
    assert list(r["tracks"]) == ["drums", "vocals", "guitar", "other"]
    out = tmp_path / "pistas"
    other = S._decode(ffmpeg, str(out / "Otros.flac"))
    np.testing.assert_allclose(other, (1 - 0.5 - 0.001 - 0.25) * mix, atol=2e-4)


def test_a_source_that_only_plays_a_moment_stays():
    """Un piano que solo entra en el puente: poco en conjunto, pero fuerte ahi."""
    mix = np.ones(100)
    piano = np.zeros(100)
    piano[40:45] = 0.5  # un 5 % de la cancion, a -3 dB de ella
    level = S.presence(piano, mix)
    assert level["db"] < S.QUIET_DB + 20 and level["active"] == pytest.approx(0.05)
    assert S.keeps(level)
    bleed = np.full(100, 1e-4)  # lo que se cuela de las demas: siempre muy abajo
    assert not S.keeps(S.presence(bleed, mix))
    assert not S.keeps(S.presence(np.zeros(100), mix))
    assert S.presence(np.zeros(100), np.zeros(100)) == {"db": -120.0, "active": 0.0}


def test_the_tracks_can_be_kept_in_opus(tmp_path):
    from danplay import stems

    ffmpeg, song, _mix = _song(tmp_path, seconds=3.0)
    if not stems._has_opus(ffmpeg):
        pytest.skip("este ffmpeg no sabe hacer Opus")
    job = _job(tmp_path, ffmpeg, song, [["drums"]], format="opus")
    r = S.run(job, open_net=lambda *a: Identity((0.5, 0, 0, 0, 0, 0)))
    assert r["tracks"] == {"drums": "Bateria.opus", "other": "Otros.opus"}
    out = tmp_path / "pistas"
    assert sorted(p.name for p in out.iterdir()) == ["Bateria.opus", "Otros.opus"]
