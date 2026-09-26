#!/usr/bin/env python3
"""Cuanto separa bien cada combinacion de modelos del separador, y cuanto tarda.

    uv run --no-project -p 3.13 --with musdb --with onnxruntime --with onnx \\
        python scripts/evaluar-separador.py [--pistas 50] [--int8]

Mide con las mismas piezas que la app (`danplay/separation.py`, los grafos
de `danplay/data/separador/` y los pesos oficiales) sobre las muestras de 7
segundos de MUSDB18: las 50 canciones de su parte de prueba, que ningun
modelo de Demucs vio al entrenar. Esas muestras (150 MB) se bajan la primera
vez a `--datos` y solo sirven para medir: la app no las lleva ni las usa.

Por cada combinacion y cada fuente (bateria, bajo, voces y el resto) sale el
SDR, lo que la pista estimada se parece a la de verdad, en dB: la energia de
la pista de verdad entre la del error, como en el reto MDX. Mas es mejor; un
dB se nota. Tambien el de la cancion «sin bateria» (lo que suena al callar
la bateria, que es para lo que la quiere un baterista) y «sin voces».

Y el tiempo: cuanto tarda esa combinacion por segundo de cancion en esta
maquina (0,5 es la mitad de lo que dura), y lo que supone para una de 4:30.

Las combinaciones:

- `6s`: htdemucs_6s solo, lo de 1.16.0 (guitarra y piano van en «el resto»).
- `4`: htdemucs solo.
- `ft`: htdemucs_ft, un especialista afinado por fuente (cuatro redes).
- `ft3`: los especialistas de bateria, bajo y voces; el resto, lo que queda
  de la mezcla (la mezcla menos esas tres), sin la cuarta red.
- `app`: la de la app: la rapida (6s), y la bateria y el bajo de sus
  especialistas.
- `... =`: la misma combinacion con el resto como lo que queda de la mezcla:
  asi las pistas juntas suenan exactamente como la cancion.
- con `--int8`, las redes cuantizadas a 8 bits (dinamico, solo las MatMul).

Con `--presencia` [canciones...] no mide el SDR: mira que pistas se quedaria
la app (las fuentes que casi no suenan no llegan a pista) y lo compara con lo
que suena de verdad en cada muestra; con canciones, lo enseña para mirarlo.
"""

import argparse
import importlib.util
import json
import math
import os
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
GRAPHS = ROOT / "danplay" / "data" / "separador"
# el motor tal cual, sin el paquete (que tira de la configuracion de la app)
_spec = importlib.util.spec_from_file_location("engine", ROOT / "danplay" / "separation.py")
assert _spec and _spec.loader
engine = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(engine)

MUSDB = ("drums", "bass", "other", "vocals")
NAMES = {"drums": "batería", "bass": "bajo", "other": "resto", "vocals": "voces"}

# Los especialistas de htdemucs_ft: la red de htdemucs con otros pesos, uno
# por fuente (el yaml de la bolsa pone a cada uno solo para la suya).
FT_REPO = "https://huggingface.co/adefossez/HTDemucs-ft/resolve/main"
FT = {"drums": "f7e0c4bc", "bass": "d12395a8", "other": "92cfc3b6", "vocals": "04573f0d"}


# ------------------------------------------------------------ pesos y redes


def fetch(url: str, target: Path) -> Path:
    if target.is_file():
        return target
    target.parent.mkdir(parents=True, exist_ok=True)
    print(f"  bajando {target.name}…", flush=True)
    part = target.with_suffix(".part")
    req = urllib.request.Request(url, headers={"User-Agent": "DanPlay (evaluacion)"})
    with urllib.request.urlopen(req, timeout=60) as r, open(part, "wb") as f:
        while block := r.read(1 << 20):
            f.write(block)
    os.replace(part, target)
    return target


class Net:
    """Una red lista para separar: grafo, receta de pesos y archivo de pesos."""

    def __init__(self, label: str, graph: str, weights_url: str, weights: Path, int8: bool):
        self.label = label
        self.graph = GRAPHS / f"{graph}.onnx"
        self.manifest = json.loads((GRAPHS / f"{graph}.json").read_text(encoding="utf-8"))
        self.sources = self.manifest["sources"]
        self.weights = fetch(weights_url, weights)
        self.int8 = int8

    def session(self, scratch: Path, threads: int | None):
        if not self.int8:
            return engine.open_session(self.graph, self.manifest, self.weights, threads)
        return quantized_session(self, scratch, threads)


