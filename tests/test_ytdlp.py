"""yt-dlp al dia (contrato B): el motor de JavaScript, y la actualizacion en
caliente desde PyPI con un PyPI de mentira en este equipo (sin red): la rueda
se comprueba, se descomprime, se carga con sus submodulos desde la carpeta
descargada, y si no carga se vuelve a la de la app."""

import hashlib
import http.server
import io
import json
import os
import stat
import sys
import threading
import zipfile

import pytest
from conftest import run_job

from danplay import config, youtube, ytdlp

# ------------------------------------------------------------ motor de JS


def _tool(folder, name, output):
    path = folder / name
    path.write_text(f"#!/bin/sh\necho '{output}'\n")
    path.chmod(path.stat().st_mode | stat.S_IEXEC)


@pytest.fixture
def motores(tmp_path, monkeypatch):
    """Una carpeta de herramientas propia: solo lo que cada prueba ponga."""
    if os.name == "nt":
        pytest.skip("los motores de mentira son guiones de shell")
    monkeypatch.setattr(
        config,
        "find_tool",
        lambda name: str(tmp_path / name) if (tmp_path / name).is_file() else None,
    )
    monkeypatch.setattr(ytdlp, "_runtime_cache", {"at": -1e9, "value": None, "found": []})
    return tmp_path


def test_el_motor_preferido_es_el_primero_que_sirve(motores):
    _tool(motores, "node", "v22.3.0")
    _tool(motores, "deno", "deno 2.5.1 (stable, release, x86_64)")
    rt = ytdlp.js_runtime(refresh=True)
    assert rt["name"] == "deno" and rt["version"] == "2.5.1"
    assert ytdlp.options() == {"js_runtimes": {"deno": {"path": str(motores / "deno")}}}
    assert ytdlp.js_runtime_hint() == ""


def test_uno_viejo_no_vale_y_se_dice(motores):
    _tool(motores, "node", "v18.19.0")
    assert ytdlp.js_runtime(refresh=True) is None
    hint = ytdlp.js_runtime_hint()
    assert "node" in hint and "18.19.0" in hint and "22.0.0" in hint
    assert ytdlp.options() == {}


def test_sin_ninguno_se_dice_que_instalar(motores):
    assert ytdlp.js_runtime(refresh=True) is None
    assert "Deno" in ytdlp.js_runtime_hint() and "Node.js 22" in ytdlp.js_runtime_hint()


@pytest.mark.parametrize(
    "output,ok",
    [
        ("QuickJS version 2024-01-13", True),
        ("QuickJS version 2021-03-27", False),
        ("QuickJS-ng version 0.10.1", True),
    ],
)
def test_quickjs_se_llama_qjs_y_se_mira_con_help(motores, output, ok):
    _tool(motores, "qjs", output)
    rt = ytdlp.js_runtime(refresh=True)
    assert (rt is not None and rt["name"] == "quickjs") == ok


def test_las_opciones_llegan_a_yt_dlp(monkeypatch):
    monkeypatch.setattr(
        ytdlp,
        "js_runtime",
        lambda refresh=False: {"name": "node", "path": "/opt/node/bin/node", "version": "22.1.0"},
    )
    opts = youtube._base_options()
    assert opts["js_runtimes"] == {"node": {"path": "/opt/node/bin/node"}}


def test_version_como_numeros():
    assert ytdlp.version_tuple("2026.08.19") == ytdlp.version_tuple("2026.8.19") == (2026, 8, 19)
    assert ytdlp.version_tuple("2026.9.1") > ytdlp.version_tuple("2026.08.19")
    assert ytdlp.version_tuple("") == ()


# ------------------------------------------------------ PyPI de mentira


def _wheel(files: dict) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        for name, text in files.items():
            z.writestr(name, text)
    return buf.getvalue()


def _yt_dlp_wheel(version, broken=False) -> bytes:
    return _wheel(
        {
            "yt_dlp/__init__.py": (
                "raise ImportError('rota a proposito')\n"
                if broken
                else "from . import version\nfrom .extractor import ALGO\n"
                "__version__ = version.__version__\n"
            ),
            "yt_dlp/version.py": f"__version__ = '{version}'\n",
            "yt_dlp/extractor/__init__.py": "ALGO = 'submodulo de la carpeta'\n",
            f"yt_dlp-{version}.dist-info/METADATA": f"Name: yt-dlp\nVersion: {version}\n",
            f"yt_dlp-{version}.data/data/share/man/man1/yt-dlp.1": "no hace falta\n",
        }
    )


