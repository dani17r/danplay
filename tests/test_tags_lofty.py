"""lofty (el crate de Rust) y mutagen (lo de antes) leen y escriben lo mismo.

Las bibliotecas de verdad estan etiquetadas con mutagen: lo que escribio
tiene que leerse igual con lofty, y lo que escribe lofty lo tiene que leer
igual cualquier programa que leyera lo de antes (mutagen hace de testigo).
"""

import subprocess
from pathlib import Path

import pytest
from conftest import make_audio

from danplay import tags
from danplay import tags_mutagen as legacy

pytestmark = pytest.mark.skipif(tags._rust is None, reason="hace falta el crate de Rust")

FORMATS = {
    "mp3": ["-codec:a", "libmp3lame", "-b:a", "64k"],
    "flac": ["-codec:a", "flac"],
    "ogg": ["-codec:a", "libvorbis", "-q:a", "1"],
    "opus": ["-codec:a", "libopus", "-b:a", "32k"],
    "m4a": ["-codec:a", "aac", "-b:a", "64k"],
    "wav": ["-codec:a", "pcm_s16le"],
}
JPEG = b"\xff\xd8\xff\xe0" + b"portada" * 60
FIELDS = {
    "artist": "Miel San Marcos",
    "title": "Señor, Tú eres fiel",
    "album": "Pentecostés",
    "year": "2018",
    "genre": "Adoración",
    "album_artist": "Varios",
    "comment": "en vivo",
    "lyrics": "Primera línea\nsegunda, con ñandú",
    "key": "F#m",
    "bpm": 72.5,
    "stars": 3,
    "favorite": "1",
    "blur": "0",
    "tags": "domingo; rapida",
    "playlists": "Domingo, tarde;",
    "study": '{"loop": [1.0, 2.5], "notes": "cejilla 2 — ¡ojo!"}',
}


def _make(tmp_path, ext, name):
    try:
        return make_audio(tmp_path / f"{name}.{ext}", FORMATS[ext], seconds=1.0)
    except subprocess.CalledProcessError:
        pytest.skip(f"este ffmpeg no sabe escribir {ext}")


def _same(a: dict, b: dict, what: str) -> None:
    for key in a:
        if key == "duration":
            assert a[key] == pytest.approx(b[key], abs=0.06), f"{what}: {key}"
        elif key == "bitrate":
            # cada uno la calcula a su manera (de las cabeceras, o del tamaño)
            assert (a[key] == 0) == (b[key] == 0), f"{what}: {key}"
        else:
            assert a[key] == b[key], f"{what}: {key}: lofty {a[key]!r}, mutagen {b[key]!r}"


@pytest.fixture(params=list(FORMATS))
def ext(request, synthetic_ok):
    return request.param


def test_lo_que_escribio_mutagen_se_lee_igual(tmp_path, ext):
    path = _make(tmp_path, ext, "mutagen")
    assert legacy.write_fields(path, FIELDS)
    assert legacy.write_cover(path, JPEG, "image/jpeg")
    _same(tags.read_all(path), legacy.read_all(path), ext)
    assert tags.extract_cover(path) == legacy.extract_cover(path) == (JPEG, "image/jpeg")


def test_lo_que_escribe_lofty_lo_lee_mutagen(tmp_path, ext):
    path = _make(tmp_path, ext, "lofty")
    assert tags._write_fields(path, FIELDS)
    assert tags.write_cover(path, JPEG, "image/jpeg")
    mine, theirs = tags.read_all(path), legacy.read_all(path)
    _same(mine, theirs, ext)
    # el bpm se redondea al par, como round() de Python
    assert mine["bpm"] == 72 and mine["stars"] == 3 and mine["playlists"] == ["Domingo, tarde"]
    assert legacy.extract_cover(path) == (JPEG, "image/jpeg")


def test_lofty_retoca_lo_de_mutagen_sin_duplicar_nada(tmp_path, ext):
    """El caso de cada dia: una cancion que ya etiqueto mutagen, a la que
    ahora se le cambian las estrellas y la portada con lofty."""
    path = _make(tmp_path, ext, "mezcla")
    assert legacy.write_fields(path, FIELDS)
    assert legacy.write_cover(path, JPEG, "image/jpeg")
    otra = b"\x89PNG\r\n\x1a\n" + b"otra" * 50
    assert tags.rate(path, 5)
    assert tags.write_cover(path, otra, "image/png")
    after = legacy.read_all(path)
    assert after["stars"] == 5 and after["title"] == FIELDS["title"]
    assert after["study"] == FIELDS["study"] and after["playlists"] == ["Domingo, tarde"]
    cover = legacy.extract_cover(path)
    assert cover is not None and cover[0] == otra
    _same(tags.read_all(path), after, ext)
    if ext in ("mp3", "wav"):
        from mutagen.id3 import ID3

        t = legacy._load(path)[2]
        assert isinstance(t, ID3) and t.version[:2] == (2, 3), "se guarda en ID3v2.3"
        assert len(t.getall("POPM")) == 1 and len(t.getall("APIC")) == 1
        assert len(t.getall("TXXX:LISTAS")) == 1
    if ext == "wav":
        # un solo bloque de etiquetas: el «id3 » que escribia mutagen
        data = Path(path).read_bytes()
        assert data.lower().count(b"id3 ") == 1
    if ext == "flac":
        from mutagen.flac import FLAC

        assert len(FLAC(str(path)).pictures) == 1


def test_un_frame_raro_no_deja_el_archivo_sin_leer(tmp_path, synthetic_ok):
    """Un año que no es fecha («2018/2019», como lo deja algun programa)
    hacia que lofty, en su modo por defecto, no leyera NADA del archivo: ni
    titulo ni duracion. Se lee como mutagen: lo raro se salta."""
    path = _make(tmp_path, "mp3", "anio")
    assert tags.write(path, year="2018/2019", title="Mi Gozo")
    d = tags.read_all(path)
    assert d["title"] == "Mi Gozo" and d["duration"] > 0.5
    assert tags.rate(path, 2), "y se puede seguir escribiendo"
    assert tags.read_all(path)["stars"] == 2 and legacy.read_all(path)["title"] == "Mi Gozo"
    assert tags.write(path, year="2019")
    assert tags.read_all(path)["year"] == legacy.read_all(path)["year"] == "2019"


def test_las_reproducciones_del_popm_se_conservan(tmp_path, synthetic_ok):
    path = _make(tmp_path, "mp3", "veces")
    assert legacy.write_fields(path, {"stars": 2, "play_count": 17})
    assert tags.rate(path, 4)
    d = tags.read_all(path)
    assert (d["stars"], d["play_count"]) == (4, 17)
    assert tags.rate(path, 1, play_count=3)
    assert legacy.read_all(path)["play_count"] == 3


def test_muchas_a_la_vez_dan_lo_mismo_que_una_a_una(tmp_path, synthetic_ok):
    paths = []
    for i, ext in enumerate(FORMATS):
        p = _make(tmp_path, ext, f"lote{i}")
        tags.write(p, artist=f"Artista {i}", title=ext)
        paths.append(str(p))
    many = tags.read_many([*paths, str(tmp_path / "no-esta.mp3")])
    for p in paths:
        assert many[p] == tags.read_all(p)
    assert many[str(tmp_path / "no-esta.mp3")]["artist"] == ""
