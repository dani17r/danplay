"""Enriquecimiento: letra, portada, acordes, metadata y artistas implicados.

Fuentes, en orden de preferencia:
  letra    -> LRCLIB (abierto, gratis, con letra sincronizada) -> IA
  portada  -> iTunes Search (sin clave) -> Cover Art Archive
  acordes  -> un cifrado publicado (Ultimate Guitar, LaCuerda: ver cifrados.py),
              nunca la IA; el tono, solo si el cifrado lo dice
  metadata -> MusicBrainz -> IA
"""

import json
import logging
import re
import urllib.parse
import urllib.request
from collections.abc import Mapping
from datetime import date
from typing import Any

from . import ai, cifrados, convert, library, tags, theory

log = logging.getLogger(__name__)

USER_AGENT = "DanPlay/0.1 (gestor de biblioteca personal)"
TIMEOUT = 15
# Tope de lo que se descarga de una caratula. Sin el, una respuesta grande
# (o una pagina de error) se incrustaba entera dentro del mp3.
MAX_IMAGE_BYTES = 5 * 1024 * 1024
# Los primeros bytes de cada formato. Una pagina HTML de error devuelta con
# `Content-Type: image/jpeg` no pasa de aqui.
MAGIC = (
    (b"\xff\xd8\xff", "image/jpeg"),
    (b"\x89PNG\r\n\x1a\n", "image/png"),
    (b"RIFF", "image/webp"),
)
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", re.I)


def _get(url, headers=None, binary=False):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        if not binary:
            return json.loads(r.read().decode("utf-8"))
        kind = (r.headers.get("content-type") or "").split(";")[0].strip().lower()
        if not kind.startswith("image/"):
            raise ValueError(f"eso no es una imagen, es {kind or 'algo sin tipo'}")
        # se lee uno de mas para saber si se paso del tope, y no el archivo entero
        data = r.read(MAX_IMAGE_BYTES + 1)
        if len(data) > MAX_IMAGE_BYTES:
            raise ValueError("la imagen pesa demasiado")
        return data


def image_type(data: bytes) -> str:
    """El tipo real segun los primeros bytes, o cadena vacia si no es imagen."""
    for magic, kind in MAGIC:
        if data.startswith(magic):
            if kind == "image/webp" and data[8:12] != b"WEBP":
                continue
            return kind
    return ""


# ---------------------------------------------------------------- letra

# La letra limpia, sin las marcas de un LRC (vive en `tags`, que es donde se
# decide que va en el USLT).
lrc_to_plain = tags.lrc_to_plain


def _lrclib_result(d) -> dict | None:
    """Lo que interesa de una respuesta de LRCLIB: la letra limpia y la LRC."""
    if not isinstance(d, dict):
        return None
    synced = str(d.get("syncedLyrics") or "")
    plain = str(d.get("plainLyrics") or "") or lrc_to_plain(synced)
    if not plain and not synced:
        return None
    return {"lyrics": plain, "synced": synced, "source": "lrclib"}


def lyrics_lrclib(artist, title, album="", duration: float = 0) -> dict | None:
    """LRCLIB devuelve letra plana y sincronizada (.lrc). Sin clave de API."""
    p = {"artist_name": artist, "track_name": title}
    if album:
        p["album_name"] = album
    if duration:
        p["duration"] = int(duration)
    try:
        found = _lrclib_result(_get("https://lrclib.net/api/get?" + urllib.parse.urlencode(p)))
        if found:
            return found
    except Exception:
        log.debug("LRCLIB no tiene %s - %s", artist, title, exc_info=True)
    try:  # busqueda difusa
        d = _get(
            "https://lrclib.net/api/search?" + urllib.parse.urlencode({"q": f"{artist} {title}"})
        )
        for r in (d if isinstance(d, list) else [])[:3]:
            found = _lrclib_result(r)
            if found:
                return found
    except Exception:
        log.debug("la busqueda en LRCLIB fallo para %s - %s", artist, title, exc_info=True)
    return None


