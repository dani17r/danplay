"""Lo que hace ffmpeg por DanPlay (convertir a mp3, encoger caratulas) y lo
que viene de fuera (letras, caratulas, la web, la huella), con archivos de
verdad y sin red: los servicios se sustituyen por respuestas preparadas."""

import io
import json
import os
import subprocess

import pytest
from conftest import FFMPEG, make_audio, make_mp3

from danplay import config, convert, enrich, fingerprint, tags, web


def _image(path, size="1600x1200", color="red"):
    subprocess.run(
        [
            FFMPEG,
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            f"color=c={color}:s={size}",
            "-frames:v",
            "1",
            str(path),
        ],
        check=True,
        capture_output=True,
        timeout=60,
    )
    return path


# ------------------------------------------------------------------ convertir


def test_un_flac_pasa_a_mp3_con_sus_etiquetas(tmp_path, synthetic_ok):
    flac = make_audio(tmp_path / "Barak - Mi Gozo.flac", ["-codec:a", "flac"], seconds=2)
    tags.write(flac, artist="Barak", title="Mi Gozo")
    r = convert.convert(flac, "medium")
    assert r["ok"] and r["target"].endswith(".mp3") and not os.path.exists(flac)
    d = tags.read_all(r["target"])
    assert d["artist"] == "Barak" and d["title"] == "Mi Gozo" and d["duration"] > 1.5
    # otra vez con el mismo nombre libre: no pisa el que hay
    flac2 = make_audio(tmp_path / "Barak - Mi Gozo.flac", ["-codec:a", "flac"], seconds=1)
    r2 = convert.convert(flac2, "high", keep_original=True)
    assert r2["target"].endswith("(2).mp3") and os.path.exists(flac2)


def test_lo_que_no_se_convierte(tmp_path, synthetic_ok, monkeypatch):
    mp3 = make_mp3(tmp_path / "ya.mp3")
    assert convert.convert(mp3)["skipped"]
    protegida = make_audio(tmp_path / "Secuencias" / "click.wav", ["-codec:a", "pcm_s16le"])
    assert convert.convert(protegida)["reason"] == "carpeta protegida"
    assert not convert.needs_convert(protegida) and convert.needs_convert(tmp_path / "x.ogg")
    monkeypatch.setattr(convert, "available", lambda: False)
    assert convert.convert(tmp_path / "x.flac")["reason"] == "falta ffmpeg"


def test_convertir_en_lote_y_simular(configured_library, synthetic_ok):
    lib, _ = configured_library
    wav = make_audio(lib / "Artistas" / "Barak" / "Barak - Pista.wav", ["-codec:a", "pcm_s16le"])
    make_audio(lib / "Secuencias" / "Barak - Click.wav", ["-codec:a", "pcm_s16le"])
    assert convert.candidates() == [wav], "Secuencias/ nunca se convierte"
    sim = convert.convert_batch(dry_run=True)
    assert sim["converted"] == 1 and sim["detail"][0]["dry_run"] and os.path.exists(wav)
    seen = []
    r = convert.convert_batch(progress=lambda i, n: seen.append((i, n)))
    assert r["converted"] == 1 and r["failures"] == 0 and seen == [(1, 1)]
    assert not os.path.exists(wav) and os.path.exists(wav[:-4] + ".mp3")


def test_una_caratula_grande_se_encoge(tmp_path, synthetic_ok):
    big = _image(tmp_path / "grande.png", "1600x1200")
    assert convert.image_size(big) == (1600, 1200)
    data, mime = convert.shrink_image(big, max_side=500)
    assert mime == "image/jpeg" and data[:3] == b"\xff\xd8\xff"
    small = _image(tmp_path / "pequena.jpg", "300x300")
    raw = small.read_bytes()
    assert convert.shrink_image(small) == (raw, "image/jpeg"), "si ya vale, tal cual"
    assert convert.shrink_bytes(big.read_bytes(), "image/png", max_side=200)[1] == "image/jpeg"
    assert convert.shrink_image(tmp_path / "no-esta.jpg") is None


# ------------------------------------------------------------------- letras


def test_letra_de_lrclib_con_y_sin_tiempos(monkeypatch):
    calls = []

    def fake(url, headers=None, binary=False):
        calls.append(url)
        if "/api/get" in url:
            return {"plainLyrics": "", "syncedLyrics": "[00:01.00]Mi gozo\n[00:02.00]en ti"}
        raise AssertionError("no hace falta buscar")

    monkeypatch.setattr(enrich, "_get", fake)
    r = enrich.lyrics_lrclib("Barak", "Mi Gozo", "Gozo", 180)
    assert r == {
        "lyrics": "Mi gozo\nen ti",
        "synced": "[00:01.00]Mi gozo\n[00:02.00]en ti",
        "source": "lrclib",
    }
    assert "duration=180" in calls[0] and "album_name=Gozo" in calls[0]


