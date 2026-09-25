"""yt-dlp al dia: que version se usa, con que motor de JavaScript, y como se
pone al dia sin reinstalar la app.

YouTube cambia cada pocas semanas y yt-dlp saca version para seguirle; dentro
del binario de PyInstaller va congelada la que habia al compilar, y a los
pocos meses deja de bajar nada. Por eso:

- **Actualizar en caliente** (`update`, contrato B): se baja de PyPI la ultima
  rueda universal (py3-none-any) de yt-dlp y la de yt-dlp-ejs que esa version
  pide, se comprueba el sha256 que publica PyPI, se descomprime en
  DATA_DIR/yt-dlp/<version>/ y a partir de ahi se usa esa en vez de la
  empaquetada. Si la descargada no carga, se vuelve a la empaquetada.
- **Cargarla de verdad dentro del binario.** PyInstaller 6 mete su importador
  como buscador de la ruta `sys._MEIPASS` (pyimod02_importers.py:
  `PyiFrozenFinder`, en `sys.path_hooks`); versiones anteriores lo ponian en
  `sys.meta_path`, por delante de todo. Aqui se pone un buscador PROPIO al
  principio de `sys.meta_path` que resuelve `yt_dlp`, `yt_dlp_ejs` y todos sus
  submodulos contra la carpeta descargada (con el `PathFinder` normal, que
  para una carpeta de fuera de `_MEIPASS` usa el `FileFinder` de siempre):
  gana en los dos casos, y no toca nada mas del programa.
- **Motor de JavaScript.** YouTube ya exige resolver sus retos con JS; sin un
  motor, yt-dlp avisa «some formats may be missing» y baja peor o nada. Se
  busca deno, node, quickjs o bun (en el orden de preferencia de yt-dlp) con
  `config.find_tool` —la carpeta de herramientas de la app y el PATH— y se le
  pasa a yt-dlp con su ruta.
"""

import contextlib
import hashlib
import importlib.machinery
import importlib.metadata
import io
import json
import logging
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import urllib.request
import zipfile
from pathlib import Path

from . import config

log = logging.getLogger(__name__)

PACKAGES = ("yt_dlp", "yt_dlp_ejs")
# `DANPLAY_PYPI_URL` apunta a un espejo de PyPI (una red que no deja salir a
# pypi.org, o las pruebas del binario empaquetado)
PYPI = os.getenv("DANPLAY_PYPI_URL", "https://pypi.org/pypi").rstrip("/")
USER_AGENT = "DanPlay (+https://github.com/dani17r/danplay)"
# topes de lo que se baja y se descomprime: una rueda de yt-dlp son ~3 MB
MAX_WHEEL_BYTES = 60 * 1024 * 1024
MAX_UNPACKED_BYTES = 200 * 1024 * 1024
TIMEOUT = 60


class UpdateError(Exception):
    """Un fallo de la actualizacion, ya dicho en castellano."""


def version_tuple(v: str) -> tuple[int, ...]:
    """«2026.08.19» y «2026.8.19» son la misma: se comparan como numeros."""
    return tuple(int(x) for x in re.findall(r"\d+", str(v or "")))


# ------------------------------------------------------------ motor de JS
# El orden de preferencia y los minimos son los de yt-dlp
# (yt_dlp/utils/_jsruntime.py y la ayuda de --js-runtimes, 2026.08.19):
# deno >= 2.3.0, node >= 22, quickjs (el binario se llama «qjs») >=
# 2023-12-09 o cualquier quickjs-ng, bun >= 1.2.11. La clave es la que acepta
# la opcion `js_runtimes` de YoutubeDL: {nombre: {"path": ruta}}.
RUNTIMES = (
    ("deno", "deno", ("--version",), r"^deno (\S+)", (2, 3, 0)),
    ("node", "node", ("--version",), r"^v(\S+)", (22, 0, 0)),
    ("quickjs", "qjs", ("--help",), r"^QuickJS(?:-ng)?\s+version\s+(\S+)", (2023, 12, 9)),
    ("bun", "bun", ("--version",), r"^(\S+)", (1, 2, 11)),
)
# Sin motor se vuelve a mirar cada tanto: instalar deno con la app abierta
# tiene que notarse sin reiniciarla.
RUNTIME_RECHECK = 60.0

