#!/usr/bin/env python3
"""Revisa el metronomo cancion por cancion sobre tu propia biblioteca.

    uv run python scripts/revisar-metronomo.py [--db RUTA] [--canciones N] [--salida informe.jsonl]

Pasa la red de pulso (Beat This!, la misma de la app) por cada cancion de la
base, SOLO LEYENDOLA, y guarda lo crudo en `.dev/revision-metronomo/` (la
segunda vez no se repite). Luego mira, en cada una, lo que la red hace mal y
como queda la rejilla que suena (`pulse.grid`):

- tramos de la red a otro nivel (el pulso al doble o a la mitad durante
  unos compases) y golpes sueltos de mas o de menos: lo que hacia que el
  clic se acelerase o metiera golpes donde no van;
- los compases de la rejilla que no son del compas de la cancion;
- los tramos sin clic (no hay pulso de verdad: una intro libre, un final);
- donde cae el clic respecto a los golpes de la propia musica (el flujo
  espectral: lo que sube de una trama a la siguiente, 200 veces por segundo,
  calibrado con golpes sinteticos en instantes exactos) y cuanto baila.

Al final, un resumen y las que peor salen, para escucharlas.
"""

import argparse
import hashlib
import json
import os
import shutil
import sqlite3
import sys
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from danplay import pulse  # noqa: E402

GRAPH = ROOT / "danplay" / "data" / "pulso" / "beat_this.onnx"
SR = 22050
HOP = 110  # el flujo, ~200 veces por segundo
NFFT = 1024
ENV_FPS = SR / HOP
# el flujo (ventana contada desde su principio) se dispara 39,2 ms antes del
# golpe: medido con golpes sinteticos en instantes exactos
ENV_LAG = 0.0392


def flux(mono: np.ndarray) -> np.ndarray:
    """Lo que sube el espectro de una trama a la siguiente: un pico en cada golpe."""
    win = np.hanning(NFFT).astype(np.float32)
    frames = 1 + (mono.size - NFFT) // HOP
    out = np.zeros(frames, dtype=np.float32)
    prev = None
    for a in range(0, frames, 4096):
        b = min(frames, a + 4096)
        idx = np.arange(NFFT)[None, :] + HOP * np.arange(a, b)[:, None]
        mag = np.log1p(100 * np.abs(np.fft.rfft(mono[idx] * win, axis=-1)))
        d = np.diff(mag, axis=0, prepend=(mag[:1] if prev is None else prev[None]))
        prev = mag[-1]
        out[a:b] = np.maximum(d, 0).sum(1)
    return out


def crude(path: str, store: Path, session) -> dict:
    """Lo crudo de una cancion: lo guardado o, si no, la red y el flujo."""
    target = store / f"{hashlib.sha1(path.encode()).hexdigest()}.npz"  # noqa: S324 - un nombre
    if target.exists():
        return dict(np.load(target))
    mono = pulse._decode(shutil.which("ffmpeg") or "ffmpeg", path)
    beat, down = pulse.logits(session, pulse.spectrogram(mono))
    d = {
        "beat": beat.astype(np.float16),
        "down": down.astype(np.float16),
        "flux": flux(mono).astype(np.float16),
        "dur": np.array(mono.size / SR),
    }
    np.savez_compressed(target, **d)
    return d


def levels(beats: np.ndarray) -> tuple[int, int]:
    """(tramos de 4+ pulsos a otro nivel, golpes sueltos fuera de tempo)."""
    ibi = np.diff(beats)
    if ibi.size < 8:
        return 0, 0
    period = float(np.median(ibi))
    ratio = ibi / period
    normal = (ratio > 0.88) & (ratio < 1.13)
    runs, odd, start = 0, 0, 0
    for i in range(1, ibi.size + 1):
        if i == ibi.size or normal[i] != normal[start]:
            if not normal[start]:
                if i - start >= 4:
                    runs += 1
                else:
                    odd += 1
            start = i
    return runs, odd


def placement(beats: np.ndarray, env: np.ndarray) -> tuple[float, float]:
    """Donde caen los golpes de la musica respecto a cada pulso (mediana, en
    ms; negativo: el clic va tarde) y cuanto bailan (desviacion mediana)."""
    env = np.convolve(env.astype(np.float32), np.ones(3) / 3, mode="same")
    half = int(0.06 * ENV_FPS)
    offs = []
    for b in beats:
        c = round(b * ENV_FPS)
        if c - half < 0 or c + half + 1 >= env.size:
            continue
        seg = env[c - half : c + half + 1]
        offs.append((int(np.argmax(seg)) - half) / ENV_FPS + ENV_LAG)
    if len(offs) < 16:
        return 0.0, 0.0
    o = np.array(offs)
    med = float(np.median(o))
    return med * 1000, float(np.median(np.abs(o - med))) * 1000


