# -*- coding: utf-8 -*-
"""Indice SQLite con busqueda de texto completo (FTS5) y carpetas gestionadas."""
import os, sqlite3, time
from pathlib import Path
from . import config, tags, names

SCHEMA = """
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS folders (
    path      TEXT PRIMARY KEY,
    label  TEXT,
    role       TEXT DEFAULT 'biblioteca',   -- biblioteca | entrada | pistas | otros
    active    INTEGER DEFAULT 1,
    added  REAL
);

CREATE TABLE IF NOT EXISTS exclusions (
    pattern  TEXT PRIMARY KEY,
    kind    TEXT DEFAULT 'glob',     -- glob | path
    note    TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS songs (
    id        INTEGER PRIMARY KEY,
    path      TEXT UNIQUE,
    root      TEXT,
    folder   TEXT,
    file   TEXT,
    artist   TEXT DEFAULT '',
    title    TEXT DEFAULT '',
    album     TEXT DEFAULT '',
    year      TEXT DEFAULT '',
    genre    TEXT DEFAULT '',
    feat      TEXT DEFAULT '',
    match_key     TEXT DEFAULT '',
    duration  REAL DEFAULT 0,
    bitrate   INTEGER DEFAULT 0,
    size   INTEGER DEFAULT 0,
    mtime     REAL DEFAULT 0,
    key      TEXT DEFAULT '',
    bpm       REAL DEFAULT 0,
    cover   TEXT DEFAULT '',
    lyrics     TEXT DEFAULT '',
    chords   TEXT DEFAULT '',
    analyzed REAL DEFAULT 0,
    stars INTEGER DEFAULT 0,
    favorite  INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS i_artist ON songs(artist);
CREATE INDEX IF NOT EXISTS i_match_key   ON songs(match_key);
CREATE INDEX IF NOT EXISTS i_root    ON songs(root);

-- Registro de descargas: manuales y las que hace el asistente. No es
-- metadata de una cancion, asi que vive aqui y no en el mp3. El escaneo no
-- la toca: solo vacia `songs`.
CREATE TABLE IF NOT EXISTS downloads (
    id        INTEGER PRIMARY KEY,
    at        REAL,
    source    TEXT DEFAULT 'manual',   -- manual | assistant
    query     TEXT DEFAULT '',
    title     TEXT DEFAULT '',
    channel   TEXT DEFAULT '',
    url       TEXT DEFAULT '',
    ok        INTEGER DEFAULT 0,
    already   INTEGER DEFAULT 0,
    reason    TEXT DEFAULT '',
    song_id   INTEGER,
    artist    TEXT DEFAULT '',
    song      TEXT DEFAULT '',
    target    TEXT DEFAULT '',
    quality   TEXT DEFAULT '',
    kbps      TEXT DEFAULT ''
);
CREATE INDEX IF NOT EXISTS i_downloads_at ON downloads(at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
    artist, title, album, file, folder, genre,
    content='songs', content_rowid='id', tokenize="unicode61 remove_diacritics 2"
);

-- El indice de busqueda se mantiene solo, fila a fila. Antes cada cambio
-- (poner una estrella, corregir un titulo, importar un archivo) lanzaba un
-- 'rebuild' del indice ENTERO: con mil canciones eso es releerlas todas para
-- guardar una. Es el patron que documenta SQLite para las tablas fts5 con
-- content externo, y cubre cualquier camino que escriba en `songs`.
-- El de UPDATE solo salta si cambia una columna indexada: las estrellas y el
-- favorito no se buscan, asi que no tienen por que tocar el indice.
CREATE TRIGGER IF NOT EXISTS songs_ai AFTER INSERT ON songs BEGIN
    INSERT INTO search_index(rowid, artist, title, album, file, folder, genre)
    VALUES (new.id, new.artist, new.title, new.album, new.file, new.folder, new.genre);
END;
CREATE TRIGGER IF NOT EXISTS songs_ad AFTER DELETE ON songs BEGIN
    INSERT INTO search_index(search_index, rowid, artist, title, album, file, folder, genre)
    VALUES ('delete', old.id, old.artist, old.title, old.album, old.file, old.folder, old.genre);
END;
CREATE TRIGGER IF NOT EXISTS songs_au
AFTER UPDATE OF artist, title, album, file, folder, genre ON songs BEGIN
    INSERT INTO search_index(search_index, rowid, artist, title, album, file, folder, genre)
    VALUES ('delete', old.id, old.artist, old.title, old.album, old.file, old.folder, old.genre);
    INSERT INTO search_index(rowid, artist, title, album, file, folder, genre)
    VALUES (new.id, new.artist, new.title, new.album, new.file, new.folder, new.genre);
END;
"""

