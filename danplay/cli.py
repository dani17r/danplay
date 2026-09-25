# -*- coding: utf-8 -*-
"""Interfaz de linea de comandos de DanPlay."""
import argparse, sys
from pathlib import Path
from . import (config, library, convert, duplicates, fingerprint, ai,
               ingest, playlists, names, youtube)

GREEN, RED, CYAN = "\033[32m", "\033[31m", "\033[36m"
YELLOW, RESET = "\033[33m", "\033[0m"


def _mmss(seg):
    m, s = divmod(int(seg or 0), 60)
    return f"{m}:{s:02d}"


def cmd_status(_):
    print(config.summary())
    print(f"\nHuella  : {fingerprint.unavailable_reason() or GREEN + 'lista' + RESET}")
    print(f"IA      : {(GREEN + 'lista' + RESET) if ai.available() else ai.unavailable_reason()}")
    if config.DATABASE.exists():
        e = library.stats_of()
        print(f"\nIndice  : {e['total']} canciones, {e['bytes']/2**30:.2f} GB, "
              f"{e['seconds']/3600:.1f} h")
        print(f"          {e['without_artist']} sin etiqueta de artista")
    else:
        print("\nIndice  : sin crear (ejecuta: danplay escanear)")


def cmd_scan(args):
    cs = library.list_folders()
    if not cs:
        print(f"{YELLOW}No hay carpetas gestionadas. Usa: danplay carpeta agregar <ruta>{RESET}"); return
    for c in cs:
        print(f"  {CYAN}{c['path']}{RESET}")
    ex = library.list_exclusions()
    if ex:
        print(f"  excluyendo: {', '.join(e['pattern'] for e in ex)}")
    r = library.scan(progress=lambda n: print(f"  ... {n}", end="\r"))
    e = library.stats_of()
    print(f"{GREEN}{r['total']} canciones indexadas{RESET} ({r['added_count']} nuevas)  "
          f"{e['bytes']/2**30:.2f} GB, {e['seconds']/3600:.1f} h")
    print(f"  {e['without_artist']} sin artista, {e['analyzed_count']} analizadas, "
          f"{e['with_lyrics']} con letra")


def cmd_folder(args):
    if args.action == "add":
        ok = library.add_folder(args.path, args.label or "")
        print((GREEN + "agregada" + RESET) if ok else (RED + "no existe esa ruta" + RESET))
    elif args.action == "remove":
        library.remove_folder(args.path); print("quitada")
    else:
        for c in library.list_folders():
            status = "" if c["active"] else " (inactiva)"
            print(f"  {c['n']:5d}  {c['label']:<24} {c['path']}{status}")


def cmd_exclude(args):
    if args.action == "add":
        library.add_exclusion(args.pattern, args.kind, args.note or "")
        print(f"{GREEN}excluido:{RESET} {args.pattern}")
    elif args.action == "remove":
        library.remove_exclusion(args.pattern); print("quitada")
    else:
        for e in library.list_exclusions():
            print(f"  [{e['kind']:<4}] {e['pattern']:<28} {e['note']}")
        print("\n  por defecto siempre se omiten: " +
              ", ".join(library.DEFAULT_EXCLUDES))


def cmd_playlist(args):
    if args.action == "create":
        made = playlists.create(args.name)
        estado = "creada" if made["created"] else "ya existia"
        print(f"{GREEN}{estado}{RESET} id={made['id']}")
    elif args.action == "remove":
        playlists.remove(int(args.name)); print("borrada")
    elif args.action == "show":
        lid = int(args.name)
        for c in playlists.songs(lid):
            info = "*" * (c.get("stars") or 0)
            print(f"  {c['sort']:2d}. {c['artist']:<24} {c['title'][:40]:<42} {info}")
    elif args.action == "export":
        print(playlists.export_m3u(int(args.name)))
    else:
        for l in playlists.list_all():
            print(f"  [{l['id']:2d}] {l['name']:<26} {l['n']:3d} temas  "
                  f"{l['seconds']/60:5.0f} min")
        fav = playlists.favorites()
        if fav:
            print(f"\n  favoritos: {len(fav)}")


