"""Acordes y tono sacados de cifrados publicados en la web, nunca inventados.

Cada fuente se consulta con su propio buscador o su propio indice: no hace
falta ningun buscador general (los gratuitos cortan si se les pregunta seguido)
ni ninguna clave.

  Ultimate Guitar  su buscador (search.php) y la pagina del cifrado. Los datos
                   vienen en JSON dentro de la pagina: el tono que puso quien
                   lo escribio, la cejilla, los votos, las secciones ([Coro])
                   y cada acorde marcado como acorde ([ch]G[/ch]).
  LaCuerda         la pagina del artista lista todas sus canciones; cada
                   cifrado marca los acordes con <A>. No guarda el tono.

Todo lo que sale de aqui se lee tal cual del cifrado: los acordes son los que
la pagina marca como acordes, el tono solo si la fuente lo dice (en sus datos
o en una linea «Tono: A» del propio texto) y siempre va el enlace para
comprobarlo. Si no se encuentra la cancion, no hay acordes: no se adivinan.
"""

import html
import json
import logging
import re
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor

from . import __version__, names, theory

log = logging.getLogger(__name__)

USER_AGENT = f"DanPlay/{__version__} (gestor de biblioteca personal)"
TIMEOUT = 12

UG_SEARCH = "https://www.ultimate-guitar.com/search.php?search_type=title&value="
LC_BASE = "https://acordes.lacuerda.net/"

# Palabras que no distinguen una cancion de otra: van en los titulos de
# YouTube y de los cifrados («en Español», «(Live)», «Acústico»...).
_NOISE_WORDS = {
    "espanol",
    "spanish",
    "version",
    "cover",
    "acustico",
    "acustica",
    "acoustic",
    "live",
    "vivo",
    "directo",
    "oficial",
    "official",
    "video",
    "audio",
    "lyric",
    "lyrics",
    "letra",
    "chords",
    "acordes",
    "remix",
    "instrumental",
    "karaoke",
}
_PARENS = re.compile(r"\([^()]*\)|\[[^\[\]]*\]")


def _no_parens(s: str) -> str:
    """Sin lo que va entre parentesis, tambien si van uno dentro de otro."""
    while True:
        out = _PARENS.sub(" ", s)
        if out == s:
            return out.replace("(", " ").replace(")", " ")
        s = out


# Secciones que se reconocen en un cifrado sin corchetes («Coro:», «VERSO 2»)
# La parte es la linea entera: la palabra y, como mucho, un numero, «x2» o
# «final» («Verso 2:», «CORO», «Pre-Coro:»). «Coro de angeles» es letra.
_SECTION = re.compile(
    r"^\s*[(\[]?\s*((?:pre\s*-?\s*)?(?:intro|introduccion|verso|estrofa|coro|precoro|puente|"
    r"final|outro|interludio|instrumental|solo|bridge|chorus|verse|pre-?chorus|tag|vamp|"
    r"riff|refran|estribillo)(?:\s*\d+|\s*x\s*\d+|\s+final)?)\s*[)\]]?\s*:?\s*$",
    re.IGNORECASE,
)
# «Tono: A», «TONALIDAD: F#m», «Key: Bb», «Tom: G» en la cabecera del texto
_KEY_LINE = re.compile(
    r"\b(?:tono|tonalidad|key|tom)\s*(?:original)?\s*[:=]\s*([A-G][#b]?m?)\b", re.I
)
_CAPO_LINE = re.compile(r"\b(?:capo|cejilla)\s*[:=]?\s*(\d{1,2})\b", re.I)


# ---------------------------------------------------------------- red


def _get(url: str) -> str:
    req = urllib.request.Request(
        url, headers={"User-Agent": USER_AGENT, "Accept-Language": "es-ES,es;q=0.9"}
    )
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        return r.read().decode("utf-8", "replace")


# ---------------------------------------------------------------- comparar


def _ascii(s: str) -> str:
    # `names.strip_accents` deja la ñ (es lo que se quiere en un nombre de
    # archivo); para comparar, «Español» y «Espanol» son lo mismo
    return unicodedata.normalize("NFKD", s or "").encode("ascii", "ignore").decode()


