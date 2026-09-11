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

from . import (ai, chat, chats, config, convert, duplicates, enrich, external,
               fingerprint, ingest, library, model_catalog, playlists,
               providers, tags, theory, youtube)

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


class PlaylistName(Body_):
    name: str = Field(min_length=1, max_length=120)


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


class PlaylistEdit(Body_):
    """Lo que se puede cambiar de una lista ya creada."""
    name: str | None = Field(default=None, min_length=1, max_length=200)
    note: str | None = Field(default=None, max_length=500)


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
    # `model` y `ai_key` van al perfil de IA activo (compatibilidad con la
    # interfaz de antes); lo demas de la IA entra por /api/ai/*
    model: str | None = Field(default=None, max_length=200)
    ai_key: str | None = Field(default=None, max_length=400)
    fingerprint_key: str | None = Field(default=None, max_length=400)
    library: str | None = Field(default=None, max_length=4096)


class AiProfileIn(Body_):
    """Un proveedor tal y como lo rellena el formulario de Ajustes.

    Sirve para guardar, para probar sin guardar y para pedir los modelos:
    en los dos ultimos casos, si no trae clave se usa la guardada.
    """
    id: str | None = Field(default=None, max_length=80)
    provider: str = Field(max_length=40)
    name: str | None = Field(default=None, max_length=80)
    key: str | None = Field(default=None, max_length=1000)
    base_url: str | None = Field(default=None, max_length=1000)
    fields: dict[str, str] | None = None
    model: str | None = Field(default=None, max_length=200)
    chat_model: str | None = Field(default=None, max_length=200)
    headers: dict[str, str] | None = None
    extra: dict | None = None
    timeout: float | None = Field(default=None, ge=5, le=600)
    activate: bool = True


class AiActivateIn(Body_):
    id: str = Field(max_length=80)


class YoutubeIn(Body_):
    query: str = Field(default="", max_length=2000)
    results: int = Field(default=5, ge=1, le=20)
    quality: Literal["high", "medium", "variable"] | None = None
    file_it: bool = True
    force: bool = False


class ChatIn(Body_):
    messages: list[dict] = Field(default_factory=list, max_length=200)
    # lo que la persona tiene delante: vista, seleccion, lo que suena (§3)
    context: dict | None = None


class ChatCreateIn(Body_):
    title: str = Field(default="", max_length=120)


class ChatRenameIn(Body_):
    title: str = Field(min_length=1, max_length=120)


class ChatAppendIn(Body_):
    messages: list[dict] = Field(default_factory=list, max_length=200)


class AiFallbackIn(Body_):
    enabled: bool


class SheetIn(Body_):
    with_lyrics: bool = False


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
            "model": ai.fast_model(), "provider": ai.provider_name(), "ai": ai.available(),
            "fingerprint": not fingerprint.unavailable_reason(),
            "fingerprint_reason": fingerprint.unavailable_reason(),
            "convert": config.CONVERT_TO_MP3, "quality": config.MP3_QUALITY,
            "never_convert": sorted(config.NEVER_CONVERT),
            "rust": duplicates.RUST, "ffmpeg": convert.available(), "stats": e,
            "youtube": youtube.available(), "youtube_reason": youtube.unavailable_reason(),
            # cuantas veces ha cambiado algo que se enseña: Rust lo vigila y
            # avisa a las ventanas cuando se mueve (`danplay://changed`)
            "revision": library.revision(),
            "jobs": JOBS}