def cmd_import(args):
    res = ingest.process_inbox(dry_run=args.dry_run, limit=args.limit,
                                    convert_mp3=args.convert)
    if not res:
        print(f"No hay nada en {config.INBOX}")
        return
    ok = rev = err = 0
    for r in res:
        if r.action == "error":
            print(f"{RED}ERROR{RESET}  {r.source_path.name}\n       {r.note}"); err += 1; continue
        if r.action == "review":
            print(f"{YELLOW}REVISAR{RESET} {r.source_path.name}\n       -> {r.target}"); rev += 1; continue
        ok += 1
        mark = "SIM " if args.dry_run else "OK  "
        rel = r.target.relative_to(config.LIBRARY)
        print(f"{GREEN}{mark}{RESET} [{r.source} {r.confidence:.2f}] {rel}")
        if args.verbose:
            print(f"       <- {r.source_path.name}")
        for av in r.warnings:
            print(f"       {YELLOW}! {av}{RESET}")
    print(f"\n{ok} procesadas, {rev} a revisar, {err} errores"
          + ("   (SIMULACION, nada se movio)" if args.dry_run else ""))


def cmd_convert(args):
    if not convert.available():
        print(f"{RED}falta ffmpeg{RESET}"); return
    paths = convert.candidates()
    if not paths:
        print(f"{GREEN}Nada que convertir{RESET}: todo es mp3 (las carpetas "
              f"{', '.join(sorted(config.NEVER_CONVERT))} nunca se tocan)")
        return
    print(f"{len(paths)} archivos a convertir a mp3 (calidad {args.quality}):")
    for r in paths[:20]:
        print(f"  {r.replace(str(config.LIBRARY) + '/', '')}")
    if len(paths) > 20:
        print(f"  ... y {len(paths)-20} mas")
    r = convert.convert_batch(paths, args.quality, args.keep, dry_run=not args.apply)
    if args.apply:
        print(f"\n{GREEN}{r['converted']} convertidos{RESET}, {r['failures']} fallos, "
              f"{r['skipped']} saltados   ahorro: {r['saved_bytes']/2**20:.0f} MB")
        for e in r["errors"][:5]:
            print(f"  {RED}{e.get('reason','')}{RESET}")
    else:
        print(f"\n(simulacion; usa --apply para convertir de verdad)")


def cmd_youtube(args):
    if not youtube.available():
        print(f"{RED}{youtube.unavailable_reason()}{RESET}"); return 1
    query = " ".join(args.query)

    if args.list_all:
        f = youtube.info(query, args.results)
        if not f["ok"]:
            print(f"{RED}{f['reason']}{RESET}"); return 1
        print(f"{f['name'] or query}  ({len(f['items'])})")
        for i, t_ in enumerate(f["items"], 1):
            print(f"  {i:2}. {t_['title']}  {CYAN}{t_['channel']}  {_mmss(t_['duration'])}{RESET}")
        return 0

    ultimo = {"linea": ""}
    def progress(p):
        line = (f"  [{p.get('index','?')}/{p.get('total','?')}] "
                 f"{p.get('phase',''):<12} {p.get('percent',0):5.1f}%  {p.get('name','')[:52]}")
        if line != ultimo["linea"]:
            print(line.ljust(96), end="\r", flush=True); ultimo["linea"] = line

    res = youtube.download(query, quality=args.quality,
                            file_it=not args.only_download,
                            results=args.results, progress=progress)
    print(" " * 96, end="\r")

    ok = err = 0
    for r in res:
        if not r.get("ok"):
            print(f"{RED}ERROR{RESET}  {r.get('source','')}\n       {r.get('reason','')}"); err += 1
            continue
        ok += 1
        if r.get("target"):
            rel = Path(r["target"]).relative_to(config.LIBRARY)
            mark = YELLOW + "REVISAR" + RESET if r.get("action") == "review" else GREEN + "OK     " + RESET
            print(f"{mark} [{r.get('identified_by','')} {r.get('confidence',0):.2f}] {rel}")
        else:
            print(f"{GREEN}OK     {RESET} {Path(r['file']).name}  ({r['kbps']} kbps)")
        for av in r.get("warnings", []):
            print(f"       {YELLOW}! {av}{RESET}")
    print(f"\n{ok} descargadas, {err} fallos")
    return 0


