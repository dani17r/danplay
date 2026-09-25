# -*- coding: utf-8 -*-
"""Busqueda en la web, sin claves de API.

Se usa el HTML de DuckDuckGo, que no pide registro ni cuota. Solo saca
titulo, enlace y resumen: lo justo para que el asistente pueda comprobar un
dato (de que año es un disco, quien toca en el, de donde sale un genero)
en vez de inventarselo.

Si falla, falla en silencio y devuelve una lista vacia: el asistente lo
interpreta como "no lo he podido comprobar", no como un error de la app.
"""
import html
import re
import urllib.parse
import urllib.request

USER_AGENT = ("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
          "(KHTML, like Gecko) Chrome/124.0 Safari/537.36")
ENDPOINTS = ("https://lite.duckduckgo.com/lite/?q=",
          "https://html.duckduckgo.com/html/?q=")

# los resultados de la version "lite" son una tabla; los de "html", divs
LINK_RE = re.compile(r'<a[^>]+class="[^"]*result-link[^"]*"[^>]+href="([^"]+)"[^>]*>(.*?)</a>',
                    re.DOTALL | re.IGNORECASE)
LINK_RE_ALT = re.compile(r'<a[^>]+class="[^"]*result__a[^"]*"[^>]+href="([^"]+)"[^>]*>(.*?)</a>',
                     re.DOTALL | re.IGNORECASE)
SNIPPET_RE = re.compile(r'class="[^"]*result-snippet[^"]*"[^>]*>(.*?)</td>',
                     re.DOTALL | re.IGNORECASE)
SNIPPET_RE_ALT = re.compile(r'class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>',
                      re.DOTALL | re.IGNORECASE)
TAG_MAP = re.compile(r"<[^>]+>")


def _plain_text(raw: str) -> str:
    return re.sub(r"\s+", " ", html.unescape(TAG_MAP.sub(" ", raw))).strip()


def _unwrap_url(u: str) -> str:
    """DuckDuckGo envuelve los enlaces en /l/?uddg=<url codificada>."""
    if "uddg=" in u:
        try:
            return urllib.parse.unquote(u.split("uddg=", 1)[1].split("&", 1)[0])
        except Exception:                                   # noqa: BLE001
            return u
    return u if u.startswith("http") else "https:" + u if u.startswith("//") else u


def search(query: str, limit=5, timeout=12) -> list[dict]:
    """Devuelve [{titulo, url, resumen}]. Lista vacia si no se pudo consultar."""
    query = (query or "").strip()
    if not query:
        return []
    for base in ENDPOINTS:
        try:
            req = urllib.request.Request(base + urllib.parse.quote_plus(query),
                                         headers={"User-Agent": USER_AGENT,
                                                  "Accept-Language": "es-ES,es;q=0.9"})
            with urllib.request.urlopen(req, timeout=timeout) as r:
                page = r.read().decode("utf-8", "replace")
        except Exception:                                   # noqa: BLE001
            continue

        links = LINK_RE.findall(page) or LINK_RE_ALT.findall(page)
        snippets = SNIPPET_RE.findall(page) or SNIPPET_RE_ALT.findall(page)
        out = []
        for i, (url, title) in enumerate(links[:limit]):
            out.append({
                "title": _plain_text(title),
                "url": _unwrap_url(url),
                "summary": _plain_text(snippets[i])[:400] if i < len(snippets) else "",
            })
        if out:
            return out
    return []
