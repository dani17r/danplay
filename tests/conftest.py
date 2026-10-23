"""Fixtures compartidas, y el aislamiento de TODAS las pruebas.

Aislamiento: `danplay.config` calcula al importarse donde viven los datos del
usuario (la base, los ajustes con las claves de IA, la cache de ondas y de
modelos). Si las pruebas lo importan con el HOME de verdad, reescriben
~/.config/danplay/danplay.env, podan la cache real de formas de onda y bajan
el catalogo de models.dev a la carpeta de datos de verdad. Por eso, ANTES de
que nadie importe danplay, todo lo que cuelga del usuario se apunta a un
temporal de la sesion, y la red se corta: una prueba que sale a internet sin
querer falla al instante en vez de colgarse o de gastar la clave de nadie.

Audio sintetico: las pruebas no deben depender de la musica de nadie. Con
ffmpeg se generan mp3 de un segundo de silencio, con etiquetas ID3 escritas
por mutagen, y con eso se monta una biblioteca temporal completa.
"""

import atexit
import ipaddress
import json
import os
import pathlib
import shutil
import socket
import subprocess
import sys
import tempfile

# ------------------------------------------------------------ aislamiento
# Va a nivel de modulo y antes de cualquier `import danplay`: pytest carga este
# archivo antes que los de las pruebas.
SANDBOX = pathlib.Path(tempfile.mkdtemp(prefix="danplay-sesion-"))
atexit.register(shutil.rmtree, SANDBOX, ignore_errors=True)
for _sub in ("home", "config", "data", "cache", "state", "Musica"):
    (SANDBOX / _sub).mkdir()
os.environ.update(
    {
        "HOME": str(SANDBOX / "home"),
        "USERPROFILE": str(SANDBOX / "home"),
        "XDG_CONFIG_HOME": str(SANDBOX / "config"),
        "XDG_DATA_HOME": str(SANDBOX / "data"),
        "XDG_CACHE_HOME": str(SANDBOX / "cache"),
        "XDG_STATE_HOME": str(SANDBOX / "state"),
        # En Windows y macOS platformdirs no mira XDG_* (pregunta al sistema
        # por sus carpetas): ahi solo valen estas dos, que config lee primero.
        "DANPLAY_DATA_DIR": str(SANDBOX / "data" / "danplay"),
        "DANPLAY_CONFIG_DIR": str(SANDBOX / "config" / "danplay"),
        "DANPLAY_LIBRARY": str(SANDBOX / "Musica"),
        # ni vigilante de carpetas ni el .env/la base del proyecto en desarrollo
        # (que lleva las claves de quien programa)
        "DANPLAY_WATCH": "0",
        "DANPLAY_PROJECT_ENV": "0",
        # un proxy local sacaria a internet lo que el corte de abajo deja pasar
        "NO_PROXY": "*",
        "no_proxy": "*",
        # lo que Hypothesis guarda (ejemplos, tablas de Unicode), fuera del proyecto
        "HYPOTHESIS_STORAGE_DIRECTORY": str(SANDBOX / "hypothesis"),
    }
)
for _var in (
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "DANPLAY_TOKEN",
    "DANPLAY_PARENT_PID",
    "DANPLAY_AI_PROVIDER",
    "DANPLAY_AI_KEY",
    "DANPLAY_AI_BASE_URL",
    "DANPLAY_AI_MODEL",
    "DANPLAY_AI_CHAT_MODEL",
    "DEEPINFRA_API_KEY",
    "DEEPINFRA_BASE_URL",
    "ACOUSTID_API_KEY",
):
    os.environ.pop(_var, None)


class NetworkBlocked(OSError):
    """Lo que recibe una prueba que intenta salir a internet."""


_LOCAL_NAMES = {"localhost", "localhost.localdomain", "testserver", ""}
_real_connect = socket.socket.connect
_real_connect_ex = socket.socket.connect_ex
_real_getaddrinfo = socket.getaddrinfo


def _is_local(host) -> bool:
    if isinstance(host, bytes):
        host = host.decode("ascii", "replace")
    host = str(host or "").strip("[]").lower()
    if host in _LOCAL_NAMES:
        return True
    try:
        return ipaddress.ip_address(host.split("%", 1)[0]).is_loopback
    except ValueError:
        return False


def _check(sock, address) -> None:
    if getattr(socket, "AF_UNIX", None) is not None and sock.family == socket.AF_UNIX:
        return
    host = address[0] if isinstance(address, tuple) else address
    if not _is_local(host):
        raise NetworkBlocked(f"las pruebas no salen a internet (se pidio {address!r})")


def _connect(self, address):
    _check(self, address)
    return _real_connect(self, address)


