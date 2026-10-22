"""La guia automatica (danplay/structure.py y su ruta): las partes de una
cancion y la guia hablada antes de cada una.

Las cuentas se prueban con rasgos sinteticos (una plantilla por parte, con
ruido): sin audio. Y de punta a punta, con una cancion hecha con numpy
(acordes, bateria y volumen distintos por parte) y una voz de mentira, como
en test_cues.py."""

import json
import wave

import numpy as np
import pytest
from conftest import run_job

# Una cancion de rasgos: la intro y el final flojos y sin voz, las estrofas
# y los coros (mas fuertes) que se repiten, y el puente una sola vez.
LEVEL = {"A": 0.05, "B": 0.2, "C": 0.45, "D": 0.25, "E": 0.2, "P": 0.22, "S": 0.3}
SUNG = {"A": 0.0, "B": 0.5, "C": 0.6, "D": 0.5, "E": 0.5, "P": 0.5, "S": 0.0}
SONG = [("A", 4), ("B", 8), ("C", 8), ("B", 8), ("C", 8), ("D", 8), ("C", 8), ("A", 4)]


def _features(plan, seed=1, noise=0.3):
    """(croma, timbre, rms, voz) por unidad: cada parte, su plantilla con
    ruido."""
    rng = np.random.default_rng(seed)
    kinds = sorted({k for k, _ in plan})
    templates = {k: (rng.random(12), rng.standard_normal(12)) for k in kinds}
    chroma, timbre, rms, voice = [], [], [], []
    for kind, n in plan:
        c, t = templates[kind]
        for _ in range(n):
            chroma.append(c + noise * rng.random(12))
            timbre.append(t + noise * rng.standard_normal(12))
            rms.append(LEVEL[kind] * (1 + 0.1 * rng.standard_normal()))
            sung = SUNG[kind] * (1 + 0.2 * rng.standard_normal())
            voice.append(sung if SUNG[kind] else 0.003 * rng.random())
    return np.array(chroma), np.array(timbre), np.array(rms), np.array(voice)


def _labels(sections):
    return [s["label"] for s in sections]


def test_the_parts_come_out_with_their_names():
    from danplay import structure

    chroma, timbre, rms, voice = _features(SONG)
    parts = structure.segment([chroma, timbre], rms, voice)
    assert [p["first"] for p in parts] == [0, 4, 12, 20, 28, 36, 44, 52]
    assert [p["cluster"] for p in parts] == ["A", "B", "C", "B", "C", "D", "C", "A"]
    named = structure.name_sections(parts, bar=1.0)
    assert _labels(named) == [
        "Intro",
        "Estrofa 1",
        "Coro 1",
        "Estrofa 2",
        "Coro 2",
        "Puente",
        "Coro 3",
        "Final",
    ]
    assert named[1] == {"start": 4.0, "end": 12.0, "label": "Estrofa 1"}
    # sin las pistas separadas, la intro y el final por lo flojos que suenan
    without = structure.name_sections(structure.segment([chroma, timbre], rms), bar=1.0)
    assert _labels(without) == _labels(named)


def test_a_solo_is_named_by_the_instrument_that_leads():
    """Sin voz entre partes cantadas: el solo del instrumento que manda. Y lo
    corto justo antes del coro, el pre coro."""
    from danplay import structure

    plan = [("A", 4), ("B", 8), ("P", 4), ("C", 8), ("B", 8), ("P", 4), ("C", 8)]
    plan += [("S", 8), ("C", 8), ("A", 4)]
    chroma, timbre, rms, voice = _features(plan, seed=3)
    n = len(rms)
    sources = {
        "drums": np.full(n, 0.2),
        "bass": np.full(n, 0.15),
        "guitar": np.full(n, 0.05),
        "other": np.full(n, 0.1),
        "vocals": voice,  # la voz no cuenta para los solos
    }
    solo = sum(k for _, k in plan[:7])
    sources["guitar"][solo : solo + 8] = 0.5
    parts = structure.segment([chroma, timbre], rms, voice, sources)
    assert [p.get("lead") for p in parts].count("guitar") == 1
    assert _labels(structure.name_sections(parts, bar=1.0)) == [
        "Intro",
        "Estrofa 1",
        "Pre coro 1",
        "Coro 1",
        "Estrofa 2",
        "Pre coro 2",
        "Coro 2",
        "Solo de guitarra",
        "Coro 3",
        "Final",
    ]
    # si ningun instrumento manda, es un instrumental
    sources["guitar"][solo : solo + 8] = 0.05
    parts = structure.segment([chroma, timbre], rms, voice, sources)
    assert "Instrumental" in _labels(structure.name_sections(parts, bar=1.0))


