"""Las etiquetas en todos los formatos que DanPlay organiza, con archivos de
verdad hechos con ffmpeg: lo que se escribe tiene que volver al leer.

Antes solo se probaba el mp3, y en flac/ogg/opus el modo estudio se escribia
y nunca se leia: perdida la base, no volvia.
"""

import json
import os
import subprocess

import pytest
from conftest import make_audio

from danplay import tags as E

FORMATS = {
    "mp3": ["-codec:a", "libmp3lame", "-b:a", "64k"],
    "flac": ["-codec:a", "flac"],
    "ogg": ["-codec:a", "libvorbis", "-q:a", "1"],
    "opus": ["-codec:a", "libopus", "-b:a", "32k"],
    "m4a": ["-codec:a", "aac", "-b:a", "64k"],
    "wav": ["-codec:a", "pcm_s16le"],
}
JPEG = b"\xff\xd8\xff\xe0" + b"portada" * 60


def _make(tmp_path, ext, name="prueba", seconds=1.0):
    try:
        return make_audio(tmp_path / f"{name}.{ext}", FORMATS[ext], seconds=seconds)
    except subprocess.CalledProcessError:
        pytest.skip(f"este ffmpeg no sabe escribir {ext}")


@pytest.fixture(params=list(FORMATS))
def audio(request, tmp_path, synthetic_ok):
    return _make(tmp_path, request.param)


def test_todo_lo_propio_vuelve_al_leer(audio):
    study = json.dumps({"loop": [1.0, 2.5], "speed": 0.75, "notes": "cejilla 2"})
    assert E.write(
        audio, artist="Barak", title="Mi Gozo", album="Radical", year="2018", genre="Adoracion"
    )
    assert E.rate(audio, 4)
    assert E.set_favorite(audio, True)
    assert E.write_analysis(audio, key="Bb", bpm=72)
    assert E.write_lyrics(audio, "una letra\nde dos lineas")
    assert E.set_labels(audio, ["domingo", "rapida"])
    assert E.set_playlists(audio, ["Herlin", "Domingo, tarde"])
    assert E.set_blurred_cover(audio, True)
    assert E.write_study(audio, study)
    assert E.write_cover(audio, JPEG, "image/jpeg")

    d = E.read_all(audio)
    assert (d["artist"], d["title"], d["album"]) == ("Barak", "Mi Gozo", "Radical")
    assert d["year"].startswith("2018") and d["genre"] == "Adoracion"
    assert d["stars"] == 4 and d["favorite"] is True and d["blur"] is True
    assert d["key"] == "Bb" and d["bpm"] == 72
    assert "dos lineas" in d["lyrics"]
    assert d["tags"] == ["domingo", "rapida"]
    assert d["playlists"] == ["Herlin", "Domingo, tarde"]
    assert d["study"] == study, "el modo estudio tiene que volver del archivo"
    assert d["cover"] is True and E.extract_cover(audio)[0] == JPEG
    assert d["duration"] > 0.5


def test_cero_estrellas_se_leen_como_cero(audio):
    E.rate(audio, 5)
    E.rate(audio, 0)
    assert E.read_all(audio)["stars"] == 0


def test_la_duracion_no_sale_a_cero(tmp_path, synthetic_ok):
    """mutagen.File() devuelve None para algunos .wav: se usa el lector concreto."""
    for ext in ("wav", "mp3", "flac"):
        path = _make(tmp_path, ext, name=f"largo-{ext}", seconds=3.0)
        assert 2.5 < E.duration(path) < 3.5, ext
        assert E.bitrate(path) >= 0


def test_sin_etiquetas_se_lee_lo_basico(tmp_path, synthetic_ok):
    path = _make(tmp_path, "flac")
    d = E.read_all(path)
    assert d["artist"] == "" and d["study"] == "" and d["duration"] > 0.5
    assert E.extract_cover(path) is None
    assert E.read(path) == {}


def test_un_formato_sin_etiquetas_lo_dice_sin_romper(tmp_path):
    raro = tmp_path / "raro.xyz"
    raro.write_bytes(b"no es audio")
    assert E.write(str(raro), artist="x") is False
    assert E.read_all(str(raro))["artist"] == ""
    assert E.extract_cover(str(raro)) is None


def test_la_letra_con_tiempos_se_limpia():
    lrc = "[ar:Barak]\n[ti:Mi Gozo]\n[00:01.00]Mi gozo\n[00:04.50] esta en ti\n"
    assert E.lrc_to_plain(lrc) == "Mi gozo\nesta en ti"
    assert E.lrc_to_plain("una letra\nnormal") == "una letra\nnormal"
    assert E.lrc_to_plain("") == ""


@pytest.mark.parametrize("ext", ["flac", "ogg", "opus"])
def test_perdida_la_base_el_estudio_vuelve_del_archivo(configured_library, ext):
    """El caso real: un flac con su modo estudio, se pierde el indice y al
    reescanear el estudio tiene que estar."""
    from danplay import config, library

    lib, _ = configured_library
    path = _make(lib / "Artistas" / "Barak", ext, name="Barak - Estudio")
    E.write(path, artist="Barak", title="Estudio")
    library.add_folder(str(lib), "prueba")
    library.scan()
    song = next(c for c in library.search("Estudio") if c["path"] == path)
    library.set_study(song["id"], {"loop": [1, 2], "notes": "despacio"})
    assert json.loads(library.by_id(song["id"])["study"])["notes"] == "despacio"

    os.remove(config.DATABASE)
    library._prepared.clear()
    library.add_folder(str(lib), "prueba")
    library.scan()
    back = next(c for c in library.search("Estudio") if c["path"] == path)
    assert json.loads(back["study"]) == {"loop": [1.0, 2.0], "notes": "despacio"}
