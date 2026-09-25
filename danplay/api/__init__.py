"""API HTTP local sobre el mismo nucleo que usa la CLI.

La app de escritorio (Tauri + Vue) habla con esto por un socket Unix (o por
loopback con un secreto, en Windows). Las rutas viven en `routes/`, un router
por dominio; los cuerpos de las peticiones, en `models.py`. Aqui se monta la
aplicacion: su arranque y su apagado (`lifespan`), la guardia de cada
peticion (token, Host, X-DanPlay, tamaño) y `serve`.
"""

import contextlib
import hmac
import ipaddress
import logging
import os
import re
import sys
import threading
import time
from pathlib import Path

from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import Response

from .. import __version__, logs, model_catalog, watcher, youtube
from .common import AUDIO_TYPES, audio_type
from .routes import ai, assistant, downloads, external, library, playlists, songs, system
from .routes.library import duplicates_report

log = logging.getLogger(__name__)

__all__ = ["AUDIO_TYPES", "all_routes", "app", "audio_type", "duplicates_report", "serve"]


@contextlib.asynccontextmanager
async def lifespan(_app: FastAPI):
    """Lo que se hace al arrancar y al apagar, fuera de `serve`: asi vale
    igual para el socket, para el puerto y para quien monte la app a mano.

    Al arrancar, antes de contestar la primera peticion (el socket ya
    escucha: las peticiones esperan en cola), se apartan las canciones de las
    carpetas que ya no estan —que la interfaz nunca vea canciones de una
    carpeta que se movio— y arranca el vigilante de carpetas. El catalogo de
    modelos se pone al dia en segundo plano, solo si hace horas de la ultima
    vez: asi el apartado de IA abre ya con la lista de hoy.

    Al apagar, se para el vigilante y se corta la descarga que hubiera.
    """
    watcher.prepare()
    watcher.start()
    model_catalog.refresh_in_background()
    try:
        yield
    finally:
        youtube.cancel()
        watcher.stop()


# Sin /docs, /redoc ni /openapi.json: quedaban fuera de la comprobacion del
# token y contaban a cualquiera que pregunte que sabe hacer la API. El
# contrato esta escrito en docs/CONTRATO-INTERNO.md.
app = FastAPI(
    title="DanPlay",
    version=__version__,
    docs_url=None,
    redoc_url=None,
    openapi_url=None,
    lifespan=lifespan,
)
for _router in (
    system.router,
    ai.router,
    library.router,
    songs.router,
    playlists.router,
    assistant.router,
    downloads.router,
    external.router,
):
    app.include_router(_router)


def all_routes():
    """Todas las rutas de la app, dentro de sus routers. FastAPI ya no las
    aplana en `app.routes`: ahi solo aparecen los routers incluidos."""
    for route in app.routes:
        included = getattr(route, "original_router", None)
        yield from (included.routes if included is not None else [route])


# La app de escritorio habla por un socket Unix y no es un navegador: no usa
# CORS para nada. Quien si lo necesitaba era `npm run dev`, y ni eso, porque
# Vite hace de proxy y el navegador lo ve como mismo origen.
#
# Estaba abierto a cualquier origen (`*`), y eso con el servidor TCP levantado
# («danplay serve» sin --uds) significa que CUALQUIER pagina web abierta en el
# navegador podia leer la respuesta: listar la biblioteca, sacar el principio y
# el final de la clave de IA, mandar canciones a la papelera o, encadenando
# /song/{id}/cover, leer archivos sueltos del disco. Ahora solo se responde a
# los origenes de desarrollo.
DEV_ORIGINS = ["http://localhost:5273", "http://127.0.0.1:5273"]


app.add_middleware(
    CORSMiddleware, allow_origins=DEV_ORIGINS, allow_methods=["*"], allow_headers=["*"]
)


# Con el puerto TCP abierto tambien hay que mirar la cabecera Host: una pagina
# puede apuntar su propio dominio a 127.0.0.1 (reenlace de DNS) y entonces el
# navegador considera que es su mismo origen y CORS ya no protege. Solo se
# exige cuando de verdad hay puerto abierto; por el socket Unix no aplica.
_ENFORCE_HOST = False


ALLOWED_HOSTS = {"localhost", "127.0.0.1", "[::1]", "::1"}