@app.get("/api/settings")
def settings():
    p = ai.profile()
    return {"convert_mp3": config.CONVERT_TO_MP3, "quality": config.MP3_QUALITY,
            "keep_original": config.KEEP_ORIGINAL,
            "write_tags": config.WRITE_TAGS, "ai_enabled": config.AI_ENABLED,
            "model": ai.fast_model(), "chat_model": ai.chat_model(),
            "provider": p["id"] if p else "", "provider_name": p["name"] if p else "",
            "library": str(config.LIBRARY),
            "ai_key": providers.mask(p["key"]) if p else "",
            "has_ai_key": bool(p and p["key"]),
            "ai_ready": ai.available(), "ai_reason": ai.unavailable_reason(),
            "fingerprint_key": bool(config.ACOUSTID_API_KEY),
            "settings_file": str(config.ENV_FILE),
            "ai_file": str(providers.PROFILES_FILE)}


@app.post("/api/settings")
def save_settings(body: SettingsIn = Body(...)):
    """Las casillas y campos de la app. Se aplican en caliente y se persisten."""
    flags = {"convert_mp3": ("CONVERT_TO_MP3", "DANPLAY_CONVERT"),
             "keep_original": ("KEEP_ORIGINAL", "DANPLAY_KEEP_ORIGINAL"),
             "write_tags": ("WRITE_TAGS", "DANPLAY_TAGS"),
             "ai_enabled": ("AI_ENABLED", "DANPLAY_AI")}
    texts = {"quality": ("MP3_QUALITY", "DANPLAY_QUALITY"),
             "fingerprint_key": ("ACOUSTID_API_KEY", "ACOUSTID_API_KEY")}
    save = {}
    profile_patch = {}
    for k, v in body.model_dump(exclude_none=True).items():
        if k in flags:
            attr, env = flags[k]
            setattr(config, attr, bool(v)); save[env] = "1" if v else "0"
        elif k in texts and isinstance(v, str) and v.strip():
            attr, env = texts[k]
            setattr(config, attr, v.strip()); save[env] = v.strip()
        elif k in ("model", "ai_key") and isinstance(v, str) and v.strip():
            profile_patch["key" if k == "ai_key" else "model"] = v.strip()
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
    if profile_patch:
        # sin proveedor elegido, la clave suelta va a DeepInfra: es lo que
        # significaba antes «clave de IA»
        pid = providers.active_id() or "deepinfra"
        providers.save_profile({"id": pid, "provider": pid if pid in providers.BY_ID else "custom",
                                **profile_patch})
        ai.reset_client()                      # fuerza recrear con la clave nueva
    return settings()


# ---------------------------------------------------------------- carpetas

@app.post("/api/settings/check-ai")
async def check_ai():
    """Prueba el proveedor activo. Lo usa el boton «Probar» de Ajustes."""
    return await asyncio.get_running_loop().run_in_executor(None, ai.check)


# ------------------------------------------------------------ proveedores

def _ai_overview() -> dict:
    """Lo que necesita el modal de IA: catalogo, perfiles guardados (sin
    claves) y el estado del catalogo de modelos."""
    p = ai.profile()
    return {"catalog": providers.catalog(), "groups": providers.GROUPS,
            "profiles": providers.profiles(), "active": providers.active_id(),
            "active_profile": ({"id": p["id"], "provider": p["provider"], "name": p["name"],
                                "model": p["model"], "chat_model": p["chat_model"],
                                "base_url": p["base_url"], "local": p["local"]} if p else None),
            "ai_enabled": config.AI_ENABLED, "ai_ready": ai.available(),
            "ai_reason": ai.unavailable_reason(),
            "fallback": providers.fallback_enabled(),
            "fallbacks": [{"id": f["id"], "name": f["name"], "chat_model": f["chat_model"]}
                          for f in providers.fallbacks(providers.active_id())],
            "catalog_status": model_catalog.status()}


@app.get("/api/ai/providers")
async def ai_providers(refresh: bool = Query(default=False)):
    """Con `refresh`, espera a consultar models.dev (el boton «Actualizar»);
    si no, lo consulta en segundo plano si hace rato de la ultima vez. En
    los dos casos la peticion es condicional: sin cambios, cero bytes."""
    if refresh:
        await asyncio.get_running_loop().run_in_executor(
            None, lambda: model_catalog.refresh(force=True))
    else:
        model_catalog.refresh_in_background(max_age=model_catalog.MIN_INTERVAL)
    return _ai_overview()


