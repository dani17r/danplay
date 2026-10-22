"""Descarga de audio desde YouTube.

Baja el mejor audio disponible, lo convierte a mp3, le pega la caratula y lo
suelta en la tuberia de importacion de siempre: se identifica, se limpia el
nombre y se archiva en Artistas/<Artista>/.

Un detalle importante: NO se usa el postprocesador de metadatos de yt-dlp. Ese
rellena el artista con el nombre del canal ("Fulanito Music", "Topic", ...) y
como las etiquetas son el primer escalon de la cascada de identificacion, se
colaria con 0.95 de confianza y archivaria la cancion bajo un artista inventado.
Solo se escriben etiquetas cuando YouTube da datos de musica de verdad
(los campos `track` y `artist`, que vienen de YouTube Music); si no, se deja
limpio y deciden la huella acustica, el heuristico o la IA.

yt-dlp es opcional: si no esta instalado la funcion queda desactivada y el
resto de la app sigue igual.

Lo que se le pide a yt-dlp y con que opciones, lo que se lee de lo que
contesta y de su avance, por que fallo, el nombre y las etiquetas de lo
bajado, donde se guarda y la fila del historial van en Rust
(library/src/youtube.rs). Aqui quedan las llamadas a yt-dlp (su API de
Python), el turno de descarga con su cerrojo y su estado, los hilos, y
archivar con `ingest`.
"""

import json
import logging
import os
import shutil
import tempfile
import threading
import time
from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any, cast

import danplay_core as _rust

from . import config, convert, ingest, names, tags, ytdlp

log = logging.getLogger(__name__)

# calidad de la app -> kbps del mp3. "variable" deja que lame elija (V0).
QUALITY_KBPS = _rust.youtube_quality_kbps()

# Topes de lo que se descarga. Sin ellos, un directo de tres horas llena el
# temporal y deja a ffmpeg trabajando un buen rato para nada. De una lista,
# como mucho los primeros 50: `download` recorria TODOS.
MAX_SECONDS, MAX_BYTES, PLAYLIST_LIMIT = _rust.youtube_limits()


class Canceled(Exception):
    """La levanta el enganche de progreso cuando el usuario cancela."""


# Estado de la descarga en curso. Vive aqui y no en la API porque lo comparten
# la pagina de Descargas y el asistente: una sola descarga a la vez y un solo
# sitio donde mirar como va.
#
# `active` en False quiere decir «TODO hecho»: tambien lo que se pidio hacer al
# terminar (`on_done` de `run_many`) y su resultado en `after`.
STATE: dict = {
    "active": False,
    "phase": "",
    "name": "",
    "percent": 0.0,
    "index": 0,
    "total": 0,
    "results": [],
    "error": "",
    # Lo que paso con lo que se pidio hacer AL TERMINAR la descarga (`on_done`
    # de `run_many`, p. ej. completar una lista): None si no se pidio nada o
    # aun no se sabe; si no, el dict que devolvio quien lo pidio (con `job`),
    # o {"state": "error", "error": ...} si fallo. Se publica SIEMPRE antes de
    # soltar el turno: con `active` en False ya esta.
    "after": None,
    # El turno de ahora: un contador que sube con cada `claim()` (es su
    # token). Solo escribe en STATE quien lo tiene (ver `_put`): una descarga
    # vieja que acaba tarde no pisa el estado de la nueva.
    "job": 0,
}
_CANCEL = {"requested": False}
# Comprobar que no hay nada en marcha y quedarse el turno van juntos bajo el
# mismo cerrojo: dos peticiones seguidas arrancaban dos descargas.
_TURN = threading.Lock()