# En Windows no hay sockets Unix que uvicorn sepa escuchar, asi que la app
# habla por TCP en loopback. Para que «solo la app» siga siendo cierto, Rust
# genera un secreto al arrancar y lo pasa por el entorno: sin el, 401.
_TOKEN = ""


# Cuerpos de peticion: 1 MB de sobra para todo menos el chat, que lleva el
# historial de la conversacion. Sin tope, una letra de 200 MB acababa dentro
# de un mp3.
MAX_BODY = 1 * 1024 * 1024


MAX_CHAT_BODY = 4 * 1024 * 1024


# Lo que el navegador pide por su cuenta con <audio> y <img>, sin poder
# añadir cabeceras: la unica excepcion a X-DanPlay (ver `_guard`).
_MEDIA = re.compile(r"^/api/song/-?\d+/(?:audio|cover)$")


@app.middleware("http")
async def _guard(request: Request, call_next):
    path = request.url.path
    if _TOKEN and path.startswith("/api/"):
        sent = request.headers.get("authorization") or ""
        expected = f"Bearer {_TOKEN}"
        # Comparacion en tiempo constante: el token no se adivina a base de
        # medir cuanto tarda en decir que no. En bytes: con cadenas, una
        # cabecera con caracteres no ASCII hacia saltar un TypeError (un 500).
        if not hmac.compare_digest(
            sent.encode("utf-8", "surrogateescape"), expected.encode("utf-8")
        ):
            return Response(status_code=401, content=b"hace falta el token de la aplicacion")

    if _ENFORCE_HOST:
        host = (request.headers.get("host") or "").rsplit(":", 1)[0]
        if host not in ALLOWED_HOSTS:
            return Response(status_code=421, content=b"host no permitido")
        # CORS impide LEER la respuesta, pero no evita que la peticion pase:
        # un POST sin cuerpo desde cualquier pagina abierta en el navegador
        # disparaba un escaneo o una importacion. Exigir una cabecera propia
        # obliga al navegador a preguntar antes (preflight), y ahi CORS si
        # corta. Tambien en los GET: hay lecturas con efectos (la ficha de IA
        # gasta tokens). Fuera quedan el preflight (OPTIONS no puede llevar
        # cabeceras propias) y los medios que el navegador pide solo.
        if (
            path.startswith("/api/")
            and request.method != "OPTIONS"
            and not (request.method in ("GET", "HEAD") and _MEDIA.match(path))
            and request.headers.get("x-danplay") != "1"
        ):
            return Response(status_code=403, content=b"falta la cabecera X-DanPlay")

    length = request.headers.get("content-length")
    if length and length.isdigit():
        tope = MAX_CHAT_BODY if path.startswith("/api/chat") else MAX_BODY
        if int(length) > tope:
            return Response(status_code=413, content=b"eso es demasiado grande")
    return await call_next(request)