class FakePyPI:
    def __init__(self):
        self.files: dict[str, bytes] = {}
        self.json: dict[str, dict] = {}
        self.requests: list[str] = []
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                outer.requests.append(self.path)
                body = outer.files.get(self.path)
                if body is None and self.path in outer.json:
                    body = json.dumps(outer.json[self.path]).encode()
                if body is None:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def publish(self, project, version, wheel, pin="", sha=None, release_path=False):
        filename = f"{project.replace('-', '_')}-{version}-py3-none-any.whl"
        self.files[f"/files/{filename}"] = wheel
        data = {
            "info": {
                "name": project,
                "version": version,
                "requires_dist": [f'yt-dlp-ejs=={pin}; extra == "default"'] if pin else [],
            },
            "urls": [
                {
                    "filename": filename,
                    "packagetype": "bdist_wheel",
                    "url": f"{self.url}/files/{filename}",
                    "size": len(wheel),
                    "digests": {"sha256": sha or hashlib.sha256(wheel).hexdigest()},
                }
            ],
        }
        self.json[
            f"/pypi/{project}/{version}/json" if release_path else f"/pypi/{project}/json"
        ] = data


@pytest.fixture
def pypi(tmp_path, monkeypatch):
    fake = FakePyPI()
    monkeypatch.setattr(ytdlp, "PYPI", fake.url + "/pypi")
    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    (tmp_path / "datos").mkdir()
    yield fake
    fake.server.shutdown()
    # que las demas pruebas vuelvan a ver el yt-dlp de verdad
    ytdlp._install(None)
    ytdlp._purge()
    ytdlp._module = None
    ytdlp._origin = ""
    youtube.recheck()


def _publish_good(pypi, version="2099.1.1"):
    pypi.publish("yt-dlp", version, _yt_dlp_wheel("2099.01.01"), pin="9.9.0")
    pypi.publish(
        "yt-dlp-ejs",
        "9.9.0",
        _wheel(
            {
                "yt_dlp_ejs/__init__.py": "version = '9.9.0'\n",
                "yt_dlp_ejs-9.9.0.dist-info/METADATA": "Name: yt-dlp-ejs\n",
            }
        ),
        release_path=True,
    )


def test_se_actualiza_desde_pypi_y_se_usa_la_nueva(pypi):
    _publish_good(pypi)
    before = ytdlp.version()
    steps = []
    r = ytdlp.update(lambda done, total, message: steps.append(message))
    assert r == {"previous": before, "version": "2099.01.01", "updated": True}
    assert steps[0] == "consultando PyPI" and len(steps) == 5
    folder = config.DATA_DIR / "yt-dlp" / "2099.1.1"
    assert ytdlp.origin() == str(folder)
    m = ytdlp.module()
    assert m.__file__.startswith(str(folder)), "el paquete sale de la carpeta descargada"
    import yt_dlp.extractor

    assert yt_dlp.extractor.ALGO == "submodulo de la carpeta", "y sus submodulos tambien"
    import yt_dlp_ejs

    assert yt_dlp_ejs.version == "9.9.0", "con el yt-dlp-ejs que pide esa version"
    assert (config.DATA_DIR / "yt-dlp" / "current").read_text() == "2099.1.1"
    assert not (folder / "yt_dlp-2099.1.1.data").exists(), "lo que no hace falta no se saca"
    # otra vez: ya esta al dia
    assert ytdlp.update() == {"previous": "2099.01.01", "version": "2099.01.01", "updated": False}


def test_al_arrancar_se_carga_la_descargada(pypi):
    _publish_good(pypi)
    ytdlp.update()
    # otro arranque del nucleo: nada cargado, solo la carpeta y su marca
    ytdlp._install(None)
    ytdlp._purge()
    ytdlp._module = None
    assert ytdlp.downloaded()[0] == "2099.1.1"
    assert ytdlp.version() == "2099.01.01"
    youtube.recheck()
    assert youtube._installed()


def test_una_rueda_que_no_coincide_con_pypi_no_se_usa(pypi):
    _publish_good(pypi)
    pypi.publish("yt-dlp", "2099.1.1", _yt_dlp_wheel("2099.01.01"), pin="9.9.0", sha="0" * 64)
    before = ytdlp.version()
    with pytest.raises(ytdlp.UpdateError, match="sha256"):
        ytdlp.update()
    assert ytdlp.version() == before and ytdlp.origin() == ""
    assert not (config.DATA_DIR / "yt-dlp" / "2099.1.1").exists()