def lyrics(artist, title, album="", duration: float = 0, permitir_ia=True) -> dict | None:
    r = lyrics_lrclib(artist, title, album, duration)
    if r:
        return r
    if permitir_ia and ai.available():
        d = ai.ask(
            f"Letra completa de la cancion '{title}' de {artist}. "
            "Si no la conoces con certeza, responde exactamente NO_LA_SE. "
            "Devuelve solo la letra, sin comentarios.",
            max_tokens=1500,
        )
        if d and "NO_LA_SE" not in d.upper() and len(d) > 80:
            return {"lyrics": d.strip(), "synced": "", "source": "ai"}
    return None


# ---------------------------------------------------------------- portada


def _cover_queries(artist, title, album) -> list[str]:
    """Consultas a probar, de la mas precisa a la mas suelta.

    Los titulos que vienen de descargas llevan ruido («- r», «(En Vivo)»,
    «(cover X)») y con eso iTunes no encuentra nada. Se prueba tambien una
    version limpia, y sin artista cuando no lo hay.
    """
    limpio = re.sub(r"\s*-\s*r\d*$", "", title or "").strip()
    sin_parentesis = re.sub(r"\s*[\(\[][^)\]]*[\)\]]", "", limpio).strip()
    salida = []
    for t in (album or "", limpio, sin_parentesis):
        if not t:
            continue
        for q in (f"{artist} {t}".strip(), t):
            if q and q not in salida:
                salida.append(q)
    return salida[:4]


def _fetch_image(url: str) -> tuple[bytes, str] | None:
    """Descarga una imagen y la deja lista para incrustar.

    Tres cosas que antes no se hacian: mirar que de verdad sea una imagen (no
    la pagina de error de un servicio caido), no pasar de 5 MB, y encogerla.
    Las caratulas que elige el usuario si se encogian; las que se bajan solas
    no, asi que una portada de 1000x1000 engordaba cada mp3 en dos megas.
    """
    try:
        data = _get(url, binary=True)
    except Exception:
        log.warning("no pude bajar la caratula %s", url, exc_info=True)
        return None
    kind = image_type(data)
    if not kind:
        log.warning("lo que devolvio %s no es una imagen", url)
        return None
    return convert.shrink_bytes(data, kind) or (data, kind)


def cover(artist, title, album="") -> tuple[bytes, str] | None:
    """Busca caratula. iTunes primero (rapido, sin clave), luego Cover Art Archive."""
    for query in _cover_queries(artist, title, album):
        try:
            d = _get(
                "https://itunes.apple.com/search?"
                + urllib.parse.urlencode({"term": query, "entity": "song", "limit": 5})
            )
        except Exception:
            log.warning("iTunes no contesto para %r", query, exc_info=True)
            continue
        for r in d.get("results", []):
            url = r.get("artworkUrl100") or ""
            if not url.startswith("https://"):
                continue
            got = _fetch_image(url.replace("100x100bb", "1000x1000bb"))
            if got:
                return got
    try:
        d = _get(
            "https://musicbrainz.org/ws/2/release/?"
            + urllib.parse.urlencode(
                {
                    "query": f'artist:"{artist}" AND release:"{album or title}"',
                    "fmt": "json",
                    "limit": 3,
                }
            )
        )
    except Exception:
        log.warning("MusicBrainz no contesto", exc_info=True)
        return None
    for rel in d.get("releases", []):
        # el id se pega dentro de una URL: si no es un UUID, no se usa
        if not UUID.match(str(rel.get("id", ""))):
            continue
        got = _fetch_image(f"https://coverartarchive.org/release/{rel['id']}/front-500")
        if got:
            return got
    return None


# ---------------------------------------------------------------- IA