def _alive_posix(pid: int) -> bool:
    """Si el proceso existe (POSIX): la señal 0 no hace nada, solo pregunta."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True  # existe, pero es de otro usuario
    return True


def _windows_watcher(pid: int):
    """Una funcion que dice si el proceso `pid` sigue vivo, en Windows.

    NUNCA `os.kill(pid, 0)` alli: en Windows no pregunta, MATA el proceso.
    Se abre el proceso una sola vez (queda sujeto a ESE proceso aunque su
    numero se reutilice despues) y se mira su codigo de salida.
    """
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel32.GetExitCodeProcess.argtypes = (wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD))
    process_query_limited_information, still_active = 0x1000, 259
    handle = kernel32.OpenProcess(process_query_limited_information, False, pid)
    if not handle:
        return None  # ya no existe (o no se puede mirar)

    def alive() -> bool:
        code = wintypes.DWORD()
        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)):
            return True  # sin respuesta no se da por muerto
        return code.value == still_active

    return alive


def _parent_check():
    """La funcion que dice si la app sigue viva, o None si no hay a quien mirar.

    La app (Rust) pasa su PID en DANPLAY_PARENT_PID y se mira ese proceso: es
    lo portable. Sin la variable (la linea de ordenes, versiones anteriores de
    la app), lo de siempre: el padre cambia cuando muere (en POSIX lo adopta
    otro). En Windows eso no vale: `getppid()` sigue dando el PID del padre
    muerto.
    """
    raw = os.environ.get("DANPLAY_PARENT_PID", "").strip()
    if raw:
        try:
            pid = int(raw)
        except ValueError:
            log.warning("DANPLAY_PARENT_PID no es un numero: %r", raw)
            return None
        if pid <= 0:
            return None
        if os.name == "nt":
            return _windows_watcher(pid) or (lambda: False)
        return lambda: _alive_posix(pid)
    if os.name == "nt":
        return None
    parent = os.getppid()
    if parent <= 1:
        return None
    return lambda: os.getppid() == parent


def _watch_parent(interval=2.0):
    """Se apaga si la app desaparece: nada de nucleos huerfanos."""
    import signal

    alive = _parent_check()
    if alive is None:
        return

    def loop():
        while alive():
            time.sleep(interval)
        log.info("la aplicacion ya no esta: el nucleo se apaga")
        # SIGTERM le da a uvicorn la ocasion de cerrar bien (el vigilante,
        # la base); si en dos segundos no ha salido, fuera sin mas
        try:
            signal.raise_signal(signal.SIGTERM)
        except (OSError, ValueError):
            pass
        time.sleep(2)
        os._exit(0)

    threading.Thread(target=loop, name="danplay-parent", daemon=True).start()


def _loopback(host: str) -> bool:
    host = (host or "").strip().strip("[]")
    if host.lower() == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def serve(host="127.0.0.1", port=8730, uds=None):
    """Levanta la API.

    Con `uds` escucha en un socket Unix, que es como la usa la app de
    escritorio en Linux y macOS: sin puerto abierto, y con permisos 0600 solo
    tu usuario puede hablar con ella.

    En Windows no hay sockets Unix que uvicorn sepa escuchar, asi que la app
    arranca esto en loopback y le pasa un secreto por `DANPLAY_TOKEN`; sin esa
    cabecera, la API contesta 401 a todo.

    Lo de arrancar y apagar (el vigilante, el catalogo) esta en `lifespan`.
    """
    global _ENFORCE_HOST, _TOKEN
    import uvicorn

    logs.setup(to_file=True)
    token = os.environ.get("DANPLAY_TOKEN", "")
    # Sin socket y sin token, la API solo se protege de los navegadores
    # (Host, CORS, X-DanPlay): escuchando fuera de este equipo, cualquiera de
    # la red podria listar la biblioteca o mandar canciones a la papelera.
    if not uds and not token and not _loopback(host):
        raise SystemExit(
            f"danplay: no se escucha en «{host}» sin DANPLAY_TOKEN; "
            "sin token, solo en este equipo (127.0.0.1, ::1 o localhost)"
        )
    # uvicorn, al acabar de apagarse por un SIGTERM, lo vuelve a lanzar con el
    # manejador que hubiera antes; con el de serie, el proceso moria ahi mismo
    # y el socket se quedaba en el disco. Asi sale por un SystemExit normal y
    # los `finally` de aqui abajo se ejecutan.
    import signal

    with contextlib.suppress(ValueError):  # fuera del hilo principal
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    _watch_parent()
    _TOKEN = token
    # La comprobacion de Host y la cabecera propia son cosa de navegadores:
    # por el socket no hay ninguno, y con token tampoco hacen falta.
    _ENFORCE_HOST = not uds and not _TOKEN
    if uds:
        import socket as _s

        p = Path(uds)
        p.parent.mkdir(parents=True, exist_ok=True)
        if p.exists():
            p.unlink()
        # uvicorn hace chmod 0666 al socket que crea el mismo, asi que lo
        # creamos nosotros. Con la umask puesta ANTES del bind: entre el bind
        # y el chmod habia un instante en el que el socket era de todos.
        previous = os.umask(0o077)
        try:
            sock = _s.socket(_s.AF_UNIX, _s.SOCK_STREAM)
            sock.bind(str(p))
        finally:
            os.umask(previous)
        os.chmod(p, 0o600)
        # Escuchando ya: lo que llegue mientras `lifespan` arranca espera en
        # cola en vez de rebotar.
        sock.listen(128)
        log.info("escuchando en %s", p)
        try:
            uvicorn.run(app, fd=sock.fileno(), log_level="warning")
        finally:
            sock.close()
            if p.exists():
                p.unlink()
        return

    log.info("escuchando en http://%s:%s%s", host, port, " (con token)" if _TOKEN else "")
    uvicorn.run(app, host=host, port=port, log_level="warning")
