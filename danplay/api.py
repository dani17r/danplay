# -*- coding: utf-8 -*-
"""API HTTP local sobre el mismo nucleo que usa la CLI.

La app de escritorio (Tauri + Vue) habla con esto en localhost.
"""
import asyncio, io, os, shutil, subprocess
from pathlib import Path
from typing import Any

from fastapi import FastAPI, HTTPException, Query, Body
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse, Response, StreamingResponse

from . import (ai, chat, config, convert, duplicates, enrich,
               fingerprint, ingest, library, names, playlists,
               tags, theory, web, youtube)

app = FastAPI(title="DanPlay", version="0.1.0")

# La app de escritorio habla por un socket Unix y no es un navegador: no usa
# CORS para nada. Quien si lo necesitaba era `npm run dev`, y ni eso, porque
# Vite hace de proxy y el navegador lo ve como mismo origen.
#
# Estaba abierto a cualquier origen (`*`), y eso con el servidor TCP levantado
# («danplay serve» sin --uds) significa que CUALQUIER pagina web abierta en el
# navegador podia leer la respuesta: listar la biblioteca, sacar el principio y
# el final de la clave de IA, mandar canciones a la papelera o, encadenando
# /song/{id}/cover, leer archivos sueltos del disco. Ahora solo se responde a
# los origenes de desarrollo.
DEV_ORIGINS = ["http://localhost:5273", "http://127.0.0.1:5273"]
app.add_middleware(CORSMiddleware, allow_origins=DEV_ORIGINS,
                   allow_methods=["*"], allow_headers=["*"])

# Con el puerto TCP abierto tambien hay que mirar la cabecera Host: una pagina
# puede apuntar su propio dominio a 127.0.0.1 (reenlace de DNS) y entonces el
# navegador considera que es su mismo origen y CORS ya no protege. Solo se
# exige cuando de verdad hay puerto abierto; por el socket Unix no aplica.
_ENFORCE_HOST = False
ALLOWED_HOSTS = {"localhost", "127.0.0.1", "[::1]", "::1"}


@app.middleware("http")
async def _only_local_hosts(request, call_next):
    if _ENFORCE_HOST:
        host = (request.headers.get("host") or "").rsplit(":", 1)[0]
        if host not in ALLOWED_HOSTS:
            return Response(status_code=421, content=b"host no permitido")
    return await call_next(request)

# estado de tareas largas (escaneo, analisis, conversion)
JOBS: dict[str, dict] = {}


def _job(name) -> dict:
    JOBS[name] = {"active": True, "done": 0, "total": 0, "message": ""}
    return JOBS[name]


# ---------------------------------------------------------------- estado

@app.get("/api/status")
def status():
    e = library.stats_of()
    folders = library.list_folders()
    return {"configured": bool(folders), "folders": len(folders),
            "library": str(config.LIBRARY), "inbox": str(config.INBOX),
            "model": config.DEEPINFRA_MODEL, "ai": ai.available(),
            "fingerprint": not fingerprint.unavailable_reason(),
            "fingerprint_reason": fingerprint.unavailable_reason(),
            "convert": config.CONVERT_TO_MP3, "quality": config.MP3_QUALITY,
            "never_convert": sorted(config.NEVER_CONVERT),
            "rust": duplicates.RUST, "ffmpeg": convert.available(), "stats": e,
            "youtube": youtube.available(), "youtube_reason": youtube.unavailable_reason(),
            "jobs": JOBS}


@app.get("/api/settings")
def settings():
    k = config.DEEPINFRA_API_KEY
    return {"convert_mp3": config.CONVERT_TO_MP3, "quality": config.MP3_QUALITY,
            "keep_original": config.KEEP_ORIGINAL,
            "write_tags": config.WRITE_TAGS, "ai_enabled": config.AI_ENABLED,
            "model": config.DEEPINFRA_MODEL,
            "library": str(config.LIBRARY),
            "ai_key": (k[:4] + "…" + k[-4:]) if len(k) > 10 else ("" if not k else "…"),
            "has_ai_key": bool(k),
            "fingerprint_key": bool(config.ACOUSTID_API_KEY),
            "settings_file": str(config.ENV_FILE)}