def claim() -> int:
    """Se queda el turno de descarga. 0 si ya hay una en marcha.

    Si lo consigue devuelve el token del turno, un entero >= 1 que tambien es
    `STATE["job"]`; para quien solo pregunta «¿lo tengo?» es verdadero, como
    el booleano de antes. Quien lo consigue tiene que llamar a
    `run_job`/`run_many` con `claimed=True` (o a `release`): el turno no se
    suelta solo. La API lo usa para contestar «ya hay una en marcha» al
    momento y arrancar la descarga en segundo plano sin que nadie se cuele
    entre medias.

    Un turno nuevo empieza con `after` en None: lo de la descarga anterior no
    se hereda.

    Antes la API ponia `STATE["active"] = True` a mano y luego llamaba a
    `run_job`, que al ver `active` contestaba «ya hay una descarga en marcha»
    y NO descargaba: el estado se quedaba en marcha para siempre y toda
    descarga posterior recibia un 409. Desde el boton y desde el asistente.
    """
    with _TURN:
        if STATE["active"]:
            return 0
        _CANCEL["requested"] = False
        STATE["job"] += 1
        STATE.update(
            {
                "active": True,
                "phase": "starting",
                "name": "",
                "percent": 0.0,
                "index": 0,
                "total": 0,
                "results": [],
                "error": "",
                "after": None,
            }
        )
        return STATE["job"]


def release(job: int | None = None) -> None:
    """Suelta el turno.

    Sin `job`, el que sea: la descarga no llego a arrancar. Con `job` (el
    token de `claim`), solo si sigue siendo ese: una descarga vieja que acaba
    tarde no suelta el turno de la nueva.
    """
    with _TURN:
        if job is not None and STATE["job"] != job:
            return
        STATE["active"] = False
        STATE["phase"] = "canceled" if _CANCEL["requested"] else "done"


def cancel() -> None:
    _CANCEL["requested"] = True


def canceled() -> bool:
    return _CANCEL["requested"]


def _put(job: int, **fields) -> bool:
    """Escribe en STATE solo si `job` sigue siendo el turno vigente.

    Si no, la descarga ya no es la de ahora (se solto el turno y otra empezo)
    y lo suyo ya no le toca a nadie: no se pisa el estado de la nueva.
    """
    with _TURN:
        if STATE["job"] != job:
            return False
        STATE.update(fields)
        return True


# Lo unico que publica el avance de una descarga (lo demas de STATE tiene
# dueno: `results`, `error`, `after`...).
_PROGRESS = ("phase", "name", "percent", "index", "total")


def _publish(p: dict, job: int | None = None) -> None:
    fields = {k: v for k, v in p.items() if k in _PROGRESS}
    if job is None:
        STATE.update(fields)
    else:
        _put(job, **fields)


def _yt_dlp():
    """El yt-dlp en uso: el que se bajo al actualizar, o el de la app (ver `ytdlp`)."""
    return ytdlp.module()


def _installed() -> bool:
    """Si yt-dlp esta instalado, SIN importarlo.

    Importarlo cuesta un cuarto de segundo, y `available()` la consulta
    `/api/status`, que la app pregunta continuamente: la primera pantalla se
    quedaba esperando a que cargara un modulo que quiza no se use nunca.
    El import de verdad se hace al bajar algo. Si no esta, se vuelve a mirar
    al rato: una actualizacion desde la app lo puede traer.
    """
    global _INSTALLED, _CHECKED
    if _INSTALLED or (_INSTALLED is False and time.monotonic() - _CHECKED < 30):
        return bool(_INSTALLED)
    from importlib.util import find_spec

    ytdlp.prepare()
    try:
        _INSTALLED = find_spec("yt_dlp") is not None
    except (ImportError, ValueError):
        _INSTALLED = False
    _CHECKED = time.monotonic()
    return _INSTALLED


_INSTALLED: bool | None = None
_CHECKED = 0.0


def recheck() -> None:
    """Que la proxima pregunta vuelva a mirar si yt-dlp esta (tras actualizarlo)."""
    global _INSTALLED
    _INSTALLED = None


def available() -> bool:
    return _installed() and convert.available()


def unavailable_reason() -> str:
    if not _installed():
        return "falta yt-dlp: actualizalo desde la pagina de Descargas (o pip install yt-dlp)"
    if not convert.available():
        return "falta ffmpeg"
    return ""


# Los dominios de YouTube, exactos. Antes se miraba si el texto CONTENIA
# alguno, asi que «https://loquesea.example/?youtube.com» contaba como
# YouTube; y si no era ninguno se le pasaba igual a yt-dlp, cuyo extractor
# generico descarga de cualquier sitio, incluidas direcciones de la red local.
ALLOWED_HOSTS = frozenset(_rust.youtube_allowed_hosts())