def quantized_session(net: Net, scratch: Path, threads: int | None):
    """La misma red con las MatMul en 8 bits: el grafo con sus pesos dentro,
    cuantizado con ONNX Runtime y guardado aparte (no cambia nada de la app)."""
    import onnx
    import onnxruntime as ort
    from onnx import numpy_helper
    from onnxruntime.quantization import QuantType, quantize_dynamic

    target = scratch / f"{net.label.replace(' ', '_')}.int8.onnx"
    if not target.is_file():
        weights = engine.read_safetensors(net.weights)
        values = {}
        for item in net.manifest["initializers"]:
            value = weights[item["source"]].astype(item.get("dtype", "float32"))
            if item.get("op") == "T":
                value = value.T
            elif item.get("op") == "R":
                value = value.reshape(item["reshape"])
            values[item["name"]] = np.ascontiguousarray(value)
        proto = onnx.load(str(net.graph), load_external_data=False)
        for tensor in proto.graph.initializer:
            if tensor.name in values:
                tensor.CopyFrom(numpy_helper.from_array(values[tensor.name], tensor.name))
        full = scratch / f"{target.stem}.f32.onnx"
        onnx.save_model(proto, str(full))
        quantize_dynamic(
            str(full), str(target), weight_type=QuantType.QInt8, op_types_to_quantize=["MatMul"]
        )
        full.unlink()
    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    options.enable_mem_pattern = False
    if threads:
        options.intra_op_num_threads = threads
    return ort.InferenceSession(str(target), options, providers=["CPUExecutionProvider"])


# ------------------------------------------------------------ medir


def sdr(ref: np.ndarray, est: np.ndarray) -> float:
    """El SDR del reto MDX: 10·log10(Σref² / Σ(ref−est)²), sobre la pista entera."""
    num = float(np.sum(ref.astype(np.float64) ** 2)) + 1e-8
    den = float(np.sum((ref.astype(np.float64) - est) ** 2)) + 1e-8
    return 10 * math.log10(num / den)


def run_net(net: Net, tracks, scratch: Path, threads: int | None, keep: list[str]):
    """Separa todas las muestras con una red. Devuelve {fuente: [array por
    muestra]} solo de las fuentes de `keep` (en float16: sobra para medir y
    ocupa la mitad), y lo que tarda por segundo de cancion.

    El tiempo se mide aparte, con un minuto de ruido: las muestras son mas
    cortas que dos trozos y lo que tarda una no dice lo que tarda una
    cancion (lo que tarda la red no depende de lo que suene)."""
    session = net.session(scratch, threads)
    # la primera vuelta calienta ORT (reserva, hilos): no cuenta
    engine.separate(session, np.zeros((2, engine.SEGMENT), np.float32), lambda b: None)
    noise = np.random.default_rng(0).normal(0, 0.1, (2, 60 * engine.RATE)).astype(np.float32)
    start = time.perf_counter()
    engine.separate(session, noise, lambda b: None)
    per_second = (time.perf_counter() - start) / 60
    out: dict[str, list[np.ndarray]] = {s: [] for s in keep}
    for i, track in enumerate(tracks):
        mix = track.audio.T.astype(np.float32)
        blocks: list[np.ndarray] = []
        engine.separate(session, mix, blocks.append)
        est = np.concatenate(blocks, axis=-1)
        for s in keep:
            if s == "rest6":  # guitarra + piano + el resto del de seis
                idx = [net.sources.index(x) for x in ("other", "guitar", "piano")]
                out[s].append(est[idx].sum(0).astype(np.float16))
            else:
                out[s].append(est[net.sources.index(s)].astype(np.float16))
        print(f"\r  {net.label}: {i + 1}/{len(tracks)}", end="", flush=True)
    print(f"  ({per_second:.2f} s por segundo de canción)")
    del session
    return out, per_second


def score(tracks, est: dict[str, list[np.ndarray]]) -> dict:
    """SDR por fuente (media y mediana entre las muestras en que suena), y de
    la cancion sin bateria y sin voces tal como se oiria."""
    per: dict[str, list[float]] = {s: [] for s in (*MUSDB, "sin batería", "sin voces")}
    for i, track in enumerate(tracks):
        refs = {s: track.targets[s].audio.T for s in MUSDB}
        mix_energy = float(np.sum(track.audio**2))
        for s in MUSDB:
            # una fuente que no suena en la muestra no se puntua (su SDR no
            # dice nada: cualquier cosa sobre cero es infinitamente peor)
            if np.sum(refs[s] ** 2) > 1e-4 * mix_energy:
                per[s].append(sdr(refs[s], est[s][i].astype(np.float32)))
        for label, without in (("sin batería", "drums"), ("sin voces", "vocals")):
            ref = sum(refs[s] for s in MUSDB if s != without)
            got = sum(est[s][i].astype(np.float32) for s in MUSDB if s != without)
            per[label].append(sdr(ref, got))
    return {k: (float(np.mean(v)), float(np.median(v)), len(v)) for k, v in per.items() if v}