@app.post("/api/ai/profile")
def ai_save_profile(body: AiProfileIn = Body(...)):
    data = body.model_dump(exclude_none=True)
    activate = data.pop("activate", True)
    try:
        pid = providers.save_profile(data, activate=activate)
    except ValueError as e:
        raise HTTPException(400, str(e))
    ai.reset_client()
    out = _ai_overview()
    out["saved"] = pid
    return out


@app.delete("/api/ai/profile/{pid}")
def ai_delete_profile(pid: str):
    providers.delete_profile(pid)
    ai.reset_client()
    return _ai_overview()


@app.post("/api/ai/activate")
def ai_activate(body: AiActivateIn = Body(...)):
    try:
        providers.activate(body.id)
    except ValueError as e:
        raise HTTPException(400, str(e))
    ai.reset_client()
    return _ai_overview()


@app.post("/api/ai/free")
async def ai_free():
    """«Probar gratis, sin clave»: prueba los servicios gratuitos por orden y
    deja activo el primero que responda. Tarda lo que tarden en contestar."""
    r = await asyncio.get_running_loop().run_in_executor(None, ai.try_free)
    out = _ai_overview()
    out["free"] = r
    return out


@app.get("/api/ai/usage")
def ai_usage():
    """Lo que gasta la IA: hoy, este mes y en total (tokens y coste)."""
    return library.ai_usage_summary()


@app.post("/api/ai/fallback")
def ai_fallback(body: AiFallbackIn = Body(...)):
    """Si, cuando el proveedor activo falla, se usan los demas configurados."""
    providers.set_fallback(body.enabled)
    ai.reset_client()
    return _ai_overview()


@app.post("/api/ai/check")
async def ai_check(body: AiProfileIn = Body(...)):
    """Prueba lo que hay en el formulario SIN guardarlo: clave, URL, los dos
    modelos y si el de conversacion sabe usar herramientas."""
    draft = body.model_dump(exclude_none=True)
    draft.pop("activate", None)
    return await asyncio.get_running_loop().run_in_executor(None, lambda: ai.check(draft))


@app.post("/api/ai/models")
async def ai_models(body: AiProfileIn = Body(...)):
    """Los modelos que ofrece ese proveedor con esa clave (su /models), mas
    los que conoce el catalogo aunque el proveedor no los liste."""
    draft = body.model_dump(exclude_none=True)
    draft.pop("activate", None)
    live = await asyncio.get_running_loop().run_in_executor(None, lambda: ai.list_models(draft))
    p = providers.BY_ID.get(body.provider) or providers.BY_ID["custom"]
    known = model_catalog.models_for(p["models_dev"])
    live["catalog"] = known
    live["suggest"] = dict(p["suggest"])
    recommended = model_catalog.recommend(known)
    for k, v in recommended.items():
        if v:
            live["suggest"][k] = v
    live["catalog_status"] = model_catalog.status()
    return live


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
    # `resolve`: esto solo LEE, y la ficha de una cancion abierta desde fuera
    # tiene que poder verse igual que la de una de la biblioteca. Editarla,
    # borrarla o ponerle estrellas son otros endpoints, y esos siguen usando
    # `library.by_id`, que no las encuentra: de ahi que no se les pueda tocar.
    c = external.resolve(cid)
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
    # `resolve`: la portada sale del propio archivo, asi que una cancion de
    # fuera de la biblioteca tambien tiene la suya.
    c = external.resolve(cid)
    if not c:
        raise HTTPException(404, "no existe")
    if not os.path.exists(c["path"]):
        raise HTTPException(404, "sin portada")
    r = _thumbnail(c["path"], size) if size in THUMBNAIL_SIZES else tags.cached_cover(c["path"])
    if not r:
        raise HTTPException(404, "sin portada")
    return Response(content=r[0], media_type=r[1],
                    headers={"cache-control": "private, max-age=300"})