_runtime_cache: dict = {"at": -1e9, "value": None, "found": []}
_runtime_lock = threading.Lock()


def _probe(name: str, binary: str, args, pattern: str, minimum) -> dict | None:
    """Lo que dice un motor de si mismo, o None si no esta."""
    path = config.find_tool(binary)
    if not path:
        return None
    try:
        r = subprocess.run([path, *args], capture_output=True, text=True, timeout=15, check=False)
    except (OSError, subprocess.SubprocessError):
        log.info("no pude preguntar su version a %s", path, exc_info=True)
        return None
    out = (r.stdout or "") + (r.stderr or "")
    m = re.search(pattern, out, re.MULTILINE)
    version = m.group(1) if m else ""
    got = version_tuple(version)
    ng = name == "quickjs" and "QuickJS-ng" in out
    supported = bool(got) and (got > (0,) if ng else got >= minimum)
    return {
        "name": name,
        "path": path,
        "version": version,
        "supported": supported,
        "minimum": ".".join(str(x) for x in minimum),
    }


def js_runtime(refresh: bool = False) -> dict | None:
    """El motor de JS que usara yt-dlp: {name, path, version}, o None.

    El primero que este y sea lo bastante nuevo, en el orden de yt-dlp. Se
    recuerda; si no hay ninguno, se vuelve a mirar al rato.
    """
    with _runtime_lock:
        cached = dict(_runtime_cache)
    if not refresh and (
        cached["value"] is not None or time.monotonic() - cached["at"] < RUNTIME_RECHECK
    ):
        return cached["value"]
    found = [p for p in (_probe(*spec) for spec in RUNTIMES) if p]
    chosen = next((p for p in found if p["supported"]), None)
    with _runtime_lock:
        _runtime_cache.update(at=time.monotonic(), value=chosen, found=found)
    return chosen


def js_runtime_hint() -> str:
    """Que hacer si no hay motor de JS, en castellano. Vacio si lo hay."""
    if js_runtime():
        return ""
    with _runtime_lock:
        found = list(_runtime_cache["found"])
    old = next((p for p in found if not p["supported"]), None)
    if old:
        return (
            f"Tu {old['name']} ({old['version'] or 'version desconocida'}) es demasiado "
            f"antiguo para yt-dlp: hace falta {old['minimum']} o posterior. Actualizalo, "
            "o instala Deno (https://deno.com)."
        )
    return (
        "YouTube ya exige un motor de JavaScript para descargar bien: sin el pueden "
        "faltar formatos o no bajar nada. Instala Deno (https://deno.com) o Node.js 22 "
        "o posterior y vuelve a esta pagina."
    )


def options() -> dict:
    """Lo que hay que pasarle a YoutubeDL para que use el motor encontrado."""
    rt = js_runtime()
    return {"js_runtimes": {rt["name"]: {"path": rt["path"]}}} if rt else {}


# --------------------------------------------- que yt-dlp se carga


class _Prefer:
    """Buscador de modulos que resuelve `yt_dlp` y `yt_dlp_ejs` (y todo lo que
    cuelga de ellos) contra la carpeta descargada. Va el primero de
    `sys.meta_path`: gana al importador congelado de PyInstaller y a la copia
    del entorno."""

    def __init__(self, folder: Path):
        self.folder = os.path.abspath(str(folder))

    def find_spec(self, fullname, path=None, target=None):
        top = fullname.partition(".")[0]
        if top not in PACKAGES:
            return None
        if "." not in fullname:
            return importlib.machinery.PathFinder.find_spec(fullname, [self.folder], target)
        parent = sys.modules.get(fullname.rpartition(".")[0])
        search = list(getattr(parent, "__path__", None) or path or [])
        if not any(self._inside(p) for p in search):
            return None  # el padre no es de la carpeta: que decida el resto
        return importlib.machinery.PathFinder.find_spec(fullname, search, target)

    def _inside(self, entry) -> bool:
        """Si esa ruta cuelga de la carpeta (por componentes: 2099.1.1 no es
        2099.1.10)."""
        try:
            return os.path.commonpath([os.path.abspath(str(entry)), self.folder]) == self.folder
        except ValueError:  # otra unidad, en Windows
            return False

    def invalidate_caches(self):
        pass


