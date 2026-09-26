#!/usr/bin/env python3
"""Cuanto acierta cada forma de sacar el pulso y el «1» del metronomo.

    uv run --no-project -p 3.13 --with mir_eval --with onnxruntime --with numpy \\
        python scripts/evaluar-pulso.py [--piezas 300] [--bateria CARPETA]

Mide, con las mismas piezas que la app, sobre GTZAN (1000 fragmentos de 30 s
de diez estilos, con los pulsos y los «1» anotados a mano por GTZAN-Rhythm,
Marchand y Peeters 2015). Beat This! no lo vio al entrenar. Los fragmentos
(1,2 GB) y las anotaciones se bajan la primera vez a `--datos`, solo para
medir: la app no los lleva ni los usa.

- `app`: el analisis de siempre de la app de escritorio (`beats.rs`), con
  la herramienta de medida de sus pruebas (`cargo test ... measure`);
- `beat this`: `danplay.pulse` con el grafo que viaja con la app, tal cual
  sale de la red (`crudo`), con la rejilla que usa la app (sin huecos y con
  el «1» de cada compas, `compases`), o forzando un compas regular
  (`regular`).

La medida es la F del MIREX: un pulso cuenta si cae a menos de 70 ms de uno
anotado; `F «1»`, lo mismo con los «1». Aparte, las de pop, rock, country y
disco, lo mas parecido a la musica de alabanza.

Con `--bateria CARPETA` (una pista de bateria por fragmento, con su mismo
nombre, hecha con el separador de la app) mide tambien sobre la bateria sola,
en los mismos fragmentos: si seguir al metronomo por la pista de bateria
acierta mas que por la cancion.
"""

import argparse
import importlib.util
import json
import os
import subprocess
import sys
import tarfile
import time
import urllib.request
import zipfile
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location("pulse", ROOT / "danplay" / "pulse.py")
assert _spec and _spec.loader
pulse = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pulse)
GRAPH = ROOT / "danplay" / "data" / "pulso" / "beat_this.onnx"

AUDIO = "https://huggingface.co/datasets/marsyas/gtzan/resolve/main/data/genres.tar.gz"
NOTES = "https://github.com/TempoBeatDownbeat/gtzan_tempo_beat/archive/refs/heads/main.zip"
POP = ("pop", "rock", "country", "disco")


def fetch(data: Path) -> None:
    """Los fragmentos y sus anotaciones, si no estan."""
    data.mkdir(parents=True, exist_ok=True)
    if not (data / "genres").is_dir():
        print("bajando GTZAN (1,2 GB)…", flush=True)
        tar = data / "genres.tar.gz"
        urllib.request.urlretrieve(AUDIO, tar)
        with tarfile.open(tar) as t:
            t.extractall(data, filter="data")
        tar.unlink()
    if not (data / "gtzan_tempo_beat-main").is_dir():
        z = data / "anotaciones.zip"
        urllib.request.urlretrieve(NOTES, z)
        with zipfile.ZipFile(z) as f:
            f.extractall(data)  # noqa: S202 - un zip fijo de GitHub, para medir
        z.unlink()


