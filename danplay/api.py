# -*- coding: utf-8 -*-
"""API HTTP local sobre el mismo nucleo que usa la CLI.

La app de escritorio (Tauri + Vue) habla con esto en localhost.
"""
import asyncio, hashlib, hmac, logging, mimetypes, os, threading, time
from pathlib import Path
from typing import Literal

from fastapi import FastAPI, HTTPException, Query, Body, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse, Response
from pydantic import BaseModel, ConfigDict, Field

from . import (ai, chat, config, convert, duplicates, enrich,
               fingerprint, ingest, library, playlists,
               tags, theory, youtube)

from . import __version__

log = logging.getLogger("danplay.api")

# Cada formato con su tipo. El navegador rechaza un .flac anunciado como mp3.
AUDIO_TYPES = {
    ".mp3": "audio/mpeg", ".flac": "audio/flac", ".ogg": "audio/ogg",
    ".opus": "audio/opus", ".m4a": "audio/mp4", ".aac": "audio/aac",
    ".wav": "audio/wav", ".wma": "audio/x-ms-wma",
}


class Body_(BaseModel):
    """Base de todos los cuerpos: lo que no se espera, se rechaza.

    Antes se pasaba el diccionario entero a la funcion del nucleo, asi que
    una clave de mas era un 500 y, en el caso de editar, una forma de tocar
    campos que solo debe cambiar el propio programa.
    """
    model_config = ConfigDict(extra="forbid")


class SongEdit(Body_):
    """Lo unico que se puede editar a mano de una cancion."""
    artist: str | None = Field(default=None, max_length=300)
    title: str | None = Field(default=None, max_length=300)
    album: str | None = Field(default=None, max_length=300)
    year: str | None = Field(default=None, max_length=10)
    genre: str | None = Field(default=None, max_length=120)
    key: str | None = Field(default=None, max_length=12)
    bpm: float | None = Field(default=None, ge=0, le=400)
    lyrics: str | None = Field(default=None, max_length=200_000)


EDITABLE = tuple(SongEdit.model_fields)


class Stars(Body_):
    stars: int = Field(ge=0, le=5)


class Favorite(Body_):
    favorite: bool = True


class Blur(Body_):
    blur: bool = True


class FolderIn(Body_):
    path: str = Field(min_length=1, max_length=4096)
    label: str = Field(default="", max_length=120)
    force: bool = False


class PathIn(Body_):
    path: str = Field(min_length=1, max_length=4096)


class ExclusionIn(Body_):
    pattern: str = Field(min_length=1, max_length=512)
    kind: Literal["glob", "path"] = "glob"
    note: str = Field(default="", max_length=300)


class ConvertIn(Body_):
    quality: Literal["high", "medium", "variable"] | None = None
    keep: bool = False
    dry_run: bool = True


class ResolveIn(Body_):
    keep: str = Field(min_length=1, max_length=4096)
    remove: list[str] = Field(default_factory=list, max_length=200)
    # Borrar es lo excepcional: hay que pedirlo. Antes bastaba con no decir
    # nada y se borraba de verdad.
    dry_run: bool = True


class ImportIn(Body_):
    dry_run: bool = False
    convert: bool | None = None


class EnrichIn(Body_):
    lyrics: bool = True
    cover: bool = True
    details: bool = True


class TransposeIn(Body_):
    text: str = Field(default="", max_length=100_000)
    from_key: str = Field(default="", max_length=12)
    to_key: str = Field(default="", max_length=12)
    semitones: int = Field(default=0, ge=-24, le=24)


class PlaylistIn(Body_):
    name: str = Field(default="Nueva lista", min_length=1, max_length=200)
    note: str = Field(default="", max_length=500)
    color: str = Field(default="", max_length=32)


class SongsIn(Body_):
    ids: list[int] = Field(default_factory=list, max_length=5000)
    id: int | None = None


class OrderIn(Body_):
    ids: list[int] = Field(default_factory=list, max_length=5000)