def _words(s: str) -> set[str]:
    """Las palabras que identifican un titulo o un artista."""
    return set(names.match_key(_no_parens(_ascii(s))).split()) - _NOISE_WORDS


def _squash(s: str) -> str:
    return re.sub(r"[^a-z0-9]", "", _ascii(s).lower())


def same_song(lib_artist: str, lib_title: str, artist: str, title: str) -> bool:
    """Si el cifrado (`artist`, `title`) es esta cancion de la biblioteca.

    El titulo tiene que tener exactamente las mismas palabras (sin ruido): «Hay
    libertad» y «Hay libertad en la casa de Dios» no son la misma. El artista
    vale si comparte una palabra, si se escribe igual sin espacios («Sovereign
    Grace» y «SovereignGraceMusic») o si va metido en el titulo, que es lo que
    hacen los canales que suben canciones de otros."""
    ca, ct = _words(artist), _words(title)
    la, lt = _words(lib_artist), _words(lib_title)
    if not ct:
        return False
    if (lt - ca) != ct and lt != ct:
        return False
    sa, sl = _squash(artist), _squash(lib_artist)
    return bool((ca & la) or (ca and ca <= lt) or (sa and sl and (sa in sl or sl in sa)))


# ---------------------------------------------------------------- leer cifrados


def _chord_list(chords: list[str]) -> list[str]:
    """Los acordes en orden, sin repetir el mismo dos veces seguidas."""
    out: list[str] = []
    for c in chords:
        c = c.strip()
        if c and (not out or out[-1] != c):
            out.append(c)
    return out


def _new_section(name: str) -> dict:
    return {"name": name, "lines": [], "chords": []}


def _close(sections: list[dict]) -> list[dict]:
    """Las secciones con algo dentro. Cada linea es {"t": texto, "c": si es
    de acordes}: al transponer solo se tocan esas."""
    out = []
    for s in sections:
        lines = s["lines"]
        while lines and not lines[-1]["t"].strip():
            lines.pop()
        while lines and not lines[0]["t"].strip():
            lines.pop(0)
        if not lines and not s["chords"]:
            continue
        out.append({"name": s["name"], "chords": _chord_list(s["chords"]), "lines": lines})
    return out


def _header_facts(lines: list[str]) -> tuple[str, int]:
    """Tono y cejilla que el propio texto dice en sus primeras lineas."""
    key, capo = "", 0
    for line in lines[:12]:
        if not key and (m := _KEY_LINE.search(line)):
            key = m.group(1)
        if not capo and (m := _CAPO_LINE.search(line)):
            capo = int(m.group(1))
    return key, capo


def parse_ug(content: str) -> dict:
    """El texto de un cifrado de Ultimate Guitar: secciones entre corchetes
    ([Coro]) y cada acorde marcado como [ch]G[/ch]."""
    content = content.replace("\r", "").replace("[tab]", "").replace("[/tab]", "")
    sections = [_new_section("")]
    plain_lines = []
    for line in content.split("\n"):
        head = re.fullmatch(r"\s*\[([^\[\]]{1,40})\]\s*", line)
        if head and head.group(1).lower() not in ("ch", "/ch"):
            sections.append(_new_section(head.group(1).strip()))
            continue
        chords = re.findall(r"\[ch\](.*?)\[/ch\]", line)
        text = re.sub(r"\[/?ch\]", "", line).rstrip()
        # hay quien escribe las partes como texto («Coro:») dentro de una [Verse]
        if not chords and (sm := _SECTION.match(text)):
            sections.append(_new_section(sm.group(1).strip().capitalize()))
            continue
        sections[-1]["chords"].extend(chords)
        sections[-1]["lines"].append({"t": text, "c": bool(chords)})
        plain_lines.append(text)
    key, capo = _header_facts(plain_lines)
    return {"sections": _close(sections), "key": key, "capo": capo}