@app.post("/api/settings")
def save_settings(body: dict = Body(...)):
    """Las casillas y campos de la app. Se aplican en caliente y se persisten."""
    flags = {"convert_mp3": ("CONVERT_TO_MP3", "DANPLAY_CONVERT"),
             "keep_original": ("KEEP_ORIGINAL", "DANPLAY_KEEP_ORIGINAL"),
             "write_tags": ("WRITE_TAGS", "DANPLAY_TAGS"),
             "ai_enabled": ("AI_ENABLED", "DANPLAY_AI")}
    texts = {"quality": ("MP3_QUALITY", "DANPLAY_QUALITY"),
             "model": ("DEEPINFRA_MODEL", "DEEPINFRA_MODEL"),
             "ai_key": ("DEEPINFRA_API_KEY", "DEEPINFRA_API_KEY"),
             "fingerprint_key": ("ACOUSTID_API_KEY", "ACOUSTID_API_KEY")}
    save = {}
    for k, v in body.items():
        if k in flags:
            attr, env = flags[k]
            setattr(config, attr, bool(v)); save[env] = "1" if v else "0"
        elif k in texts and isinstance(v, str) and v.strip():
            attr, env = texts[k]
            setattr(config, attr, v.strip()); save[env] = v.strip()
        elif k == "library" and v:
            path = Path(v).expanduser()
            if not path.is_dir():
                raise HTTPException(400, "esa carpeta no existe")
            config.LIBRARY = path
            config.INBOX = path / "Entrada"
            config.ARTISTS_DIR = path / "Artistas"
            config.REVIEW_DIR = path / "Revisar"
            save["DANPLAY_LIBRARY"] = str(path)
    if save:
        config.save_env(save)
    if "ai_key" in body:
        ai._client = None                      # fuerza recrear con la clave nueva
    return settings()


# ---------------------------------------------------------------- carpetas

@app.post("/api/settings/check-ai")
async def check_ai():
    """Prueba la clave contra DeepInfra. Lo usa el boton «Probar» de Ajustes."""
    return await asyncio.get_running_loop().run_in_executor(None, ai.check)


@app.get("/api/folders")
def folders():
    return {"folders": library.list_folders(), "exclusions": library.list_exclusions(),
            "always_excluded": library.DEFAULT_EXCLUDES}


@app.post("/api/check-folder")
def check_folder(body: dict = Body(...)):
    """Mira si la carpeta repite musica ya indexada, antes de añadirla."""
    path = body.get("path", "")
    if not os.path.isdir(os.path.expanduser(path)):
        raise HTTPException(400, "esa ruta no existe")
    return {"notice": library.check_overlap(path)}


@app.post("/api/folders")
def add_folder(body: dict = Body(...)):
    """Añade una carpeta. Es idempotente: repetir la misma no duplica nada.

    accion:
      ya_estaba   -> ya indexada (o cubierta por otra); no se toca nada
      reemplaza   -> la nueva contiene a otra ya indexada; sustituye a la interna
      confirmar   -> parece una copia de otra distinta; hace falta force
      agregada    -> añadida
    """
    path = body.get("path", "")
    if not os.path.isdir(os.path.expanduser(path)):
        raise HTTPException(400, "esa ruta no existe")

    notice = library.check_overlap(path)
    kind = (notice or {}).get("kind")

    if kind in ("same", "inside"):
        return {"action": "already_there", "notice": notice, **folders()}

    if kind == "contains":
        library.remove_folder(notice["other"])
        library.add_folder(path, body.get("label", ""))
        return {"action": "replaced", "notice": notice, **folders()}

    if kind == "copy" and not body.get("force"):
        return {"action": "confirm", "notice": notice, **folders()}

    if not library.add_folder(path, body.get("label", "")):
        raise HTTPException(400, "esa ruta no existe")
    return {"action": "added", "notice": notice, **folders()}


@app.delete("/api/folders")
def remove_folder(path: str = Query(...)):
    library.remove_folder(path)
    return folders()