def host_of(text: str) -> str:
    """El dominio de una direccion, o cadena vacia si no lo es.

    Una direccion cuyo «sitio» trae algo mas que un nombre (una barra invertida,
    un usuario@, espacios...) NO es de YouTube aunque lo nombre: cada libreria
    de red la lee a su manera (Python ve `youtube.com` en
    `https://malo.example\\@youtube.com`, y urllib3, que es lo que usa yt-dlp,
    ve `malo.example`) y las de YouTube de verdad se escriben sin nada de eso.
    Esas devuelven su «sitio» tal cual, que nunca esta en `ALLOWED_HOSTS`.
    """
    return _rust.youtube_host_of(text)


def is_url(text: str) -> bool:
    """Una direccion de YouTube de verdad, mirando el dominio y no el texto."""
    return _rust.youtube_is_url(text)


class NotYouTube(ValueError):
    """Una direccion que no es de YouTube. No se le pasa a yt-dlp."""


def normalize(inbox: str, results=5) -> str:
    """URL de YouTube tal cual, o busqueda si el usuario escribio texto suelto."""
    return _rust.youtube_normalize(inbox, results)


def wanted(info, *, incomplete=False):
    """Filtro de yt-dlp: nada de mas de media hora.

    Los directos y las recopilaciones de tres horas llenan el temporal y
    dejan a ffmpeg trabajando un buen rato para nada.
    """
    return _rust.youtube_wanted(info.get("duration"))


def _base_options(quiet=True) -> dict:
    # con el motor de JavaScript para los retos de YouTube (deno, node...)
    return _rust.youtube_base_options(quiet, ytdlp.options())


def _pick_fields(d: Mapping[str, Any]) -> dict:
    """Los pocos campos que nos interesan de lo que devuelve yt-dlp (`artist`,
    `track` y `album` solo los trae YouTube Music; son los unicos fiables)."""
    return _rust.youtube_pick_fields(d)


def _plain(message) -> str:
    """Un mensaje de yt-dlp en una linea, sin colores ni el «ERROR:» del principio."""
    return _rust.youtube_plain(message)


def failure_kind(reason: str) -> str:
    """De que tipo es un fallo de YouTube, por el `reason` que dio `info`.

    "bot": pide verificar que no eres un robot. "rate": 429, demasiadas
    peticiones. "network": sin red o tiempo agotado. "": cualquier otro (un
    video no disponible, un enlace malo...). Sirve para dejar de insistir:
    seguir preguntando cuando YouTube ya ha dicho que no solo empeora las cosas.
    """
    return _rust.youtube_failure_kind(reason)


# El `logger` de yt-dlp en las consultas: apunta lo que sale mal. Con
# `ignoreerrors` (que se usa siempre aqui) yt-dlp no levanta nada cuando
# YouTube pide verificar que no eres un robot, contesta 429 o no hay red:
# escribe el motivo en su salida de errores y devuelve `None`. Sin recogerlo,
# todo eso salia como un «no se encontro nada» que no dice nada.
_Seen = _rust.YoutubeSeen


def _is_list_url(url: str) -> bool:
    """Si el enlace es de una LISTA a proposito, y no de un video suelto.

    Lo que da compartir un video desde dentro de una lista o de un mix
    (`watch?v=ID&list=RD...`, `youtu.be/ID?list=...`) es UN video: con la
    lista, `_info` bajaba hasta 50. Manda el video (`v=`, o el id en el camino
    de youtu.be, shorts, live...); `list=` solo cuenta cuando no hay video:
    `/playlist?list=...` o `watch?list=...`.
    """
    return _rust.youtube_is_list_url(url)


def info(inbox: str, results=5, *, timeout=None) -> dict:
    """Consulta sin descargar: sirve para enseñar que se va a bajar.

    Con `timeout` (segundos) es una consulta LIGERA, de las que se lanzan por
    docenas: espera como mucho eso por cada respuesta de la red y no reintenta
    (`retries` y `extractor_retries` a 0), asi que un YouTube lento o que pide
    verificar no la deja colgada. Sin `timeout` es la de siempre (30 s y tres
    reintentos). El tiempo es por respuesta, no del total.

    Si falla, `reason` dice POR QUE (YouTube pide verificar que no eres un
    robot, 429, sin red...; ver `failure_kind`) y no un «no se encontro nada»
    generico. Un enlace que no es de YouTube se rechaza sin consultar nada.
    """
    with ytdlp.using():
        return _info(inbox, results, timeout=timeout)