# Estrellas, favorito y difuminado se guardan DENTRO del archivo, asi que
# solo valen para la biblioteca: a una cancion abierta desde fuera no se le
# escribe nada. Antes esto contestaba 200 con un `null` y quien llamaba se
# quedaba creyendo que habia funcionado.


@app.post("/api/song/{cid}/stars")
def stars(cid: int, body: Stars = Body(...)):
    # Se mira que EXISTA, no si la escritura funciono: en un archivo de solo
    # lectura la etiqueta no se puede poner y aun asi la cancion esta.
    if not library.by_id(cid):
        raise HTTPException(404, "no esta en la biblioteca")
    playlists.rate(cid, body.stars)
    return library.by_id(cid)


@app.post("/api/song/{cid}/favorite")
def favorite(cid: int, body: Favorite = Body(...)):
    if not library.by_id(cid):
        raise HTTPException(404, "no esta en la biblioteca")
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


@app.patch("/api/playlists/{lid}")
def edit_playlist(lid: int, body: PlaylistEdit = Body(...)):
    """Renombra una lista o cambia su nota. `409` si el nombre ya es de otra."""
    if body.name is None and body.note is None:
        raise HTTPException(400, "no hay nada que cambiar")
    if body.name is not None:
        other = playlists.by_name(body.name)
        if other and other["id"] != lid:
            raise HTTPException(409, f"ya hay una lista que se llama «{other['name']}»")
    out = playlists.edit(lid, name=body.name, note=body.note)
    if not out:
        raise HTTPException(404, "esa lista no existe")
    return {"playlist": out, "playlists": playlists.list_all()}


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
    d, cached = await asyncio.get_running_loop().run_in_executor(None, enrich.details_for, c)
    return {"details": d, "cached": cached}


@app.post("/api/chat")
async def converse(body: ChatIn = Body(...)):
    """Chat con acceso a la biblioteca. `messages`: [{role: 'user'|'ai', text}]"""
    if not body.messages:
        raise HTTPException(400, "no hay mensajes")
    def work():
        return chat.reply(body.messages, context=body.context)
    return await asyncio.get_running_loop().run_in_executor(None, work)


# ------------------------------------------------- el chat, en vivo
# La respuesta se pide en un hilo y la interfaz la va leyendo: el texto
# segun sale del modelo, las herramientas segun terminan, y al final el
# mismo resultado que da /api/chat. Se hace preguntando (cada pocos
# cientos de milisegundos por el socket) y no con un flujo abierto: asi no
# hay que enseñar al puente de Rust a leer respuestas a trozos, y vale
# igual en el navegador.
CHAT_JOBS: dict[str, dict] = {}
_CHAT_JOBS_LOCK = threading.Lock()
CHAT_JOB_TTL = 600


def _chat_job_new(messages, context) -> dict:
    import secrets
    with _CHAT_JOBS_LOCK:
        now = time.time()
        for key, job in list(CHAT_JOBS.items()):
            if job["done"] and now - job["at"] > CHAT_JOB_TTL:
                del CHAT_JOBS[key]
        job = {"id": secrets.token_hex(8), "at": now, "text": "", "tools": [],
               "done": False, "result": None, "cancel": threading.Event()}
        CHAT_JOBS[job["id"]] = job

    def work():
        try:
            r = chat.reply(messages, context=context,
                           on_text=lambda t: job.__setitem__("text", t),
                           on_tool=lambda t: job["tools"].append(t),
                           cancel=job["cancel"])
        except Exception as e:                               # noqa: BLE001
            log.warning("el chat en vivo fallo", exc_info=True)
            r = {"error": f"no pude responder: {e}"}
        job["result"] = r
        job["done"] = True
        job["at"] = time.time()
    threading.Thread(target=work, name="danplay-chat", daemon=True).start()
    return job