def details(song: Mapping[str, Any]) -> dict | None:
    """Artistas implicados, album, año, genero y contexto. El tono y los
    acordes no: esos salen de un cifrado publicado (`chords`)."""
    if not ai.available():
        return None
    meta = (
        f"Artista: {song.get('artist', '?')}\n"
        f"Titulo: {song.get('title', '?')}\n"
        f"Album: {song.get('album', '') or '?'}\n"
        f"Duracion: {int(song.get('duration', 0) // 60)}:{int(song.get('duration', 0) % 60):02d}"
    )
    schema = """Devuelve SOLO un JSON con esta forma, con las claves EXACTAS en ingles:
{
 "album": "", "year": "", "genre": "", "composers": "",
 "involved_artists": ["nombre1","nombre2"],
 "about_the_song": "dos o tres frases",
 "confidence": 0.0
}
Los valores van en español. Reglas: sin tildes salvo la ñ; nada en MAYUSCULA
SOSTENIDA. Si no conoces la cancion, confidence baja. Nunca inventes datos con
confidence alta.

MUY IMPORTANTE: lo que no sepas va como cadena VACIA "". Nunca escribas
"desconocido", "n/a", "varios" ni nada parecido: eso ensucia la ficha y hace
creer que el dato ya esta. El año son cuatro cifras o nada."""
    return ai.ask_json(
        f"{schema}\n\nCancion:\n{meta}", "Eres un musico de sesion y catalogador musical."
    )


# Lo que la IA decia de memoria sobre el tono y los acordes antes de 1.21. Ya
# no se pide, y lo que quede guardado no se enseña: eran aproximaciones.
_GUESSED = ("likely_key", "progression", "section_chords")
# lo que va en la columna `chords` y no es de la IA: el cifrado de la web
_SHEET = ("sheet", "sheet_checked", "sheet_tried")


def _doc(song: Mapping[str, Any]) -> dict:
    """El JSON de la columna `chords`: la ficha de la IA y el cifrado."""
    try:
        d = json.loads(song.get("chords") or "{}")
    except ValueError:
        return {}
    return d if isinstance(d, dict) else {}


def stored_sheet(song: Mapping[str, Any]) -> dict | None:
    """El cifrado guardado de una cancion, sin buscar nada."""
    sheet = _doc(song).get("sheet")
    return sheet if isinstance(sheet, dict) else None


def _save_doc(song_id, doc: dict) -> None:
    library.update(song_id, chords=json.dumps(doc, ensure_ascii=False))


def _save_details(song: Mapping[str, Any], d: dict) -> None:
    """Guarda lo que dijo la IA sin perder el cifrado que ya hubiera."""
    keep = {k: v for k, v in _doc(song).items() if k in _SHEET}
    fresh = {k: v for k, v in d.items() if k not in _GUESSED and k not in _SHEET}
    _save_doc(song["id"], {**fresh, **keep})


def cached_details(song: Mapping[str, Any]) -> dict | None:
    """La ficha de la IA guardada, si merece la pena. Una respuesta con
    confianza baja no se reutiliza: la dio un modelo que no conocia la
    cancion, y con otro mejor —o el mismo otro dia— puede salir."""
    d = _doc(song)
    if _confidence(d) < 0.5:
        return None
    return {k: v for k, v in d.items() if k not in _GUESSED and k not in _SHEET}


def _confidence(d: dict) -> float:
    """La confianza que dio la IA, como numero (a veces llega «alta» o nada)."""
    try:
        return float(d.get("confidence") or 0)
    except (TypeError, ValueError):
        return 0.0


def details_for(song: Mapping[str, Any]) -> tuple[dict | None, bool]:
    """Los detalles de una cancion: los guardados si valen, y si no, se
    piden a la IA y se guardan. Devuelve (detalles, venian_guardados)."""
    cached = cached_details(song)
    if cached:
        return cached, True
    d = details(song)
    if not isinstance(d, dict):
        return None, False
    if not d.get("error"):
        _save_details(song, d)
    return d, False


# ---------------------------------------------------------------- acordes