def _connect_ex(self, address):
    _check(self, address)
    return _real_connect_ex(self, address)


def _getaddrinfo(host, *args, **kwargs):
    # tambien la resolucion de nombres: una consulta DNS ya es salir fuera
    if not _is_local(host):
        raise NetworkBlocked(f"las pruebas no resuelven nombres de fuera ({host!r})")
    return _real_getaddrinfo(host, *args, **kwargs)


socket.socket.connect = _connect
socket.socket.connect_ex = _connect_ex
socket.getaddrinfo = _getaddrinfo

# --------------------------------------------------------------- fixtures
import pytest  # noqa: E402  (despues del aislamiento a proposito)


def pytest_collection_modifyitems(config, items):
    """Con `DANPLAY_SHARD=k/n`, solo la parte k de n: la CI de Windows reparte
    las pruebas entre varias maquinas a la vez. Por archivos enteros (las de
    un archivo cuentan con lo que hicieron las de antes), y por lo que tarda
    cada uno (tests/durations.json; los que no estan, 0,1 s por prueba): del
    mas lento al mas rapido, cada uno a la parte que menos lleva. Siempre la
    misma reparticion. Para rehacer los tiempos:
    `pytest -n auto --dist loadfile --no-cov --junitxml=t.xml` y sumarlos."""
    shard = os.environ.get("DANPLAY_SHARD", "")
    if not shard:
        return
    k, n = (int(x) for x in shard.split("/"))
    known = json.loads((pathlib.Path(__file__).parent / "durations.json").read_text("utf-8"))
    cost: dict[str, float] = {}
    for item in items:
        path = item.nodeid.split("::")[0]
        cost.setdefault(path, 0.0)
        if path not in known:
            cost[path] += 0.1
    for path in cost:
        cost[path] = known.get(path, cost[path])
    load, part = [0.0] * n, {}
    for path in sorted(cost, key=lambda p: (-cost[p], p)):
        j = load.index(min(load))
        part[path] = j
        load[j] += cost[path]
    keep = [it for it in items if part[it.nodeid.split("::")[0]] == k - 1]
    dropped = [it for it in items if part[it.nodeid.split("::")[0]] != k - 1]
    if dropped:
        config.hook.pytest_deselected(items=dropped)
        items[:] = keep


# Hypothesis (tests/test_properties.py): sin plazo por ejemplo. En un disco
# lento o en la CI el primero tarda, y fallaba por tiempo y no por el codigo.
try:
    from hypothesis import settings as _hsettings
except ImportError:  # sin el grupo de pruebas instalado
    pass
else:
    _hsettings.register_profile("danplay", deadline=None)
    _hsettings.load_profile("danplay")

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

FFMPEG = shutil.which("ffmpeg")


# ------------------------------------------------------- topes de tiempo
# Algunas pruebas ponen un tope de tiempo (una entrada hostil no puede tardar
# mas de un segundo): es lo que caza una expresion regular que se vuelve
# exponencial. Los topes se fijaron en un portatil corriente y sin medir la
# cobertura; el Mac Intel de la CI es una maquina virtual varias veces mas
# lenta, y con la cobertura el codigo de DanPlay va unas tres veces mas
# despacio: los pasaban sin que hubiera nada mal. `time_limit` escala el tope
# por lo lento que va AQUI un trabajo de referencia con el propio codigo de
# DanPlay (el lector de listas: la cobertura lo frena igual que a lo que se
# mide), y nunca lo baja.
_SPEED_REFERENCE = 0.16  # segundos de `_reference_work` donde se fijaron los topes
_SLOWNESS: list[float] = []
_REFERENCE_TEXT = "\n".join(
    f"*{i}.- Canción Número {i}* ✅ @Ana Torres Voz https://youtu.be/AbCdEfGh{i % 1000:03d}"
    for i in range(1, 300)
)


def _reference_work() -> None:
    from danplay.chat import intake

    for _ in range(4):
        intake.parse_setlist(_REFERENCE_TEXT)
        intake.read_intent(_REFERENCE_TEXT)


def slowness() -> float:
    """Cuantas veces mas lento va esto que donde se fijaron los topes (>= 1)."""
    if not _SLOWNESS:
        import time as _time

        _reference_work()  # lo que se carga la primera vez no cuenta
        took = []
        for _ in range(3):
            t0 = _time.perf_counter()
            _reference_work()
            took.append(_time.perf_counter() - t0)
        _SLOWNESS.append(max(1.0, min(took) / _SPEED_REFERENCE))
    return _SLOWNESS[0]


def time_limit(seconds: float) -> float:
    """El tope de tiempo de una prueba, escalado a la velocidad de la maquina."""
    return seconds * slowness()


