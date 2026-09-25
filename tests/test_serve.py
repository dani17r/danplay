"""Arrancar el nucleo: que se apague con la app (DANPLAY_PARENT_PID) y que
arranque de verdad por el socket Unix contestando /api/status."""

import os
import pathlib
import subprocess
import sys
import time

import pytest

from danplay import api


def test_sin_variable_se_mira_el_padre_como_siempre(monkeypatch):
    monkeypatch.delenv("DANPLAY_PARENT_PID", raising=False)
    check = api._parent_check()
    if os.name == "nt":
        assert check is None
    else:
        assert check is not None and check() is True


def test_con_la_variable_se_mira_ese_proceso(monkeypatch):
    monkeypatch.setenv("DANPLAY_PARENT_PID", str(os.getpid()))
    assert api._parent_check()() is True
    # un proceso que ya termino
    child = subprocess.Popen([sys.executable, "-c", "pass"])
    child.wait()
    monkeypatch.setenv("DANPLAY_PARENT_PID", str(child.pid))
    assert api._parent_check()() is False


@pytest.mark.parametrize("raw", ["", "abc", "0", "-5"])
def test_un_pid_raro_no_vigila_nada(monkeypatch, raw):
    monkeypatch.setenv("DANPLAY_PARENT_PID", raw)
    if raw == "":
        return  # sin variable: lo de siempre
    assert api._parent_check() is None


@pytest.mark.skipif(os.name == "nt", reason="en Windows se prueba otro camino")
def test_en_posix_se_pregunta_con_la_senal_cero(monkeypatch):
    """os.kill(pid, 0) solo pregunta en POSIX (en Windows MATARIA el proceso:
    alli se usa OpenProcess/GetExitCodeProcess)."""
    asked = []
    monkeypatch.setattr(os, "kill", lambda pid, sig: asked.append((pid, sig)))
    monkeypatch.setenv("DANPLAY_PARENT_PID", "4242")
    assert api._parent_check()() is True
    assert asked == [(4242, 0)]


def test_el_nucleo_arranca_por_el_socket_y_contesta(tmp_path):
    """`danplay serve --uds` con HOME/XDG aislados arranca y responde."""
    if not hasattr(__import__("socket"), "AF_UNIX") or os.name == "nt":
        pytest.skip("sockets Unix")
    import http.client
    import socket

    sock_path = tmp_path / "danplay.sock"
    env = {
        **os.environ,
        "DANPLAY_PARENT_PID": str(os.getpid()),
        "DANPLAY_LIBRARY": str(tmp_path / "Musica"),
    }
    root = pathlib.Path(__file__).resolve().parent.parent
    proc = subprocess.Popen(
        [sys.executable, "-m", "danplay.cli", "serve", "--uds", str(sock_path)],
        cwd=root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    try:
        deadline = time.time() + 60
        while not sock_path.exists() and time.time() < deadline and proc.poll() is None:
            time.sleep(0.1)
        assert sock_path.exists(), proc.stdout.read().decode() if proc.poll() is not None else ""
        assert oct(sock_path.stat().st_mode & 0o777) == oct(0o600)

        class _Conn(http.client.HTTPConnection):
            def connect(self):
                self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self.sock.connect(str(sock_path))

        for _ in range(100):
            try:
                c = _Conn("localhost", timeout=10)
                c.request("GET", "/api/status")
                r = c.getresponse()
                break
            except OSError:
                time.sleep(0.1)
        assert r.status == 200
        assert b'"revision"' in r.read()
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            proc.kill()
    assert not sock_path.exists(), "el socket se borra al salir"


def test_al_arrancar_y_al_apagar(monkeypatch):
    """Lo que antes hacia `serve` a mano va en `lifespan`: vale igual para el
    socket, el puerto y quien monte la app, y al apagar se para todo."""
    from fastapi.testclient import TestClient

    from danplay import model_catalog, watcher, youtube

    calls = []
    monkeypatch.setattr(watcher, "prepare", lambda: calls.append("prepare"))
    monkeypatch.setattr(watcher, "start", lambda: calls.append("start"))
    monkeypatch.setattr(watcher, "stop", lambda: calls.append("stop"))
    monkeypatch.setattr(model_catalog, "refresh_in_background", lambda **k: calls.append("catalog"))
    monkeypatch.setattr(youtube, "cancel", lambda: calls.append("cancel"))
    with TestClient(api.app) as c:
        assert calls == ["prepare", "start", "catalog"]
        assert c.get("/api/status").status_code == 200
    assert calls[-2:] == ["cancel", "stop"]


def test_al_servir_se_escribe_un_registro_que_rota(tmp_path, monkeypatch):
    import logging

    from danplay import config, logs

    monkeypatch.setattr(config, "DATA_DIR", tmp_path)
    monkeypatch.setattr(logs, "_file_handler", None)
    path = logs.setup(to_file=True)
    try:
        assert path == tmp_path / "logs" / "danplay.log"
        logging.getLogger("danplay.prueba").warning("algo que contar")
        for h in logging.getLogger().handlers:
            h.flush()
        text = path.read_text(encoding="utf-8")
        assert "WARNING danplay.prueba: algo que contar" in text
        assert text[:4].isdigit(), "con la hora delante"
        assert logs.setup(to_file=True) == path, "una sola vez"
    finally:
        logging.getLogger().removeHandler(logs._file_handler)
        logs._file_handler.close()