def test_what_is_left_before_the_first_chorus_is_the_verse():
    """Una parte que no se repite antes del primer coro es mas estrofa (y
    va con la de al lado, que ya lo era); una de un compas se junta con la
    vecina que mas se le parece."""
    from danplay import structure

    plan = [("A", 4), ("B", 8), ("E", 8), ("C", 8), ("B", 8), ("C", 8), ("A", 4)]
    chroma, timbre, rms, voice = _features(plan, seed=5)
    parts = structure.segment([chroma, timbre], rms, voice)
    assert _labels(structure.name_sections(parts, bar=1.0)) == [
        "Intro",
        "Estrofa 1",
        "Coro 1",
        "Estrofa 2",
        "Coro 2",
        "Final",
    ]
    blocks = [np.r_[np.ones((1, 3)), np.ones((4, 3)), -np.ones((4, 3))]]
    blocks[0][:, 0] += np.arange(9)  # que no sean todas iguales
    spans = structure._merge_short([(0, 1), (1, 5), (5, 9)], [structure._prepared(blocks[0])])
    assert spans == [(0, 5), (5, 9)]


def test_the_voice_also_marks_a_change_and_short_parts_join_a_neighbour():
    from danplay import structure

    flat = [np.ones((24, 12))]  # siempre lo mismo: ningun cambio por lo que suena
    voice = np.r_[np.zeros(10), np.ones(14)]
    voice[3] = 1.0  # una frase suelta en la intro: no cambia la parte
    parts = structure.segment(flat, np.ones(24), voice)
    assert [(p["first"], p["last"]) for p in parts] == [(0, 10), (10, 24)]
    assert [p["vocal"] for p in parts] == [0.0, 1.0]
    named = structure.name_sections(parts)
    assert _labels(named) == ["Intro", "Parte"]
    # sin voz: lo que suena a lo mismo, una sola parte
    assert len(structure.segment(flat, np.ones(24))) == 1
    assert structure.segment([np.zeros((0, 12))], []) == []
    assert structure.name_sections([]) == []


def test_the_units_are_the_bars_or_two_second_windows():
    from danplay import structure

    bars = [1.0 + 2 * i for i in range(10)]  # del 1 al 19, y la cancion dura 25
    units, phase = structure.units_from(bars, 25.0)
    assert units[0] == (0.0, 1.0) and phase == 1, "lo de antes del primer compas, aparte"
    assert units[1] == (1.0, 3.0)
    assert units[-3:] == [(19.0, 21.0), (21.0, 23.0), (23.0, 25.0)]
    # con un primer compas casi en el cero, ese trozo va con el
    units, phase = structure.units_from([0.2 + 2 * i for i in range(10)], 20.2)
    assert units[0] == (0.0, 2.2) and phase == 0
    units, phase = structure.units_from([0.0, 2.0, 4.0], 10.0)
    assert phase is None and len(units) == 5 and units[-1][1] == 10.0


def test_the_frames_hear_the_notes():
    from danplay import structure

    t = np.arange(structure.RATE * 2) / structure.RATE
    f = structure.frames(0.5 * np.sin(2 * np.pi * 440 * t))
    assert f["chroma"].shape[1] == 12 and f["timbre"].shape[1] == 12
    assert int(np.argmax(f["chroma"].mean(axis=0))) == 9, "el la (A) es la nota 9"
    assert f["rms"].max() == pytest.approx(0.5 / np.sqrt(2), rel=0.05)
    assert structure.frames(np.zeros(100))["rms"].shape == (1,)


def test_each_guide_ends_a_beat_before_its_part_and_none_overlap():
    from danplay import structure

    sections = [
        {"start": 0.0, "label": "Intro"},
        {"start": 8.0, "label": "Estrofa 1"},
        {"start": 9.5, "label": "Pre coro"},
        {"start": 10.0, "label": "Coro 1"},
        {"start": 10.3, "label": "Puente"},
        {"start": 20.0, "label": "Final"},
    ]
    clips = {"Estrofa 1": 0.8, "Pre coro": 0.6, "Coro 1": 0.5, "Puente": 0.6, "Final": 0.7}
    plan = structure.plan_cues(sections, clips, beat=0.5)
    assert plan == [
        {"t": 6.7, "text": "Estrofa 1"},  # un pulso antes de que empiece
        {"t": 8.4, "text": "Pre coro"},
        # encima de la anterior: justo detras de ella, acabando al empezar
        {"t": 9.5, "text": "Coro 1"},
        # «Puente» ya no cabe antes de su parte: no se anuncia
        {"t": 18.8, "text": "Final"},
    ]
    # sin pulso, medio segundo; nunca antes del principio
    plan = structure.plan_cues([{"start": 1.2, "label": "Coro"}], {"Coro": 0.4})
    assert plan == [{"t": 0.3, "text": "Coro"}]
    plan = structure.plan_cues([{"start": 1.5, "label": "Coro"}], {"Coro": 1.2}, beat=0.6)
    assert plan == [{"t": 0.0, "text": "Coro"}]