def cmd_search(args):
    rows = library.search(" ".join(args.text), sort=args.sort, limit=args.limit)
    for f in rows:
        info = ("*" * (f.get("stars") or 0)).ljust(5)
        fav = "\u2665" if f.get("favorite") else " "
        feat = f"  feat. {f['feat']}" if f["feat"] else ""
        print(f"  {fav}{info} {CYAN}{f['artist'][:22]:<22}{RESET} {f['title'][:40]:<42}"
              f"[{_mmss(f['duration'])} {(f['bitrate'] or 0)//1000:>3}k]{feat}")
    print(f"\n{len(rows)} resultados")


def cmd_duplicates(args):
    paths = [c["path"] for c in library.search("", limit=100000)]
    print(f"Analizando {len(paths)} archivos ...\n")
    ident = duplicates.identical(paths)
    if ident:
        print(f"{RED}== IDENTICOS byte a byte =={RESET}")
        for g in ident:
            print()
            for r in sorted(g):
                print(f"   {Path(r).relative_to(config.LIBRARY)}")
    watcher = duplicates.same_song(paths, config.DUPLICATE_THRESHOLD)
    if watcher:
        print(f"\n{YELLOW}== MISMA CANCION, archivo distinto =={RESET}")
        from . import tags as E
        for g in watcher:
            print()
            for r in sorted(g):
                print(f"   [{_mmss(E.duration(r))} {E.bitrate(r)//1000:>3}k] "
                      f"{Path(r).relative_to(config.LIBRARY)}")
    print(f"\n{len(ident)} grupos identicos, {len(watcher)} grupos parecidos")
    if not args.apply:
        print("Nada se ha borrado. Revisa y borra tu manualmente.")


def cmd_organize(args):
    """Renombra/reubica una carpeta que ya existe (por defecto solo simula)."""
    folder = Path(args.folder)
    if not folder.is_absolute():
        folder = config.LIBRARY / folder
    if not folder.is_dir():
        print(f"{RED}No existe: {folder}{RESET}"); return
    vocab = names.vocabulary(config.ARTISTS_DIR)
    files = [p for p in sorted(folder.iterdir())
                if p.is_file() and p.suffix.lower() in config.EXTENSIONS]
    for p in files:
        d = names.detect_artist(p.name, vocab)
        new = names.final_name(d["artist"], d["title"], d["feat"], ext=p.suffix.lower())
        if new == p.name:
            continue
        print(f"  {p.name}\n    -> {new}   [{d['confidence']:.2f}]")
        if args.apply:
            target = p.parent / names.free_name(str(p.parent), new)
            p.rename(target)
    print("\n(simulacion; usa --apply para hacerlo de verdad)" if not args.apply
          else "\nAplicado.")


def cmd_watch(args):
    from watchdog.observers import Observer
    from watchdog.events import FileSystemEventHandler
    import time

    config.INBOX.mkdir(parents=True, exist_ok=True)

    class H(FileSystemEventHandler):
        def on_created(self, e):
            if e.is_directory:
                return
            p = Path(e.src_path)
            if p.suffix.lower() not in config.EXTENSIONS:
                return
            time.sleep(1.5)                      # deja que termine de copiarse
            r = ingest.process(p)
            status = GREEN + "OK" + RESET if r.action == "moved" else YELLOW + r.action.upper() + RESET
            print(f"{status} {r.target.relative_to(config.LIBRARY) if r.target else p.name}")

    observer = Observer()
    observer.schedule(H(), str(config.INBOX), recursive=False)
    observer.start()
    print(f"Vigilando {config.INBOX}  (Ctrl+C para salir)")
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        observer.stop()
    observer.join()


