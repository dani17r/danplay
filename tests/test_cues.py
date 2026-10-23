"""Las guias habladas (danplay/cues.py y danplay/speech.py), sin bajar voces:
el texto como se dice, los fonemas, el archivo, la seguridad de las rutas
y la API."""

import json
import os
import time
import wave

import numpy as np
import pytest


def test_numbers_and_shorthand_are_said_in_words():
    from danplay import speech

    assert speech.normalize("Puente x2") == "Puente dos veces."
    assert speech.normalize("4 tiempos") == "cuatro tiempos."
    assert speech.normalize("Sube 1/2 tono") == "Sube medio tono."
    assert speech.normalize("a 124 bpm") == "a ciento veinticuatro pulsos por minuto."
    assert speech.normalize("¿Otra vez?") == "¿Otra vez?"
    assert speech.normalize("x1") == "una vez."
    assert speech.normalize("Bridge x2", "en") == "Bridge two times."
    assert (
        speech.number(21) == "veintiuno"
        and speech.number(1999) == "mil novecientos noventa y nueve"
    )


def test_the_phonemes_are_the_ones_the_spanish_voices_learnt():
    """Como los da espeak-ng (con el que se entrenaron las voces de Piper)."""
    from danplay import speech

    assert speech.phonemes("Coro, cuatro tiempos") == "kˈoɾo, kwˈatɾo tjˈempos."
    assert speech.phonemes("Puente") == "pwˈɛnte."
    # España distingue la z; America, no
    assert speech.phonemes("Cielo") == "θjˈelo."
    assert speech.phonemes("Cielo", "es-419") == "sjˈelo."
    # la b suave entre vocales y la n que se vuelve m
    assert speech.phonemes("un bajo") == "ˈum bˈaxo."
    ids = speech.piper_ids("ˈa", {"^": [1], "_": [0], "$": [2], "a": [14], "ˈ": [120]})
    assert ids == [1, 0, 120, 0, 14, 0, 2]


