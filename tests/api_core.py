"""El núcleo contra el que hablan las pruebas de la API.

Sin nada, el de Python: el `TestClient` de FastAPI sobre `danplay.api.app`,
en el mismo proceso, como siempre. Con `DANPLAY_API_CORE=rust`, el de Rust:
el binario `danplay-server` (`DANPLAY_SERVER_BIN`, o target/debug) en un
proceso aparte, por un socket Unix, y un `httpx.Client` con la misma cara
que el `TestClient` (`base_url` «http://testserver», sigue las
redirecciones). Es el punto 3 de la fase 4 de docs/MIGRATION.md.

Las pruebas lo piden con el fixture `connect_api` (tests/conftest.py), y
así se sabe cuáles hablan con la API: en modo Rust, las demás ni se
recogen.

**La configuración es la de Python.** Lo que la prueba deja en
`danplay.config` (la biblioteca, la base, la carpeta de datos, los
interruptores), en `providers.PROFILES_FILE` y `ai.VERIFIED_FILE`, en el
token de `danplay.api` y en el entorno se le da al servidor al arrancar:
como variables (`DANPLAY_LIBRARY`, `DANPLAY_TAGS`...) y como sus carpetas de
datos y de ajustes. Si la base o los perfiles no están donde el núcleo de
Rust los buscaría (`DATA_DIR/danplay.db`, `CONFIG_DIR/ai.json`), esas
carpetas son de este servidor y llevan enlaces a lo de Python (SQLite sigue
el enlace: el -wal va junto a la base de verdad). Antes de cada petición se
mira si algo de eso cambió, y si cambió, el servidor se reinicia con lo
nuevo: un `monkeypatch.setattr(config, ...)` a mitad de prueba también le
llega. De la configuración solo se pasan los valores que la prueba cambió:
los demás, el servidor los lee de su danplay.env como haría Python (así lo
que guarda una ruta de ajustes sigue valiendo después de un reinicio).

**Todo en caja de arena.** El servidor hereda el entorno de las pruebas
(conftest.py ya apunta HOME, XDG_* y DANPLAY_* a temporales) más su propio
XDG_RUNTIME_DIR y TMPDIR; antes de lanzarlo se comprueba que cada carpeta
esté dentro de la carpeta temporal del sistema, y si no, no se lanza. Va en
una red propia sin salida (`unshare -rn`, con el uid de siempre dentro):
el corte de la red de conftest.py es de este proceso y al servidor no le
llega. Sin `unshare`, solo lo frena un proxy que no existe.

**Lo que hace falta dentro de esa red** lo pone tests/rust_netns.py: con
`danplay.api._ENFORCE_HOST` (la guardia del navegador, que el núcleo de Rust
solo pone escuchando en un puerto), el servidor escucha en un puerto de su
red y las pruebas le llegan por el socket; y una IA de mentira (`FakeAI`,
el fixture `fake_ai`) contesta donde el servidor espera a su proveedor.

**Lo que el TestClient no hace.** El servidor de Rust arranca como en la
app (el `lifespan` de FastAPI, que el TestClient no corre): aparta las
carpetas que no están, recupera el diario y lo escribe. El diario de la
carpeta de datos de la sesión es de cada servidor: si no, el de una prueba
rellenaría la base nueva de la siguiente.
"""

from __future__ import annotations

import atexit
import http.server
import json
import os
import pathlib
import shutil
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import weakref
from typing import TYPE_CHECKING, Any

import httpx

if TYPE_CHECKING:
    from fastapi.testclient import TestClient

CORE = (os.environ.get("DANPLAY_API_CORE") or "python").strip().lower()
RUST = CORE == "rust"
if CORE not in ("python", "rust"):
    raise SystemExit(f"DANPLAY_API_CORE={CORE!r}: tiene que ser «python» o «rust»")

ROOT = pathlib.Path(__file__).resolve().parent.parent


def _flag(v: Any) -> str:
    return "1" if v else "0"


