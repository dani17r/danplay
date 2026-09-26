"""El separador por dentro: Demucs v4 (HTDemucs) con numpy y ONNX Runtime.

La red va en un grafo ONNX sin pesos (`danplay/data/separador/`); los pesos
son el archivo oficial del autor, que se baja la primera vez. Lo que Demucs
hace con PyTorch alrededor de la red se hace aqui igual, con numpy:

- el espectrograma de cada trozo y su inversa (`spec`, `ispec`), con las
  mismas ventanas, rellenos y normalizacion que `HTDemucs._spec`/`_ispec`;
- la cancion en trozos de 7,8 s que se solapan un cuarto y se funden con un
  peso en triangulo, como `demucs.apply.apply_model` (sin desplazamientos
  aleatorios: el mismo archivo da siempre las mismas pistas).

Comparado con Demucs sobre una cancion de verdad, la diferencia es ruido
numerico (mas de 55 dB por debajo de cada pista).

Esto corre en un proceso aparte (`main`), con prioridad baja: la red usa
todos los nucleos y un par de GB durante un par de minutos, y asi ni la
musica ni la interfaz lo notan, y la memoria se devuelve al acabar. Cuenta
como va por la salida estandar, una linea JSON por aviso.

No importa nada del resto de DanPlay: el proceso hijo arranca rapido.
"""

import hashlib
import json
import math
import os
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from pathlib import Path

import numpy as np

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


_DTYPES = {"F32": np.float32, "F16": np.float16, "I64": np.int64, "I32": np.int32}


def read_safetensors(path) -> dict[str, np.ndarray]:
    """Los tensores de un .safetensors, sin la biblioteca: una cabecera JSON
    y los datos crudos detras. Se mapea el archivo, no se copia."""
    path = Path(path)
    with open(path, "rb") as f:
        size = int.from_bytes(f.read(8), "little")
        if size <= 0 or size > 50_000_000:
            raise ValueError(f"{path.name} no es un safetensors")
        header = json.loads(f.read(size))
    data = np.memmap(path, dtype=np.uint8, mode="r", offset=8 + size)
    out = {}
    for name, meta in header.items():
        if name == "__metadata__":
            continue
        dtype = _DTYPES.get(meta["dtype"])
        if dtype is None:
            raise ValueError(f"tipo {meta['dtype']} desconocido en {path.name}")
        begin, end = meta["data_offsets"]
        out[name] = data[begin:end].view(dtype).reshape(meta["shape"])
    return out