def test_polish_trims_the_silence_and_levels_it():
    from danplay import cues

    rate = 22050
    t = np.arange(rate) / rate
    voice = 0.2 * np.sin(2 * np.pi * 220 * t).astype(np.float32)
    wav = np.concatenate([np.zeros(rate // 2), voice, np.zeros(rate)]).astype(np.float32)
    out = cues.polish(wav, rate)
    # medio segundo de silencio delante: fuera (queda el margen)
    assert abs(out.size - rate - 2 * int(cues.MARGIN * rate)) < rate // 50
    assert abs(float(np.max(np.abs(out))) - 10 ** (-1 / 20)) < 0.01
    assert abs(out[0]) < 1e-3 and abs(out[-1]) < 1e-3, "sin chasquido en los bordes"
    assert cues.polish(np.zeros(1000, np.float32), rate).size == 0


@pytest.fixture
def guides(tmp_path, monkeypatch):
    from danplay import config

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    return tmp_path


def _recording(path, rate=48000, seconds=1.0):
    t = np.arange(int(rate * seconds)) / rate
    x = np.concatenate(
        [np.zeros(rate // 4), 0.3 * np.sin(2 * np.pi * 300 * t), np.zeros(rate // 4)]
    )
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes((x * 32767).astype("<i2").tobytes())


def test_a_recording_becomes_a_guide_and_only_guides_are_read(guides):
    from danplay import cues

    rec = guides / "danplay-guia-1.wav"
    _recording(rec)
    made = cues.import_recording(str(rec))
    assert cues.own_file(made["file"]) == made["file"]
    assert 1.0 <= made["seconds"] <= 1.1, "sin el silencio de los lados"
    assert cues.audio(made["file"])[:4] == b"RIFF"
    # lo de fuera de la carpeta de guias, ni se lee ni se guarda con la cancion
    assert cues.own_file(str(rec)) is None
    assert cues.own_file(str(cues.folder() / ".." / "danplay.db")) is None
    assert cues.own_file(str(cues.folder() / "x.mp3")) is None
    with pytest.raises(cues.CueError):
        cues.audio(str(rec))
    with pytest.raises(cues.CueError, match="no encuentro"):
        cues.import_recording(str(guides / "no-esta.wav"))
    silent = guides / "silencio.wav"
    with wave.open(str(silent), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(8000)
        w.writeframes(b"\0\0" * 8000)
    with pytest.raises(cues.CueError, match="silencio"):
        cues.import_recording(str(silent))


def test_an_imported_audio_lasts_at_most_the_song(guides, monkeypatch):
    from danplay import cues

    long = guides / "aviso-largo.wav"
    _recording(long, rate=8000, seconds=70)
    with pytest.raises(cues.CueError, match="un minuto"):
        cues.import_recording(str(long))
    with pytest.raises(cues.CueError, match="más que la canción"):
        cues.import_recording(str(long), longest=40)
    made = cues.import_recording(str(long), longest=200)
    assert 69.9 <= made["seconds"] <= 70.1
    assert long.exists(), "un audio del equipo no se borra al importarlo"
    monkeypatch.setattr(cues, "MAX_IMPORT_SECONDS", 50.0)
    with pytest.raises(cues.CueError, match="como mucho"):
        cues.import_recording(str(long), longest=30_000)


def test_a_guide_is_cut_faded_and_cleaned_into_a_new_one(guides, monkeypatch):
    from danplay import cues

    rec = guides / "danplay-guia-9.wav"
    _recording(rec, rate=16000, seconds=2.0)
    made = cues.import_recording(str(rec))
    w = cues.wave_of(made["file"], 64)
    assert len(w["peaks"]) == 64 and max(w["peaks"]) == 1.0
    assert 2.0 <= w["seconds"] <= 2.1
    with pytest.raises(cues.CueError):
        cues.wave_of(str(guides / "fuera.wav"))

    cut = cues.edit_clip(
        made["file"], start=0.5, end=1.5, fade_in=0.2, fade_out=0.3, normalize=True
    )
    assert cut["file"] != made["file"] and cues.own_file(cut["file"]) == cut["file"]
    assert 0.99 <= cut["seconds"] <= 1.01
    with wave.open(cut["file"]) as f:
        x = np.frombuffer(f.readframes(f.getnframes()), "<i2").astype(np.float32) / 32767
    assert abs(x[0]) < 1e-3 and abs(x[-1]) < 1e-3, "los fundidos, en los bordes"
    assert abs(float(np.max(np.abs(x))) - 10 ** (-1 / 20)) < 0.01, "a su volumen"
    assert os.path.exists(made["file"]), "la de antes no se toca"
    again = cues.edit_clip(
        made["file"], start=0.5, end=1.5, fade_in=0.2, fade_out=0.3, normalize=True
    )
    assert again["file"] == cut["file"], "lo mismo, el mismo archivo"

    trial = cues.edit_clip(made["file"], end=1.0, preview=True)
    assert "previews" in trial["file"] and cues.own_file(trial["file"]) is None
    assert cues.own_file(trial["file"], previews=True) == trial["file"]
    # una prueba se puede guardar después (editarla otra vez, sin preview)
    kept = cues.edit_clip(trial["file"])
    assert cues.own_file(kept["file"]) == kept["file"]

    with pytest.raises(cues.CueError, match="corto"):
        cues.edit_clip(made["file"], start=1.0, end=1.01)
    with pytest.raises(cues.CueError):
        cues.edit_clip("/etc/passwd")

    # quitar el ruido y los graves pasa por ffmpeg con esos filtros
    seen = []
    monkeypatch.setattr(cues, "_ffmpeg_filter", lambda x, rate, chain: seen.append(chain) or x)
    cues.edit_clip(made["file"], denoise=True, highpass=True)
    assert seen == ["highpass=f=100,afftdn=nf=-25"]


def test_cleaning_a_guide_with_ffmpeg(guides):
    from danplay import convert, cues

    if not convert.tool("ffmpeg"):
        pytest.skip("sin ffmpeg")
    rate = 16000
    t = np.arange(rate) / rate
    noisy = (
        0.3 * np.sin(2 * np.pi * 440 * t) + 0.02 * np.random.default_rng(1).standard_normal(rate)
    ).astype(np.float32)
    out = cues._ffmpeg_filter(noisy, rate, "highpass=f=100,afftdn=nf=-25")
    assert abs(out.size - rate) < rate // 100


def test_the_voices_and_what_they_weigh(guides):
    from danplay import cues

    voices = {v["id"]: v for v in cues.voices()}
    assert len([v for v in voices.values() if v["engine"] == "supertonic"]) == 10
    assert voices["piper-es_AR-daniela-high"]["lang"] == "Argentina"
    assert not any(v["installed"] for v in voices.values())
    st = voices["st-F1"]
    assert 390e6 < st["size"] < 410e6
    # cada archivo con su sha256 y su revision fijada
    for f in cues._find("st-M3")["files"] + cues._find("piper-es_MX-ald-medium")["files"]:
        assert len(f["sha256"]) == 64 and f["bytes"] > 0
        assert "/resolve/" in f["url"] and "/main/" not in f["url"]
    with pytest.raises(cues.CueError, match="no conozco"):
        cues._find("robot")


def test_installing_a_voice_checks_what_arrives(guides, monkeypatch):
    from danplay import cues

    voice = cues._find("piper-es_ES-carlfm-x_low")
    good = {f["path"]: b"x" * f["bytes"] for f in voice["files"]}

    class Answer:
        def __init__(self, data):
            self.data = data

        def __enter__(self):
            return self

        def __exit__(self, *a):
            return False

        def read(self, n):
            block, self.data = self.data[:n], self.data[n:]
            return block

    def urlopen(req, timeout):
        path = next(p for p in good if req.full_url.endswith(p))
        return Answer(good[path])

    monkeypatch.setattr(cues.urllib.request, "urlopen", urlopen)
    # lo que llega no es lo que debe: no se queda nada
    with pytest.raises(cues.CueError, match="sha256"):
        cues.install(voice["id"], wait=True)
    assert not cues.installed(voice["id"])
    assert not list((cues._voices_dir() / "piper").rglob("*.part"))
    assert next(v for v in cues.voices() if v["id"] == voice["id"])["error"]
    # con lo de verdad (aqui, su sha256 cambiado a lo que llega)
    import hashlib

    for f in voice["files"]:
        f["sha256"] = hashlib.sha256(good[f["path"]]).hexdigest()
    monkeypatch.setattr(cues, "_find", lambda vid: voice)
    with cues._LOCK:
        cues._INSTALLS.clear()
    assert cues.install(voice["id"], wait=True) == {"installed": True}
    assert cues.installed(voice["id"])


@pytest.mark.python_core("la voz de mentira sustituye cues.install, cues.installed y cues.say")
def test_the_api_for_guides(configured_library, guides, monkeypatch, connect_api):
    from danplay import cues, library

    root, _ = configured_library
    library.add_folder(root)
    library.scan()
    client = connect_api()
    c = library.search("", limit=1)[0]
    r = client.get("/api/cues/voices").json()
    assert len(r["voices"]) == 17 and r["available"] == ""
    # una voz sin bajar: empieza a bajarse y se dice que espere
    started = []
    monkeypatch.setattr(
        cues, "install", lambda vid, wait=False: started.append(vid) or {"installed": False}
    )
    r = client.post(f"/api/song/{c['id']}/cues", json={"text": "Coro", "voice": "st-F1"})
    assert r.status_code == 409 and started == ["st-F1"]
    # con la voz (aqui, una de mentira) se dice y se guarda
    monkeypatch.setattr(cues, "installed", lambda vid: True)
    monkeypatch.setattr(
        cues, "say", lambda text, vid: (0.3 * np.sin(np.arange(8000) / 5).astype(np.float32), 16000)
    )
    r = client.post(f"/api/song/{c['id']}/cues", json={"text": "Coro", "voice": "st-F1"})
    assert r.status_code == 200, r.text
    made = r.json()
    assert made["file"].endswith(".wav") and made["seconds"] == 0.5
    again = client.post(f"/api/song/{c['id']}/cues", json={"text": "Coro", "voice": "st-F1"}).json()
    assert again["file"] == made["file"], "la misma frase con la misma voz es el mismo archivo"
    audio = client.post("/api/cues/audio", json={"file": made["file"]}).json()
    assert audio["mime"] == "audio/wav" and audio["data"].startswith("UklGR")
    assert client.post("/api/cues/audio", json={"file": "/etc/passwd"}).status_code == 404
    # se guarda con el estudio; lo de fuera de la carpeta, no
    d = client.put(
        f"/api/song/{c['id']}/study",
        json={
            "cues": [
                {"t": 12.345, "file": made["file"], "text": "Coro", "voice": "st-F1"},
                {"t": 3, "file": "/etc/passwd"},
                {"t": 5, "file": made["file"], "gain": 1.5},
            ],
            "metronome": {
                "own": {"beats": [1.0, 1.5, 1.5, 2.0, -1], "bars": [0, 2, 9], "meter": 3}
            },
        },
    ).json()
    study = json.loads(d["study"])
    assert study["cues"] == [
        {"t": 5.0, "file": made["file"], "gain": 1.5},
        {"t": 12.35, "file": made["file"], "text": "Coro", "voice": "st-F1"},
    ]
    assert study["metronome"]["own"] == {"beats": [1.0, 1.5, 2.0], "bars": [0, 2], "meter": 3}
    # los carriles de las guías (hasta 6), y cada guía en el suyo
    d = client.put(
        f"/api/song/{c['id']}/study",
        json={
            "cues": [
                {"t": 1, "file": made["file"], "lane": "g2"},
                {"t": 2, "file": made["file"], "lane": "../x"},
            ],
            "cueLanes": [
                {"key": "g1", "name": "Banda", "auto": False},
                {"key": "g2", "name": "Baterista", "gain": 1.4, "mute": True, "solo": False},
                {"key": "g2", "name": "repetido"},
                {"key": "Mal Nombre"},
                {"key": "g3", "name": "Guía automática", "auto": True},
            ],
        },
    ).json()
    study = json.loads(d["study"])
    assert study["cues"] == [
        {"t": 1.0, "file": made["file"], "lane": "g2"},
        {"t": 2.0, "file": made["file"]},
    ]
    # el de la guía automática lo dice (los demás, ni la clave)
    assert study["cueLanes"] == [
        {"key": "g1", "name": "Banda"},
        {"key": "g2", "name": "Baterista", "gain": 1.4, "mute": True},
        {"key": "g3", "name": "Guía automática", "auto": True},
    ]
    seven = [{"key": f"g{i}"} for i in range(7)]
    assert client.put(f"/api/song/{c['id']}/study", json={"cueLanes": seven}).status_code == 422, (
        "más de 6 carriles, no"
    )
    rec = guides / "danplay-guia-2.wav"
    _recording(rec, rate=16000)
    r = client.post(f"/api/song/{c['id']}/cues/import", json={"path": str(rec)})
    assert r.status_code == 200 and "grabada-" in r.json()["file"]
    assert client.post("/api/song/999999/cues/import", json={"path": str(rec)}).status_code == 404
    # un audio del equipo, hasta lo que dura la canción
    long = guides / "cuenta.wav"
    _recording(long, rate=8000, seconds=65)
    r = client.post(f"/api/song/{c['id']}/cues/import", json={"path": str(long)})
    assert r.status_code == 422, "sin decir cuánto dura la canción, un minuto"
    assert (c["duration"] or 0) < 60
    r = client.post(f"/api/song/{c['id']}/cues/import", json={"path": str(long), "limit": 120})
    assert r.status_code == 422 and "canción" in r.json()["detail"], (
        "lo que dice la app, pero nunca más de lo que dura la canción"
    )
    # su onda, y una nueva recortada
    guide = made["file"]
    w = client.get("/api/cues/wave", params={"file": guide, "buckets": 32}).json()
    assert len(w["peaks"]) == 32 and w["seconds"] == 0.5
    assert client.get("/api/cues/wave", params={"file": "/etc/passwd"}).status_code == 404
    r = client.post("/api/cues/edit", json={"file": guide, "start": 0.1, "fade_out": 0.1})
    assert r.status_code == 200 and "editada-" in r.json()["file"]
    assert r.json()["seconds"] == 0.4
    assert client.post("/api/cues/edit", json={"file": guide, "fade_in": 9}).status_code == 422


def test_the_old_and_the_new_name_are_merged(tmp_path):
    """La 1.30 vuelve a crear sus carpetas si una de ahora las renombro
    mientras seguia en uso: al arrancar se juntan, con lo mas reciente de
    cada cosa, y no queda el nombre de antes."""
    from danplay import config

    def write(path, text, age):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        when = time.time() - age
        os.utime(path, (when, when))

    write(tmp_path / "pulso" / "a.json", "solo antes", 50)
    write(tmp_path / "pulso" / "b.json", "b antes, nueva", 10)
    write(tmp_path / "beats" / "b.json", "b ahora, vieja", 100)
    write(tmp_path / "pulso" / "c.json", "c antes, vieja", 100)
    write(tmp_path / "beats" / "c.json", "c ahora, nueva", 10)
    write(tmp_path / "diario.json", "diario nuevo", 10)
    write(tmp_path / "journal.json", "diario viejo", 100)
    write(tmp_path / "voces" / "v.onnx", "voz", 10)
    config.rename_old(tmp_path, config.OLD_DATA_NAMES)
    read = lambda p: (tmp_path / p).read_text(encoding="utf-8")  # noqa: E731
    assert read("beats/a.json") == "solo antes"
    assert read("beats/b.json") == "b antes, nueva"
    assert read("beats/c.json") == "c ahora, nueva"
    assert read("journal.json") == "diario nuevo"
    assert read("voices/v.onnx") == "voz"
    assert not any((tmp_path / gone).exists() for gone in ("pulso", "diario.json", "voces"))


def test_the_guides_of_before_1_31_move_to_their_new_folder(
    configured_library, tmp_path, monkeypatch
):
    """Hasta la 1.30 las guias estaban en DATA_DIR/guias/ (y las de prueba en
    guias/prueba/): se renombra al arrancar, y lo guardado que apuntaba alli
    (en la base o en las etiquetas) apunta a la carpeta nueva."""
    from danplay import config, cues, library
    from danplay.library import db

    data = tmp_path / "datos"
    monkeypatch.setattr(config, "DATA_DIR", data)
    old = data / "guias"
    (old / "prueba").mkdir(parents=True)
    (old / "coro-1.wav").write_bytes(b"RIFF")
    config.rename_old(data, config.OLD_DATA_NAMES)
    config.rename_old(data / "cues", (("prueba", "previews"),))
    new = data / "cues" / "coro-1.wav"
    assert new.is_file() and (data / "cues" / "previews").is_dir() and not old.exists()
    # lo que llega con la ruta de antes (de las etiquetas, por ejemplo)
    assert cues.own_file(str(old / "coro-1.wav")) == str(new)
    assert cues.own_file(str(old / "prueba" / "x.wav"), previews=True) == str(
        data / "cues" / "previews" / "x.wav"
    )
    # y lo que ya estaba en la base, al abrirla
    library.add_folder(configured_library[0])
    library.scan()
    cid = library.search("", limit=1, light=True)[0]["id"]
    study = {"cues": [{"t": 1.0, "file": str(old / "coro-1.wav"), "lane": "g1"}]}
    with library.connect() as conn:
        conn.execute("UPDATE songs SET study = ? WHERE id = ?", (json.dumps(study), cid))
        conn.commit()
    db._prepared.clear()
    with library.connect() as conn:
        row = conn.execute("SELECT study FROM songs WHERE id = ?", (cid,)).fetchone()
    assert json.loads(row["study"])["cues"][0]["file"] == str(new)