# Lo de `danplay.config` que el núcleo de Rust lee del entorno: (atributo,
# variable, cómo se escribe). La biblioteca y la Entrada van aparte (ver
# `_library_env`).
_SETTINGS: tuple[tuple[str, str, Any], ...] = (
    ("AI_ENABLED", "DANPLAY_AI", _flag),
    ("WRITE_TAGS", "DANPLAY_TAGS", _flag),
    ("CONVERT_TO_MP3", "DANPLAY_CONVERT", _flag),
    ("KEEP_ORIGINAL", "DANPLAY_KEEP_ORIGINAL", _flag),
    ("MP3_QUALITY", "DANPLAY_QUALITY", str),
    ("NEVER_CONVERT", "DANPLAY_NEVER_CONVERT", lambda v: ",".join(sorted(v))),
    ("STEMS_FORMAT", "DANPLAY_STEMS_FORMAT", str),
    ("DUPLICATE_THRESHOLD", "DANPLAY_DUP_THRESHOLD", repr),
    ("TOOLS_DIR", "DANPLAY_TOOLS_DIR", str),
    ("ACOUSTID_API_KEY", "ACOUSTID_API_KEY", str),
)
_PATHS = ("LIBRARY", "INBOX", "LIBRARY_CHOSEN")
# lo que cambia de una prueba a otra sin que le importe al servidor
_VOLATILE = {"PYTEST_CURRENT_TEST"}

# Lo que va en la carpeta de datos (lo de Rust y lo de Python): en una
# carpeta propia del servidor, todo esto apunta a la de Python.
_DATA_DIRS = (
    "beats",
    "covers",
    "cues",
    "voices",
    "waveforms",
    "separator",
    "logs",
    "tools",
    "yt-dlp",
)
_DATA_FILES = ("terms.json", "journal.json", "reuse.json", "models-catalog.json")

# Las variables que tienen que apuntar dentro de la carpeta temporal...
_SANDBOXED = (
    "HOME",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "TMPDIR",
    "DANPLAY_DATA_DIR",
    "DANPLAY_CONFIG_DIR",
)
# y estas, si dicen algo
_SANDBOXED_IF_SET = ("DANPLAY_LIBRARY", "XDG_MUSIC_DIR")

_baseline: dict[str, Any] = {}
# las IAs de mentira: «host:puerto» donde el servidor espera al proveedor →
# el socket donde contesta (ver `FakeAI`)
_FAKES: dict[str, str] = {}
_live: weakref.WeakSet[RustClient] = weakref.WeakSet()
REQUESTS = 0  # las peticiones que han llegado al servidor de Rust


def remember_baseline() -> None:
    """Lo que vale la configuración de Python antes de las pruebas: de ella,
    al servidor solo se le pasa lo que una prueba cambie."""
    from danplay import config

    if not _baseline:
        _baseline.update({k: getattr(config, k) for k, _, _ in _SETTINGS})
        _baseline.update({k: getattr(config, k) for k in _PATHS})
        _baseline["DATA_DIR"] = config.DATA_DIR


def connect(**kwargs: Any) -> TestClient | RustClient:
    """Un cliente de la API: el `TestClient` de siempre o el de Rust."""
    if not RUST:
        from fastapi.testclient import TestClient

        from danplay import api

        return TestClient(api.app, **kwargs)
    return RustClient(**kwargs)


def wait_job(
    client: httpx.Client, name: str, timeout: float = 60.0, headers: dict[str, str] | None = None
) -> None:
    """Espera a que acabe el trabajo `name`: `jobs.wait` en el núcleo de
    Python; en el de Rust, preguntando por él (con esas cabeceras)."""
    if not RUST:
        from danplay.api import jobs

        jobs.wait(name, timeout)
        return
    limit = time.monotonic() + timeout
    while time.monotonic() < limit:
        r = client.get(f"/api/jobs/{name}", headers=headers)
        if r.status_code == 404 or not r.json().get("active"):
            return
        time.sleep(0.02)
    raise AssertionError(f"el trabajo «{name}» no acaba")


def live() -> set[RustClient]:
    return set(_live)


def close_all(keep: frozenset[RustClient] | set[RustClient] = frozenset()) -> None:
    for c in list(_live):
        if c not in keep:
            c.close()


atexit.register(close_all)


# --------------------------------------------------------------- la red
_NETNS: list[bool] = []

# Dentro de la red nueva (como root de mentira): se enciende el loopback y
# se vuelve al uid de siempre, sin privilegios, para lanzar el servidor.
_INNER = (
    "ip link set lo up || exit 97; uid=$1; gid=$2; shift 2; "
    'exec unshare --user --map-user="$uid" --map-group="$gid" -- "$@"'
)


def _isolated(cmd: list[str]) -> list[str]:
    return ["unshare", "-rn", "sh", "-c", _INNER, "sh", str(os.getuid()), str(os.getgid()), *cmd]


