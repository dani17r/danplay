"""Busqueda en la web, sin claves de API.

Se usa el HTML de DuckDuckGo, que no pide registro ni cuota. Solo saca
titulo, enlace y resumen: lo justo para que el asistente pueda comprobar un
dato (de que año es un disco, quien toca en el, de donde sale un genero)
en vez de inventarselo.

Si falla, falla en silencio y devuelve una lista vacia: el asistente lo
interpreta como "no lo he podido comprobar", no como un error de la app.

Leer la pagina lo hace el crate de Rust (biblioteca/src/web.rs); pedirla, de
momento, urllib (asi las pruebas la cortan o la sustituyen).
"""

import urllib.request

import danplay_core as _rust

USER_AGENT = _rust.WEB_USER_AGENT
ENDPOINTS = tuple(_rust.WEB_ENDPOINTS)


def _fetch(url: str, timeout) -> str:
    req = urllib.request.Request(
        url, headers={"User-Agent": USER_AGENT, "Accept-Language": "es-ES,es;q=0.9"}
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read().decode("utf-8", "replace")


def search(query: str, limit=5, timeout=12) -> list[dict]:
    """Devuelve [{titulo, url, resumen}]. Lista vacia si no se pudo consultar."""
    query = query or ""
    try:
        query.encode("utf-8")
    except UnicodeEncodeError:
        return []  # no se puede poner en una direccion: ninguna version responde
    return _rust.web_search(query, limit, lambda url: _fetch(url, timeout))