@app.post("/api/exclusions")
def add_exclusion(body: dict = Body(...)):
    library.add_exclusion(body.get("pattern", ""), body.get("kind", "glob"),
                        body.get("note", ""))
    return folders()


@app.delete("/api/exclusions")
def remove_exclusion(pattern: str = Query(...)):
    library.remove_exclusion(pattern)
    return folders()


@app.post("/api/scan")
async def scan():
    t = _job("escaneo")
    def work():
        try:
            r = library.scan(progress=lambda n: t.update(done=n))
            t.update(active=False, message=f"{r['total']} canciones ({r['added_count']} nuevas)")
        except Exception as ex:
            t.update(active=False, message=f"error: {ex}")
    await asyncio.get_running_loop().run_in_executor(None, work)
    return {"job": t, "stats": library.stats_of()}


# ---------------------------------------------------------------- busqueda

@app.get("/api/search")
def search(q: str = "", sort: str = "artist",
           limit: int = Query(200, ge=1, le=5000),
           from_key: int = Query(0, ge=0), artist: str = "", album: str = "",
           genre: str = "", folder: str = "", only_favorites: bool = False,
           min_stars: int = 0):
    filters = {k: v for k, v in
               {"artist": artist, "album": album, "genre": genre,
                "folder": folder}.items() if v}
    rows = library.search(q, filters, sort, limit, from_key,
                          only_favorites=only_favorites, min_stars=min_stars)
    return {"total": len(rows), "songs": rows}


@app.get("/api/facets")
def facets():
    return library.facets()


@app.get("/api/song/{cid}")
def song(cid: int):
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    c["playlists"] = playlists.playlists_of(cid)
    c["existe"] = os.path.exists(c["path"])
    return c


@app.patch("/api/song/{cid}")
def edit(cid: int, body: dict = Body(...)):
    c = library.edit(cid, **body)
    if not c:
        raise HTTPException(404, "no existe")
    return c


@app.delete("/api/song/{cid}")
def delete_song(cid: int):
    """Manda el archivo a la papelera del sistema y lo saca del indice.

    A la papelera y no `unlink`: borrar musica del usuario sin vuelta atras
    por un clic en un menu es demasiado definitivo. Si el escritorio no
    tiene papelera se avisa y no se borra nada.
    """
    r = library.trash(cid)
    if not r["ok"]:
        raise HTTPException(404 if "no existe" in r["error"] else 501, r["error"])
    return r


@app.get("/api/song/{cid}/audio")
def audio(cid: int):
    c = library.by_id(cid)
    if not c or not os.path.exists(c["path"]):
        raise HTTPException(404, "archivo no encontrado")
    return FileResponse(c["path"], media_type="audio/mpeg", filename=c["file"])


@app.get("/api/song/{cid}/cover")
def cover(cid: int):
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    r = tags.cached_cover(c["path"]) if os.path.exists(c["path"]) else None
    if not r:
        raise HTTPException(404, "sin portada")
    return Response(content=r[0], media_type=r[1])


@app.post("/api/song/{cid}/stars")
def stars(cid: int, body: dict = Body(...)):
    playlists.rate(cid, int(body.get("stars", 0)))
    return library.by_id(cid)


@app.post("/api/song/{cid}/favorite")
def favorite(cid: int, body: dict = Body(...)):
    playlists.favorite(cid, bool(body.get("favorite", True)))
    return library.by_id(cid)


# ---------------------------------------------------------------- listas

@app.get("/api/playlists")
def list_playlists():
    return {"playlists": playlists.list_all(), "favorites": len(playlists.favorites())}


@app.post("/api/playlists")
def create_playlist(body: dict = Body(...)):
    lid = playlists.create(body.get("name", "Nueva lista"), body.get("note", ""),
                  body.get("color", ""))
    return {"id": lid, "playlists": playlists.list_all()}


@app.delete("/api/playlists/{lid}")
def delete_playlist(lid: int):
    playlists.remove(lid)
    return {"playlists": playlists.list_all()}


@app.get("/api/playlists/{lid}/songs")
def playlist_songs_of(lid: int):
    return {"songs": playlists.songs(lid)}