def test_si_la_nueva_no_carga_se_vuelve_a_la_de_la_app(pypi):
    pypi.publish("yt-dlp", "2099.1.1", _yt_dlp_wheel("2099.01.01", broken=True), pin="9.9.0")
    pypi.publish(
        "yt-dlp-ejs",
        "9.9.0",
        _wheel({"yt_dlp_ejs/__init__.py": "version = '9.9.0'\n"}),
        release_path=True,
    )
    before = ytdlp.version()
    with pytest.raises(ytdlp.UpdateError, match="no se pudo cargar"):
        ytdlp.update()
    assert ytdlp.version() == before and ytdlp.origin() == ""
    assert not (config.DATA_DIR / "yt-dlp" / "2099.1.1").exists()
    import yt_dlp

    assert "2099" not in yt_dlp.version.__version__


def test_una_descargada_rota_al_arrancar_se_aparta(pypi):
    folder = config.DATA_DIR / "yt-dlp" / "2099.1.1"
    (folder / "yt_dlp").mkdir(parents=True)
    (folder / "yt_dlp" / "__init__.py").write_text("raise ImportError('rota')\n")
    (config.DATA_DIR / "yt-dlp" / "current").write_text("2099.1.1")
    ytdlp._install(None)
    ytdlp._purge()
    ytdlp._module = None
    assert ytdlp.module() is not None, "se usa la de la app"
    assert ytdlp.origin() == ""
    assert not folder.exists() and not (config.DATA_DIR / "yt-dlp" / "current").exists()


def test_una_descargada_mas_vieja_que_la_de_la_app_no_se_usa(pypi):
    folder = config.DATA_DIR / "yt-dlp" / "2001.1.1"
    (folder / "yt_dlp").mkdir(parents=True)
    (folder / "yt_dlp" / "__init__.py").write_text("")
    (config.DATA_DIR / "yt-dlp" / "current").write_text("2001.1.1")
    if not ytdlp.bundled_version():
        pytest.skip("sin yt-dlp instalado no hay con que comparar")
    assert ytdlp.downloaded() is None


def test_una_rueda_con_rutas_raras_no_se_descomprime(tmp_path):
    bad = _wheel({"yt_dlp/__init__.py": "", "../../fuera.py": "print('hola')"})
    with pytest.raises(ytdlp.UpdateError):
        ytdlp._unpack([bad], tmp_path / "dentro")
    assert not (tmp_path / "fuera.py").exists()


def test_lo_que_pypi_no_tiene_se_dice_en_castellano(pypi):
    pypi.publish("yt-dlp", "2099.1.1", _yt_dlp_wheel("2099.01.01"), pin="9.9.0")
    with pytest.raises(ytdlp.UpdateError, match="error 404"):
        ytdlp.update()  # el yt-dlp-ejs que pide no esta


def test_sin_red_se_dice_en_castellano(pypi, monkeypatch):
    monkeypatch.setattr(ytdlp, "PYPI", "https://pypi.org/pypi")  # la red esta cortada
    with pytest.raises(ytdlp.UpdateError, match="no se pudo consultar PyPI"):
        ytdlp.update()


def test_la_api_cuenta_la_version_y_actualiza_como_trabajo(pypi, monkeypatch):
    from fastapi.testclient import TestClient

    from danplay import api

    monkeypatch.setattr(ytdlp, "js_runtime", lambda refresh=False: None)
    monkeypatch.setattr(ytdlp, "js_runtime_hint", lambda: "instala Deno")
    c = TestClient(api.app)
    d = c.get("/api/youtube").json()
    assert d["version"] and d["bundled_version"] == ytdlp.bundled_version()
    assert d["js_runtime"] is None and d["js_runtime_hint"] == "instala Deno"
    _publish_good(pypi)
    result = run_job(c, "/api/youtube/update", "yt-dlp")
    assert result["updated"] is True and result["version"] == "2099.01.01"
    assert c.get("/api/youtube").json()["version"] == "2099.01.01"
    # un fallo previsto sale tal cual en el trabajo
    monkeypatch.setattr(ytdlp, "PYPI", "https://pypi.org/pypi")
    r = c.post("/api/youtube/update")
    assert r.status_code == 202
    from danplay.api import jobs

    job = jobs.wait("yt-dlp")
    assert job["error"].startswith("no se pudo consultar PyPI")
    assert sys.modules["yt_dlp"].version.__version__ == "2099.01.01", "sigue la buena"


def test_el_buscador_solo_reconoce_su_carpeta(tmp_path):
    finder = ytdlp._Prefer(tmp_path / "2099.1.1")
    assert finder._inside(tmp_path / "2099.1.1" / "yt_dlp")
    assert not finder._inside(tmp_path / "2099.1.10" / "yt_dlp")
    assert finder.find_spec("otro_paquete") is None
