"""Buscar en el indice: texto completo (FTS5), filtros, orden, y las filas
ligeras que dan las listas de la API."""

import json
import logging

from . import db
from .db import _song_columns

log = logging.getLogger(__name__)


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
    """Clausula ORDER BY para un campo y una direccion.

    Dos cosas que no son obvias:

    - Los vacios van SIEMPRE al final, se ordene como se ordene. Una lista que
      empieza con veinte «sin album» no dice nada de como esta ordenada, y al
      invertir el orden esos veinte volverian arriba.
    - Al final se desempata siempre por artista y titulo, para que dos temas
      con el mismo bpm no se intercambien de sitio entre una consulta y otra.
    """
    column, kind = SORT_FIELDS.get(sort) or SORT_FIELDS["artist"]
    empty = f"{column}=''" if kind == "text" else f"{column} IS NULL OR {column}=0"
    direction = "DESC" if desc else "ASC"
    return f"({empty}), {column} {direction}, c.artist, c.title"


def sort_options() -> list[str]:
    """Por que campos se puede ordenar. Lo usa la interfaz para no inventarse."""
    return list(SORT_FIELDS)


def _fts_query(words) -> str:
    """Consulta FTS5 a partir de las palabras sueltas del usuario.

    Las comillas se escapan doblandolas: sin esto, buscar  rock"n roll  o un
    apostrofo tipografico rompia la sintaxis de MATCH y la busqueda entera
    reventaba con un error de sqlite en vez de devolver resultados.
    """
    return " AND ".join('"' + p.replace('"', '""') + '"*' for p in words)


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
    prefix = f"{alias}." if alias else ""
    present = _song_columns(conn)
    cols = [f"{prefix}{c}" for c in present if c not in HEAVY_COLUMNS]
    cols += [
        f"(COALESCE({prefix}{col}, '') != '') AS {flag}"
        for col, flag in LIGHT_FLAGS.items()
        if col in present
    ]
    if "stems" in present:
        # y si sus pistas ya son las mejores: las rapidas, o las de antes de
        # haber dos pasadas, se pueden separar otra vez mejor
        cols.append(
            # con CASE: con AND, sqlite puede mirar el JSON aunque no lo sea
            f"(CASE WHEN json_valid({prefix}stems)"
            f" THEN json_extract({prefix}stems, '$.quality') = 'mejor' ELSE 0 END) AS stems_best"
        )
    return ", ".join(cols)


def _best(stems) -> bool:
    try:
        data = json.loads(stems or "")
    except (TypeError, ValueError):
        return False
    return isinstance(data, dict) and data.get("quality") == "mejor"


def light(song: dict) -> dict:
    """Una fila (entera o ya ligera) en su forma ligera, con `has_*` booleanos."""
    out = {k: v for k, v in song.items() if k not in HEAVY_COLUMNS}
    for col, flag in LIGHT_FLAGS.items():
        out[flag] = bool(song[flag] if flag in song else song.get(col))
    out["stems_best"] = bool(
        song["stems_best"] if "stems_best" in song else _best(song.get("stems"))
    )
    return out