@app.post("/api/chat/start")
def chat_start(body: ChatIn = Body(...)):
    if not body.messages:
        raise HTTPException(400, "no hay mensajes")
    job = _chat_job_new(body.messages, body.context)
    return {"id": job["id"]}


@app.get("/api/chat/poll/{job_id}")
def chat_poll(job_id: str):
    job = CHAT_JOBS.get(job_id)
    if not job:
        raise HTTPException(404, "esa respuesta ya no esta")
    return {"id": job_id, "text": job["text"], "tools": list(job["tools"]),
            "done": job["done"], "result": job["result"]}


@app.post("/api/chat/cancel/{job_id}")
def chat_cancel(job_id: str):
    job = CHAT_JOBS.get(job_id)
    if not job:
        raise HTTPException(404, "esa respuesta ya no esta")
    job["cancel"].set()
    return {"ok": True}


# ------------------------------------------------- conversaciones guardadas

@app.get("/api/chats")
def chats_list():
    return {"chats": chats.list_all()}


@app.post("/api/chats")
def chats_create(body: ChatCreateIn = Body(default=ChatCreateIn())):
    return chats.create(body.title)


@app.get("/api/chats/search")
def chats_search(q: str = Query(default="", max_length=200)):
    return {"hits": chats.search(q)}


@app.get("/api/chats/{chat_id}")
def chats_get(chat_id: int):
    c = chats.get(chat_id)
    if not c:
        raise HTTPException(404, "no existe esa conversacion")
    return c


@app.post("/api/chats/{chat_id}/messages")
def chats_append(chat_id: int, body: ChatAppendIn = Body(...)):
    try:
        n = chats.append(chat_id, body.messages)
    except ValueError as e:
        raise HTTPException(404, str(e))
    return {"n": n}


@app.patch("/api/chats/{chat_id}")
def chats_rename(chat_id: int, body: ChatRenameIn = Body(...)):
    if not chats.rename(chat_id, body.title):
        raise HTTPException(404, "no existe esa conversacion")
    return {"ok": True}


@app.delete("/api/chats/{chat_id}")
def chats_delete(chat_id: int):
    if not chats.delete(chat_id):
        raise HTTPException(404, "no existe esa conversacion")
    return {"ok": True}


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
        # Los argumentos son los de la herramienta tal y como los declara
        # `chat.TOOLS`: `items` (lista), `quality`, `file_it` y `force`.
        # Aqui se leia `query`, que la herramienta no tiene, asi que TODA
        # confirmacion de descarga acababa en «hace falta algo que
        # descargar» aunque la persona acabara de decir que si.
        plan = chat.download_plan(body.args)
        if not plan["items"]:
            raise HTTPException(400, "hace falta algo que descargar")
        if not youtube.available():
            raise HTTPException(503, youtube.unavailable_reason())
        if not youtube.claim():
            raise HTTPException(409, "ya hay una descarga en marcha")
        asyncio.get_running_loop().run_in_executor(
            None, lambda: youtube.run_many(plan["items"], quality=plan["quality"],
                                           file_it=plan["file_it"], results=1,
                                           force=plan["force"], source="assistant",
                                           claimed=True))
        n = len(plan["items"])
        return {"ok": True,
                "result": {"active": True, "items": plan["items"],
                           "force": plan["force"], "trimmed": plan["trimmed"]},
                "text": ("Descargando" + (f" {n} temas" if n > 1 else "")
                         + ". Te cuento cuando termine.")}

    def work():
        return chat.confirm(body.tool, body.args)
    return await asyncio.get_running_loop().run_in_executor(None, work)


@app.post("/api/playlists/{pid}/sheet")
def playlist_sheet(pid: int, body: SheetIn = Body(default=SheetIn())):
    """La hoja para el atril del repertorio, en HTML dentro de Listas/."""
    try:
        path = playlists.export_sheet(pid, with_lyrics=body.with_lyrics)
    except ValueError as e:
        raise HTTPException(400, str(e))
    return {"file": path}


