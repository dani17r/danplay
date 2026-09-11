#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""El banco de pruebas del asistente contra un modelo DE VERDAD.

    ./.venv/bin/python scripts/evaluar-asistente.py               el proveedor activo
    ./.venv/bin/python scripts/evaluar-asistente.py --proveedor openrouter
    ./.venv/bin/python scripts/evaluar-asistente.py --solo lista --repetir 3

Las pruebas normales (tests/) usan un modelo falso: comprueban la mecanica
(ids que no existen se rechazan, lo destructivo se confirma, la narracion
se detecta) pero no que un modelo real siga el prompt. Esto si: monta una
biblioteca sintetica (mp3 de silencio con etiquetas, como las pruebas), y
corre las conversaciones de tests/evaluacion.json contra el proveedor
configurado en la app, comprobando que herramientas uso, que quedo en la
biblioteca y que dijo. Sale una tabla con lo que paso, lo que tardo y lo que
costo; con --repetir se ve lo estable que es cada caso.

Cuesta dinero (poco) y red, por eso no va en scripts/test.sh: se lanza a
mano cuando se toca el prompt, las herramientas o se cambia de modelo.
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "tests"))

# La biblioteca de mentira. Ids reales solo despues de escanear: los casos
# se refieren a las canciones por «Artista - Titulo» y aqui se traducen.
SONGS = [("Barak", "Mi Gozo", "Bb", 120), ("Barak", "Vivo Estas", "G", 76),
         ("Barak", "Sera Llena La Tierra", "D", 130), ("New Wine", "Shekinah", "A", 70),
         ("Miel San Marcos", "Que Se Abra El Cielo", "E", 72)]

GREEN, RED, DIM, RESET = "\033[32m", "\033[31m", "\033[2m", "\033[0m"


def build_library(tmp: pathlib.Path) -> dict:
    """Crea la biblioteca temporal, la indexa y devuelve {"Artista - Titulo": id}."""
    from conftest import make_mp3
    from danplay import config, library
    lib = tmp / "Musica"
    for artist, title, _, _ in SONGS:
        make_mp3(lib / "Artistas" / artist / f"{artist} - {title}.mp3",
                 artist=artist, title=title, album="Pruebas", seconds=1.0)
    for sub in ("Entrada", "Revisar"):
        (lib / sub).mkdir(parents=True, exist_ok=True)
    config.LIBRARY, config.INBOX = lib, lib / "Entrada"
    config.ARTISTS_DIR, config.REVIEW_DIR = lib / "Artistas", lib / "Revisar"
    config.DATABASE = tmp / "evaluacion.db"
    config.WRITE_TAGS = False
    library.add_folder(str(lib), "evaluacion")
    library.scan()
    ids = {}
    for c in library.search("", limit=100):
        ids[f"{c['artist']} - {c['title']}"] = c["id"]
    for artist, title, key, bpm in SONGS:
        library.update(ids[f"{artist} - {title}"], key=key, bpm=bpm)
    return ids


def resolve_ids(value, ids: dict):
    """«$Artista - Titulo» → id real, en cualquier sitio de una estructura."""
    if isinstance(value, str) and value.startswith("$"):
        return ids[value[1:]]
    if isinstance(value, list):
        return [resolve_ids(v, ids) for v in value]
    if isinstance(value, dict):
        return {k: resolve_ids(v, ids) for k, v in value.items()}
    return value


def reset_state(ids: dict, setup: dict | None) -> None:
    """Cada caso empieza igual: sin listas, sin estrellas ni favoritos."""
    from danplay import library, playlists
    for pl in playlists.list_all():
        playlists.remove(pl["id"])
    for cid in ids.values():
        library.update(cid, stars=0, favorite=0)
    for spec in ((setup or {}).get("playlists") or ([setup["playlist"]] if setup and "playlist" in setup else [])):
        made = playlists.create(spec["name"])
        playlists.add(made["id"], [ids[s] for s in spec.get("songs", [])])