# Todo esto sale del PROPIO ARCHIVO. La base es solo un indice: si se pierde,
# un escaneo la reconstruye entera, porque estrellas, favorito, letra, tono,
# bpm y caratula viajan dentro del mp3.
COLUMNS = ("path","root","folder","file","artist","title","album","year",
           "genre","feat","match_key","duration","bitrate","size","mtime",
           "key","bpm","cover","lyrics","stars","favorite")


# Tablas y columnas del esquema anterior (estaban en castellano). Se migran
# solas la primera vez que se abre la base: nadie tiene que reescanear.
_ANTIGUO = {
    "carpetas": ("folders",
                 [("ruta", "path"), ("etiqueta", "label"), ("rol", "role"),
                  ("activa", "active"), ("agregada", "added")]),
    "exclusiones": ("exclusions",
                    [("patron", "pattern"), ("tipo", "kind"), ("nota", "note")]),
    "canciones": ("songs",
                  [("id", "id"), ("ruta", "path"), ("raiz", "root"),
                   ("carpeta", "folder"), ("archivo", "file"),
                   ("artista", "artist"), ("titulo", "title"), ("album", "album"),
                   ("anio", "year"), ("genero", "genre"), ("feat", "feat"),
                   ("clave", "match_key"), ("duracion", "duration"),
                   ("bitrate", "bitrate"), ("tamanio", "size"), ("mtime", "mtime"),
                   ("tono", "key"), ("bpm", "bpm"), ("portada", "cover"),
                   ("letra", "lyrics"), ("acordes", "chords"),
                   ("analizado", "analyzed"), ("estrellas", "stars"),
                   ("favorito", "favorite")]),
    "listas": ("playlists",
               [("id", "id"), ("nombre", "name"), ("nota", "note"),
                ("color", "color"), ("creada", "created")]),
    "lista_canciones": ("playlist_songs",
                        [("lista_id", "playlist_id"), ("cancion_id", "song_id"),
                         ("orden", "position")]),
}