# ------------------------------------------------------------ de punta a punta

RATE = 22050
BAR = 2.0  # 120 pulsos por minuto, en 4/4
# lo que suena: intro, estrofa, coro, estrofa, coro y final (compases)
PLAN = [("intro", 4), ("verse", 8), ("chorus", 8), ("verse", 8), ("chorus", 8), ("intro", 4)]
TRUTH = [0.0, 8.0, 24.0, 40.0, 56.0, 72.0]


def _hz(midi):
    return 440 * 2 ** ((midi - 69) / 12)


def _chord(notes, harmonics, amp):
    t = np.arange(int(BAR * RATE)) / RATE
    parts = range(1, 9) if harmonics else range(1, 2)
    x = sum(
        np.sin(2 * np.pi * _hz(n) * k * t) / k ** (harmonics or 1) for n in notes for k in parts
    )
    return amp * np.asarray(x) / len(notes)


def _drums(amp, hats):
    x = np.zeros(int(BAR * RATE))
    t = np.arange(int(0.15 * RATE)) / RATE
    kick = np.sin(2 * np.pi * 60 * t) * np.exp(-t * 30)
    for b in np.arange(0, BAR, 0.5):
        i = int(b * RATE)
        x[i : i + kick.size] += kick
    if hats:
        n = int(0.03 * RATE)
        hat = np.random.default_rng(0).standard_normal(n) * np.exp(-np.arange(n) / RATE * 150)
        for b in np.arange(0.25, BAR, 0.5):
            i = int(b * RATE)
            x[i : i + n] += 0.5 * hat
    return amp * x