def check(case: dict, result: dict, ids: dict) -> list[str]:
    """Que no se cumplio de lo esperado (vacio = todo bien)."""
    from danplay import library, playlists
    exp = case["expect"]
    problems = []
    if result.get("error"):
        return [f"error: {result['error'][:120]}"]
    text = result.get("text") or ""
    used = [t["name"] for t in result.get("tools", [])]
    if "tools" in exp and used != exp["tools"]:
        problems.append(f"herramientas {used} (esperaba {exp['tools']})")
    for name in exp.get("tools_include", []):
        if name not in used:
            problems.append(f"no uso {name} (uso {used})")
    for name in exp.get("tools_exclude", []):
        if name in used:
            problems.append(f"uso {name} y no debia")
    if exp.get("no_narration") and result.get("narrated"):
        problems.append("narro: dijo haber hecho algo sin hacerlo")
    if "text_any" in exp and not any(x.lower() in text.lower() for x in exp["text_any"]):
        problems.append(f"el texto no dice ninguno de {exp['text_any']}")
    for bad in exp.get("text_none", []):
        if bad.lower() in text.lower():
            problems.append(f"el texto dice «{bad}»")
    if "confirm" in exp:
        got = (result.get("confirm") or {}).get("tool")
        if got != exp["confirm"]:
            problems.append(f"confirmacion {got!r} (esperaba {exp['confirm']!r})")
    if "playlist" in exp:
        spec = exp["playlist"]
        pl = playlists.by_name(spec["name"])
        if not pl:
            problems.append(f"no existe la lista «{spec['name']}»")
        else:
            songs = playlists.songs(pl["id"])
            if "n" in spec and len(songs) != spec["n"]:
                problems.append(f"la lista tiene {len(songs)} temas (esperaba {spec['n']})")
            if "titles" in spec and [c["title"] for c in songs] != spec["titles"]:
                problems.append(f"la lista tiene {[c['title'] for c in songs]} (esperaba {spec['titles']})")
    if "playlist_absent_or_empty" in exp:
        pl = playlists.by_name(exp["playlist_absent_or_empty"])
        if pl and playlists.songs(pl["id"]):
            problems.append("creo la lista con canciones inventadas")
    if "stars" in exp:
        c = next(x for x in library.search("", limit=100) if x["title"] == exp["stars"]["title"])
        if int(c.get("stars") or 0) != exp["stars"]["n"]:
            problems.append(f"«{c['title']}» tiene {c.get('stars')} estrellas (esperaba {exp['stars']['n']})")
    if "favorite" in exp:
        c = next(x for x in library.search("", limit=100) if x["title"] == exp["favorite"]["title"])
        if not c.get("favorite"):
            problems.append(f"«{c['title']}» no quedo como favorita")
    if "action" in exp:
        want = exp["action"]
        actions = result.get("actions") or []
        hit = [a for a in actions if a.get("kind") == want["kind"]]
        if not hit:
            problems.append(f"no llego la accion {want['kind']} (llegaron {[a.get('kind') for a in actions]})")
        elif "song" in want and hit[0].get("song_id") != ids[want["song"]]:
            problems.append(f"puso el id {hit[0].get('song_id')} (esperaba «{want['song']}»)")
    return problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--proveedor", help="id del perfil a usar (por defecto, el activo)")
    ap.add_argument("--solo", help="solo el caso con este id (o varios, separados por coma)")
    ap.add_argument("--repetir", type=int, default=1, help="cuantas veces cada caso")
    ap.add_argument("--casos", default=str(ROOT / "tests" / "evaluacion.json"))
    ap.add_argument("--verboso", action="store_true", help="enseña las respuestas enteras")
    args = ap.parse_args()

    if args.proveedor:
        os.environ["DANPLAY_AI_PROVIDER"] = args.proveedor
    if not shutil.which("ffmpeg"):
        print("hace falta ffmpeg para generar la biblioteca de prueba")
        return 2
    from danplay import ai, chat, providers
    providers.reload()
    ai.reset_client()
    if not ai.available():
        print(f"la IA no esta lista: {ai.unavailable_reason()}")
        return 2
    p = ai.profile()
    print(f"proveedor: {p['name']} · conversacion: {p['chat_model']} · rapido: {p['model']}")

    cases = json.loads(pathlib.Path(args.casos).read_text(encoding="utf-8"))
    if args.solo:
        wanted = set(args.solo.split(","))
        cases = [c for c in cases if c["id"] in wanted]
    tmp = pathlib.Path(tempfile.mkdtemp(prefix="danplay-evaluacion-"))
    try:
        ids = build_library(tmp)
        total_cost, total_calls, passed, runs = 0.0, 0, 0, 0
        rows = []
        for case in cases:
            for k in range(args.repetir):
                reset_state(ids, case.get("setup"))
                context = resolve_ids(case.get("context"), ids)
                t0 = time.time()
                r = chat.reply([dict(m) for m in case["messages"]], context=context)
                dt = time.time() - t0
                problems = check(case, r, ids)
                usage = r.get("usage") or {}
                cost = usage.get("cost")
                total_cost += cost or 0
                total_calls += usage.get("calls", 0)
                runs += 1
                ok = not problems
                passed += ok
                tag = f"{GREEN}ok  {RESET}" if ok else f"{RED}FALLA{RESET}"
                extra = f" ×{k + 1}" if args.repetir > 1 else ""
                rows.append((case["id"] + extra, ok))
                print(f"{tag} {case['id'] + extra:26} {dt:5.1f}s  {usage.get('prompt', 0) + usage.get('completion', 0):6} tok  "
                      f"{('$%.4f' % cost) if cost is not None else '   ?  '}  {DIM}{case.get('que', '')}{RESET}")
                if problems:
                    for pr in problems:
                        print(f"        - {pr}")
                    print(f"        {DIM}dijo: {(r.get('text') or r.get('error') or '')[:200]!r}{RESET}")
                    print(f"        {DIM}herramientas: {[(t['name'], t.get('args')) for t in r.get('tools', [])]}{RESET}")
                elif args.verboso:
                    print(f"        {DIM}{(r.get('text') or '')[:300]!r}{RESET}")
        print(f"\n{passed}/{runs} bien · {total_calls} llamadas · ${total_cost:.4f}")
        if args.repetir > 1:
            by = {}
            for name, ok in rows:
                base = name.rsplit(" ×", 1)[0]
                by.setdefault(base, []).append(ok)
            for base, oks in by.items():
                print(f"  {base:26} {sum(oks)}/{len(oks)}")
        return 0 if passed == runs else 1
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