def _info(inbox: str, results=5, timeout=None) -> dict:
    yt = _yt_dlp()
    if yt is None:
        return {"ok": False, "reason": unavailable_reason(), "items": []}
    try:
        target_url = normalize(inbox, results)
    except NotYouTube as e:
        return {"ok": False, "reason": str(e), "items": []}
    # `noplaylist` salvo que se haya pegado un enlace de LISTA a proposito
    # (`/playlist?list=`): con un video delante, `list=` no cuenta (ver
    # `_is_list_url`). Sin esto, «watch?v=X&list=Y» consultaba la lista entera.
    seen = _Seen()
    options = _rust.youtube_query_options(_base_options(), target_url, timeout, wanted, seen)
    try:
        with yt.YoutubeDL(cast(Any, options)) as ydl:
            data = ydl.extract_info(target_url, download=False)
    except Exception as e:  # noqa: BLE001
        return _rust.youtube_query_failed(e, seen)
    return _rust.youtube_query_result(data, seen)


def _provisional_name(data: dict) -> str:
    """Nombre de archivo ya sin el ruido tipico de YouTube."""
    return _rust.youtube_provisional_name(data)


def _tag_if_trustworthy(path: Path, data: dict) -> bool:
    """Escribe artista/titulo solo si YouTube dio metadatos de musica de verdad
    («Fulano - Topic» es el canal automatico de YouTube Music, no el artista)."""
    trusted = _rust.youtube_trusted_tags(data)
    if trusted is None:
        return False
    tags.write(path, **trusted)
    return True


def download_one(target_url, quality="high", folder=None, progress=None, cancel=None) -> dict:
    """Baja un video y devuelve el mp3 ya listo en `folder` (por defecto Entrada/).

    `progress(dict)` recibe {fase, porcentaje, nombre}. `cancelar()` devuelve
    True para abortar.
    """
    yt = _yt_dlp()
    if yt is None or not convert.available():
        return {"ok": False, "reason": unavailable_reason()}

    from . import library  # aqui dentro: library no debe cargar yt-dlp

    try:
        folder = library.ensure_folder(Path(folder) if folder else config.INBOX)
    except library.FolderGone as e:
        return {"ok": False, "reason": str(e)}
    kbps = QUALITY_KBPS.get(quality, QUALITY_KBPS["high"])

    def hook(status):
        if cancel and cancel():
            raise Canceled()
        if not progress:
            return
        step = _rust.youtube_progress(status)
        if step is not None:
            progress(step)

    with tempfile.TemporaryDirectory(prefix="danplay-yt-") as tmp:
        # el mejor audio, al temporal, con tope de tamaño y de duracion: un
        # directo de tres horas no deberia poder llenar el disco por accidente
        options = _rust.youtube_download_options(_base_options(), tmp, kbps, wanted, hook)
        try:
            with yt.YoutubeDL(cast(Any, options)) as ydl:
                data = ydl.extract_info(target_url, download=True)
        except Canceled:
            return {"ok": False, "reason": "canceled", "canceled": True}
        except Exception as e:  # noqa: BLE001
            return {"ok": False, "reason": str(e)[:200]}
        data, why = _rust.youtube_single(data)  # si vino una lista, el primero
        if data is None:
            return {"ok": False, "reason": why}

        mp3 = next((p for p in Path(tmp).glob("*.mp3")), None)
        if mp3 is None:
            return {"ok": False, "reason": "ffmpeg no dejo ningun mp3"}

        d = _pick_fields(data)
        tagged = _tag_if_trustworthy(mp3, d)
        target = folder / names.free_name(str(folder), _provisional_name(d) + ".mp3")
        shutil.move(str(mp3), str(target))

    return _rust.youtube_downloaded(d, str(target), tagged, kbps)


# Lo que usa lo descargado dentro de su carpeta. Al elegirla se crean las que
# falten; las que ya estaban se usan tal cual.
SUBFOLDERS = ("Artistas", *ingest.CATEGORY_FOLDER.values())