class SettingsIn(Body_):
    convert_mp3: bool | None = None
    keep_original: bool | None = None
    write_tags: bool | None = None
    ai_enabled: bool | None = None
    quality: Literal["high", "medium", "variable"] | None = None
    model: str | None = Field(default=None, max_length=200)
    ai_key: str | None = Field(default=None, max_length=400)
    fingerprint_key: str | None = Field(default=None, max_length=400)
    library: str | None = Field(default=None, max_length=4096)


class YoutubeIn(Body_):
    query: str = Field(default="", max_length=2000)
    results: int = Field(default=5, ge=1, le=20)
    quality: Literal["high", "medium", "variable"] | None = None
    file_it: bool = True
    force: bool = False


class ChatIn(Body_):
    messages: list[dict] = Field(default_factory=list, max_length=200)


class ChatConfirmIn(Body_):
    tool: str = Field(min_length=1, max_length=64)
    args: dict = Field(default_factory=dict)


def audio_type(path) -> str:
    ext = os.path.splitext(str(path))[1].lower()
    return AUDIO_TYPES.get(ext) or mimetypes.guess_type(str(path))[0] or "application/octet-stream"

app = FastAPI(title="DanPlay", version=__version__)

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

# En Windows no hay sockets Unix que uvicorn sepa escuchar, asi que la app
# habla por TCP en loopback. Para que «solo la app» siga siendo cierto, Rust
# genera un secreto al arrancar y lo pasa por el entorno: sin el, 401.
_TOKEN = ""

# Cuerpos de peticion: 1 MB de sobra para todo menos el chat, que lleva el
# historial de la conversacion. Sin tope, una letra de 200 MB acababa dentro
# de un mp3.
MAX_BODY = 1 * 1024 * 1024
MAX_CHAT_BODY = 4 * 1024 * 1024


@app.middleware("http")
async def _guard(request: Request, call_next):
    path = request.url.path
    if _TOKEN and path.startswith("/api/"):
        sent = (request.headers.get("authorization") or "")
        expected = f"Bearer {_TOKEN}"
        # comparacion en tiempo constante: el token no se adivina a base de
        # medir cuanto tarda en decir que no
        if not hmac.compare_digest(sent, expected):
            return Response(status_code=401, content=b"hace falta el token de la aplicacion")

    if _ENFORCE_HOST:
        host = (request.headers.get("host") or "").rsplit(":", 1)[0]
        if host not in ALLOWED_HOSTS:
            return Response(status_code=421, content=b"host no permitido")
        # CORS impide LEER la respuesta, pero no evita que la peticion pase:
        # un POST sin cuerpo desde cualquier pagina abierta en el navegador
        # disparaba un escaneo o una importacion. Exigir una cabecera propia
        # obliga al navegador a preguntar antes (preflight), y ahi CORS si
        # corta.
        if request.method not in ("GET", "HEAD", "OPTIONS"):
            if request.headers.get("x-danplay") != "1":
                return Response(status_code=403, content=b"falta la cabecera X-DanPlay")

    length = request.headers.get("content-length")
    if length and length.isdigit():
        tope = MAX_CHAT_BODY if path.startswith("/api/chat") else MAX_BODY
        if int(length) > tope:
            return Response(status_code=413, content=b"eso es demasiado grande")
    return await call_next(request)

# estado de tareas largas (escaneo, analisis, conversion)
JOBS: dict[str, dict] = {}
_JOBS_LOCK = threading.Lock()
# Cuanto se guarda un trabajo ya terminado antes de olvidarlo. Sin esto,
# `JOBS` crecia para siempre y se devolvia entero en cada /api/status.
JOB_TTL = 600


def _job(name) -> dict:
    """Reserva el trabajo. Si ya hay uno igual en marcha, no deja empezar otro."""
    with _JOBS_LOCK:
        now = time.time()
        for key, job in list(JOBS.items()):
            if not job.get("active") and now - job.get("ended", now) > JOB_TTL:
                del JOBS[key]
        current = JOBS.get(name)
        if current and current.get("active"):
            raise HTTPException(409, f"ya hay un(a) {name} en marcha")
        JOBS[name] = {"active": True, "done": 0, "total": 0, "message": ""}
        return JOBS[name]


