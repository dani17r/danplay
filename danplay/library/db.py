"""El indice por dentro: el esquema, sus migraciones y la conexion.

La base es solo un indice (ver docs/ARQUITECTURA.md, «La base de datos es
desechable»): todo lo que el usuario crea vive dentro de sus archivos, y un
escaneo la reconstruye entera. Aun asi, la de quien ya usa la app se pone al
dia sola al abrirla: el esquema lleva su version (`PRAGMA user_version`) y
cada cambio es una migracion numerada que se aplica una sola vez, dentro de
una transaccion `BEGIN IMMEDIATE` (si dos procesos abren la base a la vez, el
segundo espera y ya la encuentra migrada).
"""

import json
import logging
import os
import sqlite3
import threading
import time
from pathlib import Path
from typing import NotRequired, TypedDict

from .. import config

log = logging.getLogger(__name__)


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
    blur      INTEGER DEFAULT 0,   -- la portada se pinta difuminada
    lyrics_synced TEXT DEFAULT '', -- la letra con tiempos (LRC) de LRCLIB
    study     TEXT DEFAULT ''      -- el modo estudio, en JSON
);
CREATE INDEX IF NOT EXISTS i_artist ON songs(artist);
CREATE INDEX IF NOT EXISTS i_match_key   ON songs(match_key);
CREATE INDEX IF NOT EXISTS i_root    ON songs(root);

-- Canciones cuyo archivo se fue por fuera de la app: se movio, se borro o su
-- disco no esta montado. No se enseñan en ningun sitio (no estan en `songs`),
-- pero se guardan un tiempo CON SU ID: si el archivo vuelve a su sitio o
-- aparece en otro, la cancion recupera el id y, con el, las listas en las que
-- estaba, los acordes, el analisis y el modo estudio. Pasado un mes sin
-- volver se olvidan del todo (`_purge_missing`).
CREATE TABLE IF NOT EXISTS songs_missing (
    id       INTEGER PRIMARY KEY,
    path     TEXT,
    root     TEXT,
    file     TEXT,
    size     INTEGER,
    mtime    REAL,
    gone_at  REAL,
    data     TEXT              -- la fila entera, en JSON
);
CREATE INDEX IF NOT EXISTS i_missing_path ON songs_missing(path);
CREATE INDEX IF NOT EXISTS i_missing_size ON songs_missing(size);
CREATE INDEX IF NOT EXISTS i_missing_root ON songs_missing(root);

-- Valores sueltos del propio indice.
CREATE TABLE IF NOT EXISTS meta (
    key    TEXT PRIMARY KEY,
    value
);

-- El id mas alto que ha tenido nunca una cancion. Sin AUTOINCREMENT, SQLite
-- da a una fila nueva el id mas alto que haya AHORA mas uno: si la ultima
-- cancion salia del indice, la siguiente heredaba su id y, con el, las listas
-- en las que estaba. Los ids nuevos salen de aqui (`_next_song_id`).
CREATE TRIGGER IF NOT EXISTS songs_hwm AFTER INSERT ON songs BEGIN
    INSERT OR REPLACE INTO meta (key, value) VALUES ('song_id_hwm',
        MAX(new.id, COALESCE((SELECT value FROM meta WHERE key='song_id_hwm'), 0)));
END;

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
COLUMNS = (
    "path",
    "root",
    "folder",
    "file",
    "artist",
    "title",
    "album",
    "year",
    "genre",
    "feat",
    "match_key",
    "duration",
    "bitrate",
    "size",
    "mtime",
    "key",
    "bpm",
    "cover",
    "lyrics",
    "stars",
    "favorite",
    "blur",
)


class Song(TypedDict):
    """Una fila entera de `songs`, como la devuelve `by_id`. Todas las
    columnas estan siempre: las que se añadieron despues las pone la migracion.
    Las listas de la API dan filas ligeras, sin lo pesado (ver `search.light`)."""

    id: int
    path: str
    root: str
    folder: str
    file: str
    artist: str
    title: str
    album: str
    year: str
    genre: str
    feat: str
    match_key: str
    duration: float
    bitrate: int
    size: int
    mtime: float
    key: str
    bpm: float
    cover: str
    lyrics: str
    chords: str
    analyzed: float
    stars: int
    favorite: int
    blur: int
    lyrics_synced: str
    study: str
    # lo añaden `edit`, `set_blur`...: si el archivo se escribio de verdad
    tags_written: NotRequired[bool]


