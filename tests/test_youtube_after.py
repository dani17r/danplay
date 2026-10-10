"""Descargas que rematan una lista, y consultas que no se cuelgan.

- `run_many(on_done=...)`: lo que hay que hacer AL TERMINAR corre dentro del
  turno, antes de soltarlo; `STATE["after"]` esta siempre antes de que
  `active` pase a False; el token de `claim()` (`STATE["job"]`) hace que una
  descarga vieja no pise a la nueva.
- `requested` en todo resultado, y `song_id` en lo que «ya tenias».
- `info(timeout=...)`: consulta ligera, con el motivo real si falla; nunca
  consulta un host que no sea de YouTube y `list=` no convierte un video en
  una lista.
"""

import math
import os
import threading
import time
import types
from pathlib import Path
from typing import Any, ClassVar, cast

import pytest
from conftest import make_mp3

from danplay import config, library, playlists, youtube


@pytest.fixture(autouse=True)
def idle():
    """Cada prueba empieza y acaba con el turno libre y sin restos de otra."""

    def reset():
        youtube.STATE.update(active=False, phase="", after=None, error="", results=[])
        youtube._CANCELAR["requested"] = False

    reset()
    yield
    reset()


@pytest.fixture
def downloads(monkeypatch):
    """`youtube.download` de mentira. `plan[consulta]` es la lista de resultados
    (o una funcion que los da, o una excepcion que lanzar); sin plan, un
    resultado bueno. `seen` apunta lo que se pidio, en orden."""
    seen = []
    plan = {}

    def fake(query, **kwargs):
        seen.append(query)
        step = plan.get(query)
        if isinstance(step, BaseException):
            raise step
        if callable(step):
            return step(query)
        return step if step is not None else [{"ok": True, "title": query}]

    monkeypatch.setattr(youtube, "download", fake)
    return types.SimpleNamespace(seen=seen, plan=plan)


@pytest.fixture
def releases(monkeypatch):
    """Apunta lo que habia en STATE justo cuando se suelta el turno."""
    at_release = []
    real = youtube.release

    def spy(job=None):
        at_release.append({"active": youtube.STATE["active"], "after": youtube.STATE["after"]})
        real(job)

    monkeypatch.setattr(youtube, "release", spy)
    return at_release


# ------------------------------------------------------- on_done y el turno


def test_on_done_corre_antes_de_soltar_el_turno(downloads):
    seen = {}

    def on_done(done):
        seen["active"] = youtube.STATE["active"]
        seen["phase"] = youtube.STATE["phase"]
        seen["busy"] = youtube.claim()  # el turno sigue cogido: nadie se cuela
        seen["after"] = youtube.STATE["after"]
        seen["requested"] = [r["requested"] for r in done]
        return {"state": "done", "added": 2}

    rs = youtube.run_many(["uno", "dos"], source="assistant", on_done=on_done)
    assert seen == {
        "active": True,
        "phase": "listing",
        "busy": 0,
        "after": None,
        "requested": ["uno", "dos"],
    }
    assert [r["ok"] for r in rs] == [True, True]
    st = youtube.STATE
    assert st["active"] is False and st["phase"] == "done"
    assert st["after"] == {"state": "done", "added": 2, "job": st["job"]}
    assert st["results"] == rs


def test_after_esta_publicado_antes_de_soltar_el_turno(downloads, releases):
    youtube.run_many(["a"], on_done=lambda done: {"state": "done", "n": len(done)})
    job = youtube.STATE["job"]
    assert releases == [{"active": True, "after": {"state": "done", "n": 1, "job": job}}]


def test_sin_on_done_after_es_none_y_no_se_hereda_el_de_antes(downloads, releases):
    youtube.run_many(["a"], on_done=lambda done: {"state": "done"})
    assert youtube.STATE["after"]["state"] == "done"
    releases.clear()
    youtube.run_many(["b"])
    assert youtube.STATE["after"] is None
    assert releases == [{"active": True, "after": None}], "tambien None, y antes de soltar"


def test_on_done_que_devuelve_none_deja_after_en_none(downloads, releases):
    youtube.run_many(["a"], on_done=lambda done: None)
    assert youtube.STATE["after"] is None and not youtube.STATE["active"]
    assert releases == [{"active": True, "after": None}]