def main(argv=None):
    # Un solo sitio donde se decide como se ven los avisos. Sin esto, todo lo
    # que el nucleo tragaba en silencio seguia sin verse ni ejecutandolo a mano.
    import logging
    logging.basicConfig(level=logging.INFO, format="danplay: %(message)s")
    p = argparse.ArgumentParser(prog="danplay", description="Gestor de biblioteca musical")
    sub = p.add_subparsers(dest="cmd", required=True)

    sub.add_parser("status", help="configuracion y resumen").set_defaults(f=cmd_status)
    sub.add_parser("scan", help="reindexar la biblioteca").set_defaults(f=cmd_scan)

    i = sub.add_parser("import", help="procesar la carpeta Entrada/")
    i.add_argument("-s", "--dry-run", action="store_true")
    i.add_argument("-n", "--limit", type=int)
    i.add_argument("-v", "--verbose", action="store_true")
    i.add_argument("--convert", dest="convert", action="store_true", default=None,
                   help="convertir a mp3 lo que no lo sea")
    i.add_argument("--no-convert", dest="convert", action="store_false")
    i.set_defaults(f=cmd_import)

    cv = sub.add_parser("convert", help="pasar a mp3 lo que no lo sea")
    cv.add_argument("--quality", default="high", choices=["high","medium","variable"])
    cv.add_argument("--keep", action="store_true", help="no borrar el original")
    cv.add_argument("--apply", action="store_true")
    cv.set_defaults(f=cmd_convert)

    y = sub.add_parser("youtube", help="descargar audio de YouTube y archivarlo")
    y.add_argument("query", nargs="+", help="URL, lista, o texto a buscar")
    y.add_argument("--quality", default="high", choices=["high","medium","variable"])
    y.add_argument("-n", "--results", type=int, default=5,
                   help="cuantos traer si es una busqueda")
    y.add_argument("-l", "--list", dest="list_all", action="store_true",
                   help="solo enseñar que se bajaria")
    y.add_argument("--only-download", action="store_true",
                   help="dejarlo en Entrada/ sin identificar ni archivar")
    y.set_defaults(f=cmd_youtube)

    b = sub.add_parser("search", help="buscar (soporta artista:x tono:y bpm>100)")
    b.add_argument("text", nargs="+")
    b.add_argument("--sort", default="artist",
                   choices=["artist","title","duration","bpm","recent","album"])
    b.add_argument("--limit", type=int, default=60)
    b.set_defaults(f=cmd_search)

    fo = sub.add_parser("folder", help="gestionar carpetas indexadas")
    fo.add_argument("action", nargs="?", default="list",
                    choices=["list","add","remove"])
    fo.add_argument("path", nargs="?"); fo.add_argument("--label")
    fo.set_defaults(f=cmd_folder)

    ex = sub.add_parser("exclude", help="carpetas a omitir al indexar")
    ex.add_argument("action", nargs="?", default="list",
                    choices=["list","add","remove"])
    ex.add_argument("pattern", nargs="?")
    ex.add_argument("--kind", default="glob", choices=["glob","path"])
    ex.add_argument("--note")
    ex.set_defaults(f=cmd_exclude)

    pl = sub.add_parser("playlist", help="listas de reproduccion")
    pl.add_argument("action", nargs="?", default="list",
                    choices=["list","create","remove","show","export"])
    pl.add_argument("name", nargs="?")
    pl.set_defaults(f=cmd_playlist)

    d = sub.add_parser("duplicates", help="informe de duplicados")
    d.add_argument("--apply", action="store_true"); d.set_defaults(f=cmd_duplicates)

    o = sub.add_parser("organize", help="renombrar una carpeta existente")
    o.add_argument("folder"); o.add_argument("--apply", action="store_true")
    o.set_defaults(f=cmd_organize)

    sub.add_parser("watch", help="vigilar Entrada/ en segundo plano").set_defaults(f=cmd_watch)

    sv = sub.add_parser("serve", help="levantar la API para la app de escritorio")
    sv.add_argument("--port", type=int, default=8730)
    sv.add_argument("--host", default="127.0.0.1")
    sv.add_argument("--uds", help="socket Unix (sin puerto TCP; lo usa la app)")
    sv.set_defaults(f=lambda a: __import__("danplay.api", fromlist=["api"])
                    .serve(a.host, a.port, a.uds))

    a = p.parse_args(argv)
    return a.f(a) or 0


if __name__ == "__main__":
    sys.exit(main())
