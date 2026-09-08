# -*- coding: utf-8 -*-
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
"""
import re
import shutil
import tempfile
from pathlib import Path

from . import config, convert, tags, ingest, names

# calidad de la app -> kbps del mp3. "variable" deja que lame elija (V0).
QUALITY_KBPS = {"high": "320", "medium": "192", "variable": "0", "copy": "256"}

# lo que sabemos bajar. Se acepta pegar la URL con parametros de sobra.
# Topes de lo que se descarga. Sin ellos, un directo de tres horas llena el
# temporal y deja a ffmpeg trabajando un buen rato para nada.
MAX_SECONDS = 30 * 60
MAX_BYTES = 200 * 1024 * 1024
# De una lista, como mucho los primeros 50: `download` recorria TODOS.
PLAYLIST_LIMIT = "1:50"


class Canceled(Exception):
    """La levanta el enganche de progreso cuando el usuario cancela."""


# Estado de la descarga en curso. Vive aqui y no en la API porque lo comparten
# la pagina de Descargas y el asistente: una sola descarga a la vez y un solo
# sitio donde mirar como va.
STATE: dict = {"active": False, "phase": "", "name": "", "percent": 0.0,
                "index": 0, "total": 0, "results": [], "error": ""}
_CANCELAR = {"requested": False}


def cancel() -> None:
    _CANCELAR["requested"] = True


def canceled() -> bool:
    return _CANCELAR["requested"]


def _publish(p: dict) -> None:
    STATE.update({k: v for k, v in p.items() if k in STATE})


def _yt_dlp():
    try:
        import yt_dlp
        return yt_dlp
    except ImportError:
        return None


def _installed() -> bool:
    """Si yt-dlp esta instalado, SIN importarlo.

    Importarlo cuesta un cuarto de segundo, y `available()` la consulta
    `/api/status`, que la app pregunta continuamente: la primera pantalla se
    quedaba esperando a que cargara un modulo que quiza no se use nunca.
    El import de verdad se hace al bajar algo.
    """
    global _INSTALLED
    if _INSTALLED is None:
        from importlib.util import find_spec
        try:
            _INSTALLED = find_spec("yt_dlp") is not None
        except (ImportError, ValueError):
            _INSTALLED = False
    return _INSTALLED


_INSTALLED = None


def available() -> bool:
    return _installed() and convert.available()


def unavailable_reason() -> str:
    if not _installed():
        return "falta yt-dlp (pip install yt-dlp)"
    if not convert.available():
        return "falta ffmpeg"
    return ""


# Los dominios de YouTube, exactos. Antes se miraba si el texto CONTENIA
# alguno, asi que «https://loquesea.example/?youtube.com» contaba como
# YouTube; y si no era ninguno se le pasaba igual a yt-dlp, cuyo extractor
# generico descarga de cualquier sitio, incluidas direcciones de la red local.
ALLOWED_HOSTS = frozenset({
    "youtube.com", "www.youtube.com", "m.youtube.com", "music.youtube.com",
    "youtu.be", "www.youtu.be",
})


def host_of(text: str) -> str:
    """El dominio de una direccion, o cadena vacia si no lo es."""
    import urllib.parse
    t = (text or "").strip()
    if not t.lower().startswith(("http://", "https://")):
        return ""
    try:
        return (urllib.parse.urlsplit(t).hostname or "").lower()
    except ValueError:
        return ""


def is_url(text: str) -> bool:
    """Una direccion de YouTube de verdad, mirando el dominio y no el texto."""
    return host_of(text) in ALLOWED_HOSTS


class NotYouTube(ValueError):
    """Una direccion que no es de YouTube. No se le pasa a yt-dlp."""


def normalize(inbox: str, results=5) -> str:
    """URL de YouTube tal cual, o busqueda si el usuario escribio texto suelto."""
    e = (inbox or "").strip()
    if is_url(e):
        return e
    if host_of(e):
        raise NotYouTube(
            f"«{host_of(e)}» no es YouTube. Pega un enlace de YouTube, "
            "o escribe lo que buscas y lo busco alli.")
    return f"ytsearch{max(1, int(results))}:{e}"


def wanted(info, *, incomplete=False):
    """Filtro de yt-dlp: nada de mas de media hora.

    Los directos y las recopilaciones de tres horas llenan el temporal y
    dejan a ffmpeg trabajando un buen rato para nada.
    """
    duration = info.get("duration") or 0
    if duration and duration > MAX_SECONDS:
        return f"dura {int(duration // 60)} minutos; el tope son {MAX_SECONDS // 60}"
    return None


def _base_options(quiet=True) -> dict:
    return {
        "quiet": quiet, "no_warnings": quiet, "noprogress": True,
        "ignoreerrors": True, "consoletitle": False,
        "nocheckcertificate": False, "retries": 3, "socket_timeout": 30,
    }


def _pick_fields(d: dict) -> dict:
    """Los pocos campos que nos interesan de lo que devuelve yt-dlp."""
    return {
        "id": d.get("id") or "",
        "title": d.get("title") or "",
        "channel": d.get("uploader") or d.get("channel") or "",
        "duration": float(d.get("duration") or 0),
        "url": d.get("webpage_url") or d.get("original_url") or "",
        "thumbnail": d.get("thumbnail") or "",
        # solo YouTube Music los trae; son los unicos fiables
        "artist": (d.get("artist") or "").strip(),
        "track": (d.get("track") or "").strip(),
        "album": (d.get("album") or "").strip(),
    }


def info(inbox: str, results=5) -> dict:
    """Consulta sin descargar: sirve para enseñar que se va a bajar."""
    yt = _yt_dlp()
    if yt is None:
        return {"ok": False, "reason": unavailable_reason(), "items": []}
    try:
        target_url = normalize(inbox, results)
    except NotYouTube as e:
        return {"ok": False, "reason": str(e), "items": []}
    # `noplaylist` salvo que se haya pegado un enlace de lista a proposito: sin
    # esto, pegar «watch?v=X&list=Y» consultaba la lista entera.
    explicit_list = "list=" in target_url
    options = {**_base_options(), "extract_flat": "in_playlist", "skip_download": True,
               "noplaylist": not explicit_list, "playlist_items": PLAYLIST_LIMIT,
               "match_filter": wanted}
    try:
        with yt.YoutubeDL(options) as ydl:
            data = ydl.extract_info(target_url, download=False)
    except Exception as e:                                  # noqa: BLE001
        return {"ok": False, "reason": str(e)[:200], "items": []}
    if not data:
        return {"ok": False, "reason": "no se encontro nada", "items": []}

    entries = [e for e in (data.get("entries") or []) if e] if "entries" in data else [data]
    return {
        "ok": True,
        "playlist": bool(data.get("entries")),
        "name": data.get("title") or "",
        "items": [_pick_fields(e) for e in entries],
    }


def _provisional_name(data: dict) -> str:
    """Nombre de archivo ya sin el ruido tipico de YouTube."""
    base = names.clean(data.get("title") or data.get("id") or "descarga")
    return (names.sanitize(base) or data.get("id") or "descarga")[:120]


def _tag_if_trustworthy(path: Path, data: dict) -> bool:
    """Escribe artista/titulo solo si YouTube dio metadatos de musica de verdad."""
    artist, track = data.get("artist", ""), data.get("track", "")
    if not (artist and track):
        return False
    # "Fulano - Topic" es el canal automatico de YouTube Music, no el artista
    artist = re.sub(r"\s*-\s*topic\s*$", "", artist, flags=re.IGNORECASE).strip()
    if not artist:
        return False
    tags.write(path, artist=names.clean(artist),
                       title=names.clean(track),
                       album=names.clean(data.get("album", "")))
    return True


def download_one(target_url, quality="high", folder=None, progress=None,
                  cancel=None) -> dict:
    """Baja un video y devuelve el mp3 ya listo en `carpeta` (por defecto Entrada/).

    `progress(dict)` recibe {fase, porcentaje, nombre}. `cancelar()` devuelve
    True para abortar.
    """
    yt = _yt_dlp()
    if yt is None or not convert.available():
        return {"ok": False, "reason": unavailable_reason()}

    folder = Path(folder) if folder else config.INBOX
    folder.mkdir(parents=True, exist_ok=True)
    kbps = QUALITY_KBPS.get(quality, QUALITY_KBPS["high"])

    def hook(status):
        if cancel and cancel():
            raise Canceled()
        if not progress:
            return
        if status.get("status") == "downloading":
            total = status.get("total_bytes") or status.get("total_bytes_estimate") or 0
            done = status.get("downloaded_bytes") or 0
            progress({"phase": "downloading",
                      "percent": round(100 * done / total, 1) if total else 0,
                      "name": Path(status.get("filename") or "").name})
        elif status.get("status") == "finished":
            progress({"phase": "converting", "percent": 100, "name": ""})

    with tempfile.TemporaryDirectory(prefix="danplay-yt-") as tmp:
        options = {
            **_base_options(),
            "format": "bestaudio/best",
            "outtmpl": str(Path(tmp) / "%(id)s.%(ext)s"),
            "noplaylist": True,
            "writethumbnail": True,
            # tope de tamaño y de duracion: un directo de tres horas no
            # deberia poder llenar el disco por accidente
            "max_filesize": MAX_BYTES,
            "match_filter": wanted,
            "progress_hooks": [hook],
            "postprocessors": [
                {"key": "FFmpegExtractAudio", "preferredcodec": "mp3",
                 "preferredquality": kbps},
                # la caratula si la queremos: es la miniatura del video
                {"key": "FFmpegThumbnailsConvertor", "format": "jpg"},
                {"key": "EmbedThumbnail", "already_have_thumbnail": False},
            ],
        }
        try:
            with yt.YoutubeDL(options) as ydl:
                data = ydl.extract_info(target_url, download=True)
        except Canceled:
            return {"ok": False, "reason": "canceled", "canceled": True}
        except Exception as e:                              # noqa: BLE001
            return {"ok": False, "reason": str(e)[:200]}
        if not data:
            return {"ok": False, "reason": "no se pudo leer el video"}
        if data.get("entries"):                            # vino una lista: el primero
            entries = [e for e in data["entries"] if e]
            if not entries:
                return {"ok": False, "reason": "lista vacia"}
            data = entries[0]

        mp3 = next((p for p in Path(tmp).glob("*.mp3")), None)
        if mp3 is None:
            return {"ok": False, "reason": "ffmpeg no dejo ningun mp3"}

        d = _pick_fields(data)
        tagged = _tag_if_trustworthy(mp3, d)
        target = folder / names.free_name(str(folder), _provisional_name(d) + ".mp3")
        shutil.move(str(mp3), str(target))

    return {"ok": True, "file": str(target), "tagged": tagged,
            "title": d["title"], "channel": d["channel"], "duration": d["duration"],
            "url": d["url"], "id": d["id"], "kbps": kbps}


def already_in_library(title: str) -> list[dict]:
    """Lo que ya tienes en la biblioteca y parece esta misma cancion."""
    from . import library
    return library.find_by_match_key(names.match_key(names.clean(title or "")))


def _log(entry: dict, query: str, source: str, quality: str) -> None:
    """Deja constancia en el historial. Que falle no debe tumbar la descarga."""
    from . import library
    try:
        library.log_download({
            "source": source, "query": query, "quality": quality,
            "title": entry.get("title") or entry.get("source") or "",
            "channel": entry.get("channel", ""), "url": entry.get("url", ""),
            "ok": entry.get("ok"), "already": entry.get("already_there"),
            "reason": entry.get("reason", ""), "song_id": entry.get("id"),
            "artist": entry.get("artist", ""), "song": entry.get("song", ""),
            "target": entry.get("target", ""), "kbps": entry.get("kbps", ""),
        })
    except Exception:                                       # noqa: BLE001
        pass


def download(inbox: str, quality=None, file_it=True, results=5, force=False,
             source="manual", progress=None, cancel=None) -> list[dict]:
    """Descarga (uno, lista o busqueda) y, si `archivar`, lo pasa por la tuberia.

    Archivar significa lo mismo que en Entrada/: identificar, renombrar segun
    las reglas de la casa y mover a Artistas/<Artista>/.
    """
    quality = quality or config.MP3_QUALITY
    meta = info(inbox, results)
    if not meta["ok"]:
        return [{"ok": False, "reason": meta["reason"]}]

    items = meta["items"]
    vocab = names.vocabulary(config.ARTISTS_DIR) if file_it else {}
    out = []
    for i, t in enumerate(items, 1):
        if cancel and cancel():
            out.append({"ok": False, "reason": "canceled", "canceled": True})
            break
        if progress:
            progress({"phase": "starting", "index": i, "total": len(items),
                      "name": t["title"], "percent": 0})

        # Si ya la tienes, no se baja: se avisa y se deja que tu decidas.
        # `force` la baja igualmente y el sufijo « - r» la marca como repetida.
        if not force:
            mine = already_in_library(t["title"])
            if mine:
                repetida = {"ok": False, "already_there": True, "title": t["title"],
                            "url": t["url"], "source": t["title"],
                            "reason": "ya la tienes en la biblioteca",
                            "matches": mine[:5]}
                _log(repetida, inbox, source, quality)
                out.append(repetida)
                continue

        def step(p, _i=i, _t=t):
            if progress:
                progress({**p, "index": _i, "total": len(items),
                          "name": p.get("name") or _t["title"]})

        # Si se va a archivar, la descarga se posa en un temporal y no en
        # Entrada/. Si cae en Entrada y ya hay una copia, `free_name` le pone
        # el sufijo « - r2», la identificacion lee ESE nombre y el sufijo se
        # queda dentro del titulo para siempre.
        stage = tempfile.mkdtemp(prefix="danplay-stage-") if file_it else None
        try:
            r = download_one(t["url"] or t["id"], quality, folder=stage,
                             progress=step, cancel=cancel)
            r["forced"] = bool(force)
            r["source"] = t["title"]
            if r.get("ok") and file_it:
                if progress:
                    progress({"phase": "filing", "index": i, "total": len(items),
                              "name": t["title"], "percent": 100})
                res = ingest.process(r["file"], vocab, convert_mp3=False)
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
                    nueva = library.index_file(str(res.target))
                    if nueva:
                        r["id"] = nueva["id"]
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


def run_job(query: str, quality=None, file_it=True, results=5, force=False,
            source="manual") -> list[dict]:
    """`descargar` publicando el avance en ESTADO. Solo una descarga a la vez."""
    if STATE["active"]:
        return [{"ok": False, "reason": "ya hay una descarga en marcha"}]
    _CANCELAR["requested"] = False
    STATE.update({"active": True, "phase": "starting", "name": "", "percent": 0.0,
                   "index": 0, "total": 0, "results": [], "error": ""})
    try:
        rs = download(query, quality=quality, file_it=file_it, results=results,
                      force=force, source=source, progress=_publish,
                      cancel=canceled)
        STATE["results"] = rs
        return rs
    except Exception as e:                                  # noqa: BLE001
        STATE["error"] = str(e)[:200]
        return [{"ok": False, "reason": str(e)[:200]}]
    finally:
        STATE["active"] = False
        STATE["phase"] = "canceled" if _CANCELAR["requested"] else "done"