def pieces(data: Path, limit: int | None) -> list[tuple[str, Path, Path]]:
    """(estilo, audio, anotacion), los mismos de cada estilo."""
    out = []
    for genre in sorted(os.listdir(data / "genres")):
        found = []
        for wav in sorted((data / "genres" / genre).glob(f"{genre}.*.wav")):
            num = wav.stem.split(".")[1]
            ann = data / "gtzan_tempo_beat-main" / "beats" / f"gtzan_{genre}_{num}.beats"
            if ann.is_file():
                found.append((genre, wav, ann))
        out += found[: limit // 10] if limit else found
    return out


def reference(ann: Path) -> tuple[np.ndarray, np.ndarray]:
    rows = [line.split() for line in ann.read_text().splitlines() if line.strip()]
    beats = np.array([float(r[0]) for r in rows])
    downs = np.array([float(r[0]) for r in rows if len(r) > 1 and r[1].split(".")[0] == "1"])
    return beats, downs


def run_app(files: list[Path], work: Path) -> dict:
    """`beats.rs`, con la herramienta de medida de sus pruebas."""
    lista, salida = work / "lista.txt", work / "app.jsonl"
    lista.write_text("\n".join(str(f) for f in files))
    env = {
        **os.environ,
        "DANPLAY_PULSO_LISTA": str(lista),
        "DANPLAY_PULSO_SALIDA": str(salida),
        "PYO3_BUILD_EXTENSION_MODULE": "1",
    }
    subprocess.run(
        [
            *("cargo", "test", "--release", "-p", "danplay-app", "beats::tests::measure"),
            *("--", "--ignored", "--exact"),
        ],
        cwd=ROOT,
        env=env,
        check=True,
        capture_output=True,
    )
    got = {}
    for line in salida.read_text().splitlines():
        d = json.loads(line)
        got[d["path"]] = {"crudo": (d.get("beats", []), d.get("downbeats", []))}
    return got


def run_beat_this(files: list[Path]) -> tuple[dict, float]:
    session = pulse.open_session(GRAPH)
    got, took = {}, 0.0
    for i, f in enumerate(files):
        try:
            mono = pulse._decode("ffmpeg", str(f))
        except RuntimeError:  # jazz.00054 de GTZAN esta roto
            got[str(f)] = {}
            continue
        start = time.perf_counter()
        beat, down = pulse.logits(session, pulse.spectrogram(mono))
        took += time.perf_counter() - start
        b, d = pulse.beats_of(beat, down)
        variants = {"crudo": (b, d)}
        if len(b) >= 4:
            g = pulse.grid(b, d, mono.size / pulse.RATE, down)
            full = np.array(g["beats"])
            variants["compases"] = (full, full[g["bars"]] if g["bars"] else np.array([]))
            m = g["meter"]
            phase = g["phase3"] if m % 3 == 0 else g["phase4"]
            variants["regular"] = (
                full,
                full[[i for i in range(len(full)) if (i - phase) % m == 0]],
            )
        got[str(f)] = variants
        print(f"\r  beat this: {i + 1}/{len(files)}", end="", flush=True)
    print()
    return got, took / max(1, len(files))


def score(data, got, label, into):
    import mir_eval

    def f(ref, est):
        ref = mir_eval.beat.trim_beats(np.asarray(ref, float))
        est = mir_eval.beat.trim_beats(np.asarray(est, float))
        return mir_eval.beat.f_measure(ref, est) if len(ref) else np.nan

    rows: dict[str, list] = {}
    for genre, audio, ann in data:
        ref_b, ref_d = reference(ann)
        for variant, (b, d) in got.get(str(audio), {}).items():
            rows.setdefault(variant, []).append((genre, f(ref_b, b), f(ref_d, d)))
    for variant, r in rows.items():
        allr = np.array([x[1:] for x in r], dtype=float)
        pop = np.array([x[1:] for x in r if x[0] in POP], dtype=float)
        m, p = np.nanmean(allr, axis=0), np.nanmean(pop, axis=0)
        into.append((f"{label} · {variant}", m[0], m[1], p[0], p[1], len(r)))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--datos", type=Path, default=ROOT / ".dev" / "evaluacion-pulso")
    ap.add_argument("--piezas", type=int, default=300, help="cuantas (las mismas de cada estilo)")
    ap.add_argument("--bateria", type=Path, default=None, help="las pistas de bateria, si se miden")
    args = ap.parse_args()
    fetch(args.datos)
    data = pieces(args.datos, args.piezas)
    work = args.datos / "trabajo"
    work.mkdir(exist_ok=True)
    rows: list = []
    sets = [("canción", data)]
    if args.bateria:
        drums = [
            (g, args.bateria / a.name, n) for g, a, n in data if (args.bateria / a.name).is_file()
        ]
        names = {a.name for _, a, _ in drums}
        sets = [("canción", [x for x in data if x[1].name in names]), ("batería", drums)]
    speed = 0.0
    for label, items in sets:
        files = [a for _, a, _ in items]
        print(f"{label}: {len(files)} fragmentos", flush=True)
        score(items, run_app(files, work), f"app ({label})", rows)
        got, speed = run_beat_this(files)
        score(items, got, f"beat this ({label})", rows)
    print("\nF de MIREX (±70 ms), media de los fragmentos; más es mejor\n")
    print(f"{'':32s}{'pulso':>8s}{'«1»':>8s}   {'pop/rock…':>10s}{'«1»':>8s}")
    for label, fb, fd, pb, pd, n in rows:
        print(f"{label:32s}{fb:8.3f}{fd:8.3f}   {pb:10.3f}{pd:8.3f}   ({n})")
    print(f"\nBeat This!: {speed:.2f} s de red por fragmento de 30 s en esta máquina")
    return 0


if __name__ == "__main__":
    sys.exit(main())