def _migrate_from_spanish(conn) -> bool:
    """Copia los datos del esquema viejo al nuevo, tabla a tabla.

    Va por pares y solo cuando el destino ya existe y esta vacio, porque las
    listas las crea otro modulo: asi da igual quien abra la base primero.
    """
    tablas = {r[0] for r in conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table'")}
    migro = False
    for old, (new, cols) in _ANTIGUO.items():
        if old not in tablas or new not in tablas:
            continue
        if conn.execute(f"SELECT COUNT(*) FROM {new}").fetchone()[0]:
            continue                      # ya hay datos nuevos: no se toca
        src = ",".join(c[0] for c in cols)
        dest = ",".join(c[1] for c in cols)
        conn.execute(f"INSERT OR IGNORE INTO {new} ({dest}) "
                     f"SELECT {src} FROM {old}")
        conn.execute(f"DROP TABLE {old}")
        migro = True
    if not migro:
        return False
    # los unicos valores fijos que tambien estaban en castellano
    if "folders" in tablas:
        conn.execute("UPDATE folders SET role='library' WHERE role='biblioteca'")
    if "exclusions" in tablas:
        conn.execute("UPDATE exclusions SET kind='path' WHERE kind='ruta'")
    if "songs" in tablas:
        conn.execute("DROP TABLE IF EXISTS busqueda")
        conn.execute("INSERT INTO search_index(search_index) VALUES('rebuild')")
    conn.commit()
    return True


# El esquema y la migracion son de una vez por base, no de cada consulta.
# Antes se reejecutaban en CADA connect(): crear las tablas, revisar
# sqlite_master y contar filas de cinco tablas, cientos de veces por pantalla.
# Ahora se hace la primera vez y despues connect() solo abre.
_prepared: set = set()
_prepare_lock = __import__("threading").Lock()

# Otros modulos (playlists) añaden sus tablas al mismo archivo. Se registran
# aqui para que su esquema tambien se cree una sola vez.
_EXTRA_SCHEMAS: list = []


def register_schema(sql: str) -> None:
    """Declara tablas de otro modulo. Se crean junto a las de aqui."""
    if sql not in _EXTRA_SCHEMAS:
        _EXTRA_SCHEMAS.append(sql)
        _prepared.discard(str(config.DATABASE))


def _prepare(conn) -> None:
    conn.executescript(SCHEMA)
    for extra in _EXTRA_SCHEMAS:
        conn.executescript(extra)
    _migrate_from_spanish(conn)


def connect():
    config.DATABASE.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(config.DATABASE)
    conn.row_factory = sqlite3.Row
    key = str(config.DATABASE)
    if key not in _prepared:
        with _prepare_lock:
            if key not in _prepared:
                _prepare(conn)
                _prepared.add(key)
    return conn


# ------------------------------------------------------------- carpetas

def check_overlap(path) -> dict | None:
    """Avisa si la carpeta nueva repite musica que ya esta indexada.

    Dos casos: que una contenga a la otra, o que sean copias distintas del
    mismo contenido (por ejemplo el original y su respaldo).
    """
    path = str(Path(path).expanduser().resolve())
    for c in list_folders():
        other = c["path"]
        if other == path:
            return {"kind": "same", "other": other, "message": "esa carpeta ya esta añadida"}
        if path.startswith(other.rstrip("/") + os.sep):
            return {"kind": "inside", "other": other,
                    "message": f"esta dentro de «{other}», que ya esta indexada"}
        if other.startswith(path.rstrip("/") + os.sep):
            return {"kind": "contains", "other": other,
                    "message": f"contiene a «{other}», que ya esta indexada"}

    # copias distintas del mismo contenido: se compara una muestra
    sample = {}
    for i, r in enumerate(audio_files(path)):
        if i >= 60:
            break
        try:
            sample[os.path.basename(r)] = os.path.getsize(r)
        except OSError:
            pass
    if len(sample) < 5:
        return None
    conn = connect()
    marcadores = ",".join("?" * len(sample))
    rows = conn.execute(
        f"SELECT file, size, root FROM songs WHERE file IN ({marcadores})",
        list(sample)).fetchall()
    conn.close()
    by_root = {}
    for f in rows:
        if sample.get(f["file"]) == f["size"]:
            by_root[f["root"]] = by_root.get(f["root"], 0) + 1
    if not by_root:
        return None
    other, matches = max(by_root.items(), key=lambda x: x[1])
    percent = round(100 * matches / len(sample))
    if percent >= 60:
        return {"kind": "copy", "other": other, "percent": percent,
                "message": (f"el {percent}% de sus archivos ya estan indexados desde "
                            f"«{other}»: parecen la misma musica duplicada")}
    return None


def add_folder(path, label="", role="library") -> bool:
    path = str(Path(path).expanduser().resolve())
    if not os.path.isdir(path):
        return False
    conn = connect()
    conn.execute("INSERT OR REPLACE INTO folders VALUES (?,?,?,1,?)",
                (path, label or os.path.basename(path), role, time.time()))
    conn.commit(); conn.close()
    return True


def remove_folder(path) -> None:
    path = str(Path(path).expanduser().resolve())
    conn = connect()
    conn.execute("DELETE FROM folders WHERE path=?", (path,))
    conn.execute("DELETE FROM songs WHERE root=?", (path,))
    conn.commit(); conn.close()


def list_folders() -> list[dict]:
    conn = connect()
    rows = conn.execute("SELECT c.*, (SELECT COUNT(*) FROM songs s WHERE s.root=c.path) n "
                        "FROM folders c ORDER BY c.label").fetchall()
    conn.close()
    return [dict(f) for f in rows]


def _roots() -> list[str]:
    conn = connect()
    rows = conn.execute("SELECT path FROM folders WHERE active=1").fetchall()
    conn.close()
    # sin carpetas configuradas no se indexa nada: la app pide elegirlas primero
    return [f["path"] for f in rows]


# ------------------------------------------------------------- escaneo

# ------------------------------------------------------------- exclusiones

DEFAULT_EXCLUDES = [".*", "@eaDir", "#recycle", "$RECYCLE.BIN",
                         "System Volume Information", "__MACOSX"]


def add_exclusion(pattern, kind="glob", note="") -> None:
    conn = connect()
    conn.execute("INSERT OR REPLACE INTO exclusions VALUES (?,?,?)",
                (pattern.rstrip("/"), kind, note))
    conn.commit(); conn.close()


def remove_exclusion(pattern) -> None:
    conn = connect()
    conn.execute("DELETE FROM exclusions WHERE pattern=?", (pattern.rstrip("/"),))
    conn.commit(); conn.close()


def list_exclusions() -> list[dict]:
    conn = connect()
    rows = conn.execute("SELECT * FROM exclusions ORDER BY pattern").fetchall()
    conn.close()
    return [dict(f) for f in rows]


def _excluded(dir_path, dir_name, exclusions) -> bool:
    """True si esta carpeta debe saltarse.

    Los patrones NO distinguen mayusculas: escribir «secuencias» tiene que
    funcionar aunque la carpeta se llame «Secuencias».
    """
    from fnmatch import fnmatch
    full_path = os.path.join(dir_path, dir_name)
    nombre_b, completa_b = dir_name.lower(), full_path.lower()
    for pat in DEFAULT_EXCLUDES:
        if fnmatch(dir_name, pat) or fnmatch(nombre_b, pat.lower()):
            return True
    for e in exclusions:
        p = (e["pattern"] or "").lower()
        if not p:
            continue
        if e["kind"] == "path":
            if completa_b == p or completa_b.startswith(p.rstrip("/") + os.sep):
                return True
        elif (fnmatch(nombre_b, p) or fnmatch(completa_b, p)
              or fnmatch(completa_b, "*/" + p.strip("*/") + "/*")):
            return True
    return False


def audio_files(root, exclusions=None):
    exclusions = list_exclusions() if exclusions is None else exclusions
    for dp, dn, fns in os.walk(root):
        dn[:] = [d for d in dn if not _excluded(dp, d, exclusions)]
        for fn in fns:
            if Path(fn).suffix.lower() in config.EXTENSIONS:
                yield os.path.join(dp, fn)


def _artist_from_folder(path, root) -> str:
    """Si el archivo cuelga de Artistas/<Nombre>/ ... la carpeta es el artista."""
    rel = Path(os.path.relpath(path, root)).parts
    # la carpeta en disco se llama "Artistas" (lo ve el usuario); "artists" se
    # acepta por si alguien la tiene en ingles. Aqui decia dos veces "artists"
    # y la rama nunca se cumplia: los archivos de Artistas/<X>/ perdian el
    # artista que da la propia carpeta.
    if len(rel) >= 2 and rel[0].lower() in ("artistas", "artists"):
        return rel[1]
    return ""


def _row(path, root, vocab=None) -> tuple:
    """Fila del indice a partir del archivo. Una sola lectura de etiquetas.

    Antes se abria tres veces (etiquetas, duracion y bitrate por separado) y
    aun asi no se leian ni la caratula ni la letra: si perdias la base, esas
    se quedaban vacias aunque estuvieran dentro del mp3.
    """
    st = os.stat(path)
    tag = tags.read_all(path)
    file = os.path.basename(path)
    stem = Path(file).stem

    artist = tag.get("artist", "") or _artist_from_folder(path, root)
    title  = tag.get("title", "")

    # los nombres ya siguen "Artista - Titulo": si el prefijo coincide, quitalo
    if artist and not title:
        prefix = artist.lower() + " - "
        title = stem[len(prefix):] if stem.lower().startswith(prefix) else stem
    if not artist:
        d = names.detect_artist(file, vocab or {})
        if d["confidence"] >= 0.80:
            artist, title = d["artist"], title or d["title"]
        else:
            title = title or d["title"]
    title = title or stem

    title, feat = names.extract_feat(title)
    return (path, root, os.path.relpath(os.path.dirname(path), root), file,
            artist, title.strip(" -"), tag.get("album", ""), tag.get("year", ""),
            tag.get("genre", ""), feat, names.match_key(file),
            tag.get("duration", 0.0), tag.get("bitrate", 0), st.st_size, st.st_mtime,
            tag.get("key", ""), tag.get("bpm", 0.0),
            "embedded" if tag.get("cover") else "",
            tag.get("lyrics", ""), int(tag.get("stars", 0) or 0),
            1 if tag.get("favorite") else 0)


def scan(progress=None) -> dict:
    """Reindexa todas las carpetas gestionadas.

    Casi todo se relee del archivo, asi que el escaneo reconstruye el indice
    aunque la base se pierda. De la base solo se rescatan los acordes y la
    marca de analizado: son lo unico que no cabe en las etiquetas.
    """
    conn = connect()
    cache = {f["path"]: f for f in conn.execute(
        "SELECT path,chords,analyzed FROM songs")}
    conn.execute("DELETE FROM songs")
    n, added_count = 0, 0
    vocab = names.vocabulary(config.ARTISTS_DIR)
    exclusions = list_exclusions()
    for root in _roots():
        if not os.path.isdir(root):
            continue
        for path in audio_files(root, exclusions):
            try:
                row = _row(path, root, vocab)
            except OSError:
                continue
            conn.execute(f"INSERT OR REPLACE INTO songs ({','.join(COLUMNS)}) "
                        f"VALUES ({','.join('?'*len(COLUMNS))})", row)
            v = cache.get(path)
            if v:
                if v["chords"] or v["analyzed"]:
                    conn.execute("UPDATE songs SET chords=?,analyzed=? WHERE path=?",
                                 (v["chords"], v["analyzed"], path))
            else:
                added_count += 1
            n += 1
            if progress and n % 50 == 0:
                progress(n)
    conn.execute("INSERT INTO search_index(search_index) VALUES('rebuild')")
    conn.commit(); conn.close()
    return {"total": n, "added_count": added_count}


# ------------------------------------------------------------- busqueda

# El usuario escribe la consulta en español ("artista:barak tono:Bb"), pero las
# columnas estan en ingles. Se aceptan los dos idiomas.
FILTER_FIELDS = {
    "artist": "artist",   "artista": "artist",
    "album": "album",
    "genre": "genre",     "genero": "genre",
    "folder": "folder",   "carpeta": "folder",
    "year": "year",       "anio": "year",
    "key": "key",         "tono": "key",
}
NUMERIC_FIELDS = {
    "bpm": "bpm", "bitrate": "bitrate",
    "duration": "duration", "duracion": "duration",
    "year": "year", "anio": "year",
}


def _fts_query(words) -> str:
    """Consulta FTS5 a partir de las palabras sueltas del usuario.

    Las comillas se escapan doblandolas: sin esto, buscar  rock"n roll  o un
    apostrofo tipografico rompia la sintaxis de MATCH y la busqueda entera
    reventaba con un error de sqlite en vez de devolver resultados.
    """
    return " AND ".join('"' + p.replace('"', '""') + '"*' for p in words)


def search(query="", filters=None, sort="artist", limit=200, offset=0,
           only_favorites=False, min_stars=0) -> list[dict]:
    """Busqueda avanzada. `consulta` usa FTS5; `filtros` son pares campo=valor.

    Soporta sintaxis inline:  artista:barak tono:Bb  bpm>100  duracion<300

    `only_favorites` y `min_stars` filtran EN SQL a proposito. Antes se
    aplicaban sobre la lista ya recortada por el LIMIT, asi que «Favoritos»
    solo enseñaba los favoritos que hubiera entre los primeros N resultados
    y el resto desaparecia sin que nada lo dijera.
    """
    conn = connect()
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
            comparisons.append(m); continue
        words.append(tok)

    where, params = [], []
    if words:
        where.append("c.id IN (SELECT rowid FROM search_index WHERE search_index MATCH ?)")
        params.append(_fts_query(words))
    if only_favorites:
        where.append("c.favorite=1")
    if min_stars:
        where.append("c.stars >= ?"); params.append(int(min_stars))
    for field, value in filters.items():
        if field in FILTER_FIELDS.values() and value:
            where.append(f"c.{field} LIKE ?"); params.append(f"%{value}%")
    for field, op, value in comparisons:
        try:
            where.append(f"c.{field} {op} ?"); params.append(float(value))
        except ValueError:
            pass

    SORTS = {"artist": "c.artist, c.title", "title": "c.title",
             "duration": "c.duration DESC", "bpm": "c.bpm DESC",
             "recent": "c.mtime DESC", "album": "c.album, c.title"}
    sql = ("SELECT c.* FROM songs c"
           + (" WHERE " + " AND ".join(where) if where else "")
           + f" ORDER BY {SORTS.get(sort, SORTS['artist'])} LIMIT ? OFFSET ?")
    rows = conn.execute(sql, params + [limit, offset]).fetchall()
    conn.close()
    return [dict(f) for f in rows]


def by_id(cid: int) -> dict | None:
    conn = connect()
    f = conn.execute("SELECT * FROM songs WHERE id=?", (cid,)).fetchone()
    conn.close()
    return dict(f) if f else None


def update(cid: int, **fields) -> None:
    allowed = {"artist","title","album","year","genre","feat","key","bpm",
                  "cover","lyrics","chords","analyzed","stars","favorite"}
    fields = {k: v for k, v in fields.items() if k in allowed}
    if not fields:
        return
    conn = connect()
    conn.execute(f"UPDATE songs SET {','.join(f'{k}=?' for k in fields)} WHERE id=?",
                list(fields.values()) + [cid])
    conn.commit(); conn.close()


def edit(cid: int, **fields) -> dict | None:
    """Cambia los datos de una cancion en el indice y en las etiquetas.

    Lo usan la API y el asistente: un solo camino para que no se separen.
    """
    c = by_id(cid)
    if not c:
        return None
    update(cid, **fields)
    if config.WRITE_TAGS and os.path.exists(c["path"]):
        tags.write(c["path"],
                   artist=fields.get("artist", c["artist"]),
                   title=fields.get("title", c["title"]),
                   album=fields.get("album", c["album"]),
                   year=str(fields.get("year", c["year"])),
                   genre=fields.get("genre", c["genre"]))
        if "key" in fields or "bpm" in fields:
            tags.write_analysis(c["path"], fields.get("key", c["key"]),
                                fields.get("bpm", c["bpm"]))
        # la letra vive en el USLT del propio mp3, no solo en el indice
        if "lyrics" in fields:
            tags.write_lyrics(c["path"], fields["lyrics"] or "")
    return by_id(cid)


def index_file(path: str) -> dict | None:
    """Mete en el indice un archivo recien llegado, sin reescanear todo.

    Sin esto, lo que se descarga o se importa se mueve a Artistas/ pero no
    aparece en la app hasta el siguiente escaneo completo: el archivo esta en
    el disco y el usuario no lo ve por ningun lado.
    """
    path = os.path.abspath(path)
    if not os.path.isfile(path):
        return None
    root = next((r for r in _roots()
                 if path.startswith(r.rstrip("/") + os.sep)), None)
    if root is None:
        return None                      # fuera de las carpetas gestionadas
    try:
        row = _row(path, root, names.vocabulary(config.ARTISTS_DIR))
    except OSError:
        return None
    conn = connect()
    conn.execute(f"INSERT OR REPLACE INTO songs ({','.join(COLUMNS)}) "
                 f"VALUES ({','.join('?' * len(COLUMNS))})", row)
    conn.commit()
    r = conn.execute("SELECT * FROM songs WHERE path=?", (path,)).fetchone()
    conn.close()
    return dict(r) if r else None


def trash(cid: int) -> dict:
    """Manda el archivo a la papelera del sistema y lo saca del indice.

    A la papelera y no `unlink`: borrar musica del usuario sin vuelta atras,
    y menos desde un menu o desde el chat, es demasiado definitivo.
    """
    import shutil as _sh, subprocess as _sp
    c = by_id(cid)
    if not c:
        return {"ok": False, "error": "no existe esa cancion"}
    path = c["path"]
    if os.path.exists(path):
        if not _sh.which("gio"):
            return {"ok": False,
                    "error": "este escritorio no tiene papelera; borralo a mano"}
        r = _sp.run(["gio", "trash", path], capture_output=True, text=True)
        if r.returncode != 0:
            # pasa con archivos fuera del home (otro sistema de archivos sin
            # papelera). Se avisa en vez de borrar sin vuelta atras.
            return {"ok": False,
                    "error": "no se pudo mandar a la papelera: "
                             + (r.stderr or "").strip()[:120]}
    forget(cid)
    return {"ok": True, "trashed": True, "path": path,
            "name": os.path.basename(path),
            "artist": c["artist"], "title": c["title"]}


def forget(cid: int) -> None:
    """Quita la cancion del indice. No toca el archivo."""
    conn = connect()
    conn.execute("DELETE FROM songs WHERE id=?", (cid,))
    conn.commit(); conn.close()


def forget_path(path: str) -> int:
    """Saca del indice el archivo que estaba en esa ruta. No toca el disco.

    Para cuando el archivo ya no esta ahi (se borro o se renombro) y solo hay
    que ponerse al dia. Devuelve cuantas filas se quitaron.
    """
    conn = connect()
    cur = conn.execute("DELETE FROM songs WHERE path=?", (os.path.abspath(path),))
    conn.commit(); conn.close()
    return cur.rowcount or 0


# ---------------------------------------------------------- historial
DOWNLOAD_FIELDS = ("at", "source", "query", "title", "channel", "url", "ok",
                   "already", "reason", "song_id", "artist", "song", "target",
                   "quality", "kbps")


def log_download(entry: dict) -> None:
    """Apunta una descarga. Da igual si vino del boton o del asistente."""
    row = {k: entry.get(k) for k in DOWNLOAD_FIELDS}
    row["at"] = row["at"] or time.time()
    row["ok"] = 1 if row["ok"] else 0
    row["already"] = 1 if row["already"] else 0
    for k in ("source", "query", "title", "channel", "url", "reason",
              "artist", "song", "target", "quality", "kbps"):
        row[k] = str(row[k] or "")
    conn = connect()
    conn.execute(f"INSERT INTO downloads ({','.join(DOWNLOAD_FIELDS)}) "
                 f"VALUES ({','.join('?' * len(DOWNLOAD_FIELDS))})",
                 [row[k] for k in DOWNLOAD_FIELDS])
    conn.commit(); conn.close()


def download_history(limit=60, offset=0) -> list[dict]:
    conn = connect()
    rows = conn.execute("SELECT * FROM downloads ORDER BY at DESC LIMIT ? OFFSET ?",
                        (int(limit), int(offset))).fetchall()
    conn.close()
    return [dict(r) for r in rows]


def download_count() -> int:
    conn = connect()
    n = conn.execute("SELECT COUNT(*) n FROM downloads").fetchone()["n"]
    conn.close()
    return n or 0


def clear_download_history() -> int:
    conn = connect()
    n = conn.execute("SELECT COUNT(*) n FROM downloads").fetchone()["n"] or 0
    conn.execute("DELETE FROM downloads")
    conn.commit(); conn.close()
    return n


def find_by_match_key(key: str) -> list[dict]:
    """Canciones cuya clave de comparacion coincide.

    La clave ignora tildes, mayusculas, palabras vacias y el ruido de los
    nombres de descarga, asi que «BARAK - Mi Gozo (Video Oficial)» y
    «Barak - Mi Gozo.mp3» dan la misma. Sirve para no bajar dos veces lo mismo.
    """
    if not key:
        return []
    conn = connect()
    rows = conn.execute(
        "SELECT id, artist, title, path, file FROM songs WHERE match_key=?",
        (key,)).fetchall()
    conn.close()
    return [dict(r) for r in rows]


# Lo justo que necesita el informe de duplicados. Pedir `SELECT *` traia
# tambien la letra entera de cada cancion —kilobytes por tema que el informe
# no usa para nada— y con una biblioteca grande eso son decenas de MB de pico
# solo para montar el indice.
BRIEF_COLUMNS = ("id", "path", "file", "artist", "title", "duration",
                 "bitrate", "size", "stars", "favorite")


def brief_by_path() -> dict:
    """{ruta: datos basicos} de toda la biblioteca, sin los campos pesados."""
    conn = connect()
    rows = conn.execute(f"SELECT {','.join(BRIEF_COLUMNS)} FROM songs").fetchall()
    conn.close()
    return {r["path"]: dict(r) for r in rows}


def facets() -> dict:
    """Valores disponibles para los filtros de la interfaz."""
    conn = connect()
    def top(col, lim=500):
        return [dict(r) for r in conn.execute(
            f"SELECT {col} value, COUNT(*) n FROM songs WHERE {col}!='' "
            f"GROUP BY {col} ORDER BY n DESC, value LIMIT ?", (lim,)).fetchall()]
    d = {"artists": top("artist"), "albums": top("album"),
         "genres": top("genre"), "folders": top("folder"), "keys": top("key")}
    conn.close()
    return d


def stats_of() -> dict:
    conn = connect()
    f = conn.execute("SELECT COUNT(*) n, SUM(size) bytes, SUM(duration) seconds, "
                    "SUM(analyzed>0) analyzed_count, SUM(lyrics!='') with_lyrics, "
                    "SUM(artist='') without_artist FROM songs").fetchone()
    conn.close()
    return {"total": f["n"] or 0, "bytes": f["bytes"] or 0, "seconds": f["seconds"] or 0,
            "analyzed_count": f["analyzed_count"] or 0, "with_lyrics": f["with_lyrics"] or 0,
            "without_artist": f["without_artist"] or 0}