# Tablas y columnas del esquema anterior (estaban en castellano). Se migran
# solas la primera vez que se abre la base: nadie tiene que reescanear.
_ANTIGUO = {
    "carpetas": (
        "folders",
        [
            ("ruta", "path"),
            ("etiqueta", "label"),
            ("rol", "role"),
            ("activa", "active"),
            ("agregada", "added"),
        ],
    ),
    "exclusiones": ("exclusions", [("patron", "pattern"), ("tipo", "kind"), ("nota", "note")]),
    "canciones": (
        "songs",
        [
            ("id", "id"),
            ("ruta", "path"),
            ("raiz", "root"),
            ("carpeta", "folder"),
            ("archivo", "file"),
            ("artista", "artist"),
            ("titulo", "title"),
            ("album", "album"),
            ("anio", "year"),
            ("genero", "genre"),
            ("feat", "feat"),
            ("clave", "match_key"),
            ("duracion", "duration"),
            ("bitrate", "bitrate"),
            ("tamanio", "size"),
            ("mtime", "mtime"),
            ("tono", "key"),
            ("bpm", "bpm"),
            ("portada", "cover"),
            ("letra", "lyrics"),
            ("acordes", "chords"),
            ("analizado", "analyzed"),
            ("estrellas", "stars"),
            ("favorito", "favorite"),
        ],
    ),
    "listas": (
        "playlists",
        [
            ("id", "id"),
            ("nombre", "name"),
            ("nota", "note"),
            ("color", "color"),
            ("creada", "created"),
        ],
    ),
    "lista_canciones": (
        "playlist_songs",
        [("lista_id", "playlist_id"), ("cancion_id", "song_id"), ("orden", "position")],
    ),
}


def _migrate_from_spanish(conn) -> None:
    """Copia los datos del esquema viejo al nuevo, tabla a tabla.

    Va por pares y solo cuando el destino existe y esta vacio: si ya hay datos
    nuevos no se toca nada. Todas las tablas de destino existen ya (ver
    `_prepare`), asi que no depende de quien abra la base primero.
    """
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    migrated = False
    # los nombres de tabla y columna salen de _ANTIGUO, no de fuera
    for old, (new, cols) in _ANTIGUO.items():
        if old not in tables or new not in tables:
            continue
        if conn.execute(f"SELECT COUNT(*) FROM {new}").fetchone()[0]:  # noqa: S608
            continue  # ya hay datos nuevos: no se toca
        src = ",".join(c[0] for c in cols)
        dest = ",".join(c[1] for c in cols)
        conn.execute(
            f"INSERT OR IGNORE INTO {new} ({dest}) "  # noqa: S608
            f"SELECT {src} FROM {old}"
        )
        conn.execute(f"DROP TABLE {old}")
        migrated = True
    if not migrated:
        return
    # los unicos valores fijos que tambien estaban en castellano
    if "folders" in tables:
        conn.execute("UPDATE folders SET role='library' WHERE role='biblioteca'")
    if "exclusions" in tables:
        conn.execute("UPDATE exclusions SET kind='path' WHERE kind='ruta'")
    if "songs" in tables:
        conn.execute("DROP TABLE IF EXISTS busqueda")
        conn.execute("INSERT INTO search_index(search_index) VALUES('rebuild')")
    _touch()


# El esquema y la migracion son de una vez por base, no de cada consulta.
# Antes se reejecutaban en CADA connect(): crear las tablas, revisar
# sqlite_master y contar filas de cinco tablas, cientos de veces por pantalla.
# Ahora se hace la primera vez y despues connect() solo abre.
_prepared: set = set()
_prepare_lock = threading.Lock()


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
_ADDED_LATER = (
    ("songs", "blur", "INTEGER DEFAULT 0"),
    # la letra con tiempos (LRC) de LRCLIB: se guarda aparte y el
    # escaneo no la toca; si se pierde, se vuelve a pedir
    ("songs", "lyrics_synced", "TEXT DEFAULT ''"),
    # modo estudio: bucle, velocidad, marcadores y notas (JSON).
    # Va tambien en una etiqueta del archivo, y de ahi se recupera
    ("songs", "study", "TEXT DEFAULT ''"),
)


