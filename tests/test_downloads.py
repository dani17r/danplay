"""La tuberia de descargas de principio a fin, con un yt-dlp de mentira que
«baja» un mp3 sintetico: nada sale a YouTube, pero todo lo demas es de
verdad (el nombre que pone YouTube, archivar en Artistas/, el indice, el
historial, lo que ya tienes, cancelar)."""

import os
import types
from typing import ClassVar

import pytest
from conftest import make_mp3

from danplay import library, youtube


class FakeYDL:
    """Lo justo de `yt_dlp.YoutubeDL`: informa de videos y, si se pide,
    deja un mp3 en la carpeta de `outtmpl` avisando por los ganchos."""

    videos: ClassVar[dict] = {}
    seen_options: ClassVar[list] = []

    def __init__(self, options):
        self.options = options
        FakeYDL.seen_options.append(options)

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def extract_info(self, url, download=False):
        if url.startswith("ytsearch"):
            entries = [dict(v, webpage_url=k) for k, v in FakeYDL.videos.items()]
            return {"title": url, "entries": entries}
        video = FakeYDL.videos.get(url)
        if video is None:
            raise RuntimeError("Video unavailable")
        data = dict(video, webpage_url=url)
        if download:
            folder = os.path.dirname(self.options["outtmpl"])
            for hook in self.options["progress_hooks"]:
                hook(
                    {
                        "status": "downloading",
                        "downloaded_bytes": 50,
                        "total_bytes": 100,
                        "filename": os.path.join(folder, "x.webm"),
                    }
                )
                hook({"status": "finished"})
            make_mp3(os.path.join(folder, f"{data['id']}.mp3"), seconds=1)
        return data


@pytest.fixture
def ytdl(configured_library, monkeypatch, synthetic_ok):
    lib, _ = configured_library
    library.add_folder(lib)
    library.scan()
    FakeYDL.videos = {
        "https://youtu.be/uno": {
            "id": "uno",
            "title": "Palisades - Personal (Official Video)",
            "uploader": "PalisadesVEVO",
            "duration": 200,
        },
        "https://youtu.be/dos": {
            "id": "dos",
            "title": "Una Cancion Nueva",
            "uploader": "Ish Melton - Topic",
            "duration": 150,
            "artist": "Ish Melton",
            "track": "Una Cancion Nueva",
        },
    }
    FakeYDL.seen_options = []
    fake = types.SimpleNamespace(YoutubeDL=FakeYDL)
    monkeypatch.setattr(youtube, "_yt_dlp", lambda: fake)
    monkeypatch.setattr(youtube, "available", lambda: True)
    monkeypatch.setattr(youtube.convert, "available", lambda: True)
    youtube.STATE["active"] = False
    library.clear_download_history()
    yield lib
    youtube.STATE["active"] = False


def test_una_descarga_entra_archivada_y_en_el_indice(ytdl):
    steps = []
    r = youtube.download("https://youtu.be/uno", quality="medium", progress=steps.append)
    assert len(r) == 1 and r[0]["ok"], r
    got = r[0]
    assert got["artist"] == "Palisades" and got["song"] == "Personal"
    assert got["target"].endswith(os.path.join("Artistas", "Palisades", "Palisades - Personal.mp3"))
    assert got["id"] and library.by_id(got["id"])["artist"] == "Palisades"
    assert got["kbps"] == "192"
    phases = [s["phase"] for s in steps]
    assert phases[0] == "starting" and "downloading" in phases and "filing" in phases
    assert library.download_history()[0]["song_id"] == got["id"]
    # se le pasa el motor de JavaScript y los topes
    options = FakeYDL.seen_options[-1]
    assert options["max_filesize"] == youtube.MAX_BYTES and options["noplaylist"]


def test_youtube_music_manda_y_escribe_etiquetas(ytdl):
    r = youtube.download("https://youtu.be/dos")
    assert r[0]["artist"] == "Ish Melton" and r[0]["identified_by"] == "youtube-music"
    from danplay import tags

    assert tags.read_all(r[0]["target"])["artist"] == "Ish Melton"


def test_lo_que_ya_tienes_no_se_baja_salvo_que_se_fuerce(ytdl):
    make_mp3(
        ytdl / "Artistas" / "Palisades" / "Palisades - Personal.mp3",
        artist="Palisades",
        title="Personal",
    )
    library.scan()
    r = youtube.download("https://youtu.be/uno")
    assert r[0]["already_there"] and not r[0]["ok"] and r[0]["matches"]
    r = youtube.download("https://youtu.be/uno", force=True)
    assert r[0]["ok"] and r[0]["target"].endswith("Palisades - Personal - r.mp3")


def test_sin_archivar_se_queda_en_entrada(ytdl):
    from danplay import config

    r = youtube.download("https://youtu.be/uno", file_it=False)
    assert r[0]["ok"] and os.path.dirname(r[0]["file"]) == str(config.INBOX)
    assert "target" not in r[0] or not r[0]["target"]


def test_una_busqueda_trae_varios_y_cancelar_corta(ytdl):
    info = youtube.info("palisades", results=2)
    assert info["ok"] and info["playlist"] and len(info["items"]) == 2
    calls = {"n": 0}

    def cancel():
        calls["n"] += 1
        return calls["n"] > 1

    r = youtube.download("palisades", results=2, cancel=cancel)
    assert r[-1].get("canceled")


def test_los_fallos_se_cuentan_y_el_turno_se_suelta(ytdl):
    assert youtube.claim()
    try:
        assert youtube.run_many(["x"]) == [{"ok": False, "reason": "ya hay una descarga en marcha"}]
    finally:
        youtube.release()
    rs = youtube.run_many(["https://youtu.be/no-existe", "https://youtu.be/uno"])
    assert not rs[0]["ok"] and "unavailable" in rs[0]["reason"].lower()
    assert rs[1]["ok"]
    assert not youtube.STATE["active"] and youtube.STATE["phase"] == "done"
    assert len(youtube.STATE["results"]) == 2


def test_un_enlace_que_no_es_de_youtube_no_se_pasa_a_yt_dlp(ytdl):
    r = youtube.info("https://otro.example/video")
    assert not r["ok"] and "no es YouTube" in r["reason"]
    assert youtube.download("https://otro.example/video")[0]["ok"] is False


def test_un_directo_largo_se_rechaza():
    assert youtube.wanted({"duration": 3 * 3600}).startswith("dura 180 minutos")
    assert youtube.wanted({"duration": 200}) is None
