"""El esquema del indice lleva su version (`PRAGMA user_version`) y cada
cambio es una migracion que se aplica una sola vez: una base nueva, una de
antes de las versiones y una del esquema en castellano acaban igual."""

import sqlite3

import pytest

from danplay import config, library
from danplay.library import db


@pytest.fixture
def base(tmp_path, monkeypatch):
    path = tmp_path / "indice.db"
    monkeypatch.setattr(config, "DATABASE", path)
    library._prepared.discard(str(path))
    yield path
    library._prepared.discard(str(path))


def _version(path) -> int:
    conn = sqlite3.connect(path)
    try:
        return conn.execute("PRAGMA user_version").fetchone()[0]
    finally:
        conn.close()


def _columns(path, table) -> set:
    conn = sqlite3.connect(path)
    try:
        return {r[1] for r in conn.execute(f"PRAGMA table_info({table})")}
    finally:
        conn.close()


def test_una_base_nueva_nace_con_la_ultima_version(base):
    with library.connect() as conn:
        assert conn.execute("SELECT COUNT(*) FROM songs").fetchone()[0] == 0
        tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    assert {
        "songs",
        "folders",
        "playlists",
        "playlist_songs",
        "external_songs",
        "recent",
        "chats",
        "chat_messages",
        "downloads",
        "ai_usage",
        "songs_missing",
    } <= tables
    assert _version(base) == library.SCHEMA_VERSION == db.MIGRATIONS[-1][0]


def test_una_base_de_antes_de_las_versiones_se_pone_al_dia(base):
    """Una base de las primeras (sin blur, sin letra con tiempos, sin estudio
    y sin tope de ids) y con una cancion borrada que sigue en una lista."""
    conn = sqlite3.connect(base)
    old_cols = [c for c in library.COLUMNS if c != "blur"]
    conn.execute(
        "CREATE TABLE songs (id INTEGER PRIMARY KEY, "
        + ", ".join(f"{c} TEXT" for c in old_cols)
        + ", chords TEXT, analyzed REAL)"
    )
    conn.execute("INSERT INTO songs (id, path) VALUES (7, '/x/uno.mp3')")
    conn.execute(
        "CREATE TABLE playlist_songs (playlist_id INTEGER, song_id INTEGER, "
        "position INTEGER, added REAL, PRIMARY KEY (playlist_id, song_id))"
    )
    conn.execute("INSERT INTO playlist_songs VALUES (1, 40, 0, 0)")
    conn.commit()
    conn.close()
    assert _version(base) == 0
    with library.connect() as conn:
        hwm = conn.execute("SELECT value FROM meta WHERE key='song_id_hwm'").fetchone()[0]
        assert conn.execute("SELECT COUNT(*) FROM songs").fetchone()[0] == 1
        assert db._next_song_id(conn) == 41, "el id de la lista no se reutiliza"
    assert hwm == 40
    assert {"blur", "lyrics_synced", "study"} <= _columns(base, "songs")
    assert _version(base) == library.SCHEMA_VERSION


def test_el_esquema_en_castellano_se_migra_entero(base):
    """Las listas vienen de otro modulo: antes solo se migraban si ese modulo
    ya habia abierto la base. Ahora estan todas antes de migrar."""
    conn = sqlite3.connect(base)
    conn.execute(
        "CREATE TABLE canciones (id INTEGER PRIMARY KEY, ruta TEXT, raiz TEXT, "
        "carpeta TEXT, archivo TEXT, artista TEXT, titulo TEXT, album TEXT, "
        "anio TEXT, genero TEXT, feat TEXT, clave TEXT, duracion REAL, bitrate "
        "INTEGER, tamanio INTEGER, mtime REAL, tono TEXT, bpm REAL, portada TEXT, "
        "letra TEXT, acordes TEXT, analizado REAL, estrellas INTEGER, favorito INTEGER)"
    )
    conn.execute(
        "INSERT INTO canciones (id, ruta, artista, titulo, estrellas) "
        "VALUES (3, '/m/Barak - Mi Gozo.mp3', 'Barak', 'Mi Gozo', 4)"
    )
    conn.execute(
        "CREATE TABLE listas (id INTEGER PRIMARY KEY, nombre TEXT, nota TEXT, "
        "color TEXT, creada REAL)"
    )
    conn.execute("INSERT INTO listas VALUES (1, 'Domingo', '', '', 0)")
    conn.execute(
        "CREATE TABLE lista_canciones (lista_id INTEGER, cancion_id INTEGER, orden INTEGER)"
    )
    conn.execute("INSERT INTO lista_canciones VALUES (1, 3, 0)")
    conn.execute(
        "CREATE TABLE carpetas (ruta TEXT PRIMARY KEY, etiqueta TEXT, rol TEXT, "
        "activa INTEGER, agregada REAL)"
    )
    conn.execute("INSERT INTO carpetas VALUES ('/m', 'm', 'biblioteca', 1, 0)")
    conn.commit()
    conn.close()
    with library.connect() as conn:
        song = conn.execute("SELECT * FROM songs WHERE id=3").fetchone()
        assert (song["artist"], song["title"], song["stars"]) == ("Barak", "Mi Gozo", 4)
        assert conn.execute("SELECT name FROM playlists WHERE id=1").fetchone()[0] == "Domingo"
        assert conn.execute("SELECT song_id FROM playlist_songs").fetchone()[0] == 3
        assert conn.execute("SELECT role FROM folders").fetchone()[0] == "library"
        found = conn.execute("SELECT rowid FROM search_index WHERE search_index MATCH 'gozo'")
        assert [r[0] for r in found] == [3], "el indice de busqueda se rehace"
        tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    assert not {"canciones", "listas", "lista_canciones", "carpetas"} & tables