def consistent(tracks, est: dict) -> dict:
    """El resto como la mezcla menos las demas: las pistas suman la cancion."""
    rest = []
    for i, track in enumerate(tracks):
        mix = track.audio.T.astype(np.float32)
        others = sum(est[s][i].astype(np.float32) for s in MUSDB if s != "other")
        rest.append((mix - others).astype(np.float16))
    return {**est, "other": rest}


# ------------------------------------------------------------ que pistas se quedan


def presence_report(net: "Net", tracks, songs: list[Path], scratch: Path, threads) -> None:
    """Lo que decide la app de cada fuente con la red rapida (`presence` y
    `keeps` de separation.py): en MUSDB, frente a si la fuente suena de
    verdad en la muestra (solo bateria, bajo y voces: guitarra y piano no
    vienen aparte); y en las canciones que se den, para mirarlo a mano."""
    session = net.session(scratch, threads)
    take = [s for s in net.sources if s != "other"]

    def levels(mix: np.ndarray) -> dict:
        blocks: list[np.ndarray] = []
        engine.separate(session, mix.copy(), blocks.append)
        est = np.concatenate(blocks, axis=-1)[[net.sources.index(s) for s in take]]
        rest = mix - est.sum(0)
        meter = engine.meter([*take, "other", "mix"], mix.shape[-1])
        meter.add(np.concatenate([est, rest[None], mix[None]]))
        e = meter.energy
        return {s: engine.presence(e[i], e[-1]) for i, s in enumerate([*take, "other"])}

    wrong = {"quitada y suena": [], "sobra": []}
    rows = []
    for i, track in enumerate(tracks):
        mix = track.audio.T.astype(np.float32)
        got = levels(mix)
        total = float(np.sum(mix**2))
        for s in ("drums", "bass", "vocals"):
            ref = 10 * math.log10(max(float(np.sum(track.targets[s].audio ** 2)), 1e-12) / total)
            kept = engine.keeps(got[s])
            rows.append((s, ref, got[s], kept))
            if ref > -25 and not kept:
                wrong["quitada y suena"].append(f"{track.name} {s} ({ref:.0f} dB)")
            if ref < -40 and kept:
                wrong["sobra"].append(f"{track.name} {s} ({ref:.0f} dB)")
        print(f"\r  presencia en MUSDB: {i + 1}/{len(tracks)}", end="", flush=True)
    print()
    for s in ("drums", "bass", "vocals"):
        mine = [r for r in rows if r[0] == s]
        absent = [r[2]["db"] for r in mine if r[1] < -40]
        present = [r[2]["db"] for r in mine if r[1] > -25]
        print(
            f"  {NAMES[s]:8s} suena en {len(present)}: estimada {min(present, default=0):6.1f} dB "
            f"la mas baja · no suena en {len(absent)}: {max(absent, default=-120):6.1f} dB la mas alta"
        )
    for what, items in wrong.items():
        print(f"  {what}: {len(items)} {items[:6]}")
    for song in songs:
        mix = engine._decode("ffmpeg", str(song))
        got = levels(mix)
        cols = "  ".join(
            f"{s}{'' if engine.keeps(v) else '✗'} {v['db']:.0f}/{v['active']:.2f}"
            for s, v in got.items()
        )
        print(f"  {song.name[:40]:40s} {cols}")


