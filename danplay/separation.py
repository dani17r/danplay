"""El separador por dentro: Demucs v4 (HTDemucs) con ONNX Runtime.

La red va en un grafo ONNX sin pesos (`danplay/data/separator/`); los pesos
son el archivo oficial del autor, que se baja la primera vez. Lo que Demucs
hace con PyTorch alrededor de la red lo hace el crate (analysis/src/separate.rs,
cuenta a cuenta lo que antes estaba aqui con numpy):

- el espectrograma de cada trozo y su inversa (`spec`, `ispec`), con las
  mismas ventanas, rellenos y normalizacion que `HTDemucs._spec`/`_ispec`;
- la cancion en trozos de 7,8 s que se solapan un cuarto y se funden con un
  peso en triangulo, como `demucs.apply.apply_model` (sin desplazamientos
  aleatorios: el mismo archivo da siempre las mismas pistas);
- la red, en el ONNX Runtime de la biblioteca que trae el paquete
  `onnxruntime`, cargada desde Rust, y el trabajo entero (`run`, `run_max`):
  leer la cancion, guardar cada pista con ffmpeg y sacar lo que queda.

Comparado con Demucs sobre una cancion de verdad, la diferencia es ruido
numerico (mas de 55 dB por debajo de cada pista).

Esto corre en un proceso aparte (`main`), con prioridad baja: la red usa
todos los nucleos y un par de GB durante un par de minutos, y asi ni la
musica ni la interfaz lo notan, y la memoria se devuelve al acabar. Cuenta
como va por la salida estandar, una linea JSON por aviso.

No importa nada del resto de DanPlay (salvo donde esta ONNX Runtime): el
proceso hijo arranca rapido.
"""

import hashlib
import json
import os
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from pathlib import Path

import danplay_core as _rust
import numpy as np

from danplay.pulse import onnxruntime_library

RATE = 44100
NFFT = 4096
HOP = NFFT // 4
# lo que dura un trozo para la red: int(39/5 * 44100), el de su entrenamiento
SEGMENT = 343980
OVERLAP = 0.25
STRIDE = int((1 - OVERLAP) * SEGMENT)
FREQS = NFFT // 2  # el ultimo bin se tira, como en Demucs

WINDOW = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(NFFT) / NFFT)).astype(np.float32)

# ------------------------------------------------------------ los pesos


def read_safetensors(path) -> dict[str, np.ndarray]:
    """Los tensores de un .safetensors, sin la biblioteca: una cabecera JSON
    y los datos crudos detras (la lee el crate). Se mapea el archivo, no se
    copia."""
    path = Path(path)
    tensors = _rust.separation_safetensors(path)
    data = np.memmap(path, dtype=np.uint8, mode="r")
    return {
        name: data[begin:end].view(np.dtype(dtype)).reshape(shape)
        for name, dtype, begin, end, shape in tensors
    }