def folder() -> dict:
    """Donde se guarda lo que se baja, y si ya se puede bajar ahi.

    `ready` es False hasta que la persona la elige (la interfaz lo pregunta
    antes de la primera descarga), si ya no esta (un disco sin montar) o si
    no es de sus carpetas de musica: lo bajado no saldria en la app.
    `suggested` es la que se le propone: una de sus carpetas de musica.
    """
    from . import library

    path = config.LIBRARY
    return _rust.youtube_folder(
        str(path),
        bool(config.LIBRARY_CHOSEN),
        library.list_folders(),
        lambda: library.within_roots(path),
    )


def choose_folder(path) -> dict:
    """La carpeta donde ira todo lo descargado, elegida por la persona.

    Tiene que existir y ser una de sus carpetas de musica (o estar dentro de
    una): si no, lo bajado no se veria. Dentro se crean las subcarpetas que
    falten (`SUBFOLDERS` y Entrada/). `ValueError` si no vale.
    """
    from . import library

    chosen = Path(os.path.abspath(os.path.expanduser(str(path))))
    if not chosen.is_dir():
        raise ValueError("esa carpeta no existe")
    if not library.within_roots(chosen):
        raise ValueError("no es una de tus carpetas de musica: añadela antes")
    config.set_library(chosen)
    for sub in (*SUBFOLDERS, config.INBOX.name):
        (chosen / sub).mkdir(exist_ok=True)
    return folder()


def already_in_library(title: str) -> list[dict]:
    """Lo que ya tienes en la biblioteca y parece esta misma cancion."""
    from . import library

    return library.find_by_match_key(_rust.youtube_title_key(title))


def _log(entry: dict, query: str, source: str, quality: str) -> None:
    """Deja constancia en el historial. Que falle no debe tumbar la descarga."""
    from . import library

    try:
        # La que ya tenias: se apunta CUAL (y donde esta), para que la proxima
        # vez que llegue este enlace se sepa sin volver a consultarlo, y sea la
        # misma cancion mientras siga ahi.
        library.log_download(_rust.youtube_history_entry(entry, query, source, quality))
    except Exception:
        log.debug("no se pudo apuntar la descarga en el historial", exc_info=True)


def download(
    inbox: str,
    quality=None,
    file_it=True,
    results=5,
    force=False,
    source="manual",
    progress=None,
    cancel=None,
) -> list[dict]:
    """Descarga (uno, lista o busqueda) y, si `archivar`, lo pasa por la tuberia.

    Archivar significa lo mismo que en Entrada/: identificar, renombrar segun
    las reglas de la casa y mover a Artistas/<Artista>/. yt-dlp no se cambia
    por otra version mientras tanto (ver `ytdlp.using`).
    """
    with ytdlp.using():
        return _download(inbox, quality, file_it, results, force, source, progress, cancel)