def _job_done(job: dict, message="") -> None:
    job.update(active=False, message=message, ended=time.time())


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
def save_settings(body: SettingsIn = Body(...)):
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
    for k, v in body.model_dump(exclude_none=True).items():
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
def check_folder(body: PathIn = Body(...)):
    """Mira si la carpeta repite musica ya indexada, antes de añadirla."""
    path = body.path
    if not os.path.isdir(os.path.expanduser(path)):
        raise HTTPException(400, "esa ruta no existe")
    return {"notice": library.check_overlap(path)}


@app.post("/api/folders")
def add_folder(body: FolderIn = Body(...)):
    """Añade una carpeta. Es idempotente: repetir la misma no duplica nada.

    accion:
      ya_estaba   -> ya indexada (o cubierta por otra); no se toca nada
      reemplaza   -> la nueva contiene a otra ya indexada; sustituye a la interna
      confirmar   -> parece una copia de otra distinta; hace falta force
      agregada    -> añadida
    """
    path = body.path
    if not os.path.isdir(os.path.expanduser(path)):
        raise HTTPException(400, "esa ruta no existe")

    notice = library.check_overlap(path)
    kind = (notice or {}).get("kind")

    if kind in ("same", "inside"):
        return {"action": "already_there", "notice": notice, **folders()}

    if kind == "contains":
        library.remove_folder(notice["other"])
        library.add_folder(path, body.label)
        return {"action": "replaced", "notice": notice, **folders()}

    if kind == "copy" and not body.force:
        return {"action": "confirm", "notice": notice, **folders()}

    if not library.add_folder(path, body.label):
        raise HTTPException(400, "esa ruta no existe")
    return {"action": "added", "notice": notice, **folders()}


@app.delete("/api/folders")
def remove_folder(path: str = Query(...)):
    library.remove_folder(path)
    return folders()


@app.post("/api/exclusions")
def add_exclusion(body: ExclusionIn = Body(...)):
    library.add_exclusion(body.pattern, body.kind, body.note)
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
            _job_done(t, f"{r['total']} canciones ({r['added_count']} nuevas)")
        except Exception as ex:                              # noqa: BLE001
            log.warning("el escaneo fallo", exc_info=True)
            _job_done(t, f"error: {ex}")
    await asyncio.get_running_loop().run_in_executor(None, work)
    return {"job": t, "stats": library.stats_of()}


# ---------------------------------------------------------------- busqueda

@app.get("/api/search")
def search(q: str = "", sort: str = "artist", desc: bool = False,
           limit: int = Query(200, ge=1, le=5000),
           from_key: int = Query(0, ge=0), artist: str = "", album: str = "",
           genre: str = "", folder: str = "", only_favorites: bool = False,
           min_stars: int = 0):
    filters = {k: v for k, v in
               {"artist": artist, "album": album, "genre": genre,
                "folder": folder}.items() if v}
    rows = library.search(q, filters, sort, limit, from_key,
                          only_favorites=only_favorites, min_stars=min_stars,
                          desc=desc)
    return {"total": len(rows), "songs": rows}


@app.get("/api/facets")
def facets():
    """Valores disponibles para los filtros, y por que se puede ordenar.

    La lista de campos sale del nucleo para que la interfaz no tenga que
    repetirla: si aqui se añade uno, aparece solo en el buscador avanzado.
    """
    return {**library.facets(),
            "sorts": library.sort_options(),
            "filters": sorted(set(library.FILTER_FIELDS.values())),
            "numeric": sorted(set(library.NUMERIC_FIELDS.values()))}


@app.get("/api/song/{cid}")
def song(cid: int):
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    c["playlists"] = playlists.playlists_of(cid)
    c["existe"] = os.path.exists(c["path"])
    return c