def review(d: dict) -> dict:
    raw, _ = pulse.beats_of(d["beat"].astype(np.float32), d["down"].astype(np.float32))
    runs, odd = levels(raw)
    out = {"red_tramos_otro_nivel": runs, "red_golpes_sueltos": odd}
    try:
        g = pulse.grid(d["beat"].astype(np.float32), d["down"].astype(np.float32))
    except ValueError as e:
        return {**out, "error": str(e)}
    beats = np.array(g["beats"])
    bars = np.diff(g["bars"]) if len(g["bars"]) > 1 else np.array([])
    ibi = np.diff(beats)
    holes = float(ibi[ibi > 1.4 * np.median(ibi)].sum())
    dur = float(d["dur"])
    offset, spread = placement(beats, d["flux"])
    rej_runs, rej_odd = levels(beats)
    return {
        **out,
        "bpm": g["bpm"],
        "compas": g["meter"],
        "compases_irregulares": int(np.sum(bars != g["meter"])) if bars.size else 0,
        "rejilla_tramos_otro_nivel": rej_runs,
        "rejilla_golpes_sueltos": rej_odd,
        "sin_clic_s": round(beats[0] + holes + (dur - beats[-1]), 1),
        "desfase_ms": round(offset, 1),
        "baile_ms": round(spread, 1),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    ap.add_argument(
        "--db", default="", help="la base de la biblioteca (la de la app si no se dice)"
    )
    ap.add_argument("--datos", type=Path, default=ROOT / ".dev" / "revision-metronomo")
    ap.add_argument("--canciones", type=int, default=0, help="solo las N primeras")
    ap.add_argument("--salida", type=Path, default=None, help="el detalle, una cancion por linea")
    args = ap.parse_args()
    if not args.db:
        from danplay import config

        args.db = str(config.DATABASE)
    args.datos.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    rows = conn.execute("SELECT artist, title, path FROM songs ORDER BY path").fetchall()
    if args.canciones:
        rows = rows[: args.canciones]
    session = pulse.open_session(GRAPH)
    report = []
    for i, (artist, title, path) in enumerate(rows, 1):
        name = f"{artist} - {title}" if artist else title or os.path.basename(path)
        if not os.path.exists(path):
            print(f"{i:4}/{len(rows)}  no esta: {path}", flush=True)
            continue
        start = time.time()
        try:
            r = {"cancion": name, "ruta": path, **review(crude(path, args.datos, session))}
        except RuntimeError as e:  # ffmpeg no la pudo leer
            r = {"cancion": name, "ruta": path, "error": str(e)}
        report.append(r)
        line = r.get("error") or (
            f"{r['bpm']:6.1f} bpm {r['compas']}/4 · la red: {r['red_tramos_otro_nivel']} tramos a otro"
            f" nivel, {r['red_golpes_sueltos']} golpes sueltos · sin clic {r['sin_clic_s']} s"
            f" · {r['desfase_ms']:+.0f} ms ±{r['baile_ms']:.0f}"
        )
        print(f"{i:4}/{len(rows)} {time.time() - start:5.1f}s  {name[:50]:50}  {line}", flush=True)
    if args.salida:
        with open(args.salida, "w", encoding="utf-8") as f:
            for r in report:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
    ok = [r for r in report if "error" not in r]
    n = len(ok) or 1

    def share(test) -> str:
        k = sum(1 for r in ok if test(r))
        return f"{k}/{len(ok)} ({100 * k / n:.0f} %)"

    print(f"\n{len(report)} canciones ({len(report) - len(ok)} sin pulso claro o ilegibles)")
    print(
        "la red, con tramos a otro nivel:       ", share(lambda r: r["red_tramos_otro_nivel"] > 0)
    )
    print("la red, con 3+ golpes sueltos:         ", share(lambda r: r["red_golpes_sueltos"] >= 3))
    print(
        "el clic, con tramos a otro nivel:      ",
        share(lambda r: r["rejilla_tramos_otro_nivel"] > 0),
    )
    print(
        "el clic, con 3+ golpes sueltos:        ", share(lambda r: r["rejilla_golpes_sueltos"] >= 3)
    )
    print("el clic, con compases irregulares:     ", share(lambda r: r["compases_irregulares"] > 0))
    print("el clic, mas de 10 ms fuera de sitio:  ", share(lambda r: abs(r["desfase_ms"]) > 10))
    print("el clic, con mas de 15 s sin sonar:    ", share(lambda r: r["sin_clic_s"] > 15))
    worst = sorted(
        ok,
        key=lambda r: (
            -(
                r["rejilla_tramos_otro_nivel"] * 3
                + r["rejilla_golpes_sueltos"]
                + abs(r["desfase_ms"]) / 5
            )
        ),
    )[:10]
    print("\nlas que peor salen (para escucharlas):")
    for r in worst:
        print(f"  {r['cancion'][:60]:60} {r['bpm']} bpm, {r['desfase_ms']:+.0f} ms")


if __name__ == "__main__":
    main()