def parse_lacuerda(page: str) -> dict:
    """La pagina de un cifrado de LaCuerda: el texto va en <div id=t_body><PRE>
    y cada acorde, entre <A></A>."""
    m = re.search(r"id=['\"]?t_body['\"]?[^>]*>\s*<pre[^>]*>(.*?)</pre>", page, re.I | re.S)
    if not m:
        return {"sections": [], "key": "", "capo": 0}
    body = m.group(1)
    sections = [_new_section("")]
    plain_lines = []
    for raw in body.split("\n"):
        chords = [html.unescape(c) for c in re.findall(r"<a[^>]*>(.*?)</a>", raw, re.I)]
        text = html.unescape(re.sub(r"<[^>]+>", "", raw)).rstrip()
        if not chords and (sm := _SECTION.match(text)):
            sections.append(_new_section(sm.group(1).strip().capitalize()))
            continue
        sections[-1]["chords"].extend(chords)
        sections[-1]["lines"].append({"t": text, "c": bool(chords)})
        plain_lines.append(text)
    key, capo = _header_facts(plain_lines)
    return {"sections": _close(sections), "key": key, "capo": capo}


def _valid_key(key: str) -> str:
    """El tono si es un tono que se puede usar (para transponer), o vacio."""
    key = (key or "").strip()
    return key if key and theory._split_key(key) else ""


# ---------------------------------------------------------------- Ultimate Guitar


def _ug_data(page: str) -> dict:
    m = re.search(r'class="js-store"\s+data-content="([^"]+)"', page)
    if not m:
        return {}
    try:
        return json.loads(html.unescape(m.group(1)))["store"]["page"]["data"]
    except (ValueError, KeyError, TypeError):
        return {}


def search_title(artist: str, title: str) -> str:
    """El titulo para buscarlo: sin parentesis, sin ruido («en Español»,
    «Live») y sin el artista, que los canales meten delante."""
    title = _no_parens(title or "")
    title = re.sub(r"\ben\s+(?:espa[nñ]ol|vivo|directo)\b", " ", title, flags=re.I)
    for name in {artist, names.channel_as_artist(artist)}:
        if name and _squash(name):
            title = re.sub(re.escape(name), " ", title, flags=re.I)
    kept = [w for w in re.split(r"[\s\-–—|/:]+", title) if w and _squash(w) not in _NOISE_WORDS]
    return " ".join(kept).strip(" -")


def _ug_candidates(artist: str, title: str) -> list[dict]:
    """Los cifrados de acordes publicos de esta cancion, el mejor primero."""
    clean_title = search_title(artist, title)
    # «MarcosWittVEVO» no lo encuentra nadie; «Marcos Witt», si
    searched_artist = names.channel_as_artist(artist) or artist
    seen, found = set(), []
    for query in (f"{searched_artist} {clean_title}", clean_title):
        query = re.sub(r"\s+", " ", query).strip()
        if not query or query in seen:
            continue
        seen.add(query)
        try:
            page = _get(UG_SEARCH + urllib.parse.quote(query))
        except urllib.error.HTTPError as e:
            if e.code != 404:  # su buscador dice «no hay nada» con un 404
                raise
            continue
        results = _ug_data(page).get("results") or []
        found = [
            r
            for r in results
            if r.get("type") == "Chords"
            and r.get("tab_access_type") == "public"
            and same_song(artist, title, r.get("artist_name", ""), r.get("song_name", ""))
        ]
        if found:
            break

    def rank(r):
        votes = int(r.get("votes") or 0)
        # Uno que dice su tono y que otros han votado gana a uno mas votado
        # que no lo dice: el tono es la mitad de lo que se busca.
        return (
            bool(_valid_key(r.get("tonality_name", ""))) and votes >= 3,
            votes,
            r.get("rating") or 0,
        )

    return sorted(found, key=rank, reverse=True)


def ultimate_guitar(artist: str, title: str) -> dict | None:
    candidates = _ug_candidates(artist, title)
    if not candidates:
        return None
    best = candidates[0]
    data = _ug_data(_get(best["tab_url"]))
    tab = data.get("tab") or {}
    view = data.get("tab_view") or {}
    meta = view.get("meta") or {}
    meta = meta if isinstance(meta, dict) else {}
    sheet = parse_ug(((view.get("wiki_tab") or {}).get("content")) or "")
    if not sheet["sections"]:
        return None
    stated = _valid_key(tab.get("tonality_name") or meta.get("tonality") or "")
    key = stated or _valid_key(sheet["key"])
    try:
        capo = int(meta.get("capo") or 0) or sheet["capo"]
    except (TypeError, ValueError):
        capo = sheet["capo"]
    return {
        "source": "Ultimate Guitar",
        "url": best["tab_url"],
        "artist": best.get("artist_name", ""),
        "title": best.get("song_name", ""),
        "key": key,
        "key_from": ("datos" if stated else "texto") if key else "",
        "capo": capo,
        "votes": int(best.get("votes") or 0),
        "rating": round(float(best.get("rating") or 0), 2),
        "versions": len(candidates),
        "sections": sheet["sections"],
    }