# ------------------------------------------------------------ la tabla


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    default = ROOT / ".dev" / "evaluacion-separador"
    ap.add_argument("--datos", type=Path, default=default, help="muestras y pesos")
    ap.add_argument("--pistas", type=int, default=50, help="cuantas muestras (de 50)")
    ap.add_argument("--hilos", type=int, default=None, help="hilos de ONNX Runtime")
    ap.add_argument("--int8", action="store_true", help="medir tambien las redes en 8 bits")
    ap.add_argument("--solo", default="", help="solo estas combinaciones, con comas")
    ap.add_argument(
        "--presencia", action="store_true", help="que pistas se quedarian, en vez del SDR"
    )
    ap.add_argument("canciones", nargs="*", type=Path, help="con --presencia, canciones a mirar")
    args = ap.parse_args()

    import musdb

    tracks = list(musdb.DB(root=str(args.datos / "musdb"), download=True, subsets="test"))
    tracks = tracks[: args.pistas]
    print(f"{len(tracks)} muestras de MUSDB18 ({tracks[0].audio.shape[0] / 44100:.1f} s cada una)")
    pesos = args.datos / "pesos"

    def m6(int8=False):
        info = json.loads((GRAPHS / "htdemucs_6s.json").read_text())["weights"]
        tag = " int8" if int8 else ""
        return Net(f"6s{tag}", "htdemucs_6s", info["url"], pesos / info["file"], int8)

    def m4(int8=False):
        info = json.loads((GRAPHS / "htdemucs.json").read_text())["weights"]
        tag = " int8" if int8 else ""
        return Net(f"4{tag}", "htdemucs", info["url"], pesos / info["file"], int8)

    def spec(source, int8=False):
        sig = FT[source]
        tag = " int8" if int8 else ""
        url = f"{FT_REPO}/{sig}.safetensors"
        return Net(f"ft {source}{tag}", "htdemucs", url, pesos / f"{sig}.safetensors", int8)

    if args.presencia:
        with tempfile.TemporaryDirectory(prefix="evaluar-separador-") as tmp:
            presence_report(m6(), tracks, args.canciones, Path(tmp), args.hilos)
        return 0

    # Cada combinacion: de que red sale cada fuente, y si «el resto» es lo que
    # queda de la mezcla (=). Las redes se pasan una vez aunque las usen varias.
    combos = {
        "6s": ({"drums": "6s", "bass": "6s", "vocals": "6s", "other": "6s"}, False),
        "6s =": ({"drums": "6s", "bass": "6s", "vocals": "6s"}, True),
        "4": (dict.fromkeys(MUSDB, "4"), False),
        "4 =": ({s: "4" for s in MUSDB if s != "other"}, True),
        "ft": ({s: f"ft {s}" for s in MUSDB}, False),
        "ft3 =": ({s: f"ft {s}" for s in MUSDB if s != "other"}, True),
        # la de la app: la rapida, y la bateria y el bajo de sus especialistas
        "app =": ({"drums": "ft drums", "bass": "ft bass", "vocals": "6s"}, True),
    }
    wanted = set(filter(None, args.solo.split(",")))
    chosen = {k: v for k, v in combos.items() if not wanted or k.removesuffix(" =") in wanted}
    if not chosen:
        ap.error(f"--solo: de {', '.join(sorted({k.removesuffix(' =') for k in combos}))}")
    results: list[tuple[str, dict, float]] = []

    with tempfile.TemporaryDirectory(prefix="evaluar-separador-") as tmp:
        scratch = Path(tmp)
        for int8 in (False, True) if args.int8 else (False,):
            tag = " int8" if int8 else ""
            # que fuentes hacen falta de cada red
            need: dict[str, set[str]] = {}
            for sources, _ in chosen.values():
                for source, net in sources.items():
                    need.setdefault(net, set()).add(source)
            nets = {"6s": lambda q=int8: m6(q), "4": lambda q=int8: m4(q)}
            nets |= {f"ft {s}": (lambda s=s, q=int8: spec(s, q)) for s in MUSDB}
            got: dict[str, dict] = {}
            took: dict[str, float] = {}
            for name, sources in need.items():
                keep = sorted(sources)
                if name == "6s" and "other" in keep:  # guitarra + piano + el resto
                    keep = [x if x != "other" else "rest6" for x in keep]
                est, took[name] = run_net(nets[name](), tracks, scratch, args.hilos, keep)
                if "rest6" in est:
                    est["other"] = est.pop("rest6")
                got[name] = est
            for label, (sources, rest) in chosen.items():
                est = {source: got[net][source] for source, net in sources.items()}
                if rest:
                    est = consistent(tracks, est)
                t = sum(took[n] for n in set(sources.values()))
                results.append((label.replace(" =", f"{tag} ="), score(tracks, est), t))

    cols = [*MUSDB, "sin batería", "sin voces"]
    head = f"{'combinación':14s}" + "".join(f"{NAMES.get(c, c):>13s}" for c in cols)
    print("\nSDR en dB, media (mediana) entre las muestras; más es mejor\n")
    print(head + f"{'× canción':>11s}{'4:30 tarda':>12s}")
    for label, res, t in results:
        row = f"{label:14s}"
        for c in cols:
            mean, median, _ = res.get(c, (float("nan"),) * 3)
            row += f"{mean:7.2f} ({median:4.1f})"
        print(row + f"{t:11.2f}{t * 270 / 60:8.1f} min")
    counts = {NAMES.get(c, c): res[c][2] for c in cols if c in results[0][1]}
    print(f"\nmuestras en que suena cada fuente: {counts}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