def test_una_base_nueva_no_pasa_por_las_migraciones(base, monkeypatch):
    ran = []
    monkeypatch.setattr(
        db, "MIGRATIONS", tuple((n, w, lambda c, n=n: ran.append(n)) for n, w, _ in db.MIGRATIONS)
    )
    library.connect().close()
    assert ran == [] and _version(base) == library.SCHEMA_VERSION


def test_las_migraciones_se_aplican_una_sola_vez(base, monkeypatch):
    library.connect().close()
    calls = []
    monkeypatch.setattr(
        db,
        "MIGRATIONS",
        (
            (1, "a", lambda c: calls.append(1)),
            (2, "b", lambda c: calls.append(2)),
            (3, "c", lambda c: calls.append(3)),
            (4, "nueva", lambda c: calls.append(4)),
        ),
    )
    monkeypatch.setattr(db, "SCHEMA_VERSION", 4)
    library._prepared.discard(str(base))
    library.connect().close()
    assert calls == [4], "solo la que le faltaba"
    library._prepared.discard(str(base))
    library.connect().close()
    assert calls == [4]
    assert _version(base) == 4


def test_una_migracion_que_falla_no_deja_nada_a_medias(base, monkeypatch):
    library.connect().close()

    def half(conn):
        conn.execute("CREATE TABLE a_medias (x)")
        raise RuntimeError("se fue la luz")

    monkeypatch.setattr(db, "MIGRATIONS", (*db.MIGRATIONS, (99, "rota", half)))
    monkeypatch.setattr(db, "SCHEMA_VERSION", 99)
    library._prepared.discard(str(base))
    with pytest.raises(RuntimeError):
        library.connect()
    assert _version(base) == 3
    conn = sqlite3.connect(base)
    try:
        assert not conn.execute("SELECT 1 FROM sqlite_master WHERE name='a_medias'").fetchone()
    finally:
        conn.close()


def test_una_base_de_un_danplay_mas_nuevo_no_se_toca(base):
    library.connect().close()
    conn = sqlite3.connect(base)
    conn.execute("PRAGMA user_version = 999")
    conn.close()
    library._prepared.discard(str(base))
    library.connect().close()
    assert _version(base) == 999


def test_con_with_la_conexion_se_confirma_y_se_cierra(base):
    with library.connect() as conn:
        conn.execute("INSERT INTO meta (key, value) VALUES ('prueba', 1)")
    with pytest.raises(sqlite3.ProgrammingError):
        conn.execute("SELECT 1")
    with pytest.raises(ValueError), library.connect() as conn:
        conn.execute("INSERT INTO meta (key, value) VALUES ('deshecha', 1)")
        raise ValueError("algo fallo")
    with library.connect() as conn:
        keys = {r[0] for r in conn.execute("SELECT key FROM meta")}
    assert "prueba" in keys and "deshecha" not in keys


def test_nueva_o_migrada_acaban_con_las_mismas_columnas(base, tmp_path, monkeypatch):
    """El esquema de partida es el de ahora: una base nueva no pasa por las
    migraciones, asi que tiene que nacer con todo lo que ellas añaden."""
    library.connect().close()
    fresh = _columns(base, "songs")
    old = tmp_path / "vieja.db"
    conn = sqlite3.connect(old)
    conn.execute(
        "CREATE TABLE songs (id INTEGER PRIMARY KEY, "
        + ", ".join(f"{c} TEXT" for c in library.COLUMNS if c != "blur")
        + ", chords TEXT, analyzed REAL)"
    )
    conn.commit()
    conn.close()
    monkeypatch.setattr(config, "DATABASE", old)
    library.connect().close()
    library._prepared.discard(str(old))
    assert _columns(old, "songs") == fresh