def make_audio(path, codec_args, seconds=1.0, tone=None) -> str:
    """Un archivo de audio real hecho con ffmpeg, con el codec que se pida."""
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    source = f"sine=frequency={tone}:sample_rate=44100" if tone else "anullsrc=r=44100:cl=mono"
    subprocess.run(
        [
            FFMPEG,
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            source,
            "-t",
            str(seconds),
            *codec_args,
            str(path),
        ],
        check=True,
        capture_output=True,
        timeout=60,
    )
    return str(path)


def make_mp3(path, artist="", title="", album="", seconds=1.0, tone=None, **extra):
    """Crea un mp3 real en `path` con las etiquetas dadas: silencio, o un
    tono de `tone` Hz si se pide (para lo que necesita oir algo)."""
    path = make_audio(path, ["-codec:a", "libmp3lame", "-b:a", "64k"], seconds, tone)
    if artist or title or album or extra:
        from mutagen.id3 import ID3, TALB, TIT2, TPE1, ID3NoHeaderError

        try:
            id3 = ID3(path)
        except ID3NoHeaderError:
            id3 = ID3()
        if artist:
            id3.add(TPE1(encoding=3, text=artist))
        if title:
            id3.add(TIT2(encoding=3, text=title))
        if album:
            id3.add(TALB(encoding=3, text=album))
        id3.save(path)
    return path


def run_job(client, path, name, timeout=60.0, **kwargs):
    """Lo que hace `api.runJob` en la interfaz: arranca un trabajo largo con
    un POST (contrato A), pregunta por GET /api/jobs/{name} hasta que acaba y
    devuelve su `result` (o falla con su `error`)."""
    import time

    r = client.post(path, **kwargs)
    assert r.status_code == 202, r.text
    limit = time.monotonic() + timeout
    while True:
        job = client.get(f"/api/jobs/{name}").json()
        if not job["active"]:
            break
        assert time.monotonic() < limit, f"el trabajo «{name}» no acaba: {job}"
        time.sleep(0.02)
    assert not job["error"], job["error"]
    return job["result"]


@pytest.fixture(scope="session")
def synthetic_ok():
    if not FFMPEG:
        pytest.skip("hace falta ffmpeg para generar audio de prueba")
    return True


@pytest.fixture
def synthetic_library(tmp_path, synthetic_ok):
    """Una biblioteca temporal con dos artistas y tres canciones sinteticas.

    Devuelve (raiz, {nombre: ruta}). No toca la configuracion global: cada
    prueba decide si apunta `config` a ella.
    """
    lib = tmp_path / "Musica"
    songs = {
        "gozo": make_mp3(
            lib / "Artistas/Barak/Barak - Mi Gozo.mp3",
            artist="Barak",
            title="Mi Gozo",
            album="Gozo",
        ),
        "tierra": make_mp3(
            lib / "Artistas/Barak/Barak - Sera Llena La Tierra.mp3",
            artist="Barak",
            title="Sera Llena La Tierra",
            album="Gozo",
        ),
        "shekinah": make_mp3(
            lib / "Artistas/New Wine/New Wine - Shekinah.mp3",
            artist="New Wine",
            title="Shekinah",
            album="Libertad",
        ),
    }
    for sub in ("Entrada", "Revisar", "Secuencias"):
        (lib / sub).mkdir(parents=True, exist_ok=True)
    return lib, songs


@pytest.fixture
def configured_library(synthetic_library, tmp_path, monkeypatch):
    """La biblioteca sintetica ya apuntada en `config`, con base de datos propia."""
    from danplay import config

    lib, songs = synthetic_library
    monkeypatch.setattr(config, "LIBRARY", lib)
    monkeypatch.setattr(config, "INBOX", lib / "Entrada")
    monkeypatch.setattr(config, "ARTISTS_DIR", lib / "Artistas")
    monkeypatch.setattr(config, "REVIEW_DIR", lib / "Revisar")
    monkeypatch.setattr(config, "DATABASE", tmp_path / "danplay.db")
    monkeypatch.setattr(config, "WRITE_TAGS", True)
    monkeypatch.setattr(config, "AI_ENABLED", False)
    return lib, songs


@pytest.fixture(autouse=True)
def _nothing_left_in_the_background():
    """Lo que se aprovecha de otra copia de una cancion va en un hilo
    (`reuse.later`): que acabe dentro de su prueba y no toque la base de la
    siguiente."""
    yield
    import sys

    reuse = sys.modules.get("danplay.reuse")
    if reuse is not None:
        assert reuse.idle(120), "lo de aprovechar se quedo trabajando"