def test_letra_por_busqueda_difusa_y_por_ia(monkeypatch):
    def fake(url, headers=None, binary=False):
        if "/api/get" in url:
            raise OSError("404")
        return [{"plainLyrics": "", "syncedLyrics": ""}, {"plainLyrics": "la segunda"}]

    monkeypatch.setattr(enrich, "_get", fake)
    assert enrich.lyrics_lrclib("A", "B")["lyrics"] == "la segunda"
    monkeypatch.setattr(enrich, "_get", lambda *a, **k: (_ for _ in ()).throw(OSError("sin red")))
    assert enrich.lyrics_lrclib("A", "B") is None
    monkeypatch.setattr(enrich.ai, "available", lambda: True)
    monkeypatch.setattr(enrich.ai, "ask", lambda *a, **k: "verso " * 30)
    assert enrich.lyrics("A", "B")["source"] == "ai"
    monkeypatch.setattr(enrich.ai, "ask", lambda *a, **k: "NO_LA_SE")
    assert enrich.lyrics("A", "B") is None


# ----------------------------------------------------------------- caratulas


class _Resp(io.BytesIO):
    def __init__(self, data: bytes, kind: str):
        super().__init__(data)
        self.headers = {"content-type": kind}

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


def test_solo_se_acepta_una_imagen_de_verdad_y_con_tope(monkeypatch):
    monkeypatch.setattr(
        enrich.urllib.request,
        "urlopen",
        lambda req, timeout=0: _Resp(b"<html>error</html>", "text/html"),
    )
    with pytest.raises(ValueError, match="no es una imagen"):
        enrich._get("https://x", binary=True)
    monkeypatch.setattr(enrich, "MAX_IMAGE_BYTES", 10)
    monkeypatch.setattr(
        enrich.urllib.request,
        "urlopen",
        lambda req, timeout=0: _Resp(b"\xff\xd8\xff" + b"x" * 50, "image/jpeg"),
    )
    with pytest.raises(ValueError, match="demasiado"):
        enrich._get("https://x", binary=True)
    monkeypatch.setattr(
        enrich.urllib.request,
        "urlopen",
        lambda req, timeout=0: _Resp(b'{"a": 1}', "application/json"),
    )
    assert enrich._get("https://x") == {"a": 1}
    assert enrich.image_type(b"RIFF1234WEBPxx") == "image/webp"
    assert enrich.image_type(b"RIFF1234WAVExx") == ""


def test_la_caratula_se_busca_en_itunes_y_luego_en_musicbrainz(monkeypatch, tmp_path, synthetic_ok):
    jpeg = _image(tmp_path / "tapa.jpg", "200x200").read_bytes()
    asked = []

    def fake(url, headers=None, binary=False):
        asked.append(url)
        if "itunes" in url:
            return {
                "results": [
                    {"artworkUrl100": "http://inseguro/100x100bb.jpg"},
                    {"artworkUrl100": "https://a/100x100bb.jpg"},
                ]
            }
        if binary:
            return jpeg
        raise AssertionError(url)

    monkeypatch.setattr(enrich, "_get", fake)
    data, mime = enrich.cover("Barak", "Mi Gozo")
    assert mime == "image/jpeg" and data[:3] == b"\xff\xd8\xff"
    assert any("1000x1000bb" in u for u in asked) and not any("inseguro" in u for u in asked)

    def fake_mb(url, headers=None, binary=False):
        if "itunes" in url:
            return {"results": []}
        if "musicbrainz" in url:
            return {
                "releases": [{"id": "../../malo"}, {"id": "12345678-1234-1234-1234-123456789abc"}]
            }
        assert "coverartarchive.org/release/12345678" in url, "un id raro no se pega en la URL"
        return jpeg

    monkeypatch.setattr(enrich, "_get", fake_mb)
    assert enrich.cover("Barak", "Mi Gozo", "Gozo")


# ------------------------------------------------------- enriquecer entera


