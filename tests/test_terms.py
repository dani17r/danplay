"""Los terminos (danplay/terms.py): se guarda que se aceptaron, cuales y
cuando, en los datos de DanPlay. El texto lo tiene la interfaz."""

import pytest
from fastapi.testclient import TestClient


@pytest.fixture
def client(tmp_path, monkeypatch):
    from danplay import api, config

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    return TestClient(api.app)


def test_at_first_nothing_was_accepted(client):
    assert client.get("/api/terms").json() == {"accepted": "", "at": None, "app": ""}


def test_accepting_is_remembered(client, tmp_path):
    from danplay import __version__, terms

    r = client.post("/api/terms/accept", json={"version": "2026-10-16"})
    assert r.status_code == 200
    got = r.json()
    assert got["accepted"] == "2026-10-16" and got["app"] == __version__ and got["at"]
    assert (tmp_path / "datos" / "terms.json").is_file()
    assert terms.status()["accepted"] == "2026-10-16"
    # otra version, la de despues
    client.post("/api/terms/accept", json={"version": "2027-01-01"})
    assert client.get("/api/terms").json()["accepted"] == "2027-01-01"


@pytest.mark.parametrize("bad", ["", "../x", "a b", "x" * 40, "-1"])
def test_a_strange_version_is_refused(client, bad):
    r = client.post("/api/terms/accept", json={"version": bad})
    assert r.status_code == 422
    assert client.get("/api/terms").json()["accepted"] == ""


def test_a_broken_file_counts_as_not_accepted(client, tmp_path):
    folder = tmp_path / "datos"
    folder.mkdir(parents=True)
    (folder / "terms.json").write_text("{roto", encoding="utf-8")
    assert client.get("/api/terms").json()["accepted"] == ""
    (folder / "terms.json").write_text('{"version": "../../x"}', encoding="utf-8")
    assert client.get("/api/terms").json()["accepted"] == ""