# Leer yt-dlp (una descarga, una consulta) frente a cambiarlo por otra
# version: se puede leer a la vez desde varios hilos, pero el cambio espera a
# que nadie lo este usando y, mientras, no deja empezar a nadie. Cambiarlo con
# una descarga a medias la dejaba con la mitad de los modulos de cada version.
class _ReadWriteLock:
    def __init__(self):
        self._cond = threading.Condition()
        self._readers = 0
        self._writing = False

    @contextlib.contextmanager
    def reading(self):
        with self._cond:
            while self._writing:
                self._cond.wait()
            self._readers += 1
        try:
            yield
        finally:
            with self._cond:
                self._readers -= 1
                self._cond.notify_all()

    @contextlib.contextmanager
    def writing(self):
        with self._cond:
            while self._writing:
                self._cond.wait()
            self._writing = True
            while self._readers:
                self._cond.wait()
        try:
            yield
        finally:
            with self._cond:
                self._writing = False
                self._cond.notify_all()


_rw = _ReadWriteLock()
_state_lock = threading.RLock()
_finder: _Prefer | None = None
_module = None  # el yt_dlp en uso, una vez cargado
_origin = ""  # "" = el empaquetado; si no, la carpeta


def using():
    """Contexto para usar yt-dlp sin que lo cambien por debajo."""
    return _rw.reading()


def root() -> Path:
    return config.DATA_DIR / "yt-dlp"


def _marker() -> Path:
    return root() / "current"


def bundled_version() -> str:
    """La version que viaja con la app (o la del entorno en desarrollo)."""
    try:
        return importlib.metadata.version("yt-dlp")
    except importlib.metadata.PackageNotFoundError:
        return ""


def downloaded() -> tuple[str, Path] | None:
    """(version, carpeta) de la descargada que toca usar: la que dice
    `current`, si esta entera y es mas nueva que la empaquetada."""
    try:
        version = _marker().read_text(encoding="utf-8").strip()
    except OSError:
        return None
    folder = root() / version
    if not version or not (folder / "yt_dlp" / "__init__.py").is_file():
        return None
    bundled = bundled_version()
    if bundled and version_tuple(version) <= version_tuple(bundled):
        return None  # la app trae ya una igual o mas nueva
    return version, folder


def _purge() -> None:
    """Olvida los modulos cargados de yt-dlp: el siguiente import los relee."""
    for name in [n for n in sys.modules if n.partition(".")[0] in PACKAGES]:
        del sys.modules[name]
    importlib.invalidate_caches()


def _install(folder: Path | None) -> None:
    global _finder
    if _finder is not None:
        with contextlib.suppress(ValueError):
            sys.meta_path.remove(_finder)
        _finder = None
    if folder is not None:
        _finder = _Prefer(folder)
        sys.meta_path.insert(0, _finder)


def _import(expected: str = ""):
    import yt_dlp
    import yt_dlp.version

    got = getattr(yt_dlp.version, "__version__", "")
    if expected and version_tuple(got) != version_tuple(expected):
        raise ImportError(f"se esperaba yt-dlp {expected} y se cargo {got}")
    return yt_dlp


def _load():
    """Carga el yt-dlp que toca: el descargado si lo hay, si no el de la app."""
    global _origin
    chosen = downloaded()
    if chosen:
        version, folder = chosen
        _install(folder)
        _purge()
        try:
            module = _import(version)
            _origin = str(folder)
            log.info("yt-dlp %s desde %s", version, folder)
            return module
        except Exception:
            log.warning(
                "yt-dlp %s descargado no carga; se usa el de la app", version, exc_info=True
            )
            _install(None)
            _purge()
            _discard(folder)
    _origin = ""
    try:
        return _import()
    except ImportError:
        return None