def _add_missing_columns(conn) -> None:
    for table, column, kind in _ADDED_LATER:
        present = {r[1] for r in conn.execute(f"PRAGMA table_info({table})")}
        if present and column not in present:
            conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {kind}")


def _has_table(conn, name: str) -> bool:
    return (
        conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,)
        ).fetchone()
        is not None
    )


def _raise_hwm(conn) -> None:
    """Sube el tope de ids por encima de cualquier id que se este usando.

    Hace falta en las bases de antes del tope: una cancion borrada pudo dejar
    su id en una lista, en el historial de descargas o en los recientes, y el
    siguiente id nuevo no debe coincidir con ninguno. Despues lo mantiene el
    trigger `songs_hwm` y `_next_song_id`.
    """
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    used = [
        ("songs", "id"),
        ("songs_missing", "id"),
        ("playlist_songs", "song_id"),
        ("recent", "song_id"),
        ("downloads", "song_id"),
    ]
    # nombres fijos, de la lista de aqui arriba
    top = max(
        (conn.execute(f"SELECT MAX({column}) FROM {table}").fetchone()[0] or 0)  # noqa: S608
        for table, column in used
        if table in tables
    )
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('song_id_hwm', "
        "MAX(?, COALESCE((SELECT value FROM meta WHERE key='song_id_hwm'), 0)))",
        (top,),
    )


# Los cambios del esquema, en orden. Cada uno se aplica UNA vez por base: la
# version que ya tiene va en `PRAGMA user_version`. Las bases de antes de que
# hubiera version estan a 0 y pasan por todos (son idempotentes: en una base
# que ya los tenia no cambian nada). Un cambio nuevo es una entrada nueva al
# final, nunca tocar una que ya se reparte.
MIGRATIONS = (
    (
        1,
        "columnas añadidas despues (portada difuminada, letra con tiempos, estudio)",
        _add_missing_columns,
    ),
    (2, "del esquema en castellano al de ahora", _migrate_from_spanish),
    (3, "tope historico de ids de cancion", _raise_hwm),
)
SCHEMA_VERSION = MIGRATIONS[-1][0]


def _migrate(conn, fresh: bool = False) -> None:
    """Aplica las migraciones que le falten a esta base, todas o ninguna.

    Una base recien creada (`fresh`) ya nace con el esquema de ahora: se
    apunta la ultima version sin pasar por las migraciones.
    """
    conn.execute("BEGIN IMMEDIATE")
    try:
        current = conn.execute("PRAGMA user_version").fetchone()[0]
        if fresh and current == 0:
            current = SCHEMA_VERSION
            conn.execute(f"PRAGMA user_version = {int(SCHEMA_VERSION)}")
        if current > SCHEMA_VERSION:
            log.warning(
                "la base es de un DanPlay mas nuevo (esquema %d, este sabe hasta el "
                "%d): no se migra",
                current,
                SCHEMA_VERSION,
            )
        for number, what, step in MIGRATIONS:
            if number > current:
                step(conn)
                log.info("base de datos al dia: %s", what)
        if current < SCHEMA_VERSION:
            conn.execute(f"PRAGMA user_version = {int(SCHEMA_VERSION)}")
        conn.commit()
    except BaseException:
        conn.rollback()
        raise


def _prepare(conn) -> None:
    """Crea lo que falte y migra. Una vez por base y proceso (`_prepared`).

    Las tablas de las listas, la lista del reproductor y las conversaciones
    las declaran sus modulos (`register_schema`); se importan aqui para que
    esten TODAS antes de migrar, abra la base quien la abra primero.
    """
    from .. import chats, external, playlists  # noqa: F401  (declaran sus tablas)

    # sin ninguna tabla: una base nueva (no una de antes de las versiones,
    # ni una del esquema en castellano, que si tienen tablas)
    fresh = conn.execute("SELECT COUNT(*) FROM sqlite_master WHERE type='table'").fetchone()[0] == 0
    # executescript confirma lo que hubiera pendiente: va antes de la
    # transaccion de las migraciones, y todo es IF NOT EXISTS
    conn.executescript(SCHEMA)
    for extra in _EXTRA_SCHEMAS:
        conn.executescript(extra)
    _migrate(conn, fresh)