def _search_where(query="", filters=None, only_favorites=False, min_stars=0) -> tuple[str, list]:
    """El WHERE (con sus parametros) de una busqueda: el mismo para la lista
    y para contarla."""
    filters = dict(filters or {})
    words = []
    comparisons = []
    for tok in (query or "").split():
        if ":" in tok:
            c, v = tok.split(":", 1)
            if c.lower() in FILTER_FIELDS and v:
                filters[FILTER_FIELDS[c.lower()]] = v
                continue
        m = None
        for op in (">=", "<=", ">", "<"):
            if op in tok:
                c, v = tok.split(op, 1)
                if c.lower() in NUMERIC_FIELDS and v:
                    m = (NUMERIC_FIELDS[c.lower()], op, v)
                break
        if m:
            comparisons.append(m)
            continue
        # Un guion suelto («Barak - Mi Gozo»), un «&» o una barra no son
        # palabras: en FTS iban como termino y no casaban con nada, asi que
        # la busqueda mas natural del mundo devolvia cero.
        if not any(ch.isalnum() for ch in tok):
            continue
        words.append(tok)

    where, params = [], []
    if words:
        where.append("c.id IN (SELECT rowid FROM search_index WHERE search_index MATCH ?)")
        params.append(_fts_query(words))
    if only_favorites:
        where.append("c.favorite=1")
    if min_stars:
        where.append("c.stars >= ?")
        params.append(int(min_stars))
    # los nombres de columna salen de listas blancas (FILTER_FIELDS y
    # NUMERIC_FIELDS), nunca de lo que escribe el usuario
    for field, value in filters.items():
        if field in FILTER_FIELDS.values() and value:
            where.append(f"c.{field} LIKE ?")
            params.append(f"%{value}%")
    for field, op, value in comparisons:
        try:
            params.append(float(value))
            where.append(f"c.{field} {op} ?")
        except ValueError:
            pass
    return (" WHERE " + " AND ".join(where) if where else ""), params


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
    where, params = _search_where(query, filters, only_favorites, min_stars)
    with db.connect() as conn:
        cols = light_columns(conn) if light else "c.*"
        sql = (
            f"SELECT {cols} FROM songs c{where}"  # noqa: S608
            f" ORDER BY {_order_by(sort, desc)} LIMIT ? OFFSET ?"
        )
        rows = conn.execute(sql, [*params, limit, offset]).fetchall()
    if light:
        return [_flags_as_bool(dict(f)) for f in rows]
    return [dict(f) for f in rows]


def _flags_as_bool(row: dict) -> dict:
    for flag in (*LIGHT_FLAGS.values(), "stems_best"):
        if flag in row:
            row[flag] = bool(row[flag])
    return row


def count(query="", filters=None, only_favorites=False, min_stars=0) -> int:
    """Cuantas canciones cumplen la busqueda, sin limite ni desplazamiento: la
    interfaz sabe asi cuanto le falta por cargar."""
    where, params = _search_where(query, filters, only_favorites, min_stars)
    with db.connect() as conn:
        return (
            conn.execute(
                f"SELECT COUNT(*) FROM songs c{where}",  # noqa: S608
                params,
            ).fetchone()[0]
            or 0
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
        rows = conn.execute(
            "SELECT id, artist, title, path, file FROM songs WHERE match_key=?", (key,)
        ).fetchall()
    return [dict(r) for r in rows]


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
        row = conn.execute(
            "SELECT id,title,artist,duration,blur FROM songs WHERE path=?", (str(path),)
        ).fetchone()
    return dict(row) if row else None


def brief_by_path() -> dict:
    """{ruta: datos basicos} de toda la biblioteca, sin los campos pesados."""
    with db.connect() as conn:
        rows = conn.execute(f"SELECT {','.join(BRIEF_COLUMNS)} FROM songs").fetchall()  # noqa: S608
    return {r["path"]: dict(r) for r in rows}


def top_artists(limit=12) -> list[dict]:
    """Los artistas con mas canciones: [{value, n}]. Para situar al asistente."""
    with db.connect() as conn:
        rows = [
            dict(r)
            for r in conn.execute(
                "SELECT artist value, COUNT(*) n FROM songs WHERE artist!='' "
                "GROUP BY artist ORDER BY n DESC, value LIMIT ?",
                (int(limit),),
            ).fetchall()
        ]
    return rows


def facets() -> dict:
    """Valores disponibles para los filtros de la interfaz."""
    with db.connect() as conn:

        def top(col, lim=500):
            return [
                dict(r)
                for r in conn.execute(
                    f"SELECT {col} value, COUNT(*) n FROM songs WHERE {col}!='' "  # noqa: S608
                    f"GROUP BY {col} ORDER BY n DESC, value LIMIT ?",
                    (lim,),
                ).fetchall()
            ]

        d = {
            "artists": top("artist"),
            "albums": top("album"),
            "genres": top("genre"),
            "folders": top("folder"),
            "keys": top("key"),
        }
    return d