def test_si_on_done_lanza_after_es_un_error_corto_y_la_descarga_sigue_hecha(downloads, releases):
    def on_done(done):
        raise RuntimeError("no se pudo armar la lista\n" + "x" * 500)

    rs = youtube.run_many(["a", "b"], on_done=on_done)
    assert [r["ok"] for r in rs] == [True, True], "lo bajado no se pierde"
    after = youtube.STATE["after"]
    assert after["state"] == "error" and after["job"] == youtube.STATE["job"]
    assert after["error"].startswith("no se pudo armar la lista xxx")
    assert "\n" not in after["error"] and len(after["error"]) <= 200
    assert not youtube.STATE["active"] and youtube.STATE["phase"] == "done"
    assert releases[-1]["after"]["state"] == "error", "ya publicado al soltar el turno"
    assert youtube.STATE["error"] == "", "el error del remate no es un error de la descarga"


def test_una_excepcion_sin_texto_dice_al_menos_su_nombre(downloads):
    def on_done(done):
        raise KeyError

    youtube.run_many(["a"], on_done=on_done)
    assert youtube.STATE["after"]["error"] == "KeyError"


@pytest.mark.parametrize("junk", [[1, 2], "listo", 7, True])
def test_on_done_que_devuelve_algo_que_no_es_un_dict_es_un_error(downloads, junk):
    youtube.run_many(["a"], on_done=lambda done: junk)
    after = youtube.STATE["after"]
    assert after["state"] == "error" and after["error"] and after["job"] == youtube.STATE["job"]


def test_after_es_una_copia_de_datos_sueltos(downloads):
    """Se publica tal cual en /api/youtube: tiene que ser JSON, y quien lo hizo
    no puede cambiarlo despues."""
    kept = {}
    file = Path("/no/existe/x.mp3")

    def on_done(done):
        kept.update({"state": "done", "order": (3, 1), "file": file, 7: "a"})
        return kept

    youtube.run_many(["a"], on_done=on_done)
    job = youtube.STATE["job"]
    assert youtube.STATE["after"] == {
        "state": "done",
        "order": [3, 1],
        "file": str(file),
        "7": "a",
        "job": job,
    }
    kept["state"] = "otra cosa"
    kept["extra"] = 1
    assert youtube.STATE["after"]["state"] == "done" and "extra" not in youtube.STATE["after"]


def _circular():
    loop: dict = {"state": "done"}
    loop["yo"] = loop
    return loop


@pytest.mark.parametrize(
    "junk",
    [
        {"state": "done", (1, 2): "clave que no es texto"},
        _circular(),
        {"state": "done", "n": math.nan},
    ],
)
def test_after_que_no_se_puede_publicar_es_un_error(downloads, junk):
    youtube.run_many(["a"], on_done=lambda done: junk)
    after = youtube.STATE["after"]
    assert after["state"] == "error" and after["job"] == youtube.STATE["job"]


def test_on_done_recibe_una_copia_de_los_resultados(downloads):
    def on_done(done):
        done.clear()
        return {"state": "done"}

    rs = youtube.run_many(["a", "b"], on_done=on_done)
    assert len(rs) == 2 and len(youtube.STATE["results"]) == 2


def test_cancelar_no_se_salta_el_remate(downloads, releases):
    def cancelling(query):
        youtube.cancel()
        return [{"ok": True, "title": query}]

    downloads.plan["uno"] = cancelling
    got = {}

    def on_done(done):
        got["requested"] = [r["requested"] for r in done]
        got["canceled"] = youtube.canceled()
        return {"state": "done"}

    youtube.run_many(["uno", "dos", "tres"], on_done=on_done)
    assert downloads.seen == ["uno"], "las que quedaban ya no se piden"
    assert got == {"requested": ["uno"], "canceled": True}
    assert youtube.STATE["phase"] == "canceled" and youtube.STATE["after"]["state"] == "done"
    assert releases[-1]["after"] is not None


def test_si_una_descarga_revienta_el_remate_corre_con_lo_que_hay(downloads):
    downloads.plan["mala"] = RuntimeError("se cayo")
    got = []

    def on_done(done):
        got.append(done)
        return {"state": "done"}

    rs = youtube.run_many(["buena", "mala", "otra"], on_done=on_done)
    assert downloads.seen == ["buena", "mala"], "la excepcion corta el bucle, como siempre"
    assert rs[1] == {"ok": False, "reason": "se cayo", "requested": "mala"}
    assert got == [rs] and youtube.STATE["after"]["state"] == "done"
    assert youtube.STATE["error"] == "se cayo"


def test_sin_consultas_el_remate_corre_igual_con_nada(downloads):
    got = []
    youtube.run_many([], on_done=lambda done: got.append(done) or {"state": "done"})
    assert got == [[]] and youtube.STATE["after"]["state"] == "done"
    assert youtube.run_many([]) == [] and youtube.STATE["after"] is None