def _netns_works() -> bool:
    if not _NETNS:
        ok = False
        if sys.platform.startswith("linux") and shutil.which("unshare") and shutil.which("ip"):
            try:
                r = subprocess.run(
                    _isolated(["true"]), check=False, capture_output=True, timeout=20
                )
                ok = r.returncode == 0
            except (OSError, subprocess.SubprocessError):
                ok = False
        if not ok:
            print(
                "AVISO: sin `unshare -rn`, al servidor de Rust solo lo frena un proxy que no existe",
                file=sys.stderr,
            )
        _NETNS.append(ok)
    return _NETNS[0]


# ------------------------------------------------------------- el servidor
def server_binary() -> pathlib.Path:
    """`DANPLAY_SERVER_BIN`, o el de `cargo build -p danplay-server`."""
    configured = os.environ.get("DANPLAY_SERVER_BIN", "")
    target = pathlib.Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target")
    path = pathlib.Path(configured) if configured else target / "debug" / "danplay-server"
    if not path.is_file():
        raise RuntimeError(
            f"no está el servidor de Rust en {path}: compílalo "
            "(cargo build -p danplay-server) o pon DANPLAY_SERVER_BIN"
        )
    return path


def _inside_temp(path: str | os.PathLike[str]) -> bool:
    temp = os.path.realpath(tempfile.gettempdir())
    real = os.path.realpath(path)
    return real == temp or real.startswith(temp + os.sep)


def _library_env(config) -> dict[str, str]:
    """La biblioteca y la Entrada, si la prueba las cambió."""
    lib = pathlib.Path(config.LIBRARY)
    if (
        pathlib.Path(config.ARTISTS_DIR) != lib / "Artistas"
        or pathlib.Path(config.REVIEW_DIR) != lib / "Revisar"
    ):
        raise RuntimeError(
            "ARTISTS_DIR o REVIEW_DIR fuera de la biblioteca: el núcleo de Rust "
            "siempre los pone en LIBRARY/Artistas y LIBRARY/Revisar"
        )
    if all(getattr(config, k) == _baseline.get(k) for k in _PATHS):
        return {}
    if config.LIBRARY_CHOSEN:
        out = {"DANPLAY_LIBRARY": str(lib)}
    else:
        # sin elegir: el núcleo de Rust la supone (`default_library`), y la
        # primera que mira y existe es la de música del escritorio
        home = pathlib.Path(os.environ["HOME"])
        dirs = pathlib.Path(os.environ["XDG_CONFIG_HOME"]) / "user-dirs.dirs"
        if not lib.is_dir() or (home / "Musica").is_dir() or dirs.exists():
            raise RuntimeError(f"LIBRARY_CHOSEN=False con {lib}: el núcleo de Rust no la supondría")
        out = {"DANPLAY_LIBRARY": "", "XDG_MUSIC_DIR": str(lib)}
    inbox = pathlib.Path(config.INBOX)
    if inbox != lib / os.environ.get("DANPLAY_INBOX", "Entrada"):
        out["DANPLAY_INBOX"] = inbox.name if inbox.parent == lib else str(inbox)
    return out


def _data_layout(config) -> dict[str, str]:
    """La carpeta de datos, y lo de Python que no está en ella: {nombre:
    dónde está en Python, o "" si es solo del servidor}."""
    data = pathlib.Path(config.DATA_DIR)
    links: dict[str, str] = {}
    if pathlib.Path(config.DATABASE) != data / "danplay.db":
        links["danplay.db"] = str(config.DATABASE)
    for module, attr, name in (
        ("danplay.ai", "VERIFIED_FILE", "models-verified.json"),
        ("danplay.model_catalog", "CACHE", "models-catalog.json"),
    ):
        path = getattr(sys.modules.get(module), attr, None)
        if path is not None and pathlib.Path(path) != data / name:
            links[name] = str(path)
    if data == _baseline.get("DATA_DIR"):
        # El diario de la carpeta de datos de la sesión, suyo (""): el núcleo
        # de Rust lo escribe siempre (el de Python, sin el `lifespan` que no
        # corre el TestClient, nunca), y el servidor de la prueba siguiente,
        # con su base nueva, recuperaría de él lo de otra prueba.
        links["journal.json"] = ""
    return {"dir": str(data), **links}


def _config_layout(config) -> dict[str, str]:
    """La carpeta de ajustes, y lo de Python que no está en ella."""
    folder = pathlib.Path(config.CONFIG_DIR)
    links: dict[str, str] = {}
    if pathlib.Path(config.ENV_FILE) != folder / "danplay.env":
        links["danplay.env"] = str(config.ENV_FILE)
    profiles = getattr(sys.modules.get("danplay.providers"), "PROFILES_FILE", None)
    if profiles is not None and pathlib.Path(profiles) != folder / "ai.json":
        links["ai.json"] = str(profiles)
    return {"dir": str(folder), **links}


