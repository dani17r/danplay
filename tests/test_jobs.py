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
def cleaned():
    with jobs._lock:
        jobs._jobs.clear()
    yield
    with jobs._lock:
        jobs._jobs.clear()


def test_a_job_reports_its_progress_and_leaves_its_result():
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


def test_the_snapshot_is_a_copy():
    jobs.start("copia", lambda p: {"lista": [1]})
    jobs.wait("copia")
    snap = jobs.snapshot("copia")
    snap["result"]["lista"].append(2)
    snap["active"] = True
    assert jobs.snapshot("copia")["result"] == {"lista": [1]}
    assert not jobs.snapshot_all()["copia"]["active"]


def test_errors_come_out_in_spanish():
    def expected_error(progress):
        raise jobs.JobError("hay una descarga en marcha: prueba luego")

    def unexpected_error(progress):
        raise ValueError("disco lleno")

    jobs.start("a", expected_error, failure="no se pudo")
    jobs.start("b", unexpected_error, failure="la conversion fallo")
    assert jobs.wait("a")["error"] == "hay una descarga en marcha: prueba luego"
    b = jobs.wait("b")
    assert b["error"] == "la conversion fallo: disco lleno" and b["result"] is None


def test_finished_jobs_are_forgotten_after_a_while(monkeypatch):
    jobs.start("viejo", lambda p: 1)
    jobs.wait("viejo")
    monkeypatch.setattr(jobs, "TTL", 0.0)
    time.sleep(0.01)
    assert jobs.snapshot("viejo") is None and jobs.snapshot_all() == {}


def test_the_status_carries_a_copy_of_the_jobs(configured_library, connect_api):
    from danplay import library

    lib, _ = configured_library
    library.add_folder(lib)
    c = connect_api()
    result = run_job(c, "/api/scan", "scan")
    assert result["total"] == 3 and result["stats"]["total"] == 3
    status = c.get("/api/status").json()
    assert status["jobs"]["scan"]["result"]["total"] == 3
    assert c.get("/api/jobs/scan").json()["message"] == "3 canciones (3 nuevas)"
    assert c.get("/api/jobs/no-existe").status_code == 404


@pytest.mark.python_core("frena el escaneo sustituyendo library.scan")
def test_asking_for_a_scan_while_one_runs_returns_the_same(
    configured_library, monkeypatch, connect_api
):
    from danplay import library

    gate = threading.Event()
    real = library.scan

    def slow(progress=None):
        gate.wait(5)
        return real(progress)

    monkeypatch.setattr(library, "scan", slow)
    c = connect_api()
    first = c.post("/api/scan")
    second = c.post("/api/scan")
    assert first.status_code == second.status_code == 202
    assert "already_running" not in first.json()
    assert second.json()["already_running"] is True
    assert second.json()["job"]["started"] == first.json()["job"]["started"]
    gate.set()
    jobs.wait("scan")


@pytest.mark.python_core("la conversión de mentira sustituye convert.convert_batch")
def test_a_real_convert_is_a_job_and_a_dry_run_is_not(configured_library, monkeypatch, connect_api):
    from danplay import convert

    c = connect_api()
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


def test_import_is_a_job_that_counts_each_file(configured_library, connect_api):
    from conftest import make_mp3

    from danplay import config, library

    lib, _ = configured_library
    library.add_folder(lib)
    make_mp3(config.INBOX / "New Wine - Nueva.mp3", artist="New Wine", title="Nueva")
    c = connect_api()
    result = run_job(c, "/api/import", "import", json={})
    assert [r["action"] for r in result["results"]] == ["moved"]
    assert pathlib.PurePath(result["results"][0]["target"]).parts == (
        "Artistas",
        "New Wine",
        "New Wine - Nueva.mp3",
    )
    job = c.get("/api/jobs/import").json()
    assert job["done"] == job["total"] == 1
    assert any(s["title"] == "Nueva" for s in library.search("Nueva")), "entra en el indice"