@app.patch("/api/song/{cid}")
def edit(cid: int, body: SongEdit = Body(...)):
    """Solo los campos que el usuario puede corregir a mano.

    Antes se pasaba el cuerpo entero, asi que se podian poner estrellas o el
    favorito directamente en la base, sin escribirlos en el archivo: el
    indice decia una cosa y el mp3 otra.
    """
    c = library.edit(cid, **body.model_dump(exclude_none=True))
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
    return FileResponse(c["path"], media_type=audio_type(c["path"]), filename=c["file"])


# Tamaños de miniatura que se generan y se guardan. Pedir otro devuelve la
# imagen tal cual: si no, cada pixel distinto seria un archivo nuevo en cache.
THUMBNAIL_SIZES = (96, 192, 320)


def _thumbnail(path: str, size: int) -> tuple[bytes, str] | None:
    """Miniatura cuadrada de la caratula, guardada en disco para la proxima.

    Una lista de mil canciones pedia mil caratulas completas —a menudo de dos
    megas cada una— para pintarlas a 40 pixeles. Ahora se pide el tamaño que
    se va a enseñar.
    """
    original = tags.cached_cover(path)
    if not original:
        return None
    try:
        stamp = os.path.getmtime(path)
    except OSError:
        stamp = 0
    key = hashlib.sha1(f"{path}:{stamp}:{size}".encode()).hexdigest()
    folder = config.DATA_DIR / "covers"
    folder.mkdir(parents=True, exist_ok=True)
    cached = folder / f"{key}.jpg"
    if cached.is_file():
        return cached.read_bytes(), "image/jpeg"
    made = convert.shrink_bytes(original[0], original[1], max_side=size)
    if not made:
        return original
    try:
        cached.write_bytes(made[0])
    except OSError:
        log.warning("no pude guardar la miniatura %s", cached, exc_info=True)
    return made


@app.get("/api/song/{cid}/cover")
def cover(cid: int, size: int | None = Query(default=None)):
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    if not os.path.exists(c["path"]):
        raise HTTPException(404, "sin portada")
    r = _thumbnail(c["path"], size) if size in THUMBNAIL_SIZES else tags.cached_cover(c["path"])
    if not r:
        raise HTTPException(404, "sin portada")
    return Response(content=r[0], media_type=r[1],
                    headers={"cache-control": "private, max-age=300"})


@app.post("/api/song/{cid}/stars")
def stars(cid: int, body: Stars = Body(...)):
    playlists.rate(cid, body.stars)
    return library.by_id(cid)


@app.post("/api/song/{cid}/favorite")
def favorite(cid: int, body: Favorite = Body(...)):
    playlists.favorite(cid, body.favorite)
    return library.by_id(cid)


@app.post("/api/song/{cid}/blur")
def blur_cover(cid: int, body: Blur = Body(default=Blur())):
    """Difumina la portada al pintarla. La imagen no se toca.

    Para portadas que uno no quiere tener delante. Se guarda dentro del mp3,
    asi que la decision no se pierde ni al rehacer el indice.
    """
    c = library.set_blur(cid, body.blur)
    if not c:
        raise HTTPException(404, "no existe")
    return c


# ---------------------------------------------------------------- listas

@app.get("/api/playlists")
def list_playlists():
    return {"playlists": playlists.list_all(), "favorites": len(playlists.favorites())}


@app.post("/api/playlists")
def create_playlist(body: PlaylistIn = Body(...)):
    # `created` importa: con un nombre repetido se devuelve la que ya habia,
    # y la interfaz tiene que poder decirlo en vez de fingir que creo una.
    made = playlists.create(body.name, body.note, body.color)
    return {**made, "playlists": playlists.list_all()}


@app.delete("/api/playlists/{lid}")
def delete_playlist(lid: int):
    playlists.remove(lid)
    return {"playlists": playlists.list_all()}


@app.get("/api/playlists/{lid}/songs")
def playlist_songs_of(lid: int):
    return {"songs": playlists.songs(lid)}