@app.get("/api/chat/tools")
def chat_tools():
    return {"model": ai.chat_model(), "provider": ai.provider_name(),
            "available": ai.available(), "reason": ai.unavailable_reason(),
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


# --------------------------------------------- la lista del reproductor
# Lo que se abre desde fuera de DanPlay. Ver `danplay/external.py`: NO se
# importa a la biblioteca, solo se recuerda que sono.


@app.post("/api/external/play")
def external_play(body: PathIn = Body(...)):
    """Esa ruta acaba de sonar. Devuelve la cancion, venga de donde venga.

    Es POST y no GET porque la clave es una ruta: con acentos, espacios, `&` y
    `#` dentro, meterla en la parte de consulta de una URL es pedir un fallo
    de codificacion.

    Si el archivo ya esta indexado, devuelve la cancion de la biblioteca tal
    cual, con su caratula y sus estrellas. Si no, se le leen las etiquetas una
    vez y se le da un id negativo. En los dos casos sube al principio de la
    lista, y volver a ponerla no la duplica.
    """
    song = external.played(body.path)
    if not song:
        raise HTTPException(404, "ese archivo no esta")
    return {"song": song}


@app.get("/api/external")
def external_list():
    """La lista, de lo ultimo que sono a lo mas antiguo."""
    return {"songs": external.listing()}


@app.delete("/api/external")
def external_clear():
    """Descarta la lista. Ni toca los archivos ni deshace lo ya guardado."""
    return {"removed": external.clear()}


@app.delete("/api/external/{cid}")
def external_forget(cid: int):
    """Quita una sola cancion de la lista."""
    return {"removed": external.forget(cid)}


@app.post("/api/external/save")
def external_save(body: PlaylistName = Body(...)):
    """Guarda la lista de ahora como una lista de reproduccion de DanPlay."""
    try:
        return external.save_as_playlist(body.name)
    except ValueError as e:
        raise HTTPException(400, str(e)) from e


@app.get("/api/song/{cid}/path")
def audio_path(cid: int):
    """Devuelve la ruta en disco. La usa Rust para servir el audio sin pasar por Python.

    `resolve` y no `by_id`: aqui solo se REPRODUCE, y una cancion abierta
    desde fuera de la biblioteca tiene que sonar igual que las demas.
    """
    c = external.resolve(cid)
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


@app.post("/api/youtube/download")
async def youtube_download(body: YoutubeIn = Body(default=YoutubeIn())):
    """Arranca la descarga y vuelve enseguida. El avance se consulta en /api/youtube.

    Una descarga a la vez. Quedarse el turno es cosa de `youtube.claim()`,
    que comprueba y reserva bajo un mismo cerrojo; luego `run_job` corre con
    `claimed=True`. Antes se marcaba `active` aqui a mano y `run_job`, al
    verlo puesto, se negaba a descargar: el boton decia «Bajando…» para
    siempre y no bajaba nada.
    """
    query = body.query.strip()
    if not query:
        raise HTTPException(400, "hace falta una URL o algo que buscar")
    if not youtube.available():
        raise HTTPException(503, youtube.unavailable_reason())
    if not youtube.claim():
        raise HTTPException(409, "ya hay una descarga en marcha")
    asyncio.get_running_loop().run_in_executor(
        None, lambda: youtube.run_job(query,
                                      quality=body.quality or config.MP3_QUALITY,
                                      file_it=body.file_it,
                                      results=body.results,
                                      force=body.force, claimed=True))
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
    # El catalogo de modelos se pone al dia al arrancar (en segundo plano y
    # solo si hace horas de la ultima vez): asi el apartado de IA abre ya
    # con la lista de hoy aunque no se pulse nada.
    model_catalog.refresh_in_background()
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
