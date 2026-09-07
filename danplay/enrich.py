# -*- coding: utf-8 -*-
"""Enriquecimiento: letra, portada, acordes, metadata y artistas implicados.

Fuentes, en orden de preferencia:
  letra    -> LRCLIB (abierto, gratis, con letra sincronizada) -> IA
  portada  -> iTunes Search (sin clave) -> Cover Art Archive
  acordes  -> IA (aproximados) + transposicion deterministica
  metadata -> MusicBrainz -> IA
"""
import json, re, urllib.parse, urllib.request
from . import ai, library, tags, theory

USER_AGENT = "DanPlay/0.1 (gestor de biblioteca personal)"
TIMEOUT = 15


def _get(url, headers=None, binary=False):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        return r.read() if binary else json.loads(r.read().decode("utf-8"))


# ---------------------------------------------------------------- letra

def lyrics_lrclib(artist, title, album="", duration=0) -> dict | None:
    """LRCLIB devuelve letra plana y sincronizada (.lrc). Sin clave de API."""
    p = {"artist_name": artist, "track_name": title}
    if album:   p["album_name"] = album
    if duration: p["duration"] = int(duration)
    try:
        d = _get("https://lrclib.net/api/get?" + urllib.parse.urlencode(p))
        if d.get("plainLyrics") or d.get("syncedLyrics"):
            return {"lyrics": d.get("plainLyrics") or "", "synced": d.get("syncedLyrics") or "",
                    "source": "lrclib"}
    except Exception:
        pass
    try:  # busqueda difusa
        d = _get("https://lrclib.net/api/search?" +
                 urllib.parse.urlencode({"q": f"{artist} {title}"}))
        for r in (d or [])[:3]:
            if r.get("plainLyrics") or r.get("syncedLyrics"):
                return {"lyrics": r.get("plainLyrics") or "",
                        "synced": r.get("syncedLyrics") or "", "source": "lrclib"}
    except Exception:
        pass
    return None


def lyrics(artist, title, album="", duration=0, permitir_ia=True) -> dict | None:
    r = lyrics_lrclib(artist, title, album, duration)
    if r:
        return r
    if permitir_ia and ai.available():
        d = ai.ask(
            f"Letra completa de la cancion '{title}' de {artist}. "
            "Si no la conoces con certeza, responde exactamente NO_LA_SE. "
            "Devuelve solo la letra, sin comentarios.", max_tokens=1500)
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


def cover(artist, title, album="") -> tuple[bytes, str] | None:
    """Busca caratula. iTunes primero (rapido, sin clave), luego Cover Art Archive."""
    for query in _cover_queries(artist, title, album):
        try:
            d = _get("https://itunes.apple.com/search?" + urllib.parse.urlencode(
                {"term": query, "entity": "song", "limit": 5}))
            for r in d.get("results", []):
                url = r.get("artworkUrl100") or ""
                if url:
                    url = url.replace("100x100bb", "1000x1000bb")
                    return _get(url, binary=True), "image/jpeg"
        except Exception:
            continue
    try:
        d = _get("https://musicbrainz.org/ws/2/release/?" + urllib.parse.urlencode(
            {"query": f'artist:"{artist}" AND release:"{album or title}"',
             "fmt": "json", "limit": 3}))
        for rel in d.get("releases", []):
            try:
                return _get(f"https://coverartarchive.org/release/{rel['id']}/front-500",
                            binary=True), "image/jpeg"
            except Exception:
                continue
    except Exception:
        pass
    return None


# ---------------------------------------------------------------- IA

def details(song: dict) -> dict | None:
    """Acordes, artistas implicados, album, año, genero y contexto."""
    if not ai.available():
        return None
    meta = (f"Artista: {song.get('artist','?')}\n"
             f"Titulo: {song.get('title','?')}\n"
             f"Album: {song.get('album','') or '?'}\n"
             f"Duracion: {int(song.get('duration',0)//60)}:{int(song.get('duration',0)%60):02d}\n"
             f"Tono detectado: {song.get('key','') or 'sin analizar'}\n"
             f"BPM detectado: {song.get('bpm',0) or 'sin analizar'}")
    schema = """Devuelve SOLO un JSON con esta forma, con las claves EXACTAS en ingles:
{
 "album": "", "year": "", "genre": "", "composers": "",
 "involved_artists": ["nombre1","nombre2"],
 "likely_key": "Bb",
 "progression": "| Bb | Gm7 | Eb | F |",
 "section_chords": {"intro":"| Bb | Gm |","verso":"...","coro":"..."},
 "about_the_song": "dos o tres frases",
 "confidence": 0.0
}
Los valores van en español. Reglas: sin tildes salvo la ñ; nada en MAYUSCULA
SOSTENIDA. Los acordes son una aproximacion: si no conoces la cancion, deja
progression vacia y confidence baja. Nunca inventes datos con confidence alta.

MUY IMPORTANTE: lo que no sepas va como cadena VACIA "". Nunca escribas
"desconocido", "n/a", "varios" ni nada parecido: eso ensucia la ficha y hace
creer que el dato ya esta. El año son cuatro cifras o nada."""
    return ai.ask_json(f"{schema}\n\nCancion:\n{meta}",
                       "Eres un musico de sesion y catalogador musical.")


