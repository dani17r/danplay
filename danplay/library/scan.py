"""El escaneo: poner el indice al dia con lo que hay en el disco.

Incremental y por ruta; lo que se va se aparta (`songs_missing`) y, si
vuelve, recupera su id. Ver docs/ARCHITECTURE.md, «La biblioteca sigue al
disco».

El recorrido y el indice los hace Rust (library/src/index/scan.rs:
leer las etiquetas por tandas en paralelo, meter, actualizar, recuperar ids);
aqui queda lo de despues, que aun es de Python: las pistas separadas, las
ondas y caratulas de lo que se fue, lo que se aprovecha de otro archivo de
la misma cancion y las listas que vuelven.
"""

import contextlib
import logging
import os
import time

import danplay_core as _rust

from .. import config, tags
from . import db
from .db import _SCAN_LOCK, SCAN_BATCH, _has_table, _touch
from .folders import _roots, _warn_unstorable

log = logging.getLogger(__name__)


# Cuanto se guarda una cancion apartada esperando a que su archivo vuelva.
MISSING_DAYS = 30

# Si se juntan las gemelas: un archivo nuevo que es una cancion apartada
# (mismo artista y titulo, misma duracion) recupera su id. Las pruebas lo
# apagan para ver como era antes.
_JOIN_TWINS = True


def _carry_grid(old: dict, fresh: dict) -> None:
    from types import SimpleNamespace

    from .. import beatgrid

    with contextlib.suppress(OSError, KeyError, TypeError, ValueError):
        before = SimpleNamespace(st_size=int(old["size"]), st_mtime=float(old["mtime"]))
        beatgrid.moved(old["path"], fresh["path"], before, os.stat(fresh["path"]))


def _purge_missing(conn, days=None) -> int:
    """Olvida del todo las canciones apartadas hace mas de `days` dias y sus
    recientes. En las listas se quedan, con su ficha (`playlists.keep_cards`):
    la lista sigue diciendo que estaba ahi («No está») y la recupera si vuelve.
    Devuelve cuantas."""
    days = MISSING_DAYS if days is None else days
    cutoff = time.time() - days * 86400
    ids = [r[0] for r in conn.execute("SELECT id FROM songs_missing WHERE gone_at < ?", (cutoff,))]
    if not ids:
        return 0
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if "song_cards" in tables:
        from .. import playlists as _playlists

        _playlists.keep_cards(conn, ids)
    for i in range(0, len(ids), SCAN_BATCH):
        chunk = ids[i : i + SCAN_BATCH]
        marks = ",".join("?" * len(chunk))
        conn.execute(f"DELETE FROM songs_missing WHERE id IN ({marks})", chunk)  # noqa: S608
        for table in ("recent",):
            if table in tables:
                conn.execute(
                    f"DELETE FROM {table} WHERE song_id IN ({marks}) "  # noqa: S608
                    f"AND song_id NOT IN (SELECT id FROM songs)",
                    chunk,
                )
    conn.commit()
    return len(ids)


def scan(progress=None) -> dict:
    """Pone el indice al dia con lo que hay en las carpetas gestionadas.

    `progress(done, total)` cuenta el avance (el trabajo «escaneo»).

    Incremental y por ruta: lo nuevo se inserta, lo que cambio se actualiza
    y lo que ya no esta en el disco se aparta (`_to_missing`). Los ids NO
    cambian nunca: ni al reescanear, ni cuando un archivo se mueve de carpeta
    o se renombra por fuera, ni cuando su carpeta entera se va y vuelve.
    Antes se vaciaba la tabla y se reinsertaba todo: los ids volvian a
    empezar en 1 en el orden del disco, y un archivo nuevo desplazaba a todos
    los siguientes, con lo que las listas (que guardan ids) pasaban a
    apuntar a otras canciones.

    Casi todo se relee del archivo, asi que el escaneo reconstruye el indice
    aunque la base se pierda; de la base se conservan los acordes y la marca
    de analizado (las columnas que no estan en COLUMNS no se tocan). Las
    listas se recrean al final desde la etiqueta LISTAS de cada archivo.

    No se reconstruye el indice de busqueda: los triggers lo mantienen fila
    a fila, y solo para las filas que de verdad cambian. Y solo se avisa a la
    interfaz (`_touch`) si algo de lo que enseña ha cambiado: el vigilante
    escanea cada vez que se toca un archivo, y la mayoria no cambia nada.
    """
    with _SCAN_LOCK:
        return _scan(progress)


