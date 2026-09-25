"""Tareas largas en segundo plano (contrato A): se arrancan, se consultan
por su nombre, uno de cada a la vez, y el resultado o el error (en
castellano) quedan en su instantanea."""

import pathlib
import threading
import time

import pytest
from conftest import run_job

from danplay.api import jobs


@pytest.fixture(autouse=True)
def limpio():
    with jobs._lock:
        jobs._jobs.clear()
    yield
    with jobs._lock:
        jobs._jobs.clear()


def test_un_trabajo_cuenta_su_avance_y_deja_su_resultado():
    gate = threading.Event()

    def work(progress):
        progress(1, 3, "empezando")
        gate.wait(5)
        progress(3)
        return {"hecho": True}

    snap, already = jobs.start("prueba", work)
    assert not already and snap["active"] and snap["name"] == "prueba"
    assert set(snap) == {
        "name",
        "active",
        "done",
        "total",
        "message",
        "result",
        "error",
        "started",
        "ended",
    }
    time.sleep(0.05)
    mid = jobs.snapshot("prueba")
    assert mid["done"] == 1 and mid["total"] == 3 and mid["message"] == "empezando"
    # otro igual mientras corre: el que ya esta, no uno nuevo
    again, already = jobs.start("prueba", lambda p: pytest.fail("no se arranca otro"))
    assert already and again["started"] == snap["started"]
    gate.set()
    done = jobs.wait("prueba")
    assert not done["active"] and done["done"] == 3 and done["result"] == {"hecho": True}
    assert done["error"] == "" and done["ended"] >= done["started"]


def test_la_instantanea_es_una_copia():
    jobs.start("copia", lambda p: {"lista": [1]})
    jobs.wait("copia")
    snap = jobs.snapshot("copia")
    snap["result"]["lista"].append(2)
    snap["active"] = True
    assert jobs.snapshot("copia")["result"] == {"lista": [1]}
    assert not jobs.snapshot_all()["copia"]["active"]


def test_los_errores_salen_en_castellano():
    def previsto(progress):
        raise jobs.JobError("hay una descarga en marcha: prueba luego")

    def inesperado(progress):
        raise ValueError("disco lleno")

    jobs.start("a", previsto, failure="no se pudo")
    jobs.start("b", inesperado, failure="la conversion fallo")
    assert jobs.wait("a")["error"] == "hay una descarga en marcha: prueba luego"
    b = jobs.wait("b")
    assert b["error"] == "la conversion fallo: disco lleno" and b["result"] is None


def test_lo_terminado_se_olvida_al_rato(monkeypatch):
    jobs.start("viejo", lambda p: 1)
    jobs.wait("viejo")
    monkeypatch.setattr(jobs, "TTL", 0.0)
    time.sleep(0.01)
    assert jobs.snapshot("viejo") is None and jobs.snapshot_all() == {}


def test_el_estado_lleva_una_copia_de_los_trabajos(configured_library):
    from fastapi.testclient import TestClient

    from danplay import api, library

    lib, _ = configured_library
    library.add_folder(lib)
    c = TestClient(api.app)
    result = run_job(c, "/api/scan", "escaneo")
    assert result["total"] == 3 and result["stats"]["total"] == 3
    status = c.get("/api/status").json()
    assert status["jobs"]["escaneo"]["result"]["total"] == 3
    assert c.get("/api/jobs/escaneo").json()["message"] == "3 canciones (3 nuevas)"
    assert c.get("/api/jobs/no-existe").status_code == 404


def test_pedir_otro_escaneo_mientras_corre_devuelve_el_mismo(configured_library, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api, library

    gate = threading.Event()
    real = library.scan

    def slow(progress=None):
        gate.wait(5)
        return real(progress)

    monkeypatch.setattr(library, "scan", slow)
    c = TestClient(api.app)
    first = c.post("/api/scan")
    second = c.post("/api/scan")
    assert first.status_code == second.status_code == 202
    assert "already_running" not in first.json()
    assert second.json()["already_running"] is True
    assert second.json()["job"]["started"] == first.json()["job"]["started"]
    gate.set()
    jobs.wait("escaneo")


def test_convertir_de_verdad_es_un_trabajo_y_simular_no(configured_library, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api, convert

    c = TestClient(api.app)
    monkeypatch.setattr(
        convert,
        "convert_batch",
        lambda quality, keep_original, dry_run, progress=None: (
            (progress and progress(1, 1)) or {"converted": 0, "dry_run": dry_run}
        ),
    )
    r = c.post("/api/convert", json={"dry_run": True})
    assert r.status_code == 200 and r.json()["dry_run"] is True
    result = run_job(c, "/api/convert", "conversion", json={"dry_run": False})
    assert result == {"converted": 0, "dry_run": False}
    assert c.get("/api/jobs/conversion").json()["done"] == 1


def test_importar_es_un_trabajo_que_cuenta_cada_archivo(configured_library):
    from conftest import make_mp3
    from fastapi.testclient import TestClient

    from danplay import api, config, library

    lib, _ = configured_library
    library.add_folder(lib)
    make_mp3(config.INBOX / "New Wine - Nueva.mp3", artist="New Wine", title="Nueva")
    c = TestClient(api.app)
    result = run_job(c, "/api/import", "importacion", json={})
    assert [r["action"] for r in result["results"]] == ["moved"]
    assert pathlib.PurePath(result["results"][0]["target"]).parts == (
        "Artistas",
        "New Wine",
        "New Wine - Nueva.mp3",
    )
    job = c.get("/api/jobs/importacion").json()
    assert job["done"] == job["total"] == 1
    assert any(s["title"] == "Nueva" for s in library.search("Nueva")), "entra en el indice"