# ---------------------------------------------------------------- LaCuerda


def _slug(s: str) -> str:
    s = _ascii(s).lower()
    s = s.replace("'", "").replace("’", "")
    return re.sub(r"[^a-z0-9]+", "_", s).strip("_")


def lacuerda(artist: str, title: str) -> dict | None:
    slug = _slug(artist)
    if not slug:
        return None
    try:
        index = _get(f"{LC_BASE}{slug}/")
    except OSError:
        return None
    songs = re.findall(r"<li id='r\d+'[^>]*><a href=\"([a-z0-9_]+)\">([^<]+)<em>", index, re.I)
    match, listed = next(
        (
            (s, html.unescape(name).strip())
            for s, name in songs
            if same_song(artist, title, artist, html.unescape(name).strip())
        ),
        (None, ""),
    )
    if not match:
        return None
    versions_page = _get(f"{LC_BASE}{slug}/{match}")
    # las versiones vienen ordenadas por valoracion: la primera es la mejor
    versions = re.findall(r"<li id='liElm\d+'.*?<a href='([^']+\.shtml)'", versions_page, re.S)
    url = f"{LC_BASE}{slug}/{versions[0] if versions else match + '.shtml'}"
    page = _get(url)
    sheet = parse_lacuerda(page)
    if not sheet["sections"] or not any(s["chords"] for s in sheet["sections"]):
        return None
    name = re.search(r"<H1>([^<]+)<br>\s*<A[^>]*>([^<]+)</A>", page, re.I)
    key = _valid_key(sheet["key"])
    return {
        "source": "LaCuerda",
        "url": url,
        "artist": html.unescape(name.group(2)).strip() if name else artist,
        "title": html.unescape(name.group(1)).strip() if name else listed,
        "key": key,
        "key_from": "texto" if key else "",
        "capo": sheet["capo"],
        "votes": 0,
        "rating": 0,
        "versions": max(len(versions), 1),
        "sections": sheet["sections"],
    }


# ---------------------------------------------------------------- buscar


def sheet_key(sheet: dict | None) -> str:
    """El tono de la cancion segun su cifrado, si se puede afirmar: lo dice la
    fuente y se toca sin cejilla (con cejilla, lo escrito no es lo que suena)."""
    if not sheet or int(sheet.get("capo") or 0):
        return ""
    return str(sheet.get("key") or "")


SOURCES = (("Ultimate Guitar", ultimate_guitar), ("LaCuerda", lacuerda))


def find(artist: str, title: str) -> dict:
    """Busca el cifrado de una cancion en todas las fuentes a la vez.

    Devuelve {"sheet": el mejor o None, "tried": [fuentes consultadas],
    "failed": [las que no respondieron], "seconds": lo que tardo}. Gana el que
    dice su tono; entre iguales, Ultimate Guitar (tiene votos)."""
    artist, title = (artist or "").strip(), (title or "").strip()
    started = time.monotonic()
    if not title:
        return {"sheet": None, "tried": [], "failed": [], "seconds": 0.0}
    sheets, failed = [], []
    with ThreadPoolExecutor(len(SOURCES)) as pool:
        futures = {name: pool.submit(fn, artist, title) for name, fn in SOURCES}
        for name, fut in futures.items():
            try:
                if (s := fut.result()) is not None:
                    sheets.append(s)
            except Exception as e:  # noqa: BLE001 - una fuente caida no tumba las demas
                log.info("acordes: %s no respondio (%s)", name, e)
                failed.append(name)
    sheets.sort(key=lambda s: bool(s["key"]), reverse=True)
    return {
        "sheet": sheets[0] if sheets else None,
        "tried": [name for name, _ in SOURCES],
        "failed": failed,
        "seconds": round(time.monotonic() - started, 1),
    }