def _download(inbox, quality, file_it, results, force, source, progress, cancel) -> list[dict]:
    quality = quality or config.MP3_QUALITY
    meta = _info(inbox, results)
    if not meta["ok"]:
        # `requested` en TODO resultado, tambien en los que fallan: quien
        # completa una lista al terminar casa cada resultado con lo que pidio
        return [{"ok": False, "reason": meta["reason"], "requested": inbox}]

    items = meta["items"]
    vocab = names.vocabulary(config.ARTISTS_DIR) if file_it else {}
    out = []
    for i, t in enumerate(items, 1):
        if cancel and cancel():
            out.append({"ok": False, "reason": "canceled", "canceled": True, "requested": inbox})
            break
        if progress:
            progress(
                {
                    "phase": "starting",
                    "index": i,
                    "total": len(items),
                    "name": t["title"],
                    "percent": 0,
                }
            )

        # Si ya la tienes, no se baja: se avisa y se deja que tu decidas.
        # `force` la baja igualmente y el sufijo « - r» la marca como repetida.
        if not force:
            mine = already_in_library(t["title"])
            if mine:
                duplicate = {
                    "ok": False,
                    "already_there": True,
                    "title": t["title"],
                    "url": t["url"],
                    "source": t["title"],
                    "requested": inbox,
                    "reason": "ya la tienes en la biblioteca",
                    "matches": mine[:5],
                }
                _log(duplicate, inbox, source, quality)
                out.append(duplicate)
                continue

        def step(p, _i=i, _t=t):
            if progress:
                progress(
                    {**p, "index": _i, "total": len(items), "name": p.get("name") or _t["title"]}
                )

        # Si se va a archivar, la descarga se posa en un temporal y no en
        # Entrada/. Si cae en Entrada y ya hay una copia, `free_name` le pone
        # el sufijo « - r2», la identificacion lee ESE nombre y el sufijo se
        # queda dentro del titulo para siempre.
        stage = tempfile.mkdtemp(prefix="danplay-stage-") if file_it else None
        try:
            r = download_one(
                t["url"] or t["id"], quality, folder=stage, progress=step, cancel=cancel
            )
            r["forced"] = bool(force)
            r["source"] = t["title"]
            # lo que se pidio (URL o texto): el chat lo usa para saber si esta
            # descarga es la suya
            r["requested"] = inbox
            if r.get("ok") and file_it:
                if progress:
                    progress(
                        {
                            "phase": "filing",
                            "index": i,
                            "total": len(items),
                            "name": t["title"],
                            "percent": 100,
                        }
                    )
                # El nombre lo pone YouTube (limpio), no la huella acustica:
                # una «Drum Cam» de una cancion no es del artista original.
                known = names.from_video(
                    r.get("title") or t["title"],
                    r.get("channel") or t.get("channel", ""),
                    vocab,
                    t.get("artist", ""),
                    t.get("track", ""),
                )
                res = ingest.process(r["file"], vocab, convert_mp3=False, known=known)
                r["action"] = res.action
                r["artist"] = res.artist
                r["song"] = res.title
                r["identified_by"] = res.source
                r["confidence"] = res.confidence
                r["target"] = str(res.target) if res.target else ""
                r["warnings"] = res.warnings
                # al indice, o la cancion existe en el disco pero no en la app
                if res.target:
                    from . import library

                    new = library.index_file(str(res.target))
                    if new:
                        r["id"] = new["id"]
                    # Una Drum Cam, un tutorial, una secuencia: se archiva
                    # como todas, con su artista, y el aviso ofrece llevarla
                    # a su carpeta. Solo se sugiere: el titulo puede engañar.
                    kind = names.video_kind(r.get("title") or t["title"])
                    if kind and new and res.target.is_relative_to(config.ARTISTS_DIR):
                        r["kind"] = {**kind, "folder": ingest.CATEGORY_FOLDER[kind["category"]]}
                if res.action == "error":
                    r["ok"] = False
                    r["reason"] = res.note
        finally:
            # se limpia DESPUES de archivar: si se borra antes, `ingest` se
            # encuentra sin archivo que mover
            if stage:
                shutil.rmtree(stage, ignore_errors=True)
        _log(r, inbox, source, quality)
        out.append(r)
    return out


def run_job(
    query: str, quality=None, file_it=True, results=5, force=False, source="manual", claimed=False
) -> list[dict]:
    """`download` publicando el avance en STATE. Solo una descarga a la vez.

    `claimed=True` dice que quien llama ya se quedo el turno con `claim()`.
    """
    return run_many(
        [query],
        quality=quality,
        file_it=file_it,
        results=results,
        force=force,
        source=source,
        claimed=claimed,
    )


def _short(e: BaseException) -> str:
    """El motivo de un fallo en una linea corta (para enseñarlo, no para depurar)."""
    return _rust.youtube_short(e)


