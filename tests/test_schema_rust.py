"""El esquema y las migraciones en Rust (library/src/index/schema.rs)
contra los de Python (danplay/library/db.py): partiendo de la misma base
(nueva, de antes de las versiones, en castellano, de la version 3), las dos
acaban con las mismas tablas, indices, triggers, version y datos."""

import shutil
import sqlite3

import danplay_core as R
import pytest

from danplay import config, library
from danplay.library import db


def _dump(path) -> dict:
    conn = sqlite3.connect(path)
    try:
        schema = sorted(
            tuple(r)
            for r in conn.execute(
                "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'"
            )
        )
        version = conn.execute("PRAGMA user_version").fetchone()[0]
        data = {}
        for (name,) in conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'search_index%'"
        ):
            data[name] = sorted(map(repr, conn.execute(f"SELECT * FROM {name}")))  # noqa: S608
        found = conn.execute(
            "SELECT rowid FROM search_index WHERE search_index MATCH 'gozo'"
        ).fetchall()
        return {"schema": schema, "version": version, "data": data, "fts": found}
    finally:
        conn.close()


def _both(tmp_path, monkeypatch, build) -> tuple[dict, dict]:
    py, rs = tmp_path / "python.db", tmp_path / "rust.db"
    build(py)
    shutil.copy(py, rs)
    monkeypatch.setattr(config, "DATABASE", py)
    library._prepared.discard(str(py))
    library.connect().close()
    library._prepared.discard(str(py))
    R._compat("db_prepare", [str(rs)])
    a, b = _dump(py), _dump(rs)
    # meta.song_id_hwm y lo demas, igual; library_uid no lo pone nadie aqui
    return a, b


def _empty(path):
    sqlite3.connect(path).close()


def _before_versions(path):
    conn = sqlite3.connect(path)
    old_cols = [c for c in library.COLUMNS if c != "blur"]
    conn.execute(
        "CREATE TABLE songs (id INTEGER PRIMARY KEY, "
        + ", ".join(f"{c} TEXT" for c in old_cols)
        + ", chords TEXT, analyzed REAL)"
    )
    conn.execute("INSERT INTO songs (id, path, title) VALUES (7, '/x/uno.mp3', 'Mi Gozo')")
    conn.execute(
        "CREATE TABLE playlist_songs (playlist_id INTEGER, song_id INTEGER, "
        "position INTEGER, added REAL, PRIMARY KEY (playlist_id, song_id))"
    )
    conn.execute("INSERT INTO playlist_songs VALUES (1, 40, 0, 0)")
    conn.commit()
    conn.close()


def _spanish(path):
    conn = sqlite3.connect(path)
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
    conn.execute("INSERT INTO carpetas VALUES ('/m', 'Musica', 'biblioteca', 1, 0)")
    conn.execute("CREATE TABLE exclusiones (patron TEXT PRIMARY KEY, tipo TEXT, nota TEXT)")
    conn.execute("INSERT INTO exclusiones VALUES ('/m/x', 'ruta', '')")
    conn.commit()
    conn.close()


def _version_3(path):
    """La de la 1.15: el esquema de entonces, sin la columna de las pistas."""
    lines = db.SCHEMA.splitlines(keepends=True)
    at = next(i for i, line in enumerate(lines) if line.lstrip().startswith("stems "))
    lines[at - 1] = lines[at - 1].replace("DEFAULT '',", "DEFAULT '' ", 1)
    old = "".join(lines[:at] + lines[at + 1 :])
    conn = sqlite3.connect(path)
    conn.executescript(old)
    conn.execute("INSERT INTO songs (id, path, title) VALUES (5, '/m/x.mp3', 'Gozo')")
    conn.execute("PRAGMA user_version = 3")
    conn.commit()
    conn.close()


@pytest.mark.parametrize("build", [_empty, _before_versions, _spanish, _version_3])
def test_rust_leaves_the_db_like_python(tmp_path, monkeypatch, build):
    a, b = _both(tmp_path, monkeypatch, build)
    assert a["version"] == b["version"] == library.SCHEMA_VERSION == R._compat("schema_version", [])
    assert a["schema"] == b["schema"]
    assert a["data"] == b["data"]
    assert a["fts"] == b["fts"]


def test_an_up_to_date_db_does_not_change(tmp_path):
    path = tmp_path / "al-dia.db"
    assert R._compat("db_prepare", [str(path)]) == []
    before = _dump(path)
    assert R._compat("db_prepare", [str(path)]) == []
    assert _dump(path) == before


def test_a_db_from_a_newer_danplay_is_left_alone(tmp_path):
    path = tmp_path / "nueva.db"
    R._compat("db_prepare", [str(path)])
    conn = sqlite3.connect(path)
    conn.execute("PRAGMA user_version = 999")
    conn.close()
    assert R._compat("db_prepare", [str(path)]) == []
    assert _dump(path)["version"] == 999