def test_si_hay_otra_en_marcha_no_se_toca_su_estado(downloads):
    token = youtube.claim()
    youtube.STATE["after"] = {"state": "pending"}
    called = []
    rs = youtube.run_many(["x"], on_done=lambda done: called.append(1) or {"state": "done"})
    assert rs == [{"ok": False, "reason": "ya hay una descarga en marcha"}]
    assert called == [] and downloads.seen == []
    assert youtube.STATE["after"] == {"state": "pending"}
    assert youtube.STATE["active"] and youtube.STATE["job"] == token
    youtube.release()


def test_cuando_active_pasa_a_false_la_lista_ya_esta_completa(downloads, tmp_path, monkeypatch):
    """Lo que ve la interfaz: sondea el estado sin parar y, en cuanto `active`
    es False, mira la lista. Con el remate dentro del turno, ya esta entera y
    `after` esta; con el remate DESPUES de soltarlo (como se iba a hacer) la
    primera foto con `active` en False no tendria ni lo uno ni lo otro."""
    monkeypatch.setattr(config, "DATABASE", tmp_path / "listas.db")
    a, b, new = _songs(3)
    lid = playlists.create("domingo")["id"]
    playlists.add(lid, [a, b])
    downloads.plan["nueva"] = [{"ok": True, "title": "Nueva", "id": new}]
    mine = youtube.STATE["job"] + 1  # el turno que va a coger run_many
    seen_listing, stop = threading.Event(), threading.Event()
    photos = []

    def poll():
        while not stop.is_set():
            st = dict(youtube.STATE)
            if st["job"] == mine:
                done = not st["active"]
                photos.append(
                    {
                        "active": st["active"],
                        "phase": st["phase"],
                        "after": st["after"],
                        "list": [s["id"] for s in playlists.songs(lid)] if done else None,
                    }
                )
                if st["phase"] == "listing":
                    seen_listing.set()
            time.sleep(0.001)

    def on_done(done):
        # no acaba hasta que la interfaz lo haya visto trabajando en la lista
        assert seen_listing.wait(10)
        wanted = [a, *(r["id"] for r in done if r.get("ok")), b]
        return {"state": "done", **playlists.place(lid, wanted)}

    watcher = threading.Thread(target=poll, daemon=True)
    watcher.start()
    try:
        youtube.run_many(["nueva"], on_done=on_done)
        time.sleep(0.05)  # unas fotos mas, ya terminada
    finally:
        stop.set()
        watcher.join(10)
    assert any(p["phase"] == "listing" and p["active"] and p["after"] is None for p in photos)
    finished = [p for p in photos if not p["active"]]
    assert finished, "la interfaz no llego a ver el final"
    for p in finished:
        assert p["after"] and p["after"]["state"] == "done", p
        assert p["list"] == [a, new, b], p
    assert finished[0]["after"]["order"] == [a, new, b]


# ----------------------------------------------------------- claim() y el token


def test_claim_da_un_token_que_sube_y_es_state_job():
    first = youtube.claim()
    assert first >= 1 and youtube.STATE["job"] == first and youtube.STATE["active"]
    assert youtube.claim() == 0, "ocupado: no hay token"
    assert youtube.STATE["job"] == first, "y no cambia nada"
    youtube.release()
    second = youtube.claim()
    assert second == first + 1 and youtube.STATE["job"] == second
    youtube.release()


