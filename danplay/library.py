# -*- coding: utf-8 -*-
"""Indice SQLite con busqueda de texto completo (FTS5) y carpetas gestionadas."""
import logging, os, shutil, sqlite3, subprocess, sys, time
from pathlib import Path
from . import config, tags, names

log = logging.getLogger("danplay")

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
    favorite  INTEGER DEFAULT 0,
    blur      INTEGER DEFAULT 0    -- la portada se pinta difuminada
);
CREATE INDEX IF NOT EXISTS i_artist ON songs(artist);
CREATE INDEX IF NOT EXISTS i_match_key   ON songs(match_key);
CREATE INDEX IF NOT EXISTS i_root    ON songs(root);

-- Lo que gasta la IA: cada llamada con su proveedor, modelo y tokens, y el
-- coste si el catalogo conocia el precio. Para enseñar «este mes: X».
CREATE TABLE IF NOT EXISTS ai_usage (
    id         INTEGER PRIMARY KEY,
    at         REAL,
    provider   TEXT DEFAULT '',
    model      TEXT DEFAULT '',
    purpose    TEXT DEFAULT '',
    prompt     INTEGER DEFAULT 0,
    completion INTEGER DEFAULT 0,
    cost       REAL
);
CREATE INDEX IF NOT EXISTS i_ai_usage_at ON ai_usage(at);

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
           "key","bpm","cover","lyrics","stars","favorite","blur")


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
    tables = {r[0] for r in conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table'")}
    migrated = False
    for old, (new, cols) in _ANTIGUO.items():
        if old not in tables or new not in tables:
            continue
        if conn.execute(f"SELECT COUNT(*) FROM {new}").fetchone()[0]:
            continue                      # ya hay datos nuevos: no se toca
        src = ",".join(c[0] for c in cols)
        dest = ",".join(c[1] for c in cols)
        conn.execute(f"INSERT OR IGNORE INTO {new} ({dest}) "
                     f"SELECT {src} FROM {old}")
        conn.execute(f"DROP TABLE {old}")
        migrated = True
    if not migrated:
        return False
    # los unicos valores fijos que tambien estaban en castellano
    if "folders" in tables:
        conn.execute("UPDATE folders SET role='library' WHERE role='biblioteca'")
    if "exclusions" in tables:
        conn.execute("UPDATE exclusions SET kind='path' WHERE kind='ruta'")
    if "songs" in tables:
        conn.execute("DROP TABLE IF EXISTS busqueda")
        conn.execute("INSERT INTO search_index(search_index) VALUES('rebuild')")
    conn.commit()
    _touch()
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


# Columnas añadidas despues de la primera version. `CREATE TABLE IF NOT
# EXISTS` no toca una tabla que ya existe, asi que a quien ya tenia su base
# hay que añadirselas a mano.
_ADDED_LATER = (("songs", "blur", "INTEGER DEFAULT 0"),
                # la letra con tiempos (LRC) de LRCLIB: se guarda aparte y el
                # escaneo no la toca; si se pierde, se vuelve a pedir
                ("songs", "lyrics_synced", "TEXT DEFAULT ''"))


def _add_missing_columns(conn) -> None:
    for table, column, kind in _ADDED_LATER:
        present = {r[1] for r in conn.execute(f"PRAGMA table_info({table})")}
        if present and column not in present:
            conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {kind}")


def _prepare(conn) -> None:
    conn.executescript(SCHEMA)
    for extra in _EXTRA_SCHEMAS:
        conn.executescript(extra)
    _add_missing_columns(conn)
    _migrate_from_spanish(conn)


def connect():
    config.DATABASE.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(config.DATABASE)
    conn.row_factory = sqlite3.Row
    # Con WAL ya activo, esto evita esperar al disco en cada commit. Se puede
    # perder la ultima escritura si se va la luz de golpe (no si se cierra la
    # app), y aqui eso no cuesta nada: la base es solo un indice y un escaneo
    # la reconstruye entera desde los propios archivos. En un disco duro
    # normal la diferencia por escritura es grande.
    conn.execute("PRAGMA synchronous=NORMAL")
    # Si otra conexion esta escribiendo (el escaneo, por ejemplo), se espera
    # hasta cinco segundos en vez de fallar al instante con «database is
    # locked»: puntuar una cancion mientras se escanea tiene que funcionar.
    conn.execute("PRAGMA busy_timeout=5000")
    key = str(config.DATABASE)
    if key not in _prepared:
        with _prepare_lock:
            if key not in _prepared:
                _prepare(conn)
                _prepared.add(key)
    return conn


# ------------------------------------------------------------- carpetas

def _inside(path, root) -> bool:
    """True si `path` es `root` o cuelga de el.

    Sobre rutas reales (enlaces simbolicos y «..» resueltos) y por
    componentes, no pegando «/» a una cadena: asi `../../x` o un enlace que
    apunte fuera no cuentan como «dentro», y en Windows tampoco importa si
    la unidad viene en mayuscula o minuscula.
    """
    try:
        p, r = os.path.realpath(path), os.path.realpath(root)
        return os.path.commonpath([p, r]) == r
    except ValueError:                 # unidades distintas, o relativa y absoluta
        return False


def _same_or_under(path: str, folder: str) -> bool:
    """Como `_inside` pero puramente textual: para patrones ya normalizados."""
    try:
        return os.path.commonpath([path, folder]) == os.path.normpath(folder)
    except ValueError:
        return False


def within_roots(path) -> bool:
    """Si la ruta esta dentro de alguna carpeta gestionada (activa).

    Es la barrera de todo lo que borra o mueve: nada que venga de la API o
    del asistente debe tocar un archivo fuera de la biblioteca.
    """
    return any(_inside(path, r) for r in _roots())


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
        if _inside(path, other):
            return {"kind": "inside", "other": other,
                    "message": f"esta dentro de «{other}», que ya esta indexada"}
        if _inside(other, path):
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
    placeholders = ",".join("?" * len(sample))
    rows = conn.execute(
        f"SELECT file, size, root FROM songs WHERE file IN ({placeholders})",
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
    _touch()


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


def roots() -> list[str]:
    """Carpetas gestionadas activas. Para pasarselas a `index_file` en bucle
    y no consultar la base por cada archivo."""
    return _roots()


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
    name_lc, full_lc = dir_name.lower(), full_path.lower()
    for pat in DEFAULT_EXCLUDES:
        if fnmatch(dir_name, pat) or fnmatch(name_lc, pat.lower()):
            return True
    for e in exclusions:
        p = (e["pattern"] or "").lower()
        if not p:
            continue
        if e["kind"] == "path":
            if _same_or_under(full_lc, p):
                return True
        elif (fnmatch(name_lc, p) or fnmatch(full_lc, p)
              or fnmatch(full_lc, "*/" + p.strip("*/") + "/*")):
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


def _row(path, root, vocab=None, tag=None) -> tuple:
    """Fila del indice a partir del archivo. Una sola lectura de etiquetas.

    Antes se abria tres veces (etiquetas, duracion y bitrate por separado) y
    aun asi no se leian ni la caratula ni la letra: si perdias la base, esas
    se quedaban vacias aunque estuvieran dentro del mp3. `tag` permite pasar
    lo que `tags.read_all` ya leyo, para no abrir el archivo dos veces.
    """
    st = os.stat(path)
    tag = tags.read_all(path) if tag is None else tag
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
            1 if tag.get("favorite") else 0,
            1 if tag.get("blur") else 0)


# Cada cuantos archivos se confirma la transaccion del escaneo. Una sola
# transaccion para toda la biblioteca dejaba la base bloqueada minutos: poner
# una estrella mientras tanto fallaba con «database is locked».
SCAN_BATCH = 500

_INSERT_SQL = (f"INSERT INTO songs ({','.join(COLUMNS)}) "
               f"VALUES ({','.join('?' * len(COLUMNS))})")
_UPDATE_SQL = f"UPDATE songs SET {','.join(f'{c}=?' for c in COLUMNS)} WHERE id=?"


def scan(progress=None) -> dict:
    """Pone el indice al dia con lo que hay en las carpetas gestionadas.

    Incremental y por ruta: lo nuevo se inserta, lo que cambio se actualiza
    y lo que ya no esta en el disco se borra. Los ids NO cambian nunca.
    Antes se vaciaba la tabla y se reinsertaba todo: los ids volvian a
    empezar en 1 en el orden del disco, y un archivo nuevo desplazaba a todos
    los siguientes, con lo que las listas (que guardan ids) pasaban a
    apuntar a otras canciones.

    Casi todo se relee del archivo, asi que el escaneo reconstruye el indice
    aunque la base se pierda; de la base se conservan los acordes y la marca
    de analizado (las columnas que no estan en COLUMNS no se tocan). Las
    listas se recrean al final desde la etiqueta LISTAS de cada archivo.

    No se reconstruye el indice de busqueda: los triggers lo mantienen fila
    a fila, y solo para las filas que de verdad cambian.
    """
    conn = connect()
    existing = {r["path"]: r for r in conn.execute(
        "SELECT id, path, artist, mtime, size FROM songs")}
    vocab = names.vocabulary(config.ARTISTS_DIR)
    exclusions = list_exclusions()
    seen: set = set()
    playlists_found: dict = {}
    n = added_count = reused = updated = pending = 0
    for root in _roots():
        if not os.path.isdir(root):
            continue
        for path in audio_files(root, exclusions):
            v = existing.get(path)
            # Si el archivo no se ha tocado desde el ultimo escaneo, sus
            # etiquetas no pueden haber cambiado: se deja la fila como esta
            # en vez de volver a abrirlo y parsearlo. Es lo que hace que un
            # reescaneo sea casi instantaneo. Escribir etiquetas (estrellas,
            # favorito, un titulo corregido) cambia la fecha del archivo, asi
            # que eso siempre se relee. Los que no tienen artista tambien:
            # pueden resolverse ahora que hay mas carpetas de artista.
            if v is not None and v["artist"]:
                try:
                    st = os.stat(path)
                except OSError:
                    continue                 # desaparecio: se borra al final
                if st.st_mtime == v["mtime"] and st.st_size == v["size"]:
                    seen.add(path)
                    reused += 1; n += 1
                    if progress and n % 50 == 0:
                        progress(n)
                    continue
            try:
                tag = tags.read_all(path)
                row = _row(path, root, vocab, tag)
            except OSError:
                log.warning("no se pudo indexar %s", path, exc_info=True)
                continue
            seen.add(path)
            if tag.get("playlists"):
                playlists_found[path] = tag["playlists"]
            if v is None:
                conn.execute(_INSERT_SQL, row)
                added_count += 1
            else:
                conn.execute(_UPDATE_SQL, row + (v["id"],))
                updated += 1
            n += 1; pending += 1
            if pending >= SCAN_BATCH:
                conn.commit(); pending = 0
            if progress and n % 50 == 0:
                progress(n)
    # lo que ya no esta en el disco (o quedo fuera de las carpetas activas)
    gone = [p for p in existing if p not in seen]
    for i in range(0, len(gone), SCAN_BATCH):
        chunk = gone[i:i + SCAN_BATCH]
        conn.execute(f"DELETE FROM songs WHERE path IN ({','.join('?' * len(chunk))})",
                     chunk)
        conn.commit()
    conn.commit(); conn.close()
    _touch()
    restored = restore_playlists_from_tags(playlists_found)
    return {"total": n, "added_count": added_count, "reused": reused,
            "updated": updated, "removed": len(gone), "playlists_restored": restored}


def restore_playlists_from_tags(found: dict | None = None) -> int:
    """Recrea listas y pertenencias a partir de lo que dicen los archivos.

    Cada cancion lleva dentro (etiqueta LISTAS / DANPLAY_PLAYLISTS) los
    nombres de las listas a las que pertenece. Antes se escribia y nadie lo
    leia, asi que las listas no sobrevivian a perder la base, en contra de lo
    que promete la arquitectura.

    `found` es {ruta: [nombres]} ya leido durante el escaneo; sin el, se
    releen las etiquetas de todo el indice (lento, pero sirve suelto).

    Solo AÑADE lo que falte: nunca quita canciones de una lista ni borra
    listas, porque la base puede ir por delante del archivo. Devuelve cuantas
    pertenencias se han añadido.
    """
    from . import playlists as _playlists   # noqa: F401  (registra su esquema)
    conn = connect()
    if found is None:
        found = {}
        for r in conn.execute("SELECT path FROM songs").fetchall():
            if os.path.exists(r["path"]):
                lists = tags.read_all(r["path"]).get("playlists") or []
                if lists:
                    found[r["path"]] = lists
    if not found:
        conn.close()
        return 0
    ids = {r["name"]: r["id"] for r in conn.execute("SELECT id, name FROM playlists")}
    added, now = 0, time.time()
    for path, lists in found.items():
        song = conn.execute("SELECT id FROM songs WHERE path=?", (path,)).fetchone()
        if not song:
            continue
        for name in lists:
            lid = ids.get(name)
            if lid is None:
                cur = conn.execute("INSERT INTO playlists (name,note,color,created) "
                                   "VALUES (?,?,?,?)", (name, "", "", now))
                lid = ids[name] = cur.lastrowid
            position = conn.execute(
                "SELECT COALESCE(MAX(position),-1)+1 FROM playlist_songs "
                "WHERE playlist_id=?", (lid,)).fetchone()[0]
            cur = conn.execute(
                "INSERT OR IGNORE INTO playlist_songs (playlist_id,song_id,position,added) "
                "VALUES (?,?,?,?)", (lid, song["id"], position, now))
            added += cur.rowcount if cur.rowcount and cur.rowcount > 0 else 0
    conn.commit(); conn.close()
    if added:
        log.info("listas recuperadas desde las etiquetas: %d pertenencias", added)
    return added


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


# Por que se puede ordenar: nombre que usa la interfaz -> (columna, tipo).
# Se define aqui y no en la interfaz para que las dos no puedan separarse: la
# API rechaza cualquier otro nombre.
SORT_FIELDS = {
    "artist":   ("c.artist",   "text"),
    "title":    ("c.title",    "text"),
    "album":    ("c.album",    "text"),
    "genre":    ("c.genre",    "text"),
    "year":     ("c.year",     "text"),
    "key":      ("c.key",      "text"),
    "folder":   ("c.folder",   "text"),
    "file":     ("c.file",     "text"),
    "duration": ("c.duration", "num"),
    "bpm":      ("c.bpm",      "num"),
    "bitrate":  ("c.bitrate",  "num"),
    "size":     ("c.size",     "num"),
    "stars":    ("c.stars",    "num"),
    "recent":   ("c.mtime",    "num"),
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


def search(query="", filters=None, sort="artist", limit=200, offset=0,
           only_favorites=False, min_stars=0, desc=False) -> list[dict]:
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

    sql = ("SELECT c.* FROM songs c"
           + (" WHERE " + " AND ".join(where) if where else "")
           + f" ORDER BY {_order_by(sort, desc)} LIMIT ? OFFSET ?")
    rows = conn.execute(sql, params + [limit, offset]).fetchall()
    conn.close()
    return [dict(f) for f in rows]


def set_blur(cid: int, value=True) -> dict | None:
    """Difumina (o deja de difuminar) la portada de una cancion.

    La imagen no se toca: se guarda una marca dentro del propio mp3 y la
    interfaz la pinta borrosa. Quitarla devuelve la portada intacta.
    """
    c = by_id(cid)
    if not c:
        return None
    value = bool(value)
    written = False
    if config.WRITE_TAGS and os.path.exists(c["path"]):
        written = tags.set_blurred_cover(c["path"], value)
    update(cid, blur=1 if value else 0)
    out = by_id(cid)
    if out is not None:
        out["tags_written"] = bool(written)
    return out


def by_id(cid: int) -> dict | None:
    conn = connect()
    f = conn.execute("SELECT * FROM songs WHERE id=?", (cid,)).fetchone()
    conn.close()
    return dict(f) if f else None


def update(cid: int, **fields) -> int:
    """Cambia columnas del indice (solo el indice). Devuelve filas tocadas."""
    allowed = {"artist","title","album","year","genre","feat","key","bpm",
                  "cover","lyrics","lyrics_synced","chords","analyzed","stars","favorite","blur"}
    fields = {k: v for k, v in fields.items() if k in allowed}
    if not fields:
        return 0
    conn = connect()
    cur = conn.execute(f"UPDATE songs SET {','.join(f'{k}=?' for k in fields)} WHERE id=?",
                       list(fields.values()) + [cid])
    conn.commit(); conn.close()
    _touch()
    return cur.rowcount or 0


_METADATA_FIELDS = ("artist", "title", "album", "year", "genre")


def edit(cid: int, **fields) -> dict | None:
    """Cambia los datos de una cancion en el indice y en las etiquetas.

    Lo usan la API y el asistente: un solo camino para que no se separen.
    La ficha que devuelve lleva `tags_written`: si el archivo se actualizo
    de verdad. Antes se ignoraba el resultado y en un flac o un m4a la
    edicion cambiaba el indice y el archivo se quedaba igual, sin aviso.
    """
    c = by_id(cid)
    if not c:
        return None
    update(cid, **fields)
    written = False
    if config.WRITE_TAGS and os.path.exists(c["path"]):
        written = True
        if any(k in fields for k in _METADATA_FIELDS):
            written = tags.write(c["path"],
                                 artist=fields.get("artist", c["artist"]),
                                 title=fields.get("title", c["title"]),
                                 album=fields.get("album", c["album"]),
                                 year=str(fields.get("year", c["year"])),
                                 genre=fields.get("genre", c["genre"])) and written
        if "key" in fields or "bpm" in fields:
            written = tags.write_analysis(c["path"], fields.get("key", c["key"]),
                                          fields.get("bpm", c["bpm"])) and written
        # la letra vive en el USLT del propio archivo, no solo en el indice
        if "lyrics" in fields:
            written = tags.write_lyrics(c["path"], fields["lyrics"] or "") and written
    out = by_id(cid)
    if out is not None:
        out["tags_written"] = bool(written)
    return out


def index_file(path: str, roots=None, vocab=None) -> dict | None:
    """Mete en el indice un archivo recien llegado, sin reescanear todo.

    Sin esto, lo que se descarga o se importa se mueve a Artistas/ pero no
    aparece en la app hasta el siguiente escaneo completo: el archivo esta en
    el disco y el usuario no lo ve por ningun lado.

    `roots` y `vocab` se pueden pasar ya calculados: la importacion y las
    descargas llaman a esto en bucle, y consultar las carpetas y relistar
    Artistas/ por cada archivo era lo que mas tardaba.
    """
    path = os.path.abspath(path)
    if not os.path.isfile(path):
        return None
    roots = _roots() if roots is None else roots
    root = next((r for r in roots if _inside(path, r)), None)
    if root is None:
        return None                      # fuera de las carpetas gestionadas
    try:
        row = _row(path, root, names.vocabulary(config.ARTISTS_DIR)
                   if vocab is None else vocab)
    except OSError:
        log.warning("no se pudo indexar %s", path, exc_info=True)
        return None
    conn = connect()
    # si ya estaba, se actualiza en sitio: un INSERT OR REPLACE le daria un
    # id nuevo y las listas que lo tuvieran lo perderian
    old = conn.execute("SELECT id FROM songs WHERE path=?", (path,)).fetchone()
    if old:
        conn.execute(_UPDATE_SQL, row + (old["id"],))
    else:
        conn.execute(_INSERT_SQL, row)
    conn.commit()
    r = conn.execute("SELECT * FROM songs WHERE path=?", (path,)).fetchone()
    conn.close()
    _touch()
    return dict(r) if r else None


def trash_path(path) -> dict:
    """Manda un archivo a la papelera del sistema. No toca el indice.

    `send2trash` sabe de la papelera de Linux, Windows y macOS. En Linux, si
    falla (archivos en otro disco montado, por ejemplo), se prueba con
    `gio trash`, que conoce las papeleras de cada volumen. Si nada funciona
    se avisa: borrar sin vuelta atras no es una opcion.
    """
    path = str(path)
    if not os.path.lexists(path):
        return {"ok": True, "missing": True}
    reason = ""
    try:
        from send2trash import send2trash
        send2trash(path)
        return {"ok": True}
    except Exception as e:                                  # noqa: BLE001
        log.warning("send2trash no pudo con %s", path, exc_info=True)
        reason = str(e)[:120]
    if sys.platform.startswith("linux") and shutil.which("gio"):
        r = subprocess.run(["gio", "trash", path], capture_output=True, text=True)
        if r.returncode == 0:
            return {"ok": True}
        reason = (r.stderr or "").strip()[:120] or reason
    return {"ok": False, "error": "no se pudo mandar a la papelera: " + reason}


def trash(cid: int) -> dict:
    """Manda el archivo a la papelera del sistema y lo saca del indice.

    A la papelera y no `unlink`: borrar musica del usuario sin vuelta atras,
    y menos desde un menu o desde el chat, es demasiado definitivo.
    """
    c = by_id(cid)
    if not c:
        return {"ok": False, "error": "no existe esa cancion"}
    path = c["path"]
    r = trash_path(path)
    if not r["ok"]:
        return r
    forget(cid)
    return {"ok": True, "trashed": True, "path": path,
            "name": os.path.basename(path),
            "artist": c["artist"], "title": c["title"]}


def forget(cid: int) -> None:
    """Quita la cancion del indice. No toca el archivo."""
    conn = connect()
    conn.execute("DELETE FROM songs WHERE id=?", (cid,))
    conn.commit(); conn.close()
    _touch()


def forget_path(path: str) -> int:
    """Saca del indice el archivo que estaba en esa ruta. No toca el disco.

    Para cuando el archivo ya no esta ahi (se borro o se renombro) y solo hay
    que ponerse al dia. Devuelve cuantas filas se quitaron.
    """
    conn = connect()
    cur = conn.execute("DELETE FROM songs WHERE path=?", (os.path.abspath(path),))
    conn.commit(); conn.close()
    _touch()
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


def log_ai_usage(provider: str, model: str, purpose: str, prompt: int,
                 completion: int, cost: float | None) -> None:
    conn = connect()
    conn.execute("INSERT INTO ai_usage (at, provider, model, purpose, prompt, completion, cost) "
                 "VALUES (?,?,?,?,?,?,?)",
                 (time.time(), provider, model, purpose, int(prompt), int(completion), cost))
    conn.commit(); conn.close()


def ai_usage_summary() -> dict:
    """Lo gastado hoy, este mes y en total: llamadas, tokens y coste (solo
    de las llamadas con precio conocido; las demas se cuentan aparte)."""
    import datetime as _dt
    now = _dt.datetime.now()
    day = _dt.datetime(now.year, now.month, now.day).timestamp()
    month = _dt.datetime(now.year, now.month, 1).timestamp()
    conn = connect()

    def part(since):
        f = conn.execute(
            "SELECT COUNT(*) calls, COALESCE(SUM(prompt),0) prompt, COALESCE(SUM(completion),0) completion, "
            "COALESCE(SUM(cost),0) cost, SUM(cost IS NULL) unpriced FROM ai_usage WHERE at>=?",
            (since,)).fetchone()
        return {"calls": f["calls"] or 0, "prompt": f["prompt"] or 0,
                "completion": f["completion"] or 0, "cost": round(f["cost"] or 0, 6),
                "unpriced": f["unpriced"] or 0}
    out = {"today": part(day), "month": part(month), "total": part(0)}
    conn.close()
    return out


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


def by_path(path: str) -> dict | None:
    """La cancion que vive en esa ruta exacta, o None si no esta indexada.

    Para cuando el sistema abre un archivo con DanPlay: si resulta ser una de
    la biblioteca se reproduce como tal (con su caratula, sus estrellas y su
    id) en vez de como un archivo suelto. Pregunta por UNA ruta; el
    `brief_by_path` de abajo se trae la biblioteca entera y para esto seria
    tirar la casa por la ventana.
    """
    conn = connect()
    row = conn.execute(
        "SELECT id,title,artist,duration,blur FROM songs WHERE path=?", (str(path),)
    ).fetchone()
    conn.close()
    return dict(row) if row else None


def brief_by_path() -> dict:
    """{ruta: datos basicos} de toda la biblioteca, sin los campos pesados."""
    conn = connect()
    rows = conn.execute(f"SELECT {','.join(BRIEF_COLUMNS)} FROM songs").fetchall()
    conn.close()
    return {r["path"]: dict(r) for r in rows}


def top_artists(limit=12) -> list[dict]:
    """Los artistas con mas canciones: [{value, n}]. Para situar al asistente."""
    conn = connect()
    rows = [dict(r) for r in conn.execute(
        "SELECT artist value, COUNT(*) n FROM songs WHERE artist!='' "
        "GROUP BY artist ORDER BY n DESC, value LIMIT ?", (int(limit),)).fetchall()]
    conn.close()
    return rows


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


# `/api/status` pide las cifras continuamente y calcularlas es un agregado
# sobre toda la tabla que ademas evalua `lyrics != ''`, la columna mas pesada.
# Se recuerdan unos segundos, y cualquier escritura en `songs` las olvida al
# instante con `_touch()`, asi que nunca se enseña un total viejo despues de
# indexar o borrar algo.
_STATS_TTL = 5.0
_stats_cache: dict = {"at": 0.0, "db": "", "value": None}


# Cuantas veces ha cambiado algo que la interfaz enseña: el indice, las
# listas, las estrellas. Va en /api/status; Rust lo mira al vigilar el nucleo
# y, si se ha movido, avisa a las ventanas para que se refresquen. Es lo que
# hace que un cambio hecho «por detras» —una descarga que termina, el
# asistente, la linea de ordenes— se vea sin tener que salir y volver a entrar.
_REVISION = {"n": 0}


def revision() -> int:
    return _REVISION["n"]


def _touch() -> None:
    """Algo cambio en el indice: la proxima `stats_of` vuelve a contar."""
    _stats_cache["at"] = 0.0
    _REVISION["n"] += 1


def stats_of() -> dict:
    key = str(config.DATABASE)
    cached = _stats_cache
    if (cached["value"] is not None and cached["db"] == key
            and time.monotonic() - cached["at"] < _STATS_TTL):
        return dict(cached["value"])
    conn = connect()
    f = conn.execute("SELECT COUNT(*) n, SUM(size) bytes, SUM(duration) seconds, "
                    "SUM(analyzed>0) analyzed_count, SUM(lyrics!='') with_lyrics, "
                    "SUM(artist='') without_artist FROM songs").fetchone()
    conn.close()
    value = {"total": f["n"] or 0, "bytes": f["bytes"] or 0, "seconds": f["seconds"] or 0,
             "analyzed_count": f["analyzed_count"] or 0, "with_lyrics": f["with_lyrics"] or 0,
             "without_artist": f["without_artist"] or 0}
    _stats_cache.update(at=time.monotonic(), db=key, value=value)
    return dict(value)