def test_enriquecer_guarda_letra_caratula_y_ficha(configured_library, monkeypatch, tmp_path):
    from danplay import library

    lib, _songs = configured_library
    library.add_folder(lib)
    library.scan()
    song = next(s for s in library.search("Mi Gozo") if s["title"] == "Mi Gozo")
    jpeg = _image(tmp_path / "tapa.jpg", "200x200").read_bytes()
    monkeypatch.setattr(
        enrich, "lyrics", lambda *a, **k: {"lyrics": "Mi gozo", "synced": "", "source": "lrclib"}
    )
    monkeypatch.setattr(enrich, "cover", lambda *a, **k: (jpeg, "image/jpeg"))
    monkeypatch.setattr(
        enrich,
        "details",
        lambda c: {
            "album": "desconocido",
            "year": "grabado en 2018",
            "genre": "Adoracion",
            "likely_key": "Bb",
            "confidence": 0.8,
        },
    )
    r = enrich.enrich(song["id"])
    assert (
        r["lyrics"] == "lrclib" and r["cover"].endswith("KB") and r["details"]["likely_key"] == "Bb"
    )
    c = library.by_id(song["id"])
    assert c["lyrics"] == "Mi gozo" and c["cover"] == "embedded"
    assert c["album"] == "Gozo", "«desconocido» no pisa el album de verdad"
    assert c["year"] == "2018" and c["genre"] == "Adoracion"
    assert json.loads(c["chords"])["likely_key"] == "Bb"
    d = tags.read_all(song["path"])
    assert d["lyrics"] == "Mi gozo" and d["cover"] and d["genre"] == "Adoracion"
    assert enrich.enrich(999999) == {"error": "no existe esa cancion"}


def test_transponer_la_ficha():
    d = enrich.transpose_details(
        {
            "likely_key": "Bb",
            "progression": "| Bb | Gm |",
            "section_chords": {"coro": "| Eb | F |"},
        },
        "G",
    )
    assert d["progression"] == "| G | Em |" and d["section_chords"]["coro"] == "| C | D |"
    assert d["likely_key"] == "G" and d["capo"]
    assert enrich.transpose_details(None, "G") == {}


# ------------------------------------------------------------------ la web

PAGE = """
<table>
<tr><td><a rel="nofollow" class="result-link" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fes.wikipedia.org%2Fwiki%2FBarak&amp;rut=x">Barak - <b>Wikipedia</b></a></td></tr>
<tr><td class="result-snippet">Barak es una banda de <b>adoracion</b> colombiana.</td></tr>
<tr><td><a class="result-link" href="https://barak.example/">Web oficial</a></td></tr>
</table>
"""


def test_la_busqueda_web_saca_titulo_enlace_y_resumen(monkeypatch):
    monkeypatch.setattr(
        web.urllib.request, "urlopen", lambda req, timeout=0: _Resp(PAGE.encode(), "text/html")
    )
    r = web.search("barak banda", limit=5)
    assert r[0] == {
        "title": "Barak - Wikipedia",
        "url": "https://es.wikipedia.org/wiki/Barak",
        "summary": "Barak es una banda de adoracion colombiana.",
    }
    assert r[1]["url"] == "https://barak.example/" and r[1]["summary"] == ""
    assert web.search("  ") == []


def test_sin_red_la_web_no_devuelve_nada():
    assert web.search("barak") == [], "la red esta cortada en las pruebas: lista vacia, sin error"


# ---------------------------------------------------------------- la huella


def test_la_huella_dice_lo_que_le_falta(monkeypatch):
    monkeypatch.setattr(fingerprint, "available", lambda: False)
    assert "fpcalc" in fingerprint.unavailable_reason()
    assert fingerprint.identify("/x.mp3") is None
    assert fingerprint.AVAILABLE is False
    monkeypatch.setattr(fingerprint, "available", lambda: True)
    monkeypatch.setattr(config, "ACOUSTID_API_KEY", "")
    assert "ACOUSTID_API_KEY" in fingerprint.unavailable_reason()


def test_la_huella_elige_la_mejor_coincidencia(monkeypatch):
    import types

    monkeypatch.setattr(fingerprint, "available", lambda: True)
    monkeypatch.setattr(config, "ACOUSTID_API_KEY", "clave")
    monkeypatch.setattr(fingerprint.convert, "tool", lambda name: None)
    fake = types.SimpleNamespace(
        match=lambda key, path, meta=None: iter(
            [
                (0.9, "r1", "Mi Gozo", "Barak"),
                (0.95, "r2", None, "Nadie"),
                (0.97, "r3", "Mi Gozo (Live)", "Barak"),
                (0.5, "r4", "Otra", "Otro"),
            ]
        )
    )
    monkeypatch.setitem(__import__("sys").modules, "acoustid", fake)
    r = fingerprint.identify("/x.mp3")
    assert r == {
        "artist": "Barak",
        "title": "Mi Gozo (Live)",
        "album": "",
        "year": "",
        "score": 0.97,
        "source": "fingerprint",
    }

    def boom(*a, **k):
        raise RuntimeError("servicio caido")
        yield

    fake.match = boom
    assert fingerprint.identify("/x.mp3") is None, "un fallo del servicio no tumba nada"