def prepare() -> None:
    """Pone el buscador de la carpeta descargada, sin importar nada: para que
    preguntar si yt-dlp esta (`find_spec`) ya mire la version buena."""
    with _state_lock:
        if _module is None and _finder is None:
            chosen = downloaded()
            if chosen:
                _install(chosen[1])


def module():
    """El modulo `yt_dlp` en uso, o None si no hay ninguno."""
    global _module
    if _module is not None:
        return _module
    with _state_lock:
        if _module is None:
            _module = _load()
        return _module


def version() -> str:
    """La version que se esta usando (cargandola si hace falta)."""
    m = module()
    if m is None:
        return ""
    with contextlib.suppress(Exception):
        return str(m.version.__version__)
    return ""


def origin() -> str:
    """De donde sale el yt-dlp en uso: vacio si es el de la app."""
    return _origin


def _discard(folder: Path) -> None:
    """Una version descargada que no sirve: fuera, y que no se vuelva a probar."""
    with contextlib.suppress(OSError):
        if _marker().read_text(encoding="utf-8").strip() == folder.name:
            _marker().unlink()
    shutil.rmtree(folder, ignore_errors=True)


# ------------------------------------------------------ actualizar


def _get(url: str, limit: int) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        data = r.read(limit + 1)
    if len(data) > limit:
        raise UpdateError(f"la descarga de {url} pesa demasiado")
    return data


def _pypi(project: str, release: str = "") -> dict:
    url = f"{PYPI}/{project}/{release + '/' if release else ''}json"
    try:
        data = json.loads(_get(url, 20 * 1024 * 1024).decode("utf-8"))
    except UpdateError:
        raise
    except Exception as e:
        raise UpdateError(f"no se pudo consultar PyPI ({_why(e)})") from e
    if not isinstance(data, dict) or "info" not in data:
        raise UpdateError("PyPI ha contestado algo que no se entiende")
    return data


def _why(e: Exception) -> str:
    """Un fallo de red, en una frase que se entienda."""
    code = getattr(e, "code", None)
    if isinstance(code, int):
        return f"el servidor contesto con un error {code}"
    text = str(e)
    low = text.lower()
    if "timed out" in low or "timeout" in low:
        return "tarda demasiado en responder"
    if "resolve" in low or "name or service" in low or "getaddrinfo" in low or "nodename" in low:
        return "sin conexion a internet"
    return text[:160]


def _wheel(data: dict) -> dict:
    """La rueda universal (py3-none-any) de una version de PyPI."""
    for f in data.get("urls") or []:
        name = str(f.get("filename") or "")
        if f.get("packagetype") == "bdist_wheel" and name.endswith("-py3-none-any.whl"):
            if not (f.get("digests") or {}).get("sha256"):
                break
            return f
    raise UpdateError(f"PyPI no tiene una rueda universal de {data['info'].get('name')}")


def _ejs_pin(data: dict) -> str:
    """La version de yt-dlp-ejs que pide esa version de yt-dlp (su extra
    «default»). yt-dlp exige que casen: con otra, no resuelve los retos."""
    for req in data["info"].get("requires_dist") or []:
        m = re.match(r"^\s*yt-dlp-ejs\s*==\s*([\w.]+)", str(req))
        if m and "default" in str(req):
            return m.group(1)
    return ""


def _fetch(wheel: dict) -> bytes:
    """La rueda, comprobada contra el sha256 que publica PyPI."""
    try:
        data = _get(wheel["url"], MAX_WHEEL_BYTES)
    except UpdateError:
        raise
    except Exception as e:
        raise UpdateError(f"no se pudo bajar {wheel.get('filename')} ({_why(e)})") from e
    digest = hashlib.sha256(data).hexdigest()
    if digest != wheel["digests"]["sha256"]:
        raise UpdateError(
            f"{wheel.get('filename')} no coincide con lo que publica PyPI "
            "(sha256 distinto): no se usa"
        )
    return data


