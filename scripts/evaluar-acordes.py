#!/usr/bin/env python3
"""Cuantas canciones de la biblioteca encuentran su cifrado en la web, y como.

    uv run python scripts/evaluar-acordes.py [--canciones 80] [--db RUTA] [--salida x.json]

Abre la base de la biblioteca SOLO PARA LEER y busca con `danplay.cifrados`
el cifrado de una muestra al azar (las que tienen artista y titulo). Por cada
una apunta que fuente lo dio, de que cancion dice ser, si dice su tono y
cuanto tardo. Al final: cuantas encontro, cuantas con tono, el tiempo medio y
el peor. Con `--salida` deja el detalle en JSON para revisar a mano que lo
encontrado es de verdad la cancion (sin la letra ni los acordes, solo el
enlace).

Espera un segundo entre cancion y cancion: no es una prueba de carga contra
las paginas de acordes.
"""

import argparse
import json
import random
import sqlite3
import statistics
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from danplay import cifrados


def main() -> None:
    ap = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    ap.add_argument("--db", default=str(Path.home() / ".local/share/danplay/danplay.db"))
    ap.add_argument("--canciones", type=int, default=80)
    ap.add_argument("--semilla", type=int, default=7)
    ap.add_argument("--salida", default="")
    args = ap.parse_args()

    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    rows = conn.execute(
        "SELECT artist, title, path FROM songs WHERE trim(artist) != '' AND trim(title) != ''"
    ).fetchall()
    random.Random(args.semilla).shuffle(rows)  # noqa: S311 - una muestra, no un secreto
    rows = rows[: args.canciones]

    detail, times = [], []
    for i, (artist, title, path) in enumerate(rows, 1):
        r = cifrados.find(artist, title)
        times.append(r["seconds"])
        s = r["sheet"]
        detail.append(
            {
                "biblioteca": f"{artist} - {title}",
                "carpeta": Path(path).parent.name,
                "fuente": s["source"] if s else "",
                "cifrado": f"{s['artist']} - {s['title']}" if s else "",
                "tono": s["key"] if s else "",
                "tono_de": s["key_from"] if s else "",
                "votos": s["votes"] if s else 0,
                "url": s["url"] if s else "",
                "fallaron": r["failed"],
                "segundos": r["seconds"],
            }
        )
        mark = (f"{s['source']:15} tono {s['key'] or '—':4}") if s else "—"
        print(f"{i:3}/{len(rows)} {r['seconds']:5.1f}s  {mark:24} {artist} - {title}", flush=True)
        time.sleep(1)

    found = [d for d in detail if d["fuente"]]
    keyed = [d for d in found if d["tono"]]
    n = len(detail) or 1
    print()
    print(f"encontradas: {len(found)}/{len(detail)} ({100 * len(found) / n:.0f} %)")
    print(f"con tono:    {len(keyed)}/{len(detail)} ({100 * len(keyed) / n:.0f} %)")
    for source in ("Ultimate Guitar", "LaCuerda"):
        print(f"  de {source}: {sum(d['fuente'] == source for d in found)}")
    print(f"fuentes caidas: {sum(bool(d['fallaron']) for d in detail)}")
    if times:
        print(f"tiempo: mediana {statistics.median(times):.1f} s, peor {max(times):.1f} s")
    if args.salida:
        Path(args.salida).write_text(json.dumps(detail, ensure_ascii=False, indent=1), "utf-8")


if __name__ == "__main__":
    main()