def _next_song_id(conn) -> int:
    """El id de una cancion nueva: nunca uno que ya haya tenido otra.

    Todo INSERT en `songs` pasa por aqui (o reutiliza a proposito el id de
    una cancion que vuelve, ver `_add`).
    """
    top = conn.execute(
        "SELECT MAX(COALESCE((SELECT value FROM meta WHERE key='song_id_hwm'), 0),"
        "           COALESCE((SELECT MAX(id) FROM songs), 0),"
        "           COALESCE((SELECT MAX(id) FROM songs_missing), 0))"
    ).fetchone()[0]
    return int(top) + 1


def _private_database(path: Path) -> None:
    """La base lleva el historial del chat y lo que escuchas: solo para su
    dueño (0600), tambien la que ya existia y sus archivos -wal/-shm. SQLite
    crea los suyos con los permisos de la base, asi que basta con ponerlos
    bien antes de abrirla."""
    if os.name != "posix":
        return
    try:
        if not path.exists():
            os.close(os.open(path, os.O_CREAT | os.O_WRONLY, 0o600))
        for p in (path, Path(f"{path}-wal"), Path(f"{path}-shm")):
            if p.exists():
                os.chmod(p, 0o600)
    except OSError:
        log.warning("no pude cerrar los permisos de %s", path, exc_info=True)


class Connection(sqlite3.Connection):
    """Una conexion al indice. Con `with`, confirma lo hecho (o lo deshace si
    algo fallo) y SE CIERRA siempre; la de sqlite3 solo confirma, y con un
    `close()` al final de cada funcion, cualquier excepcion por medio dejaba
    la conexion abierta."""

    def __exit__(self, exc_type, exc, tb):
        try:
            return super().__exit__(exc_type, exc, tb)
        finally:
            self.close()


def connect() -> Connection:
    """Abre el indice (creandolo o migrandolo la primera vez).

    Mejor con `with connect() as conn:`: confirma y cierra solo.
    """
    key = str(config.DATABASE)
    if key not in _prepared:
        config.DATABASE.parent.mkdir(parents=True, exist_ok=True)
        _private_database(config.DATABASE)
    conn = sqlite3.connect(config.DATABASE, factory=Connection)
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
    if key not in _prepared:
        with _prepare_lock:
            if key not in _prepared:
                try:
                    _prepare(conn)
                except BaseException:
                    conn.close()
                    raise
                _prepared.add(key)
    return conn


# Cada cuantos archivos se confirma la transaccion del escaneo. Una sola
# transaccion para toda la biblioteca dejaba la base bloqueada minutos: poner
# una estrella mientras tanto fallaba con «database is locked».
SCAN_BATCH = 500


# Un escaneo a la vez. Ahora no solo los lanza el boton: tambien el vigilante
# de carpetas (watcher.py), al arrancar y cada vez que algo cambia en el
# disco. Dos a la vez veian el mismo archivo nuevo y el segundo reventaba al
# insertarlo.
_SCAN_LOCK = threading.RLock()


_INSERT_SQL = (
    f"INSERT INTO songs (id,{','.join(COLUMNS)}) "  # noqa: S608
    f"VALUES (?,{','.join('?' * len(COLUMNS))})"
)
_UPDATE_SQL = f"UPDATE songs SET {','.join(f'{c}=?' for c in COLUMNS)} WHERE id=?"  # noqa: S608


def _song_columns(conn) -> list[str]:
    return [r[1] for r in conn.execute("PRAGMA table_info(songs)")]


def _insert_row(conn, data: dict, columns=None) -> None:
    """Mete una fila completa (id incluido) con las columnas que existan hoy."""
    columns = columns or _song_columns(conn)
    keys = [c for c in columns if c in data]
    conn.execute(
        f"INSERT INTO songs ({','.join(keys)}) VALUES ({','.join('?' * len(keys))})",  # noqa: S608
        [data[k] for k in keys],
    )