def chords(song: Mapping[str, Any], refresh: bool = False) -> dict:
    """El cifrado de una cancion: el guardado, o el que se encuentre en la
    web si no se habia buscado (o si se pide otra vez con `refresh`).

    Devuelve {"sheet": cifrado o None, "cached", "checked": cuando se busco,
    "tried": fuentes consultadas, "failed": las que no respondieron}. Que no
    este en ninguna se guarda tambien (no se vuelve a buscar sola cada vez);
    que no respondieran, no: es la red, no la cancion."""
    doc = _doc(song)
    if not refresh and "sheet" in doc:
        return {
            "sheet": doc["sheet"],
            "cached": True,
            "checked": doc.get("sheet_checked", ""),
            "tried": doc.get("sheet_tried", []),
            "failed": [],
        }
    if not str(song.get("title") or "").strip():
        return {
            "sheet": None,
            "cached": False,
            "checked": "",
            "tried": [],
            "failed": [],
            "error": "la cancion no tiene titulo",
        }
    r = cifrados.find(str(song.get("artist") or ""), str(song.get("title") or ""))
    today = date.today().isoformat()
    if r["sheet"] or not r["failed"]:
        doc.update(sheet=r["sheet"], sheet_checked=today, sheet_tried=r["tried"])
        _save_doc(song["id"], doc)
    return {**r, "cached": False, "checked": today}


def transpose_sheet(sheet: Mapping[str, Any], semitones: int = 0, to_key: str = "") -> dict:
    """El cifrado en otro tono: solo se tocan las lineas de acordes (en una
    de letra, «Dios» no es un Re)."""
    from_key = str(sheet.get("key") or "")
    if to_key and from_key:
        n = theory.distance(from_key, to_key) or 0
        flats = to_key in theory.FLAT_KEYS
    else:
        n, flats = semitones % 12, None
        to_key = theory.transpose(from_key, n) if from_key else ""
    out = dict(sheet)
    out["sections"] = [
        {
            **s,
            "chords": [theory.transpose(c, n, flats) for c in s.get("chords") or []],
            "lines": [
                {**ln, "t": theory.transpose(ln["t"], n, flats)} if ln.get("c") else ln
                for ln in s.get("lines") or []
            ],
        }
        for s in sheet.get("sections") or []
    ]
    out["key"] = to_key
    out["semitones"] = n
    out["capo_hint"] = theory.suggested_capo(to_key) if to_key else []
    return out


# campos de ficha que la IA puede rellenar
FILLABLE = ("album", "year", "genre", "key")

# Lo que devuelve un modelo cuando no sabe algo. Guardarlo seria peor que
# dejarlo vacio: el campo parece relleno y ya nadie vuelve a mirarlo.
NO_SABE = {
    "desconocido",
    "desconocida",
    "unknown",
    "n/a",
    "na",
    "none",
    "null",
    "sin album",
    "sin genero",
    "sin datos",
    "no disponible",
    "-",
    "--",
    "?",
    "??",
    "sin especificar",
    "varios",
    "no aplica",
}


def _dato_util(campo: str, valor) -> str:
    """El valor si sirve, o cadena vacia. Evita guardar «desconocido»."""
    v = str(valor or "").strip()
    if not v or v.lower() in NO_SABE:
        return ""
    if campo == "year":
        m = re.search(r"\b(1[89]\d{2}|20\d{2})\b", v)  # un año de verdad
        return m.group(0) if m else ""
    return v


