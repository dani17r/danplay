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


# una Drum Cam: la toca Ish Melton, no es de Miel San Marcos
CAM = {
    "id": "cam",
    "title": "QUE SE ABRA EL CIELO - ISH MELTON DRUM CAM",
    "uploader": "Ish Melton",
    "duration": 300,
}


def test_una_drum_cam_se_archiva_con_su_artista_y_se_sugiere_pistas(ytdl):
    FakeYDL.videos["https://youtu.be/cam"] = CAM
    r = youtube.download("https://youtu.be/cam")[0]
    assert r["ok"] and r["target"].endswith(
        os.path.join("Artistas", "Ish Melton", "Ish Melton - Que Se Abra El Cielo (Drum Cam).mp3")
    )
    assert r["kind"] == {"category": "track", "what": "una Drum Cam", "folder": "Pistas"}
    # la de siempre no trae sugerencia
    assert "kind" not in youtube.download("https://youtu.be/uno")[0]


def test_moverla_a_pistas_es_la_misma_cancion(ytdl):
    from fastapi.testclient import TestClient

    from danplay import api, playlists

    FakeYDL.videos["https://youtu.be/cam"] = CAM
    r = youtube.download("https://youtu.be/cam")[0]
    cid, old = r["id"], r["target"]
    playlists.rate(cid, 4)
    pl = playlists.create("ensayo")
    playlists.add(pl["id"], [cid])
    client = TestClient(api.app)
    got = client.post(f"/api/song/{cid}/move", json={"category": "track"})
    assert got.status_code == 200, got.text
    moved = got.json()
    assert moved["id"] == cid and moved["stars"] == 4 and moved["folder"] == "Pistas"
    assert moved["path"] == str(ytdl / "Pistas" / os.path.basename(old))
    assert os.path.isfile(moved["path"]) and not os.path.exists(old)
    # la carpeta del artista se quedo vacia y se quita; Artistas/ no
    assert not os.path.exists(os.path.dirname(old)) and (ytdl / "Artistas").is_dir()
    assert [s["id"] for s in playlists.songs(pl["id"])] == [cid]
    # un escaneo despues no la ve como nueva ni como perdida
    library.scan()
    again = library.by_id(cid)
    assert again and again["path"] == moved["path"]
    # lo que no vale
    assert client.post("/api/song/999999/move", json={"category": "track"}).status_code == 404
    bad = client.post(f"/api/song/{cid}/move", json={"category": "Artistas/../../etc"})
    assert bad.status_code == 422
    # fuera de su carpeta de musica, no: ni se mueve ni se toca el indice
    with pytest.raises(ValueError):
        library.move_to(cid, ytdl.parent / "fuera")
    assert os.path.isfile(moved["path"]) and not (ytdl.parent / "fuera").exists()


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


def test_sin_carpeta_elegida_no_se_baja_nada_y_se_propone_una(ytdl, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api, config

    monkeypatch.setattr(config, "LIBRARY_CHOSEN", False)
    ours = library.list_folders()[0]["path"]
    got = youtube.folder()
    assert (got["ready"], got["reason"], got["suggested"]) == (False, "unset", ours)
    client = TestClient(api.app)
    r = client.post("/api/youtube/download", json={"query": "https://youtu.be/uno"})
    assert r.status_code == 409 and "elige antes" in r.json()["detail"]
    # por el asistente tampoco
    r = client.post("/api/chat/confirm", json={"tool": "download_music", "args": {"items": ["x"]}})
    assert r.status_code == 409 and "elige antes" in r.json()["detail"]
    assert not youtube.STATE["active"]


def test_una_carpeta_que_ya_no_esta_o_fuera_de_la_biblioteca_se_vuelve_a_preguntar(
    ytdl, monkeypatch, tmp_path
):
    from danplay import config

    monkeypatch.setattr(config, "LIBRARY", tmp_path / "disco-sin-montar")
    assert youtube.folder()["reason"] == "gone"
    fuera = tmp_path / "fuera"
    fuera.mkdir()
    monkeypatch.setattr(config, "LIBRARY", fuera)
    got = youtube.folder()
    assert got["reason"] == "unmanaged" and got["suggested"] == library.list_folders()[0]["path"]


def test_al_elegirla_se_usan_sus_subcarpetas_y_se_crean_las_que_faltan(ytdl, monkeypatch, tmp_path):
    from dotenv import dotenv_values
    from fastapi.testclient import TestClient

    from danplay import api, config

    monkeypatch.setattr(config, "LIBRARY_CHOSEN", False)
    monkeypatch.setattr(config, "ENV_FILE", tmp_path / "danplay.env")
    (ytdl / "Pistas").mkdir()
    (ytdl / "Pistas" / "ya estaba.mp3").write_bytes(b"x")
    client = TestClient(api.app)
    r = client.put("/api/downloads/folder", json={"path": str(ytdl)})
    assert r.status_code == 200, r.text
    assert r.json()["ready"] and r.json()["path"] == str(ytdl)
    for sub in ("Artistas", "Pistas", "Secuencias", "Tutoriales y Play Along", "Entrada"):
        assert (ytdl / sub).is_dir(), sub
    assert (ytdl / "Pistas" / "ya estaba.mp3").read_bytes() == b"x"
    now = config.ARTISTS_DIR
    assert config.LIBRARY_CHOSEN and now == ytdl / "Artistas"
    # se recuerda: la proxima vez que arranque la app ya no se pregunta
    assert dotenv_values(config.ENV_FILE)["DANPLAY_LIBRARY"] == str(ytdl)
    # y ya se baja ahi
    got = youtube.download("https://youtu.be/uno")[0]
    assert got["ok"] and got["target"].startswith(str(ytdl / "Artistas"))
    # lo que no vale: una que no es de tus carpetas de musica, o que no existe
    fuera = tmp_path / "fuera"
    fuera.mkdir()
    for path in (fuera, ytdl / "no-existe"):
        r = client.put("/api/downloads/folder", json={"path": str(path)})
        assert r.status_code == 409, path
    now = config.LIBRARY
    assert now == ytdl and not (fuera / "Artistas").exists()