@app.post("/api/playlists/{lid}/songs")
def add_to_playlist(lid: int, body: SongsIn = Body(...)):
    ids = body.ids or ([body.id] if body.id else [])
    added = playlists.add(lid, [int(i) for i in ids if i])
    return {"added": added, "songs": playlists.songs(lid)}


@app.delete("/api/playlists/{lid}/songs/{cid}")
def remove_from_playlist(lid: int, cid: int):
    playlists.remove_song(lid, cid)
    return {"songs": playlists.songs(lid)}


@app.post("/api/playlists/{lid}/order")
def reorder(lid: int, body: OrderIn = Body(...)):
    playlists.reorder(lid, body.ids)
    return {"songs": playlists.songs(lid)}


@app.post("/api/playlists/{lid}/export")
def export(lid: int):
    return {"file": playlists.export_m3u(lid)}


# ---------------------------------------------------------------- IA

@app.post("/api/song/{cid}/enrich")
async def enrich_song(cid: int, body: EnrichIn = Body(default=EnrichIn())):
    def work():
        return enrich.enrich(cid, body.lyrics, body.cover, body.details)
    r = await asyncio.get_running_loop().run_in_executor(None, work)
    return {"result": r, "song": library.by_id(cid)}


@app.post("/api/song/{cid}/cover")
def set_cover(cid: int, body: PathIn = Body(...)):
    """Incrusta una imagen del disco como caratula de la cancion.

    Se comprueba que sea una imagen de verdad ANTES de pasarsela a ffmpeg y
    de meterla en el mp3: por aqui se podia leer cualquier archivo del disco
    y recuperarlo despues pidiendo la caratula.
    """
    c = library.by_id(cid)
    if not c:
        raise HTTPException(404, "no existe")
    src = body.path.strip()
    if not src or not os.path.isfile(src):
        raise HTTPException(400, "esa imagen no existe")
    if os.path.splitext(src)[1].lower() not in (".jpg", ".jpeg", ".png", ".webp"):
        raise HTTPException(400, "solo valen imagenes jpg, png o webp")
    try:
        with open(src, "rb") as f:
            head = f.read(16)
    except OSError:
        raise HTTPException(400, "no se pudo leer esa imagen")
    if not enrich.image_type(head):
        raise HTTPException(400, "ese archivo no es una imagen")
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
async def converse(body: ChatIn = Body(...)):
    """Chat con acceso a la biblioteca. `messages`: [{role: 'user'|'ai', text}]"""
    if not body.messages:
        raise HTTPException(400, "no hay mensajes")
    def work():
        return chat.reply(body.messages)
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.post("/api/chat/confirm")
async def confirm_tool(body: ChatConfirmIn = Body(...)):
    """Ejecuta lo que el asistente pidio y la persona acaba de aprobar.

    Lo que no tiene vuelta atras no lo hace el modelo por su cuenta: devuelve
    lo que iba a hacer y hasta que no pasa por aqui no ocurre nada.
    """
    if body.tool not in chat.NEEDS_CONFIRMATION:
        raise HTTPException(400, "eso no necesita confirmacion")
    # Descargar tarda minutos: se arranca y se contesta enseguida, igual que
    # el boton de Descargas. Antes bloqueaba la peticion del chat.
    if body.tool == "download_music":
        if youtube.STATE["active"]:
            raise HTTPException(409, "ya hay una descarga en marcha")
        args = dict(body.args)
        query = str(args.get("query") or "").strip()
        if not query:
            raise HTTPException(400, "hace falta algo que descargar")
        with _DOWNLOAD_LOCK:
            if youtube.STATE["active"]:
                raise HTTPException(409, "ya hay una descarga en marcha")
            youtube.STATE["active"] = True
        asyncio.get_running_loop().run_in_executor(
            None, lambda: youtube.run_job(query, quality=config.MP3_QUALITY,
                                          file_it=True, results=5,
                                          force=bool(args.get("force"))))
        return {"ok": True, "result": {"active": True},
                "text": "Descargando. Te lo cuento en Descargas."}

    def work():
        return chat.confirm(body.tool, body.args)
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.get("/api/chat/tools")
def chat_tools():
    return {"model": config.DEEPINFRA_CHAT_MODEL,
            "available": ai.available(),
            "tools": [{"name": h["function"]["name"],
                              "description": h["function"]["description"]}
                             for h in chat.TOOLS]}