def autofill(song_id) -> dict:
    """Rellena album, año, genero y tono, solo lo que este vacio.

    El tono sale del cifrado publicado de la cancion (y solo si lo dice), no
    de la IA: eso no necesita IA. Album, año y genero, de la IA.

    Devuelve que se relleno, que sigue faltando y por que, para que la interfaz
    pueda decirlo en vez de dejar al usuario adivinando.
    """
    c = library.by_id(song_id)
    if not c:
        return {"ok": False, "filled": {}, "missing": [], "reason": "no existe esa cancion"}

    faltan = [k for k in FILLABLE if not str(c.get(k) or "").strip()]
    if not faltan:
        return {"ok": True, "filled": {}, "missing": [], "reason": "", "complete": True}
    if not str(c["artist"] or "").strip():
        return {
            "ok": False,
            "filled": {},
            "missing": faltan,
            "reason": "esta cancion no tiene artista identificado, asi que no se puede reconocer",
        }

    nuevos: dict[str, str] = {}
    motivos: list[str] = []
    # lo que impidio preguntar (la IA sin configurar, la red), no que no lo sepan
    fallos = 0
    if "key" in faltan:
        r = chords(c)
        if key := cifrados.sheet_key(r["sheet"]):
            nuevos["key"] = key
        elif r["sheet"] and r["sheet"].get("key"):
            motivos.append("el cifrado va con cejilla: su tono no es el que suena")
        elif r["sheet"]:
            motivos.append(f"el cifrado de {r['sheet']['source']} no dice el tono")
        elif r["failed"]:
            fallos += 1
            motivos.append("no se pudo consultar " + " ni ".join(r["failed"]))
        else:
            motivos.append("no hay cifrado publicado en " + " ni ".join(r["tried"]))

    pide_ia = [k for k in faltan if k != "key"]
    if pide_ia and not ai.available():
        fallos += 1
        motivos.append(f"la IA no esta lista: {ai.unavailable_reason()} (Ajustes)")
    elif pide_ia:
        d = details(c)
        if not isinstance(d, dict) or not d or d.get("error"):
            fallos += 1
            motivos.append(
                (d.get("error") if isinstance(d, dict) else "") or "la IA no pudo responder"
            )
        else:
            for k in pide_ia:
                if v := _dato_util(k, d.get(k)):
                    nuevos[k] = v
            if any(k not in nuevos for k in pide_ia):
                motivos.append(
                    f"la IA no reconoce bien esta cancion (confianza {round(_confidence(d) * 100)}%)"
                )
    if nuevos:
        library.edit(song_id, **nuevos)

    restantes = [k for k in faltan if k not in nuevos]
    return {
        "ok": bool(nuevos) or not fallos,
        "filled": nuevos,
        "missing": restantes,
        "reason": "; ".join(motivos) if restantes else "",
        "complete": not restantes,
    }


# ---------------------------------------------------------------- orquestacion


def enrich(
    song_id, with_lyrics=True, with_cover=True, with_details=True, save_to_file=True
) -> dict:
    c = library.by_id(song_id)
    if not c:
        return {"error": "no existe esa cancion"}
    done = {"id": song_id, "artist": c["artist"], "title": c["title"]}

    if with_lyrics and not c["lyrics"]:
        r = lyrics(c["artist"], c["title"], c["album"], c["duration"])
        if r:
            # La letra LIMPIA va al indice y al USLT del archivo; la version
            # con tiempos, que solo da LRCLIB, se queda en el indice como cache
            # aparte. Antes se grababa la LRC en el USLT y, tras reescanear, la
            # hoja del atril imprimia las marcas delante de cada verso.
            plain = r["lyrics"] or lrc_to_plain(r.get("synced") or "")
            library.update(song_id, lyrics=plain, lyrics_synced=r.get("synced") or "")
            if save_to_file:
                tags.write_lyrics(c["path"], plain)
            done["lyrics"] = r["source"]

    if with_cover and not c["cover"]:
        r = cover(c["artist"], c["title"], c["album"])
        if r and save_to_file and tags.write_cover(c["path"], r[0], r[1]):
            library.update(song_id, cover="embedded")
            done["cover"] = f"{len(r[0]) // 1024} KB"

    if with_details:
        d = details(c)
        if isinstance(d, dict) and d and not d.get("error"):
            # OJO: por aqui tambien entra lo que dice la IA, asi que pasa por
            # el mismo filtro que `autofill`. Sin el, un «desconocido» del
            # modelo se guardaba como album de verdad.
            album = _dato_util("album", d.get("album")) or c["album"]
            year = _dato_util("year", d.get("year")) or str(c["year"] or "")
            genre = _dato_util("genre", d.get("genre")) or c["genre"]
            _save_details(c, d)
            library.update(song_id, album=album, year=year, genre=genre)
            if save_to_file:
                tags.write(c["path"], album=album, year=year, genre=genre)
            done["details"] = {k: d.get(k) for k in ("genre", "year", "confidence")}
    return done