def test_claim_a_la_vez_lo_consigue_uno_solo():
    before = youtube.STATE["job"]
    got = []
    barrier = threading.Barrier(12)

    def go():
        barrier.wait(10)
        got.append(youtube.claim())

    threads = [threading.Thread(target=go, daemon=True) for _ in range(12)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(10)
    assert sorted(got) == [0] * 11 + [before + 1]
    assert youtube.STATE["job"] == before + 1
    youtube.release()


def test_un_turno_nuevo_no_hereda_el_after_del_anterior(downloads):
    youtube.run_many(["a"], on_done=lambda done: {"state": "done", "n": 1})
    assert youtube.STATE["after"]["state"] == "done"
    token = youtube.claim()
    assert youtube.STATE["after"] is None and youtube.STATE["job"] == token
    assert youtube.STATE["results"] == [] and youtube.STATE["phase"] == "starting"
    youtube.release()
    assert youtube.STATE["after"] is None, "ni lo recupera al soltar"


def test_la_descarga_vieja_no_pisa_a_la_nueva(downloads):
    """Se suelta el turno por fuera (un reinicio, una prueba que limpia) cuando
    la vieja aun esta rematando la lista, y empieza otra: la vieja, al acabar,
    ni escribe su `after` en el estado de la nueva ni le suelta el turno."""
    inside, leave = threading.Event(), threading.Event()
    result = []

    def on_done(done):
        inside.set()
        assert leave.wait(10)
        return {"state": "done", "who": "la vieja"}

    old = threading.Thread(
        target=lambda: result.append(youtube.run_many(["a"], on_done=on_done)), daemon=True
    )
    old.start()
    assert inside.wait(10)
    assert youtube.STATE["phase"] == "listing"
    youtube.release()
    new = youtube.claim()
    assert new
    leave.set()
    old.join(10)
    assert not old.is_alive() and result and result[0][0]["ok"]
    st = youtube.STATE
    assert st["after"] is None, "el after de la vieja no es de la nueva"
    assert st["active"] is True and st["job"] == new, "y el turno sigue siendo de la nueva"
    assert st["phase"] == "starting" and st["results"] == []
    youtube.release()


def test_con_claimed_se_toma_el_token_del_turno_y_job_lo_fija(downloads):
    token = youtube.claim()
    youtube.run_many(["a"], claimed=True, on_done=lambda done: {"state": "done"})
    assert youtube.STATE["after"]["job"] == token and not youtube.STATE["active"]

    token = youtube.claim()
    youtube.run_many(["b"], claimed=True, job=token, on_done=lambda done: {"state": "done"})
    assert youtube.STATE["after"]["job"] == token and not youtube.STATE["active"]

    # un token que ya no es el vigente no escribe ni suelta nada de este turno
    token = youtube.claim()
    youtube.run_many(["c"], claimed=True, job=token - 1, on_done=lambda done: {"state": "done"})
    assert youtube.STATE["active"] and youtube.STATE["after"] is None
    assert youtube.STATE["results"] == []
    youtube.release()


def test_release_con_token_solo_suelta_su_turno():
    token = youtube.claim()
    youtube.release(token - 1)
    assert youtube.STATE["active"], "un token viejo no suelta el turno de ahora"
    youtube.release(token)
    assert not youtube.STATE["active"] and youtube.STATE["phase"] == "done"
    youtube.claim()
    youtube.release()  # sin token, suelta el que sea (lo de siempre)
    assert not youtube.STATE["active"]


def test_el_avance_solo_toca_los_campos_del_avance():
    token = youtube.claim()
    youtube._publish(
        {
            "phase": "downloading",
            "percent": 5,
            "after": {"x": 1},
            "job": 99,
            "active": False,
            "results": [1],
        }
    )
    st = youtube.STATE
    assert st["phase"] == "downloading" and st["percent"] == 5
    assert st["after"] is None and st["job"] == token and st["active"] is True
    assert st["results"] == []
    # y con el token de otro turno, nada
    youtube._publish({"phase": "filing"}, token - 1)
    assert st["phase"] == "downloading"
    youtube.release()


def test_la_api_ensena_after_y_job(downloads, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api, ytdlp

    monkeypatch.setattr(ytdlp, "js_runtime", lambda refresh=False: None)
    monkeypatch.setattr(ytdlp, "js_runtime_hint", lambda: "")
    client = TestClient(api.app)
    youtube.run_many(["a"], on_done=lambda done: {"state": "done", "added": 1})
    got = client.get("/api/youtube").json()
    assert got["active"] is False and got["job"] == youtube.STATE["job"]
    assert got["after"] == {"state": "done", "added": 1, "job": got["job"]}
    youtube.run_many(["b"])
    assert client.get("/api/youtube").json()["after"] is None


# ------------------------------------------------------------------ requested


def test_todo_resultado_de_run_many_lleva_lo_que_se_pidio(downloads):
    downloads.plan["x"] = [{"ok": False, "reason": "no"}]
    downloads.plan["y"] = [{"ok": True, "requested": "otra cosa"}]
    downloads.plan["z"] = [{"ok": True}, {"ok": False, "canceled": True}]
    rs = youtube.run_many(["x", " y ", "z"])
    assert [r["requested"] for r in rs] == ["x", "otra cosa", "z", "z"]
    assert youtube.STATE["results"] == rs


# ------------------------------------------------- yt-dlp de mentira, para info


class FakeYDL:
    """Lo justo de `yt_dlp.YoutubeDL`: apunta las opciones y lo que se le pide,
    y contesta lo que diga `hook` o, sin `hook`, con `videos` (y deja un mp3
    si se pide bajarlo)."""

    made: ClassVar[list] = []
    calls: ClassVar[list] = []
    hook: ClassVar[Any] = None
    videos: ClassVar[dict] = {}

    def __init__(self, options):
        self.options = options
        FakeYDL.made.append(options)

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def extract_info(self, url, download=False):
        FakeYDL.calls.append(url)
        if FakeYDL.hook is not None:
            return FakeYDL.hook(self, url)
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
                hook({"status": "finished"})
            make_mp3(os.path.join(folder, f"{data['id']}.mp3"), seconds=1)
        return data


def _found(self, url):
    return {"id": "abc123", "title": "Una Cancion", "webpage_url": url, "duration": 100}


@pytest.fixture
def ydl(monkeypatch):
    FakeYDL.made, FakeYDL.calls, FakeYDL.videos = [], [], {}
    FakeYDL.hook = _found
    monkeypatch.setattr(youtube, "_yt_dlp", lambda: types.SimpleNamespace(YoutubeDL=FakeYDL))
    yield FakeYDL
    FakeYDL.hook = None


def failing(message, level="error"):
    """Un yt-dlp que no consigue nada y dice por que en su salida de errores."""

    def hook(self, url):
        getattr(self.options["logger"], level)(message)

    return hook


def test_info_con_timeout_es_una_consulta_ligera(ydl):
    youtube.info("https://youtu.be/abc123", 1, timeout=4)
    o = ydl.made[-1]
    assert o["socket_timeout"] == 4 and o["retries"] == 0 and o["extractor_retries"] == 0
    assert o["skip_download"] and o["extract_flat"] == "in_playlist" and o["noplaylist"]
    assert o["playlist_items"] == youtube.PLAYLIST_LIMIT
    # sin timeout, como siempre
    youtube.info("https://youtu.be/abc123", 1)
    o = ydl.made[-1]
    assert o["socket_timeout"] == 30 and o["retries"] == 3 and "extractor_retries" not in o
    with pytest.raises(TypeError):
        cast(Any, youtube.info)("https://youtu.be/abc123", 1, 4)  # el timeout, por su nombre


REFUSED = (
    "ERROR: [generic] video: Unable to download webpage: HTTPConnection(host='x', port=9): "
    "Failed to establish a new connection: [Errno 111] Connection refused"
)
NO_DNS = (
    "ERROR: [youtube] abc123: Unable to download webpage: "
    "<urlopen error [Errno -3] Temporary failure in name resolution>"
)


@pytest.mark.parametrize(
    ("message", "kind"),
    [
        (
            "ERROR: [youtube] abc123: Sign in to confirm you’re not a bot. Use --cookies-from-browser",
            "bot",
        ),
        (
            "ERROR: [youtube] abc123: Unable to download API page: HTTP Error 429: Too Many Requests",
            "rate",
        ),
        (NO_DNS, "network"),
        ("ERROR: [youtube] abc123: The read operation timed out", "network"),
        (REFUSED, "network"),
        ("ERROR: [youtube] abc123: Video unavailable", ""),
    ],
)
def test_info_dice_por_que_fallo(ydl, message, kind):
    ydl.hook = failing(message)
    r = youtube.info("https://youtu.be/abc123", 1, timeout=5)
    assert r["ok"] is False and r["items"] == []
    assert r["reason"] == message.removeprefix("ERROR: ")
    assert youtube.failure_kind(r["reason"]) == kind
    # tambien sin timeout
    assert youtube.info("https://youtu.be/abc123", 1)["reason"] == r["reason"]


def test_info_limpia_el_motivo(ydl):
    esc = chr(27)
    ydl.hook = failing(f"{esc}[0;31mERROR:{esc}[0m [youtube] abc123:\n  Video   unavailable\n")
    assert (
        youtube.info("https://youtu.be/abc123")["reason"] == "[youtube] abc123: Video unavailable"
    )
    ydl.hook = failing("ERROR: " + "muy largo " * 80)
    assert len(youtube.info("https://youtu.be/abc123")["reason"]) <= 200


def test_info_cuenta_el_ultimo_error_y_los_avisos_solo_si_son_de_bloqueo(ydl):
    def two_errors(self, url):
        self.options["logger"].error("ERROR: primero")
        self.options["logger"].error("ERROR: [youtube] abc123: el de verdad")

    ydl.hook = two_errors
    assert youtube.info("https://youtu.be/abc123")["reason"] == "[youtube] abc123: el de verdad"
    # sin error pero con un aviso de que YouTube dijo que no
    ydl.hook = failing("WARNING: [youtube] abc123: HTTP Error 429: Too Many Requests", "warning")
    assert "429" in youtube.info("https://youtu.be/abc123")["reason"]
    # un aviso cualquiera no es un motivo
    ydl.hook = failing("WARNING: [youtube] abc123: algunos formatos se saltaron", "warning")
    assert youtube.info("https://youtu.be/abc123")["reason"] == "no se encontro nada"
    ydl.hook = lambda self, url: None
    assert youtube.info("https://youtu.be/abc123")["reason"] == "no se encontro nada"


def test_info_con_una_excepcion_da_su_texto_o_al_menos_su_nombre(ydl):
    def boom(message, logged=""):
        def hook(self, url):
            if logged:
                self.options["logger"].error(logged)
            raise RuntimeError(message)

        return hook

    ydl.hook = boom("Video unavailable")
    assert youtube.info("https://youtu.be/abc123")["reason"] == "Video unavailable"
    ydl.hook = boom("", logged="ERROR: lo que conto yt-dlp")
    assert youtube.info("https://youtu.be/abc123")["reason"] == "lo que conto yt-dlp"
    ydl.hook = boom("")
    assert youtube.info("https://youtu.be/abc123")["reason"] == "RuntimeError"


def test_info_ok_no_cambia(ydl):
    ydl.hook = None
    ydl.videos = {
        "https://youtu.be/uno": {"id": "uno", "title": "Una", "uploader": "Canal", "duration": 90}
    }
    r = youtube.info("https://youtu.be/uno", 1, timeout=3)
    assert r["ok"] and not r["playlist"] and r["items"][0]["title"] == "Una"
    assert r["items"][0]["channel"] == "Canal" and r["items"][0]["duration"] == 90


@pytest.mark.parametrize("reason", ["", None, "Video unavailable", "algo raro", "HTTP Error 404"])
def test_failure_kind_de_lo_que_no_es_bloqueo_es_vacio(reason):
    assert youtube.failure_kind(reason) == ""


# --------------------------------------------------------------- hosts ajenos

# Direcciones con un host que NO es de YouTube (aunque lo lleven dentro)
FOREIGN = [
    "https://youtube.com.evil.example/watch?v=abc123",
    "https://youtube.com@evil.example/watch?v=abc123",
    "https://www.youtube.com.evil.example/playlist?list=PLxyz",
    "https://youtu.be.evil.example/abc123",
    "https://evil.example/?youtube.com",
    "https://evil.example/watch?v=abc123&u=youtube.com",
    "http://127.0.0.1/watch?v=abc123",
    "http://localhost:8080/video",
    "HTTPS://EVIL.EXAMPLE/x",
    "https://[::1]/watch?v=abc123",
    # la misma direccion leida de dos maneras: Python ve `youtube.com` y urllib3
    # (que es lo que usa yt-dlp para conectar) ve el otro sitio
    "https://evil.example\\@youtube.com/watch?v=abc123",
    "https://evil.example\\.youtube.com/watch?v=abc123",
    "https://youtube.com\\@evil.example/watch?v=abc123",
    "https://user:pass@www.youtube.com/watch?v=abc123",
    "https://www.youtube.com:evil@evil.example/watch?v=abc123",
    "https://www.youtube.com /watch?v=abc123",
    "https://www.youtube.com%2eevil.example/watch?v=abc123",
    f"https://www.youtube.com{chr(0x3002)}evil.example/watch?v=abc123",
]
# Cosas que ni son una direccion de http(s): para yt-dlp son texto que buscar
# en YouTube, nunca una direccion que abrir
NOT_ADDRESSES = [
    "javascript:alert(1)",
    "file:///etc/passwd",
    "ftp://youtube.com/watch?v=abc123",
    "//youtube.com/watch?v=abc123",
    "data:text/html,hola",
    "ytsearch50:algo",
    "ytsearch:algo",
]


@pytest.mark.parametrize("url", FOREIGN)
def test_un_host_ajeno_no_llega_a_yt_dlp(ydl, url):
    assert youtube.host_of(url) not in youtube.ALLOWED_HOSTS and not youtube.is_url(url)
    with pytest.raises(youtube.NotYouTube):
        youtube.normalize(url)
    for r in (youtube.info(url, 1), youtube.info(url, 1, timeout=3)):
        assert r["ok"] is False and "no es YouTube" in r["reason"] and r["items"] == []
    got = youtube.download(url)
    assert got[0]["ok"] is False and "no es YouTube" in got[0]["reason"]
    assert got[0]["requested"] == url
    assert ydl.calls == [] and ydl.made == [], "ni se abre yt-dlp"


@pytest.mark.parametrize(
    ("url", "host"),
    [
        ("https://www.youtube.com/watch?v=abc123", "www.youtube.com"),
        ("  HTTPS://WWW.YOUTUBE.COM:443/watch?v=abc123  ", "www.youtube.com"),
        ("http://m.youtube.com/watch?v=abc123", "m.youtube.com"),
        ("https://www.youtube.com:/watch?v=abc123", "www.youtube.com"),
        ("https://youtu.be/abc123?t=5", "youtu.be"),
        ("https://music.youtube.com/watch?v=abc123&list=RDabc123", "music.youtube.com"),
        ("https://www.youtube.com/@un.canal/videos", "www.youtube.com"),
        ("https://evil.example/watch?v=abc123", "evil.example"),
        ("una cancion", ""),
        ("", ""),
    ],
)
def test_host_of_sigue_dando_el_dominio_de_las_direcciones_normales(url, host):
    assert youtube.host_of(url) == host


@pytest.mark.parametrize("text", NOT_ADDRESSES)
def test_lo_que_no_es_una_direccion_solo_se_busca_en_youtube(ydl, text):
    youtube.info(text, 1)
    youtube.info(text, 1, timeout=2)
    assert ydl.calls == [f"ytsearch1:{text}"] * 2, "una sola busqueda de ese texto, nunca N"
    assert youtube.host_of(ydl.calls[0]) == ""


def test_info_nunca_consulta_un_host_que_no_sea_de_youtube(ydl):
    for text in [*FOREIGN, *NOT_ADDRESSES, "https://www.youtube.com/watch?v=abc123", "una cancion"]:
        youtube.info(text, 1)
        youtube.info(text, 1, timeout=2)
    assert ydl.calls
    for call in ydl.calls:
        assert call.startswith("ytsearch1:") or youtube.is_url(call), call


# -------------------------------------------------------- v= y list= de un video

VIDEOS = [
    "https://www.youtube.com/watch?v=abc123",
    "https://www.youtube.com/watch?v=abc123&list=RDabc123&start_radio=1",
    "https://www.youtube.com/watch?list=PLxyz&v=abc123&index=3",
    "https://youtu.be/abc123?list=PLxyz",
    "https://youtu.be/abc123?list=RDabc123&start_radio=1",
    "https://music.youtube.com/watch?v=abc123&list=RDAMVMabc123",
    "https://www.youtube.com/shorts/abc123?list=PLxyz",
    "https://www.youtube.com/live/abc123?list=PLxyz",
    "https://www.youtube.com/embed/abc123?list=PLxyz",
    "mi list=favorita",  # texto: se busca, y una busqueda no es una lista
]
LISTS = [
    "https://www.youtube.com/playlist?list=PLxyz",
    "https://music.youtube.com/playlist?list=OLAKxyz",
    "https://www.youtube.com/watch?list=PLxyz",
    "https://www.youtube.com/embed/videoseries?list=PLxyz",
]


@pytest.mark.parametrize("url", VIDEOS)
def test_un_video_con_list_se_consulta_como_un_video(ydl, url):
    """Compartir un video desde dentro de una lista o de un mix da
    `watch?v=ID&list=RD...`: es UN video. Antes `list=` activaba la lista y se
    bajaban hasta 50."""
    youtube.info(url, 1)
    assert ydl.made[-1]["noplaylist"] is True
    youtube.info(url, 1, timeout=3)
    assert ydl.made[-1]["noplaylist"] is True


@pytest.mark.parametrize("url", LISTS)
def test_una_lista_a_proposito_si_se_consulta_como_lista(ydl, url):
    youtube.info(url, 1)
    o = ydl.made[-1]
    assert o["noplaylist"] is False and o["playlist_items"] == youtube.PLAYLIST_LIMIT
    assert ydl.calls[-1] == url, "el enlace llega tal cual"


# ---------------------------------------------- el historial y la tuberia de verdad


def song_id(path):
    song = library.by_path(path)
    assert song is not None
    return song["id"]


def _songs(n):
    """`n` canciones sinteticas, sin audio, directamente en el indice."""
    ids = []
    with library.connect() as conn:
        for k in range(n):
            cur = conn.execute(
                "INSERT INTO songs (path, root, folder, file, artist, title) VALUES (?,?,?,?,?,?)",
                (f"/no/existe/{k}.mp3", "/no/existe", "/no/existe", f"{k}.mp3", "Grupo", f"T{k}"),
            )
            ids.append(cur.lastrowid)
    return ids


def test_log_apunta_la_cancion_que_ya_tenias(tmp_path, monkeypatch):
    monkeypatch.setattr(config, "DATABASE", tmp_path / "historial.db")
    url = "https://youtu.be/abc123"
    mine = {"id": 7, "artist": "A", "title": "T", "path": "/no/existe/t.mp3", "file": "t.mp3"}
    other = {**mine, "id": 9, "path": "/no/existe/otra.mp3"}
    repeated = {
        "ok": False,
        "already_there": True,
        "title": "T",
        "url": url,
        "requested": url,
        "reason": "ya la tienes en la biblioteca",
        "matches": [mine, other],
    }
    youtube._log(repeated, url, "assistant", "high")
    row = library.download_history()[0]
    assert (row["song_id"], row["already"], row["ok"]) == (7, 1, 0)
    assert row["target"] == "/no/existe/t.mp3", (
        "y donde esta: asi se sabe si el id sigue siendo suyo"
    )
    # una bajada de verdad: su id y su archivo, como siempre
    youtube._log({"ok": True, "id": 11, "target": "/no/existe/nueva.mp3"}, url, "assistant", "high")
    row = library.download_history()[0]
    assert (row["song_id"], row["ok"], row["target"]) == (11, 1, "/no/existe/nueva.mp3")
    # un fallo cualquiera, y un «ya la tienes» sin ninguna coincidencia: sin cancion
    youtube._log({"ok": False, "reason": "no"}, url, "assistant", "high")
    assert library.download_history()[0]["song_id"] is None
    youtube._log({"ok": False, "already_there": True, "matches": []}, url, "assistant", "high")
    row = library.download_history()[0]
    assert row["song_id"] is None and row["already"] == 1


@pytest.fixture
def bajador(configured_library, ydl, monkeypatch, synthetic_ok):
    """La tuberia entera (archivar, indexar, historial) con un yt-dlp de mentira
    que «baja» un mp3 sintetico. Devuelve (carpeta, {nombre: ruta})."""
    lib, songs = configured_library
    library.add_folder(lib)
    library.scan()
    ydl.hook = None
    ydl.videos = {
        "https://youtu.be/uno": {
            "id": "uno",
            "title": "Orquesta Inventada - Tema Uno (Official Video)",
            "uploader": "OrquestaInventadaVEVO",
            "duration": 200,
        },
    }
    monkeypatch.setattr(youtube.convert, "available", lambda: True)
    library.clear_download_history()
    return lib, songs


def test_lo_que_ya_tienes_queda_en_el_historial_con_su_cancion(bajador):
    lib, _songs = bajador
    mine = make_mp3(
        lib / "Artistas" / "Orquesta Inventada" / "Orquesta Inventada - Tema Uno.mp3",
        artist="Orquesta Inventada",
        title="Tema Uno",
    )
    library.scan()
    r = youtube.download("https://youtu.be/uno", source="assistant")[0]
    assert r["already_there"] and not r["ok"] and r["matches"]
    row = library.download_history()[0]
    assert row["already"] == 1 and row["ok"] == 0 and row["url"] == "https://youtu.be/uno"
    assert row["song_id"] == r["matches"][0]["id"] == song_id(mine)
    found = library.by_id(row["song_id"])
    assert found and found["path"] == row["target"]
    assert r["requested"] == "https://youtu.be/uno"


def test_los_resultados_que_fallan_o_se_cancelan_dicen_que_se_pidio(bajador):
    gone = "https://youtu.be/no-existe"
    got = youtube.download(gone)
    assert len(got) == 1 and not got[0]["ok"] and "unavailable" in got[0]["reason"].lower()
    assert got[0]["requested"] == gone
    # cancelada a medias, con una busqueda de varios resultados
    FakeYDL.videos = {
        "https://youtu.be/uno": FakeYDL.videos["https://youtu.be/uno"],
        "https://youtu.be/dos": {"id": "dos", "title": "Otro Tema", "duration": 100},
    }
    calls = []

    def cancel():
        calls.append(1)
        return len(calls) > 1

    got = youtube.download("tema uno", results=2, cancel=cancel)
    assert got[-1]["canceled"] and got[-1]["requested"] == "tema uno"
    assert all(r["requested"] == "tema uno" for r in got)


def test_bajar_y_completar_la_lista_dentro_del_mismo_turno(bajador):
    """De punta a punta: una descarga buena y otra que falla, y el remate mete
    lo bajado en su sitio de «domingo» antes de soltar el turno."""
    _lib, songs = bajador
    gozo, tierra = song_id(songs["gozo"]), song_id(songs["tierra"])
    lid = playlists.create("domingo")["id"]
    playlists.add(lid, [gozo, tierra])
    good, bad = "https://youtu.be/uno", "https://youtu.be/no-existe"

    def finish(done):
        by_request = {r["requested"]: r for r in done}
        new = by_request[good]
        placed = playlists.place(lid, [gozo, new["id"], tierra])
        return {"state": "done", **placed, "failed": [bad] if not by_request[bad]["ok"] else []}

    rs = youtube.run_many([good, bad], source="assistant", on_done=finish)
    assert [r["ok"] for r in rs] == [True, False]
    after = youtube.STATE["after"]
    new_id = rs[0]["id"]
    assert not youtube.STATE["active"]
    assert after == {
        "state": "done",
        "added": 1,
        "total": 3,
        "order": [gozo, new_id, tierra],
        "failed": [bad],
        "job": youtube.STATE["job"],
    }
    assert [s["id"] for s in playlists.songs(lid)] == [gozo, new_id, tierra]