def _bar(kind, b):
    """Un compas: la intro, un acorde suave de la menor; la estrofa, do y sol
    con poco brillo y bombo; el coro, fa y la menor brillantes, con bombo y
    charles, y fuerte."""
    if kind == "intro":
        return _chord([57, 60, 64], 0, 0.05), np.zeros(int(BAR * RATE))
    if kind == "verse":
        notes = [48, 52, 55] if (b // 2) % 2 == 0 else [43, 47, 50]
        band = _chord(notes, 2, 0.15) + _drums(0.1, False)
        sung = 0.08
    else:
        notes = [53, 57, 60] if (b // 2) % 2 == 0 else [57, 60, 64]
        band = _chord(notes, 1, 0.3) + _drums(0.25, True)
        sung = 0.12
    t = np.arange(int(BAR * RATE)) / RATE
    voice = sung * np.sin(2 * np.pi * _hz(notes[-1] + 12) * t + 3 * np.sin(2 * np.pi * 5 * t))
    return band, voice


def _write(path, x):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype("<i2").tobytes())


@pytest.fixture
def song(configured_library, tmp_path, monkeypatch):
    """Una cancion con sus partes en la biblioteca, una voz de mentira y la
    carpeta de las guias aparte. Devuelve (cliente, id, raiz, banda, voz)."""
    from fastapi.testclient import TestClient

    from danplay import api, config, cues, library

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    root, _ = configured_library
    bars = [_bar(kind, b) for kind, n in PLAN for b in range(n)]
    band = np.concatenate([b for b, _ in bars])
    voice = np.concatenate([v for _, v in bars])
    peak = np.abs(band + voice).max() / 0.9
    band, voice = band / peak, voice / peak
    path = root / "Artistas" / "Prueba" / "Prueba - Partes.wav"
    path.parent.mkdir(parents=True)
    _write(path, band + voice)
    library.add_folder(root)
    library.scan()
    row = library.by_path(str(path))
    assert row is not None
    monkeypatch.setattr(cues, "installed", lambda vid: True)
    monkeypatch.setattr(
        cues, "say", lambda text, vid: (0.3 * np.sin(np.arange(8000) / 5).astype(np.float32), 16000)
    )
    return TestClient(api.app), row["id"], root, band, voice


def _near(found, truth, unit=BAR):
    return len(found) == len(truth) and all(
        abs(a - b) <= unit + 1e-6 for a, b in zip(found, truth, strict=True)
    )


def test_the_automatic_guide_finds_the_parts_and_says_them(song):
    client, cid, *_ = song
    body = {"voice": "st-F1", "bars": [i * BAR for i in range(40)], "beat": 0.5}
    r = run_job(client, f"/api/song/{cid}/cues/auto", "guia-automatica", json=body)
    sections = r["sections"]
    assert _near([s["start"] for s in sections], TRUTH), sections
    assert sections[-1]["end"] == pytest.approx(80.0, abs=0.1)
    assert _labels(sections) == ["Intro", "Estrofa 1", "Coro 1", "Estrofa 2", "Coro 2", "Final"]
    # una guia por parte (la intro no hace falta), medio segundo de voz y un
    # pulso antes de que empiece la parte
    said = r["cues"]
    assert [c["text"] for c in said] == _labels(sections)[1:]
    for c, s in zip(said, sections[1:], strict=True):
        assert c["voice"] == "st-F1" and c["seconds"] == 0.5 and c["file"].endswith(".wav")
        assert c["t"] == pytest.approx(s["start"] - 1.0, abs=0.01)
    # sin compases (trozos de 2 s), las mismas partes
    r = run_job(client, f"/api/song/{cid}/cues/auto", "guia-automatica", json={"voice": "st-F1"})
    assert _near([s["start"] for s in r["sections"]], TRUTH), r["sections"]


def test_with_its_stems_the_intro_and_the_end_are_where_nobody_sings(song):
    """Con las pistas separadas, la intro es lo de antes de la voz y el final
    lo de despues (aunque no suenen flojos)."""
    from danplay import library, stems

    client, cid, root, band, voice = song
    folder = root / "Separadas" / "Prueba - Partes"
    folder.mkdir(parents=True)
    # la intro y el final, tan fuertes como el resto: sin la voz no se sabria
    loud = band.copy()
    edge = int(8 * RATE)
    loud[:edge] *= 6
    loud[-edge:] *= 6
    _write(folder / "Otros.wav", loud)
    _write(folder / "Voces.wav", voice)
    tracks = [{"source": "vocals", "file": "Voces.wav"}, {"source": "other", "file": "Otros.wav"}]
    (folder / stems.MANIFEST).write_text(json.dumps({"tracks": tracks, "quality": "mejor"}))
    library.update(cid, stems=json.dumps({"folder": "Separadas/Prueba - Partes"}))
    _write(root / "Artistas" / "Prueba" / "Prueba - Partes.wav", loud + voice)
    body = {"voice": "st-F1", "bars": [i * BAR for i in range(40)], "beat": 0.5}
    r = run_job(client, f"/api/song/{cid}/cues/auto", "guia-automatica", json=body)
    sections = r["sections"]
    assert _near([s["start"] for s in sections], TRUTH), sections
    assert _labels(sections) == ["Intro", "Estrofa 1", "Coro 1", "Estrofa 2", "Coro 2", "Final"]


def test_named_parts_are_used_as_they_are(song, monkeypatch):
    from danplay import structure

    client, cid, *_ = song

    def no(*a, **k):
        raise AssertionError("con las partes ya puestas no se escucha la cancion")

    monkeypatch.setattr(structure, "analyse", no)
    body = {
        "voice": "st-F1",
        "beat": 0.5,
        "sections": [
            {"start": 30.5, "label": "Final"},
            {"start": 0, "label": "Intro"},
            {"start": 10, "label": "Coro"},
        ],
    }
    r = run_job(client, f"/api/song/{cid}/cues/auto", "guia-automatica", json=body)
    assert r["sections"] == [
        {"start": 0.0, "end": 10.0, "label": "Intro"},
        {"start": 10.0, "end": 30.5, "label": "Coro"},
        {"start": 30.5, "end": pytest.approx(80.0, abs=0.1), "label": "Final"},
    ]
    assert [(c["t"], c["text"]) for c in r["cues"]] == [(9.0, "Coro"), (29.5, "Final")]


def test_without_the_voice_it_starts_downloading_it(song, monkeypatch):
    from danplay import cues

    client, cid, *_ = song
    started = []
    monkeypatch.setattr(cues, "installed", lambda vid: False)
    monkeypatch.setattr(
        cues, "install", lambda vid, wait=False: started.append(vid) or {"installed": False}
    )
    r = client.post(f"/api/song/{cid}/cues/auto", json={"voice": "st-F1"})
    assert r.status_code == 409 and started == ["st-F1"]
    assert "se está bajando" in r.json()["detail"]
    assert client.post("/api/song/999999/cues/auto", json={"voice": "st-F1"}).status_code == 404
    bad = client.post(f"/api/song/{cid}/cues/auto", json={"voice": "st-F1", "beat": -1})
    assert bad.status_code == 422


def test_a_song_too_short_has_no_parts(song):
    from danplay import library
    from danplay.api import jobs

    client, *_ = song
    short = next(x for x in library.search("", limit=100) if x["file"] == "Barak - Mi Gozo.mp3")
    r = client.post(f"/api/song/{short['id']}/cues/auto", json={"voice": "st-F1"})
    assert r.status_code == 202
    job = jobs.wait("guia-automatica")
    assert job is not None and "demasiado corta" in job["error"]