@app.post("/api/transpose")
def transpose(body: TransposeIn = Body(...)):
    if body.from_key and body.to_key:
        out = theory.transpose_to(body.text, body.from_key, body.to_key)
    else:
        out = theory.transpose(body.text, body.semitones)
    return {"text": out,
            "latin": theory.to_latin(out),
            "capo": theory.suggested_capo(body.to_key),
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
async def run_import(body: ImportIn = Body(default=ImportIn())):
    def work():
        rs = ingest.process_inbox(dry_run=body.dry_run, convert_mp3=body.convert)
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
async def convert_batch(body: ConvertIn = Body(default=ConvertIn())):
    def work():
        return convert.convert_batch(quality=body.quality or config.MP3_QUALITY,
                                     keep_original=body.keep,
                                     dry_run=body.dry_run)
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.post("/api/duplicates/resolve")
async def resolve_duplicate(body: ResolveIn = Body(...)):
    """Conserva una copia y manda las demas a la papelera. Reindexa despues.

    Las rutas vienen de fuera, asi que `duplicates.resolve` comprueba que
    esten dentro de las carpetas gestionadas y que sean audio. Aqui solo se
    completan las relativas.
    """
    base = str(config.LIBRARY)
    def full_path(r):
        return r if os.path.isabs(r) else os.path.join(base, r)
    keep = full_path(body.keep)
    remove = [full_path(b) for b in body.remove]
    dry_run = body.dry_run

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
    if not r["ok"] and r.get("reason"):
        raise HTTPException(400, r["reason"])
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
                        "relative": os.path.relpath(r, config.LIBRARY),
                        "file": os.path.basename(r),
                        "artist": (c or {}).get("artist", ""),
                        "title": (c or {}).get("title", ""),
                        "duration": (c or {}).get("duration", 0),
                        "bitrate": (c or {}).get("bitrate", 0),
                        "size": (c or {}).get("size", 0),
                        "stars": (c or {}).get("stars", 0),
                        "favorite": (c or {}).get("favorite", 0),
                        "has_suffix": duplicates.name_without_suffix(os.path.basename(r)) != os.path.basename(r),
                    })
                # se sugiere la de mejor calidad como candidata a conservar
                # a igualdad de calidad, gana la que ya tiene el nombre limpio
                best = max(items, key=lambda t: (t["bitrate"], t["size"],
                                                  not t["has_suffix"]))
                out.append({"items": items, "suggested": best["path"]})
            return out
        return {"identical": enrich(duplicates.identical(paths)),
                "similar": enrich(duplicates.same_song(paths, config.DUPLICATE_THRESHOLD))}
    return await asyncio.get_running_loop().run_in_executor(None, work)


class PathIn(Body_):
    path: str


@app.post("/api/by-path")
def song_by_path(body: PathIn):
    """¿Esta esta ruta en la biblioteca? La pregunta Rust al abrir un archivo
    desde fuera («Abrir con DanPlay»).

    Es POST y no GET porque la clave es una ruta: con acentos, espacios, `&` y
    `#` dentro, meterla en la parte de consulta de la URL es pedir un fallo de
    codificacion. Solo MIRA el indice —no abre el archivo ni toca el disco—,
    asi que preguntar por una ruta cualquiera no revela nada de ella salvo si
    esta o no en la biblioteca, que es justo lo que se pregunta.
    """
    song = library.by_path(body.path)
    return {"song": song}


@app.get("/api/song/{cid}/path")
def audio_path(cid: int):
    """Devuelve la ruta en disco. La usa Rust para servir el audio sin pasar por Python."""
    c = library.by_id(cid)
    if not c or not os.path.exists(c["path"]):
        raise HTTPException(404, "no existe")
    return {"path": c["path"], "kind": audio_type(c["path"]),
            "bytes": os.path.getsize(c["path"])}