def _wanted() -> dict[str, Any]:
    """Lo que el servidor tiene que ver, sacado de Python ahora mismo."""
    from danplay import config

    env = {k: v for k, v in os.environ.items() if k not in _VOLATILE}
    # la música del escritorio de quien ejecuta las pruebas, nunca
    env.pop("XDG_MUSIC_DIR", None)
    for attr, var, show in _SETTINGS:
        value = getattr(config, attr)
        if value != _baseline.get(attr, value):
            env[var] = show(value)
    env.update(_library_env(config))
    token = getattr(sys.modules.get("danplay.api"), "_TOKEN", "")
    if token:
        env["DANPLAY_TOKEN"] = token
    else:
        env.pop("DANPLAY_TOKEN", None)
    env.pop("DANPLAY_PARENT_PID", None)
    env["DANPLAY_WATCH"] = "0"
    env["DANPLAY_PROJECT_ENV"] = "0"
    # la guardia del navegador (`Host`, `X-DanPlay`): la de Rust solo la pone
    # escuchando en un puerto y sin token, como la de Python
    port = bool(getattr(sys.modules.get("danplay.api"), "_ENFORCE_HOST", False))
    if port and token:
        raise RuntimeError("con token, el núcleo de Rust no mira el Host: no se puede decir")
    return {
        "env": env,
        "data": _data_layout(config),
        "config": _config_layout(config),
        "port": port,
        "fakes": dict(_FAKES),
    }