# campos de ficha que la IA puede rellenar
FILLABLE = ("album", "year", "genre", "key")

# Lo que devuelve un modelo cuando no sabe algo. Guardarlo seria peor que
# dejarlo vacio: el campo parece relleno y ya nadie vuelve a mirarlo.
NO_SABE = {"desconocido", "desconocida", "unknown", "n/a", "na", "none",
           "null", "sin album", "sin genero", "sin datos", "no disponible",
           "-", "--", "?", "??", "sin especificar", "varios", "no aplica"}


def _dato_util(campo: str, valor) -> str:
    """El valor si sirve, o cadena vacia. Evita guardar «desconocido»."""
    v = str(valor or "").strip()
    if not v or v.lower() in NO_SABE:
        return ""
    if campo == "year":
        m = re.search(r"\b(1[89]\d{2}|20\d{2})\b", v)   # un año de verdad
        return m.group(0) if m else ""
    return v


def autofill(song_id) -> dict:
    """Rellena album, año, genero y tono con IA, solo lo que este vacio.

    Devuelve que se relleno, que sigue faltando y por que, para que la interfaz
    pueda decirlo en vez de dejar al usuario adivinando.
    """
    c = library.by_id(song_id)
    if not c:
        return {"ok": False, "filled": {}, "missing": [],
                "reason": "no existe esa cancion"}

    faltan = [k for k in FILLABLE if not str(c.get(k) or "").strip()]
    if not faltan:
        return {"ok": True, "filled": {}, "missing": [], "reason": "",
                "complete": True}
    if not ai.available():
        return {"ok": False, "filled": {}, "missing": faltan,
                "reason": "hace falta la clave de IA en Ajustes"}
    if not str(c["artist"] or "").strip():
        return {"ok": False, "filled": {}, "missing": faltan,
                "reason": "esta cancion no tiene artista identificado, "
                          "asi que la IA no la puede reconocer"}

    d = details(c)
    if not d or d.get("error"):
        return {"ok": False, "filled": {}, "missing": faltan,
                "reason": (d or {}).get("error") or "la IA no pudo responder"}

    valores = {"album": d.get("album"), "year": d.get("year"),
               "genre": d.get("genre"), "key": d.get("likely_key")}
    nuevos = {k: _dato_util(k, valores.get(k)) for k in faltan}
    nuevos = {k: v for k, v in nuevos.items() if v}
    if nuevos:
        library.edit(song_id, **nuevos)

    restantes = [k for k in faltan if k not in nuevos]
    motivo = ""
    if restantes:
        conf = d.get("confidence") or 0
        motivo = ("la IA no reconoce bien esta cancion (confianza "
                  f"{round(conf * 100)}%); no se atreve con: "
                  + ", ".join(restantes))
    return {"ok": True, "filled": nuevos, "missing": restantes,
            "reason": motivo, "complete": not restantes}


def transpose_details(details, to_key) -> dict:
    """Aplica transposicion deterministica a lo que devolvio la IA."""
    if not details:
        return {}
    source_path = details.get("likely_key", "")
    out = dict(details)
    out["progression"] = theory.transpose_to(details.get("progression", ""), source_path, to_key)
    sec = details.get("section_chords") or {}
    out["section_chords"] = {k: theory.transpose_to(v, source_path, to_key)
                              for k, v in sec.items()}
    out["likely_key"] = to_key
    out["capo"] = theory.suggested_capo(to_key)
    return out


# ---------------------------------------------------------------- orquestacion

def enrich(song_id, with_lyrics=True, with_cover=True, with_details=True,
               save_to_file=True) -> dict:
    c = library.by_id(song_id)
    if not c:
        return {"error": "no existe esa cancion"}
    done = {"id": song_id, "artist": c["artist"], "title": c["title"]}

    if with_lyrics and not c["lyrics"]:
        r = lyrics(c["artist"], c["title"], c["album"], c["duration"])
        if r:
            library.update(song_id, lyrics=r["lyrics"])
            if save_to_file:
                tags.write_lyrics(c["path"], r["synced"] or r["lyrics"])
            done["lyrics"] = r["source"]

    if with_cover and not c["cover"]:
        r = cover(c["artist"], c["title"], c["album"])
        if r and save_to_file and tags.write_cover(c["path"], r[0], r[1]):
            library.update(song_id, cover="embedded")
            done["cover"] = f"{len(r[0])//1024} KB"

    if with_details:
        d = details(c)
        if d and not d.get("error"):
            library.update(song_id, chords=json.dumps(d, ensure_ascii=False),
                         album=d.get("album") or c["album"],
                         year=str(d.get("year") or c["year"]),
                         genre=d.get("genre") or c["genre"])
            if save_to_file:
                tags.write(c["path"], album=d.get("album",""), year=str(d.get("year","")),
                           genre=d.get("genre",""))
            done["details"] = {k: d.get(k) for k in
                                 ("likely_key","genre","year","confidence")}
    return done