# ---------------------------------------------------------------- YouTube
# El estado vive en youtube.ESTADO porque lo comparten esta pagina y el
# asistente: una descarga a la vez y un solo sitio donde mirar como va.


@app.get("/api/youtube")
def youtube_status():
    return {"available": youtube.available(), "reason": youtube.unavailable_reason(),
            "quality": config.MP3_QUALITY, **youtube.STATE}


@app.post("/api/youtube/info")
async def youtube_info(body: YoutubeIn = Body(default=YoutubeIn())):
    """Que se bajaria, sin bajar nada todavia."""
    query = body.query.strip()
    if not query:
        raise HTTPException(400, "hace falta una URL o algo que buscar")
    def work():
        return youtube.info(query, body.results)
    return await asyncio.get_running_loop().run_in_executor(None, work)


# Una descarga a la vez. La comprobacion y el arranque van juntos bajo el
# mismo cerrojo: dos peticiones seguidas arrancaban dos descargas.
_DOWNLOAD_LOCK = threading.Lock()


@app.post("/api/youtube/download")
async def youtube_download(body: YoutubeIn = Body(default=YoutubeIn())):
    """Arranca la descarga y vuelve enseguida. El avance se consulta en /api/youtube."""
    query = body.query.strip()
    if not query:
        raise HTTPException(400, "hace falta una URL o algo que buscar")
    if not youtube.available():
        raise HTTPException(503, youtube.unavailable_reason())

    with _DOWNLOAD_LOCK:
        if youtube.STATE["active"]:
            raise HTTPException(409, "ya hay una descarga en marcha")
        youtube.STATE["active"] = True
    asyncio.get_running_loop().run_in_executor(
        None, lambda: youtube.run_job(query,
                                      quality=body.quality or config.MP3_QUALITY,
                                      file_it=body.file_it,
                                      results=body.results,
                                      force=body.force))
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
    """Levanta la API.

    Con `uds` escucha en un socket Unix, que es como la usa la app de
    escritorio en Linux y macOS: sin puerto abierto, y con permisos 0600 solo
    tu usuario puede hablar con ella.

    En Windows no hay sockets Unix que uvicorn sepa escuchar, asi que la app
    arranca esto en loopback y le pasa un secreto por `DANPLAY_TOKEN`; sin esa
    cabecera, la API contesta 401 a todo.
    """
    global _ENFORCE_HOST, _TOKEN
    import uvicorn
    logging.basicConfig(level=logging.INFO, format="danplay: %(message)s")
    _watch_parent()
    _TOKEN = os.environ.get("DANPLAY_TOKEN", "")
    # La comprobacion de Host y la cabecera propia son cosa de navegadores:
    # por el socket no hay ninguno, y con token tampoco hacen falta.
    _ENFORCE_HOST = not uds and not _TOKEN
    if uds:
        import socket as _s
        p = Path(uds)
        p.parent.mkdir(parents=True, exist_ok=True)
        if p.exists():
            p.unlink()
        # uvicorn hace chmod 0666 al socket que crea el mismo, asi que lo
        # creamos nosotros. Con la umask puesta ANTES del bind: entre el bind
        # y el chmod habia un instante en el que el socket era de todos.
        previous = os.umask(0o077)
        try:
            sock = _s.socket(_s.AF_UNIX, _s.SOCK_STREAM)
            sock.bind(str(p))
        finally:
            os.umask(previous)
        os.chmod(p, 0o600)
        sock.listen(128)
        log.info("escuchando en %s", p)
        try:
            uvicorn.run(app, fd=sock.fileno(), log_level="warning")
        finally:
            sock.close()
            if p.exists():
                p.unlink()
        return

    log.info("escuchando en http://%s:%s%s", host, port,
             " (con token)" if _TOKEN else "")
    uvicorn.run(app, host=host, port=port, log_level="warning")