def sha256_of(path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def open_session(graph, manifest: dict, weights_file, n_threads: int | None = None):
    """La sesion de ONNX Runtime (desde Rust) con el grafo y los pesos
    oficiales puestos: cada peso del grafo es una referencia externa, y se le
    da el tensor del archivo tal cual, traspuesto (`op` "T") o con otra forma
    ("R"), en float32. Un checkpoint de PyTorch (el del Mega53) se lee sin
    PyTorch. Se usa como las de onnxruntime (`run`, `get_inputs`)."""
    return _rust.separation_open_session(
        onnxruntime_library(), str(graph), json.dumps(manifest), str(weights_file), n_threads
    )


# ------------------------------------------------------------ espectrograma


def spec(x: np.ndarray) -> np.ndarray:
    """HTDemucs._spec y _magnitude: (2, L) -> (4, 2048, frames), la parte
    real y la imaginaria de cada canal como canales."""
    return _rust.separation_spec(np.ascontiguousarray(x, dtype=np.float32))


def ispec(spectrum: np.ndarray, length: int) -> np.ndarray:
    """HTDemucs._mask (cac) y _ispec: (S, 4, 2048, frames) -> (S, 2, length)."""
    return _rust.separation_ispec(np.ascontiguousarray(spectrum, dtype=np.float32), int(length))


# ------------------------------------------------------------ la cancion

# lo que falta por hacer se cuenta con esto: (trozos hechos, trozos en total)
Progress = Callable[[int, int], None]


def separate(
    session,
    wav: np.ndarray,
    emit: Callable[[np.ndarray], None],
    progress: Progress | None = None,
    cancelled: Callable[[], bool] | None = None,
) -> None:
    """Separa `wav` (2, N) y va dando las pistas por tramos a `emit`, en orden:
    arrays (S, 2, n) que juntos cubren la cancion entera. Asi no hace falta
    tener todas las pistas de la cancion en memoria (seis pistas de cinco
    minutos son 640 MB). Mientras la red calcula un trozo, un hilo deshace el
    espectrograma del anterior y lo funde. `session`, la de `open_session`
    (todo en Rust) o cualquiera con `run` como las de onnxruntime."""
    _rust.separation_separate(
        session, np.ascontiguousarray(wav, dtype=np.float32), emit, progress, cancelled
    )


# ------------------------------------------------------------ el proceso


def _say(**message) -> None:
    sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _decode(ffmpeg: str, path: str) -> np.ndarray:
    """La cancion entera, en estereo a 44,1 kHz y en flotante."""
    return _rust.separation_decode(str(ffmpeg), str(path))


def _encoder(ffmpeg: str, target: Path) -> subprocess.Popen:
    """Un ffmpeg que recibe una pista cruda y la guarda en FLAC de 16 bits."""
    return subprocess.Popen(
        [
            ffmpeg,
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "f32le",
            "-ar",
            str(RATE),
            "-ac",
            "2",
            "-i",
            "-",
            "-c:a",
            "flac",
            "-sample_fmt",
            "s16",
            str(target),
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )


def _lower_priority() -> None:
    """Que la separacion ceda ante todo lo demas: la musica, la interfaz."""
    if hasattr(os, "nice"):
        try:
            os.nice(10)
        except OSError:
            pass
    elif sys.platform == "win32":  # pragma: no cover - solo Windows
        import ctypes

        below_normal = 0x4000
        handle = ctypes.windll.kernel32.GetCurrentProcess()
        ctypes.windll.kernel32.SetPriorityClass(handle, below_normal)


# La forma de onda de cada pista, calculada mientras se escribe: columnas a lo
# largo de la cancion, con pico y RMS, todas a la escala del pico mas alto (ver
# analysis/src/separate.rs). `Waves(sources, length, buckets=800)`, `add`,
# `result`, y `energy` por columnas.
Waves = _rust.Waves

# ------------------------------------------------------------ que pistas se quedan

# Una fuente que la red no encuentra en la cancion no sale callada: sale con
# lo que se le cuela de las demas, muy por debajo de la mezcla. Se queda si en
# conjunto suena a mas de QUIET_DB de la cancion, o si llega a LOUD_DB de ella
# en al menos ACTIVE de lo que suena (un piano que solo entra en el puente).
# Medido con scripts/evaluate-separator.py --presencia: en canciones de verdad
# lo que no esta sale de -44 a -63 dB y lo que esta, por encima de -20.
QUIET_DB = -30.0
LOUD_DB = -15.0
ACTIVE = 0.02
# los tramos en que se mira: un cuarto de segundo, sea larga o corta la cancion
METER = RATE // 4


def meter(sources: list[str], length: int) -> "Waves":
    """Lo que suena cada una por tramos de un cuarto de segundo (`presence`)."""
    return _rust.separation_meter(list(sources), int(length))


def presence(energy: np.ndarray, mix: np.ndarray) -> dict:
    """Cuanto suena una fuente en la cancion, por tramos (`Waves.energy`):
    `db`, su energia frente a la de la mezcla; `active`, la parte de lo que
    suena de la cancion en que la fuente llega a LOUD_DB de ella."""
    return _rust.separation_presence(
        np.ascontiguousarray(energy, dtype=np.float64), np.ascontiguousarray(mix, dtype=np.float64)
    )


def keeps(level: dict) -> bool:
    """Si una fuente con ese `presence` es un instrumento que esta de verdad."""
    return _rust.separation_keeps(float(level["db"]), float(level["active"]))


# ------------------------------------------------------------ el trabajo

CHUNK = 10 * RATE  # de cuanto en cuanto se leen las pistas al juntarlas


def _reader(ffmpeg: str, path) -> subprocess.Popen:
    """Un ffmpeg que da una pista ya guardada, cruda, para leerla por trozos."""
    return subprocess.Popen(
        [
            *(ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-i", str(path)),
            *("-f", "f32le", "-ac", "2", "-ar", str(RATE), "-"),
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def _read(reader: subprocess.Popen, n: int) -> np.ndarray:
    """Las siguientes `n` muestras (2, n); con ceros si la pista acaba antes."""
    assert reader.stdout is not None
    raw = reader.stdout.read(n * 8)
    data = np.frombuffer(raw[: len(raw) // 8 * 8], dtype=np.float32).reshape(-1, 2).T
    if data.shape[-1] < n:
        data = np.pad(data, [(0, 0), (0, n - data.shape[-1])])
    return data


def _close(procs: dict[str, subprocess.Popen], what: str) -> None:
    """Cierra los ffmpeg y falla diciendo cual si alguno fallo."""
    for proc in procs.values():
        if proc.stdin:
            proc.stdin.close()
        if proc.stdout:
            proc.stdout.close()
    failed = []
    for name, proc in procs.items():
        err = proc.stderr.read() if proc.stderr else b""
        if proc.wait() != 0:
            failed.append(f"{name}: {err.decode('utf-8', 'replace').strip()[-200:]}")
    if failed:
        raise RuntimeError(f"ffmpeg no pudo {what}: " + "; ".join(failed))


def _write(proc: subprocess.Popen, block: np.ndarray) -> None:
    assert proc.stdin is not None
    proc.stdin.write(np.ascontiguousarray(block.T, dtype=np.float32).tobytes())


def _to_opus(ffmpeg: str, source: Path) -> Path:
    """La pista en Opus, en su lugar: a 128 kbps, unas cuatro veces menos que en
    FLAC (medido con pistas de verdad; a 96 serian cinco, pero una pista sola
    deja oir mas lo que el codec quita que la cancion entera)."""
    target = source.with_suffix(".opus")
    r = subprocess.run(
        [
            *(ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i", str(source)),
            *("-c:a", "libopus", "-b:a", "128k", "-ar", "48000", str(target)),
        ],
        capture_output=True,
        check=False,
    )
    if r.returncode != 0:
        detail = r.stderr.decode("utf-8", "replace").strip()[-200:]
        raise RuntimeError(f"ffmpeg no pudo pasar {source.name} a Opus: {detail}")
    source.unlink()
    return target


def _work(job: dict, open_net, max_pass: bool) -> dict:
    own = open_net is open_session
    library = None
    if own:
        try:
            library = onnxruntime_library()
        except RuntimeError:
            library = None  # lo dira el crate al abrir la red
    return _rust.separation_run(
        json.dumps(job),
        library,
        None if own else open_net,
        lambda message: _say(**message),
        max_pass,
    )


def run(job: dict, open_net=open_session) -> dict:
    """Pasa la cancion por las redes del trabajo y deja sus pistas en
    `job["output"]`, con su forma de onda en `job["waves"]` (lo hace el
    crate: analysis/src/separate/process.rs).

    - input, ffmpeg, output, waves y, opcionales, threads y format ("opus");
    - files: {fuente: nombre de su archivo .flac}, en el orden de las pistas;
    - nets: [{graph, manifest, weights, take: [fuentes]}], las redes por las
      que pasa, en orden, y que fuentes se quedan de cada una;
    - carry: {fuente: ruta}, pistas ya hechas que entran tal cual;
    - keep: las fuentes que se quedan. Sin el, las decide lo que suena cada
      una en la primera red (`presence`): la que no esta en la cancion no
      llega a ser pista;
    - rest: la fuente que es «lo que queda»: la cancion menos todas las
      demas, asi que juntas suenan exactamente como la cancion.

    `open_net(graph, manifest, weights, threads)`, si se da, abre cada red (las
    pruebas pasan unas de mentira). Devuelve las pistas que salen ({fuente:
    archivo}) y lo que suena cada una.
    """
    return _work(job, open_net, False)


def run_max(job: dict, open_net=open_session) -> dict:
    """La pasada de maxima calidad (`danplay.mega`): reparte la bateria, las
    voces y lo melodico de las pistas buenas entre mas instrumentos.

    - input, ffmpeg, output, waves, format: como en `run`;
    - carry: {drums, bass, vocals, …: ruta}, las pistas buenas de antes;
    - mega: {graph, manifest, weights}, el Mega53;
    - files: {fuente: nombre .flac} de todas las que pueden salir.

    Cada pista nueva se queda si suena (`presence`). Si la percusion no esta,
    la bateria queda como estaba; si no hay coros, las voces tambien. «Otros»
    es la cancion menos todo lo que se queda, como siempre.
    """
    return _work(job, open_net, True)


def _orphan_guard() -> None:
    """Si el nucleo que lo lanzo se muere, esto no sigue gastando la maquina
    entera para nadie: al quedarse huerfano, el proceso cambia de padre."""
    if not hasattr(os, "getppid") or sys.platform == "win32":
        return  # en Windows muere con el Job Object de la aplicacion
    parent = os.getppid()

    def watch():
        while True:
            time.sleep(2)
            if os.getppid() != parent:
                os._exit(3)

    threading.Thread(target=watch, name="danplay-huerfano", daemon=True).start()


def main(argv=None) -> int:
    """El proceso hijo: `python -m danplay.separation TRABAJO.json`."""
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 1:
        sys.stderr.write("uso: danplay.separation TRABAJO.json\n")
        return 2
    _lower_priority()
    _orphan_guard()
    try:
        job = json.loads(Path(args[0]).read_text(encoding="utf-8"))
        result = run_max(job) if job.get("mega") else run(job)
    except Exception as e:  # noqa: BLE001 - se cuenta al proceso padre
        _say(error=str(e) or e.__class__.__name__)
        return 1
    _say(ok=True, **result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