class _Server:
    """Un `danplay-server` con la configuración de Python."""

    def __init__(self) -> None:
        self.root = pathlib.Path(tempfile.mkdtemp(prefix="danplay-rust-"))
        self.log = self.root / "server.log"
        self.proc: subprocess.Popen[bytes] | None = None
        self.wanted: dict[str, Any] | None = None
        self.transport: httpx.HTTPTransport | None = None
        self.generation = 0
        self.lock = threading.RLock()
        # los archivos enlazados de las carpetas propias: (enlace, el de Python)
        self.links: list[tuple[pathlib.Path, pathlib.Path]] = []

    def _folder(self, kind: str, layout: dict[str, str], names: tuple[str, ...], files) -> str:
        """La carpeta que ve el servidor: la de Python, o una suya con enlaces
        a lo de Python (lo que no está en ella, las subcarpetas conocidas y
        lo que ya hubiera dentro)."""
        source = pathlib.Path(layout["dir"])
        links = {k: pathlib.Path(v) for k, v in layout.items() if k != "dir" and v}
        private = {k for k, v in layout.items() if not v}
        source.mkdir(parents=True, exist_ok=True)
        if not links and not private:
            return str(source)
        own = self.root / f"{kind}-{self.generation}"
        own.mkdir(parents=True)
        for name in names:
            # que exista: por un enlace roto, create_dir_all falla
            (source / name).mkdir(exist_ok=True)
        for entry in {*names, *files, *os.listdir(source)}:
            # ni lo enlazado ni lo suyo (el -wal y el -shm de la otra base)
            if not any(entry == k or entry.startswith(f"{k}-") for k in (*links, *private)):
                links[entry] = source / entry
        for name, target in links.items():
            target.parent.mkdir(parents=True, exist_ok=True)
            os.symlink(target, own / name)
            if name not in names:
                self.links.append((own / name, target))
        return str(own)

    def heal(self) -> None:
        """Lo que el servidor guarda de una vez (un temporal que se renombra
        encima) deja un archivo en lugar del enlace: se lleva a su sitio de
        Python y vuelve el enlace."""
        for link, target in self.links:
            if not link.is_symlink() and link.is_file():
                os.replace(link, target)
                os.symlink(target, link)

    def _start(self, wanted: dict[str, Any]) -> None:
        self.generation += 1
        self.links = []
        for target in [*wanted["data"].values(), *wanted["config"].values()]:
            if target and not _inside_temp(target):
                raise RuntimeError(f"el servidor de Rust no se lanza con {target!r}")
        env = dict(wanted["env"])
        env["DANPLAY_DATA_DIR"] = self._folder("data", wanted["data"], _DATA_DIRS, _DATA_FILES)
        env["DANPLAY_CONFIG_DIR"] = self._folder(
            "config", wanted["config"], (), ("danplay.env", "ai.json")
        )
        for sub, var in (("runtime", "XDG_RUNTIME_DIR"), ("tmp", "TMPDIR")):
            (self.root / sub).mkdir(exist_ok=True, mode=0o700)
            env[var] = str(self.root / sub)
        # nada fuera de la caja de arena: ni los datos, ni los ajustes, ni la
        # música de quien ejecuta las pruebas
        for var in (*_SANDBOXED, *_SANDBOXED_IF_SET):
            if var in _SANDBOXED_IF_SET and not env.get(var):
                continue
            if not env.get(var) or not _inside_temp(env[var]):
                raise RuntimeError(f"el servidor de Rust no se lanza con {var}={env.get(var)!r}")
        sock = self.root / f"api-{self.generation}.sock"
        cmd = [str(server_binary())]
        forwards = []
        if wanted["port"]:
            # en su red no hay nadie más: el puerto de siempre
            cmd += ["--host", "127.0.0.1", "--port", "8730"]
            forwards.append(f"unix:{sock}=tcp:127.0.0.1:8730")
        else:
            cmd += ["--uds", str(sock)]
        for at, path in wanted["fakes"].items():
            port = at.rpartition(":")[2]
            forwards += [f"tcp:{host}:{port}=unix:{path}" for host in ("127.0.0.1", "[::1]")]
        if forwards:
            helper = pathlib.Path(__file__).with_name("rust_netns.py")
            forwarding = [f"--forward={f}" for f in forwards]
            cmd = [sys.executable, "-I", str(helper), *forwarding, "--", *cmd]
        if _netns_works():
            cmd = _isolated(cmd)
        elif forwards:
            import pytest

            pytest.skip("el puerto y la IA de mentira del servidor de Rust necesitan unshare -rn")
        else:
            for var in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy"):
                env[var] = "http://127.0.0.1:9"
            env["NO_PROXY"] = env["no_proxy"] = "localhost,127.0.0.1,::1"
        with open(self.log, "ab") as log:
            self.proc = subprocess.Popen(
                cmd, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, cwd=self.root
            )
        limit = time.monotonic() + 60
        while not self._listening(sock):
            if self.proc.poll() is not None:
                raise RuntimeError(f"el servidor de Rust no arrancó:\n{self.tail()}")
            if time.monotonic() > limit:
                self.stop()
                raise RuntimeError(f"el servidor de Rust no escucha:\n{self.tail()}")
            time.sleep(0.01)
        self.transport = httpx.HTTPTransport(uds=str(sock))
        self.wanted = wanted

    @staticmethod
    def _listening(sock: pathlib.Path) -> bool:
        if not sock.exists():
            return False
        probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            probe.connect(str(sock))
            return True
        except OSError:
            return False
        finally:
            probe.close()

    def tail(self, lines: int = 30) -> str:
        try:
            text = self.log.read_text("utf-8", "replace")
        except OSError:
            return ""
        return "\n".join(text.splitlines()[-lines:])

    def ensure(self) -> httpx.HTTPTransport:
        """El transporte de ahora, reiniciando el servidor si hace falta."""
        with self.lock:
            wanted = _wanted()
            if self.proc is not None and self.proc.poll() is not None:
                code, self.proc = self.proc.returncode, None
                raise RuntimeError(f"el servidor de Rust se cayó ({code}):\n{self.tail()}")
            if self.proc is None or self.transport is None or wanted != self.wanted:
                self.stop()
                self._start(wanted)
            assert self.transport is not None
            return self.transport

    def stop(self) -> None:
        if self.transport is not None:
            self.transport.close()
            self.transport = None
        proc, self.proc = self.proc, None
        if proc is not None and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(10)
        self.heal()

    def close(self) -> None:
        self.stop()
        shutil.rmtree(self.root, ignore_errors=True)


class _Transport(httpx.BaseTransport):
    def __init__(self, server: _Server) -> None:
        self.server = server

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        global REQUESTS
        transport = self.server.ensure()
        REQUESTS += 1
        try:
            return transport.handle_request(request)
        finally:
            self.server.heal()