def _to_missing(conn, paths) -> int:
    """Aparta esas canciones: salen de `songs` (de toda la app) y esperan en
    `songs_missing` con su id. Sus filas en las listas se quedan: las listas
    solo enseñan lo que esta en `songs`, y si la cancion vuelve reaparece en
    su sitio. No hace commit."""
    paths = list(paths)
    moved, now = 0, time.time()
    for i in range(0, len(paths), SCAN_BATCH):
        chunk = paths[i : i + SCAN_BATCH]
        marks = ",".join("?" * len(chunk))
        rows = conn.execute(f"SELECT * FROM songs WHERE path IN ({marks})", chunk).fetchall()  # noqa: S608
        conn.executemany(
            "INSERT OR REPLACE INTO songs_missing (id, path, root, file, size, mtime, gone_at, data) "
            "VALUES (?,?,?,?,?,?,?,?)",
            [
                (
                    r["id"],
                    r["path"],
                    r["root"],
                    r["file"],
                    r["size"],
                    r["mtime"],
                    now,
                    json.dumps(dict(r), ensure_ascii=False),
                )
                for r in rows
            ],
        )
        conn.execute(f"DELETE FROM songs WHERE path IN ({marks})", chunk)  # noqa: S608
        moved += len(rows)
    return moved


# `/api/status` pide las cifras continuamente y calcularlas es un agregado
# sobre toda la tabla que ademas evalua `lyrics != ''`, la columna mas pesada.
# Se recuerdan unos segundos, y cualquier escritura en `songs` las olvida al
# instante con `_touch()`, asi que nunca se enseña un total viejo despues de
# indexar o borrar algo.
_STATS_TTL = 5.0
_stats_cache: dict = {"at": 0.0, "db": "", "rev": -1, "value": None}
_stats_lock = threading.Lock()


# Cuantas veces ha cambiado algo que la interfaz enseña: el indice, las
# listas, las estrellas. Va en /api/status; Rust lo mira al vigilar el nucleo
# y, si se ha movido, avisa a las ventanas para que se refresquen. Es lo que
# hace que un cambio hecho «por detras» —una descarga que termina, el
# asistente, la linea de ordenes— se vea sin tener que salir y volver a entrar.
_REVISION = {"n": 0}


def revision() -> int:
    return _REVISION["n"]


def _touch() -> None:
    """Algo cambio en el indice: la proxima `stats_of` vuelve a contar.

    Bajo cerrojo: `n += 1` son tres pasos y dos hilos (el vigilante y una
    peticion) podian perder un aviso."""
    with _stats_lock:
        _REVISION["n"] += 1
        _stats_cache["at"] = 0.0


def stats_of() -> dict:
    """Las cifras de la biblioteca, recordadas unos segundos.

    La cache vale solo para la `revision` con la que se conto: si otro hilo
    escribe (y llama a `_touch`) mientras se cuenta, lo contado ya nace viejo
    y no se guarda. Antes se guardaba igual y durante cinco segundos se
    enseñaba un total de antes del cambio.
    """
    key = str(config.DATABASE)
    with _stats_lock:
        cached = dict(_stats_cache)
        rev = _REVISION["n"]
    if (
        cached["value"] is not None
        and cached["db"] == key
        and cached["rev"] == rev
        and time.monotonic() - cached["at"] < _STATS_TTL
    ):
        return dict(cached["value"])
    with connect() as conn:
        f = conn.execute(
            "SELECT COUNT(*) n, SUM(size) bytes, SUM(duration) seconds, "
            "SUM(analyzed>0) analyzed_count, SUM(lyrics!='') with_lyrics, "
            "SUM(artist='') without_artist FROM songs"
        ).fetchone()
    value = {
        "total": f["n"] or 0,
        "bytes": f["bytes"] or 0,
        "seconds": f["seconds"] or 0,
        "analyzed_count": f["analyzed_count"] or 0,
        "with_lyrics": f["with_lyrics"] or 0,
        "without_artist": f["without_artist"] or 0,
    }
    with _stats_lock:
        if _REVISION["n"] == rev:
            _stats_cache.update(at=time.monotonic(), db=key, rev=rev, value=value)
    return dict(value)