@app.post("/api/playlists/{lid}/songs")
def add_to_playlist(lid: int, body: dict = Body(...)):
    ids = body.get("ids") or [body.get("id")]
    added = playlists.add(lid, [i for i in ids if i])
    return {"added": added, "songs": playlists.songs(lid)}


@app.delete("/api/playlists/{lid}/songs/{cid}")
def remove_from_playlist(lid: int, cid: int):
    playlists.remove_song(lid, cid)
    return {"songs": playlists.songs(lid)}


@app.post("/api/playlists/{lid}/order")
def reorder(lid: int, body: dict = Body(...)):
    playlists.reorder(lid, body.get("ids", []))
    return {"songs": playlists.songs(lid)}


@app.post("/api/playlists/{lid}/export")
def export(lid: int):
    return {"file": playlists.export_m3u(lid)}


# ---------------------------------------------------------------- IA

@app.post("/api/song/{cid}/enrich")
async def enrich_song(cid: int, body: dict = Body(default={})):
    def work():
        return enrich.enrich(cid, body.get("lyrics", True), body.get("cover", True),
                             body.get("details", True))
    r = await asyncio.get_running_loop().run_in_executor(None, work)
    return {"result": r, "song": library.by_id(cid)}


@app.post("/api/song/{cid}/cover")
def set_cover(cid: int, body: dict = Body(...)):
    """Incrusta una imagen del disco como caratula de la cancion."""
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    src = str(body.get("path") or "").strip()
    if not src or not os.path.isfile(src):
        raise HTTPException(400, "esa imagen no existe")
    r = convert.shrink_image(src)
    if not r:
        raise HTTPException(400, "no se pudo leer esa imagen")
    data, mime = r
    if not tags.write_cover(c["path"], data, mime):
        raise HTTPException(500, "no se pudo incrustar la imagen")
    tags.forget_cover(c["path"])
    library.update(cid, cover="embedded")
    return {"ok": True, "kb": len(data) // 1024, "song": library.by_id(cid)}


@app.post("/api/song/{cid}/autofill")
async def autofill_song(cid: int):
    """Completa la ficha con IA. Dice que relleno y que no pudo, con el motivo."""
    r = await asyncio.get_running_loop().run_in_executor(None, enrich.autofill, cid)
    return {**r, "song": library.by_id(cid)}


@app.get("/api/song/{cid}/details")
async def details(cid: int):
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    import json
    if c["chords"]:
        try:
            return {"details": json.loads(c["chords"]), "cached": True}
        except Exception:
            pass
    d = await asyncio.get_running_loop().run_in_executor(None, enrich.details, c)
    if d and not d.get("error"):
        library.update(cid, chords=json.dumps(d, ensure_ascii=False))
    return {"details": d, "cached": False}


@app.post("/api/chat")
async def converse(body: dict = Body(...)):
    """Chat con acceso a la biblioteca. `mensajes`: [{rol: 'yo'|'ia', texto}]"""
    messages = body.get("messages") or []
    if not messages:
        raise HTTPException(400, "no hay mensajes")
    def work():
        return chat.reply(messages)
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.get("/api/chat/tools")
def chat_tools():
    return {"model": config.DEEPINFRA_CHAT_MODEL,
            "available": ai.available(),
            "tools": [{"name": h["function"]["name"],
                              "description": h["function"]["description"]}
                             for h in chat.TOOLS]}


@app.post("/api/transpose")
def transpose(body: dict = Body(...)):
    text = body.get("text", "")
    if body.get("from_key") and body.get("to_key"):
        out = theory.transpose_to(text, body["from_key"], body["to_key"])
    else:
        out = theory.transpose(text, int(body.get("semitonos", 0)))
    return {"text": out,
            "latin": theory.to_latin(out),
            "capo": theory.suggested_capo(body.get("to_key", "") or ""),
            "keys": theory.available_keys()}


# ---------------------------------------------------------------- importar

@app.get("/api/inbox")
def inbox():
    config.INBOX.mkdir(parents=True, exist_ok=True)
    files = [{"name": p.name, "bytes": p.stat().st_size,
                 "convertible": convert.needs_convert(str(p))}
                for p in sorted(config.INBOX.iterdir())
                if p.is_file() and p.suffix.lower() in config.EXTENSIONS]
    return {"files": files, "total": len(files)}


@app.post("/api/import")
async def run_import(body: dict = Body(default={})):
    def work():
        rs = ingest.process_inbox(dry_run=body.get("dry_run", False),
                                 convert_mp3=body.get("convert"))
        for r in rs:                     # que aparezcan sin reescanear todo
            if r.target and r.action != "dry_run":
                library.index_file(str(r.target))
        return [{"source_path": r.source_path.name,
                 "target": str(r.target.relative_to(config.LIBRARY)) if r.target else "",
                 "artist": r.artist, "title": r.title, "source": r.source,
                 "confidence": r.confidence, "action": r.action, "warnings": r.warnings}
                for r in rs]
    r = await asyncio.get_running_loop().run_in_executor(None, work)
    return {"results": r}


@app.get("/api/convertible")
def convertible_files():
    c = convert.candidates()
    return {"total": len(c), "files": [str(Path(x).relative_to(config.LIBRARY))
                                          for x in c[:200]],
            "protected": sorted(config.NEVER_CONVERT)}


@app.post("/api/convert")
async def convert_batch(body: dict = Body(default={})):
    def work():
        return convert.convert_batch(quality=body.get("quality", config.MP3_QUALITY),
                                 conservar_original=body.get("keep", False),
                                 dry_run=body.get("dry_run", True))
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.post("/api/duplicates/resolve")
async def resolve_duplicate(body: dict = Body(...)):
    """Conserva una copia y borra las demas. Reindexa despues."""
    base = str(config.LIBRARY)
    def full_path(r):
        return r if os.path.isabs(r) else os.path.join(base, r)
    keep = full_path(body.get("keep", ""))
    remove = [full_path(b) for b in body.get("remove", [])]
    dry_run = bool(body.get("dry_run"))

    def work():
        r = duplicates.resolve(keep, remove, dry_run)
        if r["ok"] and not dry_run:
            # Solo se pone al dia lo que ha cambiado. Antes esto lanzaba un
            # escaneo COMPLETO —releer las etiquetas de toda la biblioteca—
            # para reflejar que se han borrado una o dos copias.
            for gone in r["deleted"]:
                library.forget_path(gone)
            if r["kept"] != keep:            # se le quito el sufijo ' - r'
                library.forget_path(keep)
            library.index_file(r["kept"])
        return r
    r = await asyncio.get_running_loop().run_in_executor(None, work)
    if r["ok"] and not dry_run:
        r["stats"] = library.stats_of()
    return r


@app.get("/api/duplicates")
async def duplicates_report():
    """Grupos de duplicados, con los datos de cada cancion para poder oirlas."""
    def work():
        index = library.brief_by_path()
        paths = list(index)
        def enrich(groups):
            out = []
            for g in groups:
                items = []
                for r in sorted(g):
                    c = index.get(r)
                    items.append({
                        "id": c["id"] if c else None,
                        "path": r,
                        "relativa": os.path.relpath(r, config.LIBRARY),
                        "file": os.path.basename(r),
                        "artist": (c or {}).get("artist", ""),
                        "title": (c or {}).get("title", ""),
                        "duration": (c or {}).get("duration", 0),
                        "bitrate": (c or {}).get("bitrate", 0),
                        "size": (c or {}).get("size", 0),
                        "stars": (c or {}).get("stars", 0),
                        "favorite": (c or {}).get("favorite", 0),
                        "tiene_sufijo": duplicates.name_without_suffix(os.path.basename(r)) != os.path.basename(r),
                    })
                # se sugiere la de mejor calidad como candidata a conservar
                # a igualdad de calidad, gana la que ya tiene el nombre limpio
                best = max(items, key=lambda t: (t["bitrate"], t["size"],
                                                  not t["tiene_sufijo"]))
                out.append({"items": items, "sugerida": best["path"]})
            return out
        return {"identical": enrich(duplicates.identical(paths)),
                "similar": enrich(duplicates.same_song(paths, config.DUPLICATE_THRESHOLD))}
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.get("/api/song/{cid}/path")
def audio_path(cid: int):
    """Devuelve la ruta en disco. La usa Rust para servir el audio sin pasar por Python."""
    c = library.by_id(cid)
    if not c or not os.path.exists(c["path"]):
        raise HTTPException(404, "no existe")
    return {"path": c["path"], "kind": "audio/mpeg", "bytes": os.path.getsize(c["path"])}


# ---------------------------------------------------------------- YouTube
# El estado vive en youtube.ESTADO porque lo comparten esta pagina y el
# asistente: una descarga a la vez y un solo sitio donde mirar como va.


@app.get("/api/youtube")
def youtube_status():
    return {"available": youtube.available(), "reason": youtube.unavailable_reason(),
            "quality": config.MP3_QUALITY, **youtube.STATE}


@app.post("/api/youtube/info")
async def youtube_info(body: dict = Body(default={})):
    """Que se bajaria, sin bajar nada todavia."""
    query = (body.get("query") or "").strip()
    if not query:
        raise HTTPException(400, "hace falta una URL o algo que buscar")
    def work():
        return youtube.info(query, int(body.get("results", 5)))
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.post("/api/youtube/download")
async def youtube_download(body: dict = Body(default={})):
    """Arranca la descarga y vuelve enseguida. El avance se consulta en /api/youtube."""
    if youtube.STATE["active"]:
        raise HTTPException(409, "ya hay una descarga en marcha")
    query = (body.get("query") or "").strip()
    if not query:
        raise HTTPException(400, "hace falta una URL o algo que buscar")
    if not youtube.available():
        raise HTTPException(503, youtube.unavailable_reason())

    asyncio.get_running_loop().run_in_executor(
        None, lambda: youtube.run_job(query,
                                      quality=body.get("quality") or config.MP3_QUALITY,
                                      file_it=body.get("file_it", True),
                                      results=int(body.get("results", 5)),
                                      force=bool(body.get("force", False))))
    return {"ok": True, "active": True}


@app.get("/api/downloads/history")
def downloads_history(limit: int = Query(60, ge=1, le=500),
                      offset: int = Query(0, ge=0)):
    """Todo lo descargado, del boton o del asistente, lo mas reciente arriba."""
    return {"items": library.download_history(limit, offset),
            "total": library.download_count()}


@app.delete("/api/downloads/history")
def clear_downloads_history():
    return {"removed": library.clear_download_history()}


@app.post("/api/youtube/cancel")
def youtube_cancel():
    youtube.cancel()
    return {"ok": True}


def _watch_parent(intervalo=2.0):
    """Se apaga si el proceso padre (la app) desaparece: nada de nucleos huerfanos."""
    import signal, threading, time as _t
    parent = os.getppid()
    if parent <= 1:
        return
    def bucle():
        while True:
            _t.sleep(intervalo)
            if os.getppid() != parent:
                try:
                    os.kill(os.getpid(), signal.SIGTERM)
                except Exception:
                    pass
                _t.sleep(2)
                os._exit(0)
    threading.Thread(target=bucle, daemon=True).start()


def serve(host="127.0.0.1", port=8730, uds=None):
    global _ENFORCE_HOST
    """Levanta la API. Con `uds` escucha en un socket Unix (sin port TCP abierto),
    que es como la usa la app de escritorio: solo Rust puede hablar con ella."""
    import uvicorn
    _watch_parent()
    _ENFORCE_HOST = not uds
    if uds:
        import socket as _s
        p = Path(uds)
        p.parent.mkdir(parents=True, exist_ok=True)
        if p.exists():
            p.unlink()
        # uvicorn hace chmod 0666 al socket que crea el mismo, asi que lo creamos
        # nosotros con permisos de usuario y se lo pasamos ya escuchando.
        sock = _s.socket(_s.AF_UNIX, _s.SOCK_STREAM)
        sock.bind(str(p))
        os.chmod(p, 0o600)
        sock.listen(128)
        try:
            uvicorn.run(app, fd=sock.fileno(), log_level="warning")
        finally:
            sock.close()
            if p.exists():
                p.unlink()
        return

    uvicorn.run(app, host=host, port=port, log_level="warning")