def run_many(
    queries: list[str],
    quality=None,
    file_it=True,
    results=1,
    force=False,
    source="manual",
    claimed=False,
    on_done: Callable[[list[dict]], dict | None] | None = None,
    job: int | None = None,
) -> list[dict]:
    """Varias descargas seguidas bajo un solo turno; el avance en STATE.

    Es lo que pide el asistente: «bajame estas tres». Cada elemento va por
    `download` con su propia busqueda, y los resultados se van acumulando en
    `STATE["results"]` segun terminan, para que Descargas y el chat puedan
    contar lo que hay aunque aun queden temas.

    El indice/total que se publica es del conjunto: un tema suelto cuenta
    uno; una lista, lo que traiga. Como no se sabe de antemano cuantos trae
    cada elemento, el total se estima suponiendo uno por elemento pendiente y
    se corrige sobre la marcha.

    Todos los resultados llevan `requested`: lo que se pidio (la consulta),
    tambien los que fallan o se cancelan, para casar cada uno con su peticion.

    `on_done(results)` es lo que hay que hacer AL TERMINAR (p. ej. meter lo
    bajado en su sitio de una lista). Corre aqui mismo, tras la ultima
    descarga y ANTES de soltar el turno: `STATE["phase"]` pasa a «listing»
    con `active` aun en True, y `active` en False quiere decir «todo hecho».
    Corre tambien si se cancelo o si una descarga fallo (con lo que se llego a
    bajar; `canceled()` dice si fue cancelada). Debe devolver un dict (o None).
    Si lanza, la descarga no se tumba. En `STATE["after"]` queda, SIEMPRE
    antes de soltar el turno: None si no hubo `on_done` (o devolvio None); lo
    que devolvio, con `job` añadido; o {"state": "error", "error": <motivo
    corto>, "job"} si lanzo o devolvio otra cosa que un dict.

    `job` es el token del turno (el que dio `claim`): con `claimed=True` y sin
    `job` se toma `STATE["job"]`. Solo el dueño del turno escribe en STATE: si
    la descarga acaba cuando ya hay otra (se solto el turno por fuera), no
    pisa a la nueva ni le suelta el turno.
    """
    queries = [str(q).strip() for q in (queries or []) if str(q).strip()]
    if not claimed:
        turn = claim()
        if not turn:
            return [{"ok": False, "reason": "ya hay una descarga en marcha"}]
    else:
        turn = STATE["job"] if job is None else job
    done: list[dict] = []
    after: dict | None = None
    try:
        current = ""
        try:
            for position, query in enumerate(queries):
                if canceled():
                    break
                current = query
                offset = len(done)
                pending_after = len(queries) - position - 1

                def step(p, _offset=offset, _pending=pending_after):
                    p = dict(p)
                    if "index" in p:
                        p["index"] = _offset + int(p.get("index") or 0)
                    if "total" in p:
                        p["total"] = _offset + int(p.get("total") or 0) + _pending
                    _publish(p, turn)

                rs = download(
                    query,
                    quality=quality,
                    file_it=file_it,
                    results=results,
                    force=force,
                    source=source,
                    progress=step,
                    cancel=canceled,
                )
                for r in rs:
                    r.setdefault("requested", query)
                done.extend(rs)
                _put(turn, results=list(done))
        except Exception as e:  # noqa: BLE001
            _put(turn, error=str(e)[:200])
            failure = {"ok": False, "reason": str(e)[:200]}
            if current:
                failure["requested"] = current
            done.append(failure)
            _put(turn, results=list(done))
        if on_done is not None:
            _put(turn, phase="listing")
            after = _after(on_done, done, turn)
        return done
    finally:
        # lo ultimo antes de soltar el turno: quien vea `active` en False ya
        # tiene `after` (y una descarga que acabe tarde no pisa a la nueva)
        _put(turn, after=after)
        release(turn)


def _after(on_done: Callable[[list[dict]], dict | None], done: list[dict], job: int) -> dict | None:
    """Corre `on_done` y deja su resultado en la forma de `STATE["after"]`.

    Lo que lance NO se propaga: la descarga ya esta hecha y lo que falle aqui
    se cuenta (`state: "error"`), no se la lleva por delante.
    """
    try:
        result = on_done(list(done))
    except Exception as e:
        log.warning("lo que se hacia al terminar la descarga fallo", exc_info=True)
        return {"state": "error", "error": _short(e), "job": job}
    if result is None:
        return None
    if isinstance(result, dict):
        try:
            # una copia de datos sueltos: se publica tal cual en /api/youtube, y
            # quien lo hizo no puede cambiarlo despues
            plain = json.loads(json.dumps(result, default=str, allow_nan=False))
        except (TypeError, ValueError):
            log.warning("lo que se hacia al terminar la descarga no se puede publicar")
        else:
            return {**plain, "job": job}
    else:
        log.warning("lo que se hacia al terminar la descarga devolvio %s", type(result).__name__)
    return {"state": "error", "error": "no devolvió un resultado válido", "job": job}