class RustClient(httpx.Client):
    """Lo que las pruebas usan del `TestClient`, contra el servidor de Rust."""

    def __init__(
        self,
        base_url: str = "http://testserver",
        headers: dict[str, str] | None = None,
        cookies: Any = None,
        follow_redirects: bool = True,
        **_asgi: Any,  # raise_server_exceptions, root_path...: cosas del ASGI
    ) -> None:
        self.server = _Server()
        headers = dict(headers or {})
        headers.setdefault("user-agent", "testclient")
        super().__init__(
            base_url=base_url,
            headers=headers,
            cookies=cookies,
            follow_redirects=follow_redirects,
            transport=_Transport(self.server),
            timeout=120.0,
        )
        _live.add(self)
        weakref.finalize(self, self.server.close)

    def restart(self) -> None:
        """Lo apaga: la siguiente petición lo arranca con lo de ahora."""
        self.server.stop()

    def close(self) -> None:
        super().close()
        self.server.close()
        _live.discard(self)


# ------------------------------------------------------ la IA de mentira
class _UnixHTTPServer(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


def _ai_handler(client: Any) -> type[http.server.BaseHTTPRequestHandler]:
    """Lo que contesta la API de OpenAI con lo que diga `client` (un objeto
    con la forma del SDK, como los de mentira de las pruebas)."""

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, format: str, *args: Any) -> None:
            pass

        def address_string(self) -> str:
            return "ia-de-mentira"

        def _json(self, status: int, data: Any) -> None:
            raw = json.dumps(data).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def do_GET(self) -> None:
            if not self.path.rstrip("/").endswith("/models"):
                self._json(404, {"error": {"message": "no existe"}})
                return
            models = client.models.list().data
            listed = [{"id": m.id, "object": "model", "owned_by": "prueba"} for m in models]
            self._json(200, {"object": "list", "data": listed})

        def do_POST(self) -> None:
            body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)))
            if not self.path.rstrip("/").endswith("/chat/completions"):
                self._json(404, {"error": {"message": "no existe"}})
                return
            try:
                msg = client.chat.completions.create(**body).choices[0].message
            except Exception as e:  # noqa: BLE001  (la de mentira puede fallar a propósito)
                self._json(500, {"error": {"message": str(e)}})
                return
            calls = [
                {
                    "id": str(getattr(c, "id", "") or f"call_{i}"),
                    "type": "function",
                    "function": {"name": c.function.name, "arguments": c.function.arguments},
                }
                for i, c in enumerate(getattr(msg, "tool_calls", None) or [])
            ]
            answer: dict[str, Any] = {"role": "assistant", "content": msg.content}
            if calls:
                answer["tool_calls"] = calls
            finish = "tool_calls" if calls else "stop"
            usage = {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            if not body.get("stream"):
                choice = {"index": 0, "message": answer, "finish_reason": finish}
                self._json(
                    200,
                    {
                        "id": "prueba",
                        "object": "chat.completion",
                        "model": body.get("model", ""),
                        "choices": [choice],
                        "usage": usage,
                    },
                )
                return
            # en trozos (SSE): todo en uno, el final y, si lo pide, el uso
            delta = dict(answer)
            if calls:
                delta["tool_calls"] = [{"index": i, **c} for i, c in enumerate(calls)]
            chunks: list[dict[str, Any]] = [
                {"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                {"choices": [{"index": 0, "delta": {}, "finish_reason": finish}]},
            ]
            if (body.get("stream_options") or {}).get("include_usage"):
                chunks.append({"choices": [], "usage": usage})
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            for chunk in chunks:
                event = {"id": "prueba", "object": "chat.completion.chunk", **chunk}
                self.wfile.write(f"data: {json.dumps(event)}\n\n".encode())
            self.wfile.write(b"data: [DONE]\n\n")
            self.close_connection = True

    return Handler


class FakeAI:
    """Un proveedor de IA de mentira para el servidor de Rust: la API de
    OpenAI (`/chat/completions`, entera o en trozos, y `/models`) en un
    socket Unix, contestando con lo que diga `client`, un objeto con la forma
    del SDK (`client.chat.completions.create(**kw)`, `client.models.list()`):
    el mismo de mentira que el núcleo de Python recibe en lugar de su
    cliente. `at` es el «host:puerto» de la URL del proveedor: los servidores
    que arranquen mientras esté abierta lo encuentran ahí, en su red."""

    def __init__(self, client: Any, at: str) -> None:
        self.folder = tempfile.mkdtemp(prefix="danplay-ia-")
        self.at = at
        path = os.path.join(self.folder, "ia.sock")
        self.server = _UnixHTTPServer(path, _ai_handler(client))
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        _FAKES[at] = path

    def close(self) -> None:
        _FAKES.pop(self.at, None)
        self.server.shutdown()
        self.server.server_close()
        shutil.rmtree(self.folder, ignore_errors=True)