def _scan(progress=None) -> dict:
    with db.connect() as conn:
        r = _rust.scan_index(
            conn, config.ARTISTS_DIR, sorted(config.EXTENSIONS), _JOIN_TWINS, progress
        )
        for why in r["warnings"]:
            log.warning(why)
        for manifest in r["unreadable"]:
            log.warning("no se pudo leer %s", manifest)
        for path in r["unstorable"]:
            _warn_unstorable(path)
        # el mismo archivo en otro sitio: su clic sigue valiendo (sin esto se
        # volvia a calcular, 15-20 s por cancion, al mover la carpeta)
        for old, new_path in r["carry"]:
            _carry_grid(old, {"path": new_path})
        _purge_missing(conn)
        # las canciones abiertas desde fuera tambien tienen su onda y su caratula
        outside = (
            [row[0] for row in conn.execute("SELECT path FROM external_songs")]
            if _has_table(conn, "external_songs")
            else []
        )
    seen, gone, stems_found = r["seen"], r["gone"], r["stems"]
    playlists_found, study_found, settle = r["playlists"], r["study"], r["settle"]
    n, changed = r["total"], r["changed"]
    # Con la fila se va el estudio (bucle, marcadores, notas: es una columna).
    # La forma de onda y las miniaturas de la caratula viven en disco, aparte:
    # se borran las de cada cancion que ya no esta, y de paso las que se
    # hubieran quedado huerfanas por otro camino. Si no, esas carpetas crecian
    # con canciones que ya no existen.
    from .. import stems as _stems
    from .. import thumbnails as _thumbnails
    from .. import waveform as _waveform

    # las pistas separadas, con su cancion (una base nueva no lo sabe)
    relinked = _stems.relink(stems_found)
    known = set(seen) | set(outside) | _stems.stem_paths()
    _waveform.forget(*gone)
    _waveform.prune(known)
    _thumbnails.forget(*gone)
    _thumbnails.prune(known)
    restored = restore_playlists_from_tags(playlists_found)
    restore_study_from_tags(study_found)
    from .. import reuse as _reuse

    # las que volvieron en otro archivo, siempre; las nuevas, solo si tienen
    # gemela (en una biblioteca nueva, ninguna: nada que hacer)
    for cid, before in settle:
        if before is not None:
            _reuse.later(cid, before)
    arrived = [cid for cid, before in settle if before is None]
    fresh = _reuse.with_twins(arrived)
    for cid in fresh:
        _reuse.later(cid)
    # y las listas que la tenian como «No está» (se borro, paso el mes) la
    # recuperan en su sitio
    if arrived:
        from .. import playlists as _playlists

        if _playlists.relink_ghosts(arrived):
            changed = True
    if changed or restored or relinked:
        _touch()
    return {
        "total": n,
        "added_count": r["added_count"],
        "reused": r["reused"],
        "updated": r["updated"],
        "removed": r["removed"],
        "back": r["back"],
        "changed": bool(changed or restored),
        "playlists_restored": restored,
    }


def restore_study_from_tags(found: dict) -> int:
    """Lo del modo estudio que traen los archivos, para las filas que no lo
    tienen en el indice (una base perdida, un archivo que llega de otro
    equipo). Nunca pisa lo que ya hay: la base puede ir por delante."""
    if not found:
        return 0
    with db.connect() as conn:
        return _rust.scan_restore_study(conn, list(found.items()))


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
    from .. import playlists as _playlists  # noqa: F401  (registra su esquema)

    with db.connect() as conn:
        if found is None:
            found = {}
            for row in conn.execute("SELECT path FROM songs").fetchall():
                if os.path.exists(row["path"]):
                    lists = tags.read_all(row["path"]).get("playlists") or []
                    if lists:
                        found[row["path"]] = lists
        if not found:
            return 0
        added = _rust.scan_restore_playlists(conn, [(p, list(v)) for p, v in found.items()])
    if added:
        log.info("listas recuperadas desde las etiquetas: %d pertenencias", added)
    return added


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
    with db.connect() as conn:
        r = _rust.scan_index_file(
            conn,
            path,
            list(roots),
            config.ARTISTS_DIR,
            None if vocab is None else list(vocab.items()),
        )
    if r is None:
        return None  # fuera de las carpetas gestionadas
    _touch()
    if r["carry"]:
        old, new_path = r["carry"]
        _carry_grid(old, {"path": new_path})
    arrived = r["arrived"]
    if arrived:
        # se volvio a bajar una que ya tenia sus pistas o su clic: se aprovechan
        from .. import playlists, reuse

        if arrived[1] is not None or reuse.with_twins([arrived[0]]):
            reuse.later(*arrived)
        # y vuelve a las listas donde estaba como «No está»
        if arrived[1] is None:
            playlists.relink_ghosts([arrived[0]])
    return r["row"]