def _unpack(wheels: list[bytes], target: Path) -> None:
    """Descomprime las ruedas en `target`, sin dejar que un nombre raro
    escriba fuera («../», rutas absolutas) ni que el total se dispare."""
    total = 0
    for data in wheels:
        with zipfile.ZipFile(io.BytesIO(data)) as z:
            for info in z.infolist():
                name = info.filename
                parts = Path(name).parts
                if name.startswith(("/", "\\")) or ".." in parts or ":" in name or not parts:
                    raise UpdateError(f"la rueda trae un archivo que no toca: {name}")
                # lo de <paquete>.data/ (paginas de manual, autocompletado) no
                # hace falta para importarlo; ni nada que no sea el paquete
                if not (parts[0] in PACKAGES or parts[0].endswith(".dist-info")):
                    continue
                total += info.file_size
                if total > MAX_UNPACKED_BYTES:
                    raise UpdateError("la rueda descomprimida pesa demasiado")
                z.extract(info, target)


def update(progress=None) -> dict:
    """Pone yt-dlp al dia desde PyPI. Devuelve {previous, version, updated}.

    Es el trabajo «yt-dlp» del contrato A: `progress(done, total, message)`.
    """

    def step(n, message):
        if progress:
            progress(n, 5, message)

    previous = version()
    step(0, "consultando PyPI")
    meta = _pypi("yt-dlp")
    latest = str(meta["info"].get("version") or "")
    if not latest:
        raise UpdateError("PyPI no dice cual es la ultima version de yt-dlp")
    if previous and version_tuple(latest) <= version_tuple(previous):
        return {"previous": previous, "version": previous, "updated": False}
    wheels = [_wheel(meta)]
    pin = _ejs_pin(meta)
    ejs = _pypi("yt-dlp-ejs", pin) if pin else _pypi("yt-dlp-ejs")
    wheels.append(_wheel(ejs))
    step(1, f"bajando yt-dlp {latest}")
    blobs = [_fetch(w) for w in wheels]
    step(2, "descomprimiendo")
    root().mkdir(parents=True, exist_ok=True)
    staging = root() / f".nueva-{os.getpid()}-{int(time.time())}"
    shutil.rmtree(staging, ignore_errors=True)
    try:
        _unpack(blobs, staging)
        # compilado ya: cargar un yt-dlp en .py son un par de segundos cada vez
        with contextlib.suppress(Exception):
            import compileall

            compileall.compile_dir(str(staging), quiet=1, workers=1)
        final = root() / latest
        shutil.rmtree(final, ignore_errors=True)
        os.replace(staging, final)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    step(3, "probando la version nueva")
    _switch_to(latest, final)
    step(4, "limpiando versiones viejas")
    for old in root().iterdir():
        if old.is_dir() and old.name != latest:
            shutil.rmtree(old, ignore_errors=True)
    return {"previous": previous, "version": version(), "updated": True}


def _switch_to(new_version: str, folder: Path) -> None:
    """Deja en uso la version de `folder`, esperando a que nadie use la de
    ahora. Si no carga, se vuelve a la de antes y se dice."""
    global _module, _origin
    with _rw.writing(), _state_lock:
        before = _finder.folder if _finder else None
        _install(folder)
        _purge()
        try:
            _module = _import(new_version)
        except Exception as e:
            log.warning("yt-dlp %s no carga; se vuelve al de antes", new_version, exc_info=True)
            _install(Path(before) if before else None)
            _purge()
            _module = None
            shutil.rmtree(folder, ignore_errors=True)
            raise UpdateError(
                f"yt-dlp {new_version} no se pudo cargar ({str(e)[:120]}); "
                "se sigue con la version de antes"
            ) from e
        _origin = str(folder)
        tmp = _marker().with_name("current.tmp")
        tmp.write_text(new_version, encoding="utf-8")
        os.replace(tmp, _marker())
    log.info("yt-dlp actualizado a %s", new_version)