def sha256_of(path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def open_session(graph, manifest: dict, weights_file, n_threads: int | None = None):
    """La sesion de ONNX Runtime con el grafo y los pesos oficiales puestos.

    Cada peso del grafo es una referencia externa; aqui se le da su valor:
    el tensor del archivo tal cual, traspuesto (`op` "T") o con otra forma
    ("R"), en el tipo del grafo (los pesos oficiales van en float16 y la red
    trabaja en float32: el paso es exacto).
    """
    import onnxruntime as ort

    weights = read_safetensors(weights_file)
    names, values = [], []
    for item in manifest["initializers"]:
        value = weights[item["source"]].astype(item.get("dtype", "float32"))
        if item.get("op") == "T":
            value = value.T
        elif item.get("op") == "R":
            value = value.reshape(item["reshape"])
        names.append(item["name"])
        values.append(ort.OrtValue.ortvalue_from_numpy(np.ascontiguousarray(value)))
    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    # Sin el plan de memoria de ORT: reserva de golpe todo lo que la red
    # tendra vivo en algun momento, y con el pico pasaba de 3,5 GB a 2.
    options.enable_mem_pattern = False
    # sin decir cuantos hilos, ORT usa los nucleos de verdad (no los logicos,
    # que comparten unidad de calculo); la prioridad baja del proceso ya deja
    # sitio a lo demas
    if n_threads:
        options.intra_op_num_threads = n_threads
    options.add_external_initializers(names, values)
    return ort.InferenceSession(str(graph), options, providers=["CPUExecutionProvider"])


# ------------------------------------------------------------ espectrograma


def _stft(x: np.ndarray) -> np.ndarray:
    """torch.stft(n_fft=4096, hop=1024, hann, normalized, center, reflect)."""
    pad = NFFT // 2
    x = np.pad(x, [(0, 0)] * (x.ndim - 1) + [(pad, pad)], mode="reflect")
    frames = 1 + (x.shape[-1] - NFFT) // HOP
    idx = np.arange(NFFT)[None, :] + HOP * np.arange(frames)[:, None]
    z = np.fft.rfft(x[..., idx] * WINDOW, axis=-1) / math.sqrt(NFFT)
    return np.swapaxes(z, -1, -2)


def _istft(z: np.ndarray, length: int) -> np.ndarray:
    """torch.istft con los mismos parametros, recortada a `length`."""
    frames = z.shape[-1]
    fr = np.fft.irfft(np.swapaxes(z, -1, -2), n=NFFT, axis=-1).astype(np.float32)
    fr *= WINDOW * np.float32(math.sqrt(NFFT))
    k = NFFT // HOP
    fr = fr.reshape((*fr.shape[:-1], k, HOP))
    out = np.zeros((*z.shape[:-2], frames + k - 1, HOP), dtype=np.float32)
    square = (WINDOW**2).reshape(k, HOP)
    env = np.zeros((frames + k - 1, HOP), dtype=np.float32)
    for j in range(k):  # cada ventana son cuatro saltos: se suman desplazados
        out[..., j : j + frames, :] += fr[..., :, j, :]
        env[j : j + frames] += square[j]
    start = NFFT // 2
    out = out.reshape((*out.shape[:-2], -1))[..., start : start + length]
    env = env.reshape(-1)[start : start + length]
    return out / np.where(env > 1e-11, env, np.float32(1))


def spec(x: np.ndarray) -> np.ndarray:
    """HTDemucs._spec y _magnitude: (2, L) -> (4, 2048, frames), la parte
    real y la imaginaria de cada canal como canales."""
    length = x.shape[-1]
    le = math.ceil(length / HOP)
    pad = HOP // 2 * 3
    x = np.pad(x, [(0, 0), (pad, pad + le * HOP - length)], mode="reflect")
    z = _stft(x)[..., :-1, 2 : 2 + le]
    channels = z.shape[0]
    return (
        np.stack([np.real(z), np.imag(z)], axis=1)
        .reshape(channels * 2, FREQS, le)
        .astype(np.float32)
    )


def ispec(spectrum: np.ndarray, length: int) -> np.ndarray:
    """HTDemucs._mask (cac) y _ispec: (S, 4, 2048, frames) -> (S, 2, length)."""
    sources = spectrum.shape[0]
    parts = spectrum.reshape(sources, 2, 2, FREQS, -1)
    z = parts[:, :, 0] + 1j * parts[:, :, 1]
    z = np.pad(z, [(0, 0), (0, 0), (0, 1), (2, 2)])
    pad = HOP // 2 * 3
    le = HOP * math.ceil(length / HOP) + 2 * pad
    return _istft(z, le)[..., pad : pad + length]


# ------------------------------------------------------------ la cancion

# lo que falta por hacer se cuenta con esto: (trozos hechos, trozos en total)
Progress = Callable[[int, int], None]


def _triangle() -> np.ndarray:
    """El peso con que se funden los trozos: sube hasta el centro y baja."""
    half = SEGMENT // 2
    up = np.arange(1, half + 1, dtype=np.float32)
    down = np.arange(SEGMENT - half, 0, -1, dtype=np.float32)
    weight = np.concatenate([up, down])
    return weight / weight.max()


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
    minutos son 640 MB).

    Mientras la red calcula un trozo (sin el GIL), un hilo deshace el
    espectrograma del anterior y lo funde: un diez por ciento menos.

    La mezcla se normaliza en su sitio si es float32: `wav` no se vuelve a
    usar, y una cancion de cinco minutos son 100 MB que no hace falta copiar.
    """
    reference = wav.mean(0)
    mean = float(reference.mean())
    std = float(reference.std(ddof=1)) + 1e-8 if reference.size > 1 else 1.0
    del reference
    mix = wav if wav.dtype == np.float32 and wav.flags.writeable else wav.astype(np.float32)
    mix -= mean
    mix /= std
    length = mix.shape[-1]
    weight = _triangle()
    offsets = list(range(0, length, STRIDE)) or [0]
    acc: np.ndarray | None = None
    acc_w = np.zeros(SEGMENT, dtype=np.float32)
    base = 0  # la muestra de la cancion que es acc[..., 0]

    def fold(index: int, spectrum: np.ndarray, wave: np.ndarray) -> None:
        nonlocal acc, base
        offset = offsets[index]
        size = min(SEGMENT, length - offset)
        res = (wave + ispec(spectrum, SEGMENT))[..., :SEGMENT]
        delta = SEGMENT - size
        res = res[..., delta // 2 : delta // 2 + size]  # center_trim
        if acc is None:
            acc = np.zeros((res.shape[0], 2, SEGMENT), dtype=np.float32)
        at = offset - base
        acc[..., at : at + size] += weight[:size] * res
        acc_w[at : at + size] += weight[:size]
        # lo que queda antes del siguiente trozo ya no lo toca nadie
        last = index == len(offsets) - 1
        ready = (length if last else offsets[index + 1]) - base
        out = acc[..., :ready] / acc_w[:ready]
        emit(out * std + mean)
        acc = np.concatenate([acc[..., ready:], np.zeros_like(acc[..., :ready])], axis=-1)
        acc_w[:] = np.concatenate([acc_w[ready:], np.zeros(ready, dtype=np.float32)])
        base += ready

    worker: threading.Thread | None = None
    failure: list[BaseException] = []

    def fold_safely(*args):
        try:
            fold(*args)
        except BaseException as e:  # noqa: BLE001 - se relanza en el hilo principal
            failure.append(e)

    for index, offset in enumerate(offsets):
        if cancelled and cancelled():
            break
        size = min(SEGMENT, length - offset)
        delta = SEGMENT - size
        start = offset - delta // 2  # TensorChunk.padded: centrado, con vecinos
        lo, hi = max(0, start), min(length, start + SEGMENT)
        chunk = np.pad(mix[:, lo:hi], [(0, 0), (lo - start, start + SEGMENT - hi)])
        spectrum, wave = session.run(None, {"mix": chunk[None], "mag": spec(chunk)[None]})
        if worker is not None:
            worker.join()
        if failure:
            raise failure[0]
        worker = threading.Thread(target=fold_safely, args=(index, spectrum[0], wave[0]))
        worker.start()
        if progress:
            progress(index + 1, len(offsets))
    if worker is not None:
        worker.join()
    if failure:
        raise failure[0]


# ------------------------------------------------------------ el proceso


def _say(**message) -> None:
    sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _decode(ffmpeg: str, path: str) -> np.ndarray:
    """La cancion entera, en estereo a 44,1 kHz y en flotante."""
    r = subprocess.run(
        [
            ffmpeg,
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-i",
            path,
            "-vn",
            "-f",
            "f32le",
            "-ac",
            "2",
            "-ar",
            str(RATE),
            "-",
        ],
        capture_output=True,
        check=False,
    )
    if r.returncode != 0 or len(r.stdout) < 8:
        detail = r.stderr.decode("utf-8", "replace").strip().splitlines()
        raise RuntimeError("ffmpeg no pudo leer la cancion" + (f": {detail[-1]}" if detail else ""))
    data = np.frombuffer(r.stdout, dtype=np.float32)
    return data[: data.size // 2 * 2].reshape(-1, 2).T.copy()


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


class Waves:
    """La forma de onda de cada pista, calculada mientras se escribe.

    `buckets` columnas a lo largo de la cancion, con pico y RMS. Todas las
    pistas a la misma escala (la del pico mas alto de todas): en el
    mezclador se ve que instrumento suena mas y cual casi no esta, que es
    justo lo que interesa. La onda de la cancion entera va aparte y cada una
    a su escala; esta no.
    """

    def __init__(self, sources: list[str], length: int, buckets: int = 800):
        self.sources = sources
        self.buckets = max(1, min(buckets, length))
        self.edges = (np.arange(self.buckets + 1, dtype=np.int64) * length) // self.buckets
        self.peaks = np.zeros((len(sources), self.buckets), dtype=np.float32)
        self.energy = np.zeros((len(sources), self.buckets), dtype=np.float64)
        self.at = 0

    def add(self, block: np.ndarray) -> None:
        """Un tramo (S, 2, n) que empieza donde acabo el anterior."""
        n = block.shape[-1]
        if not n:
            return
        start, end = self.at, self.at + n
        self.at = end
        first = int(np.searchsorted(self.edges, start, side="right") - 1)
        last = int(np.searchsorted(self.edges, end - 1, side="right") - 1)
        cuts = np.clip(self.edges[first : last + 1] - start, 0, n)
        cuts[0] = 0
        loud = np.abs(block).max(axis=1)  # (S, n): el canal mas alto
        power = (block.astype(np.float64) ** 2).mean(axis=1)
        peaks = np.maximum.reduceat(loud, cuts, axis=1)
        energy = np.add.reduceat(power, cuts, axis=1)
        span = slice(first, last + 1)
        self.peaks[:, span] = np.maximum(self.peaks[:, span], peaks)
        self.energy[:, span] += energy

    def result(self, sources: list[str] | None = None) -> dict:
        """Las de `sources` (todas si no se dice), a la escala de la mas alta de ellas."""
        rows = [self.sources.index(s) for s in (self.sources if sources is None else sources)]
        counts = np.maximum(1, np.diff(self.edges)).astype(np.float64)
        rms = np.sqrt(self.energy / counts)
        top = float(self.peaks[rows].max()) if rows else 0.0
        top = top or 1.0
        return {
            "buckets": self.buckets,
            "tracks": {
                self.sources[i]: {
                    "peaks": [round(float(v), 3) for v in np.minimum(1, self.peaks[i] / top)],
                    "rms": [round(float(v), 3) for v in np.minimum(1, rms[i] / top)],
                }
                for i in rows
            },
        }


# ------------------------------------------------------------ que pistas se quedan

# Una fuente que la red no encuentra en la cancion no sale callada: sale con
# lo que se le cuela de las demas, muy por debajo de la mezcla. Se queda si en
# conjunto suena a mas de QUIET_DB de la cancion, o si llega a LOUD_DB de ella
# en al menos ACTIVE de lo que suena (un piano que solo entra en el puente).
# Medido con scripts/evaluar-separador.py --presencia: en canciones de verdad
# lo que no esta sale de -44 a -63 dB y lo que esta, por encima de -20.
QUIET_DB = -30.0
LOUD_DB = -15.0
ACTIVE = 0.02
# los tramos en que se mira: un cuarto de segundo, sea larga o corta la cancion
METER = RATE // 4


def meter(sources: list[str], length: int) -> "Waves":
    """Lo que suena cada una por tramos de un cuarto de segundo (`presence`)."""
    return Waves(sources, length, buckets=max(1, length // METER))


def presence(energy: np.ndarray, mix: np.ndarray) -> dict:
    """Cuanto suena una fuente en la cancion, por tramos (`Waves.energy`):
    `db`, su energia frente a la de la mezcla; `active`, la parte de lo que
    suena de la cancion en que la fuente llega a LOUD_DB de ella."""
    total = float(mix.sum())
    if total <= 0:
        return {"db": -120.0, "active": 0.0}
    db = 10 * math.log10(max(float(energy.sum()), total * 1e-12) / total)
    audible = mix > total / mix.size * 1e-3  # los silencios no cuentan
    loud = energy > mix * 10 ** (LOUD_DB / 10)
    active = float((loud & audible).sum()) / max(1, int(audible.sum()))
    return {"db": round(db, 1), "active": round(active, 3)}


def keeps(level: dict) -> bool:
    """Si una fuente con ese `presence` es un instrumento que esta de verdad."""
    return level["db"] > QUIET_DB or level["active"] >= ACTIVE


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


def run(job: dict, open_net=open_session) -> dict:
    """Pasa la cancion por las redes del trabajo y deja sus pistas en
    `job["output"]`, con su forma de onda en `job["waves"]`.

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

    Devuelve las pistas que salen ({fuente: archivo}) y lo que suena cada una.
    """
    started = time.monotonic()
    ffmpeg = job["ffmpeg"]
    out = Path(job["output"])
    out.mkdir(parents=True, exist_ok=True)
    files: dict[str, str] = job["files"]
    rest = job.get("rest", "other")
    carry = {s: Path(p) for s, p in (job.get("carry") or {}).items()}
    nets = job.get("nets") or []
    _say(step="decode")
    wav = _decode(ffmpeg, job["input"])
    length = wav.shape[-1]
    segments = len(range(0, length, STRIDE)) or 1
    made: dict[str, Path] = {}
    heard: dict[str, dict] = {}
    _say(step="separate", seconds=round(length / RATE, 2))
    for n, net in enumerate(nets):
        _say(step="load", net=n + 1, nets=len(nets))
        manifest = json.loads(Path(net["manifest"]).read_text(encoding="utf-8"))
        session = open_net(net["graph"], manifest, net["weights"], job.get("threads"))
        take = [s for s in net["take"] if s in manifest["sources"]]
        index = [manifest["sources"].index(s) for s in take]
        levels = meter([*take, "mix"], length)
        encoders = {s: _encoder(ffmpeg, out / files[s]) for s in take}
        at = 0

        def emit(block: np.ndarray, _index=index, _take=take, _levels=levels, _enc=encoders):
            nonlocal at
            size = block.shape[-1]
            kept = block[_index]
            _levels.add(np.concatenate([kept, wav[None, :, at : at + size]]))
            at += size
            for i, source in enumerate(_take):
                _write(_enc[source], kept[i])

        base = n * segments
        try:
            # una copia: la red la normaliza en su sitio, y la cancion hace
            # falta entera para la siguiente y para sacar lo que queda
            separate(
                session,
                wav.copy(),
                emit,
                lambda d, _t, _base=base: _say(done=_base + d, total=segments * len(nets)),
            )
        finally:
            _close(encoders, "guardar las pistas")
        del session
        made.update({s: out / files[s] for s in take})
        if n == 0:
            heard.update(
                {s: presence(levels.energy[i], levels.energy[-1]) for i, s in enumerate(take)}
            )

    keep = job.get("keep")

    def stays(source: str) -> bool:
        if keep is not None:
            return source in keep
        # lo que llega hecho (`carry`) ya se decidio en su dia
        return source not in heard or keeps(heard[source])

    parts = [s for s in files if s != rest and (s in made or s in carry) and stays(s)]
    for source, path in made.items():
        if source not in parts:
            path.unlink(missing_ok=True)

    # lo que queda: la cancion menos las pistas que se quedan, tal y como
    # quedaron guardadas (asi la suma es exacta)
    _say(step="compose")
    inputs = {s: made.get(s) or carry[s] for s in parts}
    final = Waves([*parts, rest], length)
    stats = meter([rest, "mix"], length)
    readers = {s: _reader(ffmpeg, path) for s, path in inputs.items()}
    left = {rest: _encoder(ffmpeg, out / files[rest])}
    try:
        for a in range(0, length, CHUNK):
            size = min(CHUNK, length - a)
            mix = wav[:, a : a + size]
            got = [_read(readers[s], size) for s in parts]
            remainder = mix - sum(got, np.zeros_like(mix))
            _write(left[rest], remainder)
            final.add(np.stack([*got, remainder]))
            stats.add(np.stack([remainder, mix]))
    except BaseException:
        for proc in [*readers.values(), *left.values()]:
            proc.kill()  # lo que fallo es lo que cuenta, no que se cortaran
        raise
    for reader in readers.values():
        assert reader.stdout is not None
        reader.stdout.read()  # lo que sobre, que no se quede a medias
    _close(readers, "leer las pistas")
    _close(left, "guardar lo que queda")
    heard[rest] = presence(stats.energy[0], stats.energy[1])
    if not stays(rest):
        (out / files[rest]).unlink(missing_ok=True)
    order = [s for s in files if s in parts or (s == rest and stays(rest))]
    if job.get("waves"):
        Path(job["waves"]).write_text(
            json.dumps(final.result(order), separators=(",", ":")), encoding="utf-8"
        )
    written = {s: out / files[s] for s in order}
    if job.get("format") == "opus":
        _say(step="encode")
        written = {s: _to_opus(ffmpeg, path) for s, path in written.items()}
    return {
        "seconds": round(length / RATE, 2),
        "took": round(time.monotonic() - started, 1),
        "tracks": {s: written[s].name for s in order},
        "levels": heard,
    }


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
        result = run(job)
    except Exception as e:  # noqa: BLE001 - se cuenta al proceso padre
        _say(error=str(e) or e.__class__.__name__)
        return 1
    _say(ok=True, **result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
