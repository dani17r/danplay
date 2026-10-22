"""Buscar en el indice: texto completo (FTS5), filtros, orden, y las filas
ligeras que dan las listas de la API.

Lo hace Rust (library/src/index/search.rs); esto es la puerta de
Python, con los mismos nombres. Las tablas de aqui son las mismas que alli.
"""

import danplay_core as _rust

from . import db

# ------------------------------------------------------------- busqueda

# El usuario escribe la consulta en español ("artista:barak tono:Bb"), pero las
# columnas estan en ingles. Se aceptan los dos idiomas.
FILTER_FIELDS = {
    "artist": "artist",
    "artista": "artist",
    "title": "title",
    "titulo": "title",
    "album": "album",
    "genre": "genre",
    "genero": "genre",
    "folder": "folder",
    "carpeta": "folder",
    "year": "year",
    "anio": "year",
    "key": "key",
    "tono": "key",
}


NUMERIC_FIELDS = {
    "bpm": "bpm",
    "bitrate": "bitrate",
    "duration": "duration",
    "duracion": "duration",
    "year": "year",
    "anio": "year",
}


# Por que se puede ordenar: nombre que usa la interfaz -> (columna, tipo).
# Se define aqui y no en la interfaz para que las dos no puedan separarse: la
# API rechaza cualquier otro nombre.
SORT_FIELDS = {
    "artist": ("c.artist", "text"),
    "title": ("c.title", "text"),
    "album": ("c.album", "text"),
    "genre": ("c.genre", "text"),
    "year": ("c.year", "text"),
    "key": ("c.key", "text"),
    "folder": ("c.folder", "text"),
    "file": ("c.file", "text"),
    "duration": ("c.duration", "num"),
    "bpm": ("c.bpm", "num"),
    "bitrate": ("c.bitrate", "num"),
    "size": ("c.size", "num"),
    "stars": ("c.stars", "num"),
    "recent": ("c.mtime", "num"),
}


def _order_by(sort: str, desc: bool) -> str:
    """Clausula ORDER BY para un campo y una direccion: los vacios siempre al
    final, y desempate por artista y titulo."""
    return _rust.search_order_by(sort, bool(desc))


def sort_options() -> list[str]:
    """Por que campos se puede ordenar. Lo usa la interfaz para no inventarse."""
    return _rust.search_sort_options()


def _fts_query(words) -> str:
    """Consulta FTS5 a partir de las palabras sueltas del usuario, con las
    comillas dobladas."""
    return _rust.search_fts_query([str(w) for w in words])


# Los textos pesados de una cancion: la letra (con y sin tiempos), la ficha de
# la IA con los acordes, el modo estudio y donde estan sus pistas separadas. Kilobytes por cancion que una
# LISTA no enseña: con la biblioteca entera, eran megas por cada busqueda.
# Las listas de la API llevan en su lugar si los tiene (`has_*`), y la ficha
# completa sigue en GET /api/song/{id}. Lo que usa el nucleo por dentro (el
# asistente, la hoja del atril) sigue leyendo la fila entera.
HEAVY_COLUMNS = ("lyrics", "lyrics_synced", "chords", "study", "stems")


LIGHT_FLAGS = {
    "lyrics": "has_lyrics",
    "lyrics_synced": "has_synced_lyrics",
    "chords": "has_chords",
    "study": "has_study",
    "stems": "has_stems",
}


def light_columns(conn, alias: str = "c") -> str:
    """El SELECT de una fila ligera: todo menos lo pesado, y en su lugar si
    lo tiene. Con `alias` vacio, sin prefijo de tabla."""
    return _rust.search_light_columns(conn, alias)


def light(song: dict) -> dict:
    """Una fila (entera o ya ligera) en su forma ligera, con `has_*` booleanos."""
    return _rust.search_light(dict(song))


def _search_where(query="", filters=None, only_favorites=False, min_stars=0) -> tuple[str, list]:
    """El WHERE (con sus parametros) de una busqueda: el mismo para la lista
    y para contarla."""
    return _rust.search_where(
        str(query or ""), dict(filters or {}), bool(only_favorites), int(min_stars or 0)
    )


def search(
    query="",
    filters=None,
    sort="artist",
    limit=200,
    offset=0,
    only_favorites=False,
    min_stars=0,
    desc=False,
    light=False,
) -> list[dict]:
    """Busqueda avanzada. `query` usa FTS5; `filters` son pares campo=valor.

    Soporta sintaxis inline:  artista:barak tono:Bb  bpm>100  duracion<300

    `only_favorites` y `min_stars` filtran EN SQL a proposito. Antes se
    aplicaban sobre la lista ya recortada por el LIMIT, asi que «Favoritos»
    solo enseñaba los favoritos que hubiera entre los primeros N resultados
    y el resto desaparecia sin que nada lo dijera.

    Con `light`, filas ligeras (ver HEAVY_COLUMNS): es lo que devuelve la API.
    """
    with db.connect() as conn:
        return _rust.search_songs(
            conn,
            str(query or ""),
            dict(filters or {}),
            str(sort),
            int(limit),
            int(offset),
            bool(only_favorites),
            int(min_stars or 0),
            bool(desc),
            bool(light),
        )


def count(query="", filters=None, only_favorites=False, min_stars=0) -> int:
    """Cuantas canciones cumplen la busqueda, sin limite ni desplazamiento: la
    interfaz sabe asi cuanto le falta por cargar."""
    with db.connect() as conn:
        return _rust.search_count(
            conn, str(query or ""), dict(filters or {}), bool(only_favorites), int(min_stars or 0)
        )


def find_by_match_key(key: str) -> list[dict]:
    """Canciones cuya clave de comparacion coincide.

    La clave ignora tildes, mayusculas, palabras vacias y el ruido de los
    nombres de descarga, asi que «BARAK - Mi Gozo (Video Oficial)» y
    «Barak - Mi Gozo.mp3» dan la misma. Sirve para no bajar dos veces lo mismo.
    """
    if not key:
        return []
    with db.connect() as conn:
        return _rust.search_find_by_match_key(conn, key)


# Lo justo que necesita el informe de duplicados. Pedir `SELECT *` traia
# tambien la letra entera de cada cancion —kilobytes por tema que el informe
# no usa para nada— y con una biblioteca grande eso son decenas de MB de pico
# solo para montar el indice.
BRIEF_COLUMNS = (
    "id",
    "path",
    "file",
    "artist",
    "title",
    "duration",
    "bitrate",
    "size",
    "stars",
    "favorite",
)


def by_path(path: str) -> dict | None:
    """La cancion que vive en esa ruta exacta, o None si no esta indexada.

    Para cuando el sistema abre un archivo con DanPlay: si resulta ser una de
    la biblioteca se reproduce como tal (con su caratula, sus estrellas y su
    id) en vez de como un archivo suelto. Pregunta por UNA ruta; el
    `brief_by_path` de abajo se trae la biblioteca entera y para esto seria
    tirar la casa por la ventana.
    """
    with db.connect() as conn:
        return _rust.search_by_path(conn, str(path))


def brief_by_path() -> dict:
    """{ruta: datos basicos} de toda la biblioteca, sin los campos pesados."""
    with db.connect() as conn:
        return _rust.search_brief_by_path(conn)


def top_artists(limit=12) -> list[dict]:
    """Los artistas con mas canciones: [{value, n}]. Para situar al asistente."""
    with db.connect() as conn:
        return _rust.search_top_artists(conn, int(limit))


def facets() -> dict:
    """Valores disponibles para los filtros de la interfaz."""
    with db.connect() as conn:
        return _rust.search_facets(conn)
