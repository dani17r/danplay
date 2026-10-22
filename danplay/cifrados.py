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

Leer, comparar y elegir lo hace el crate de Rust (biblioteca/src/cifrados.rs);
pedir cada pagina, de momento, urllib (`_get`: asi las pruebas la cortan o la
sustituyen). Esto es la puerta de Python, con los mismos nombres.
"""

import logging
import urllib.error
import urllib.parse
import urllib.request

import danplay_core as _rust

from . import __version__

log = logging.getLogger(__name__)

USER_AGENT = f"DanPlay/{__version__} (gestor de biblioteca personal)"
UG_SEARCH, LC_BASE, TIMEOUT = _rust.cifrados_constants()


def _txt(s) -> str:
    """Un texto que se pueda pasar a Rust: vacio si es falso, y los
    «surrogates» de uno que no es UTF-8, como «�»."""
    s = s or ""
    if isinstance(s, str):
        try:
            s.encode("utf-8")
        except UnicodeEncodeError:
            return s.encode("utf-8", "surrogateescape").decode("utf-8", "replace")
    return s


# ---------------------------------------------------------------- red


def _get(url: str) -> str:
    req = urllib.request.Request(
        url, headers={"User-Agent": USER_AGENT, "Accept-Language": "es-ES,es;q=0.9"}
    )
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        return r.read().decode("utf-8", "replace")


def _page(url: str) -> str:
    # `_get` se busca en cada pagina: las pruebas lo sustituyen
    return _get(url)


# ---------------------------------------------------------------- comparar


def same_song(lib_artist: str, lib_title: str, artist: str, title: str) -> bool:
    """Si el cifrado (`artist`, `title`) es esta cancion de la biblioteca.

    El titulo tiene que tener exactamente las mismas palabras (sin ruido): «Hay
    libertad» y «Hay libertad en la casa de Dios» no son la misma. El artista
    vale si comparte una palabra, si se escribe igual sin espacios («Sovereign
    Grace» y «SovereignGraceMusic») o si va metido en el titulo, que es lo que
    hacen los canales que suben canciones de otros."""
    return _rust.cifrados_same_song(_txt(lib_artist), _txt(lib_title), _txt(artist), _txt(title))


# ---------------------------------------------------------------- leer cifrados


def parse_ug(content: str) -> dict:
    """El texto de un cifrado de Ultimate Guitar: secciones entre corchetes
    ([Coro]) y cada acorde marcado como [ch]G[/ch]."""
    return _rust.cifrados_parse_ug(_txt(content))


def parse_lacuerda(page: str) -> dict:
    """La pagina de un cifrado de LaCuerda: el texto va en <div id=t_body><PRE>
    y cada acorde, entre <A></A>."""
    return _rust.cifrados_parse_lacuerda(_txt(page))


def search_title(artist: str, title: str) -> str:
    """El titulo para buscarlo: sin parentesis, sin ruido («en Español»,
    «Live») y sin el artista, que los canales meten delante."""
    return _rust.cifrados_search_title(_txt(artist), _txt(title))


# ---------------------------------------------------------------- fuentes


def ultimate_guitar(artist: str, title: str) -> dict | None:
    return _rust.cifrados_ultimate_guitar(_txt(artist), _txt(title), _page)


def lacuerda(artist: str, title: str) -> dict | None:
    return _rust.cifrados_lacuerda(_txt(artist), _txt(title), _page)


# ---------------------------------------------------------------- buscar


def sheet_key(sheet: dict | None) -> str:
    """El tono de la cancion segun su cifrado, si se puede afirmar: lo dice la
    fuente y se toca sin cejilla (con cejilla, lo escrito no es lo que suena)."""
    if not sheet:
        return ""
    return _rust.cifrados_sheet_key(bool(int(sheet.get("capo") or 0)), str(sheet.get("key") or ""))


SOURCES = (("Ultimate Guitar", ultimate_guitar), ("LaCuerda", lacuerda))


def find(artist: str, title: str) -> dict:
    """Busca el cifrado de una cancion en todas las fuentes a la vez.

    Devuelve {"sheet": el mejor o None, "tried": [fuentes consultadas],
    "failed": [las que no respondieron], "seconds": lo que tardo}. Gana el que
    dice su tono; entre iguales, Ultimate Guitar (tiene votos)."""
    sheet, tried, failed, seconds = _rust.cifrados_find(
        _txt(artist), _txt(title), [(name, fn) for name, fn in SOURCES]
    )
    for name, why in failed:
        log.info("acordes: %s no respondio (%s)", name, why)
    return {"sheet": sheet, "tried": tried, "failed": [n for n, _ in failed], "seconds": seconds}
