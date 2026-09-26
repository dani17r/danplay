"""El escaneo: poner el indice al dia con lo que hay en el disco.

Incremental y por ruta; lo que se va se aparta (`songs_missing`) y, si
vuelve, recupera su id. Ver docs/ARQUITECTURA.md, «La biblioteca sigue al
disco».
"""

import json
import logging
import os
import sqlite3
import time
from pathlib import Path

from .. import config, names, tags
from . import db
from .db import (
    _INSERT_SQL,
    _SCAN_LOCK,
    _UPDATE_SQL,
    COLUMNS,
    SCAN_BATCH,
    _has_table,
    _insert_row,
    _next_song_id,
    _song_columns,
    _to_missing,
    _touch,
)
from .folders import _inside, _roots, audio_files, list_exclusions

log = logging.getLogger(__name__)


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
    title = tag.get("title", "")

    # los nombres ya siguen "Artista - Titulo": si el prefijo coincide, quitalo
    if artist and not title:
        prefix = artist.lower() + " - "
        title = stem[len(prefix) :] if stem.lower().startswith(prefix) else stem
    named_feat = ""
    if not artist:
        d = names.detect_artist(file, vocab or {})
        if d["confidence"] >= 0.80:
            artist, title = d["artist"], title or d["title"]
        elif named := names.split_artist_title(file):
            # Ni etiqueta, ni carpeta de artista, ni uno que ya se conozca: el
            # propio nombre, si sigue la convencion de la casa «Artista -
            # Titulo». Solo en el indice: el archivo no se toca ni se mueve.
            artist, title, named_feat = named["artist"], title or named["title"], named["feat"]
        else:
            title = title or d["title"]
    title = title or stem

    title, feat = names.extract_feat(title)
    if named_feat and named_feat != feat:
        feat = ", ".join(x for x in (feat, named_feat) if x)
    return (
        path,
        root,
        os.path.relpath(os.path.dirname(path), root),
        file,
        artist,
        title.strip(" -"),
        tag.get("album", ""),
        tag.get("year", ""),
        tag.get("genre", ""),
        feat,
        names.match_key(file),
        tag.get("duration", 0.0),
        tag.get("bitrate", 0),
        st.st_size,
        st.st_mtime,
        tag.get("key", ""),
        tag.get("bpm", 0.0),
        "embedded" if tag.get("cover") else "",
        tag.get("lyrics", ""),
        int(tag.get("stars", 0) or 0),
        1 if tag.get("favorite") else 0,
        1 if tag.get("blur") else 0,
    )


# Cuanto se guarda una cancion apartada esperando a que su archivo vuelva.
MISSING_DAYS = 30


# Si el archivo que vuelve no trae estos datos (etiquetas desactivadas, un
# formato donde no se escriben) pero el indice si los tenia, se quedan los del
# indice: son cosas que puso la persona y no deben perderse por un traslado.
_KEEP_IF_EMPTY = ("stars", "favorite", "blur", "key", "bpm", "lyrics")


def _find_missing(conn, path, file, size, mtime):
    """La cancion apartada que es este archivo, si la hay.

    Primero la que estaba en esta misma ruta (un disco que se vuelve a montar,
    algo que se restaura de la papelera). Si no, la del mismo tamaño y con el
    mismo nombre o la misma fecha: el mismo archivo movido de carpeta, o
    renombrado en el sitio (mover y renombrar no cambian la fecha).
    """
    r = conn.execute(
        "SELECT * FROM songs_missing WHERE path=? ORDER BY gone_at DESC LIMIT 1", (path,)
    ).fetchone()
    if r is not None:
        return r
    return conn.execute(
        "SELECT * FROM songs_missing WHERE size=? AND (file=? OR mtime=?) "
        "ORDER BY (file=?) DESC, (mtime=?) DESC, gone_at DESC LIMIT 1",
        (size, file, mtime, file, mtime),
    ).fetchone()


def _add(conn, row: tuple) -> tuple[str, int]:
    """Mete en el indice un archivo que no estaba. Devuelve ("added"|"back", id).

    Si es una cancion apartada que vuelve (a su sitio o a otro), recupera su
    id y lo que solo guarda la base (acordes, analisis, estudio, letra con
    tiempos); lo del archivo se toma del archivo. Si no, id nuevo. No hace
    commit.
    """
    fresh = dict(zip(COLUMNS, row, strict=True))
    old = _find_missing(conn, fresh["path"], fresh["file"], fresh["size"], fresh["mtime"])
    if old is None:
        cid = _next_song_id(conn)
        conn.execute(_INSERT_SQL, (cid, *row))
        return "added", cid
    data = json.loads(old["data"])
    merged = {**data, **fresh}
    for k in _KEEP_IF_EMPTY:
        if not fresh.get(k) and data.get(k):
            merged[k] = data[k]
    merged["id"] = old["id"]
    _insert_row(conn, merged)
    conn.execute("DELETE FROM songs_missing WHERE id=?", (old["id"],))
    return "back", old["id"]


def _back_in_place(conn, path, st, columns) -> bool:
    """Una cancion apartada que vuelve EXACTAMENTE como se fue (misma ruta,
    tamaño y fecha): un disco que se vuelve a montar. Entra tal cual estaba,
    sin releer el archivo; con toda una biblioteca, releerla costaba minutos."""
    r = conn.execute(
        "SELECT * FROM songs_missing WHERE path=? AND size=? AND mtime=? "
        "ORDER BY gone_at DESC LIMIT 1",
        (path, st.st_size, st.st_mtime),
    ).fetchone()
    if r is None:
        return False
    data = json.loads(r["data"])
    data["id"] = r["id"]
    _insert_row(conn, data, columns)
    conn.execute("DELETE FROM songs_missing WHERE id=?", (r["id"],))
    return True


def _purge_missing(conn, days=None) -> int:
    """Olvida del todo las canciones apartadas hace mas de `days` dias, y sus
    filas en las listas y en los recientes. Devuelve cuantas."""
    days = MISSING_DAYS if days is None else days
    cutoff = time.time() - days * 86400
    ids = [r[0] for r in conn.execute("SELECT id FROM songs_missing WHERE gone_at < ?", (cutoff,))]
    if not ids:
        return 0
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    for i in range(0, len(ids), SCAN_BATCH):
        chunk = ids[i : i + SCAN_BATCH]
        marks = ",".join("?" * len(chunk))
        conn.execute(f"DELETE FROM songs_missing WHERE id IN ({marks})", chunk)  # noqa: S608
        for table in ("playlist_songs", "recent"):
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

    `progress(hechos, total)` cuenta el avance (el trabajo «escaneo»).

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
        columns = _song_columns(conn)
        existing = {
            r["path"]: r for r in conn.execute("SELECT id, path, artist, mtime, size FROM songs")
        }
        vocab = names.vocabulary(config.ARTISTS_DIR)
        exclusions = list_exclusions()

        # Primero, que hay en el disco; luego se aparta lo que ya no esta; y solo
        # despues se mete lo nuevo. En ese orden, un archivo que simplemente se ha
        # movido de carpeta encuentra su fila ya apartada y recupera el id en esta
        # misma pasada, en vez de entrar como cancion nueva.
        present = []
        stems_found: dict = {}
        for root in _roots():
            if os.path.isdir(root):
                present.extend((path, root) for path in audio_files(root, exclusions, stems_found))
        on_disk = {path for path, _ in present}
        total = len(present)
        # lo que ya no esta (o quedo fuera de las carpetas activas). Sin commit
        # aqui: se confirma con la primera tanda, junto con lo que vuelve, asi
        # que quien lea mientras tanto no ve desaparecer una cancion que solo
        # cambio de carpeta (en Windows eso llega como borrar + crear).
        gone = [p for p in existing if p not in on_disk]
        removed = _to_missing(conn, gone)

        seen: set = set()
        playlists_found: dict = {}
        study_found: dict = {}
        n = added_count = reused = updated = back = pending = 0
        changed = removed > 0
        for path, root in present:
            v = existing.get(path)
            # Si el archivo no se ha tocado desde el ultimo escaneo, sus
            # etiquetas no pueden haber cambiado: se deja la fila como esta en
            # vez de volver a abrirlo y parsearlo. Es lo que hace que un
            # reescaneo sea casi instantaneo. Escribir etiquetas (estrellas,
            # favorito, un titulo corregido) cambia la fecha del archivo, asi
            # que eso siempre se relee. Los que no tienen artista tambien:
            # pueden resolverse ahora que hay mas carpetas de artista.
            if v is None or v["artist"]:
                try:
                    st = os.stat(path)
                except OSError:
                    continue  # se fue mientras tanto: el siguiente la aparta
                if v is not None and st.st_mtime == v["mtime"] and st.st_size == v["size"]:
                    seen.add(path)
                    reused += 1
                    n += 1
                    if progress and n % 50 == 0:
                        progress(n, total)
                    continue
                if v is None and _back_in_place(conn, path, st, columns):
                    seen.add(path)
                    back += 1
                    n += 1
                    pending += 1
                    changed = True
                    if pending >= SCAN_BATCH:
                        conn.commit()
                        pending = 0
                    if progress and n % 50 == 0:
                        progress(n, total)
                    continue
            try:
                tag = tags.read_all(path)
                row = _row(path, root, vocab, tag)
            except OSError:
                log.warning("no se pudo indexar %s", path, exc_info=True)
                continue
            try:
                if v is None:
                    kind, _ = _add(conn, row)
                    if kind == "back":
                        back += 1
                    else:
                        added_count += 1
                    changed = True
                else:
                    before = conn.execute("SELECT * FROM songs WHERE id=?", (v["id"],)).fetchone()
                    conn.execute(_UPDATE_SQL, (*row, v["id"]))
                    updated += 1
                    # releer un archivo no es cambiarlo: si solo se movio la fecha
                    # (la app acaba de escribirle una estrella), no hay que avisar
                    if before is None or any(
                        before[c] != row[i]
                        for i, c in enumerate(COLUMNS)
                        if c not in ("mtime", "size")
                    ):
                        changed = True
            except sqlite3.Error:
                # un archivo que otro camino (una descarga) acaba de indexar a la
                # vez, o una ruta que SQLite no acepta: se salta, no se para todo
                log.warning("no se pudo indexar %s", path, exc_info=True)
                continue
            seen.add(path)
            if tag.get("playlists"):
                playlists_found[path] = tag["playlists"]
            if tag.get("study"):
                study_found[path] = tag["study"]
            n += 1
            pending += 1
            if pending >= SCAN_BATCH:
                conn.commit()
                pending = 0
            if progress and n % 50 == 0:
                progress(n, total)
        conn.commit()
        if progress:
            progress(n, total)
        _purge_missing(conn)
        # las canciones abiertas desde fuera tambien tienen su onda y su caratula
        outside = (
            [r[0] for r in conn.execute("SELECT path FROM external_songs")]
            if _has_table(conn, "external_songs")
            else []
        )
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
    if changed or restored or relinked:
        _touch()
    return {
        "total": n,
        "added_count": added_count,
        "reused": reused,
        "updated": updated,
        "removed": removed,
        "back": back,
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
        n = 0
        for path, study in found.items():
            cur = conn.execute(
                "UPDATE songs SET study=? WHERE path=? AND (study IS NULL OR study='')",
                (study, path),
            )
            n += cur.rowcount or 0
    return n


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
            for r in conn.execute("SELECT path FROM songs").fetchall():
                if os.path.exists(r["path"]):
                    lists = tags.read_all(r["path"]).get("playlists") or []
                    if lists:
                        found[r["path"]] = lists
        if not found:
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
                    cur = conn.execute(
                        "INSERT INTO playlists (name,note,color,created) VALUES (?,?,?,?)",
                        (name, "", "", now),
                    )
                    lid = ids[name] = cur.lastrowid
                position = conn.execute(
                    "SELECT COALESCE(MAX(position),-1)+1 FROM playlist_songs WHERE playlist_id=?",
                    (lid,),
                ).fetchone()[0]
                cur = conn.execute(
                    "INSERT OR IGNORE INTO playlist_songs (playlist_id,song_id,position,added) "
                    "VALUES (?,?,?,?)",
                    (lid, song["id"], position, now),
                )
                added += cur.rowcount if cur.rowcount and cur.rowcount > 0 else 0
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
    root = next((r for r in roots if _inside(path, r)), None)
    if root is None:
        return None  # fuera de las carpetas gestionadas
    try:
        row = _row(path, root, names.vocabulary(config.ARTISTS_DIR) if vocab is None else vocab)
    except OSError:
        log.warning("no se pudo indexar %s", path, exc_info=True)
        return None
    with db.connect() as conn:
        try:
            # si ya estaba, se actualiza en sitio: un INSERT OR REPLACE le daria un
            # id nuevo y las listas que lo tuvieran lo perderian
            old = conn.execute("SELECT id FROM songs WHERE path=?", (path,)).fetchone()
            if old:
                conn.execute(_UPDATE_SQL, (*row, old["id"]))
            else:
                # una cancion apartada que vuelve (restaurada de la papelera, la
                # copia que queda al resolver duplicados) recupera su id
                _add(conn, row)
            conn.commit()
        except sqlite3.IntegrityError:
            # el vigilante la acaba de indexar a la vez: vale la suya
            conn.rollback()
        r = conn.execute("SELECT * FROM songs WHERE path=?", (path,)).fetchone()
    _touch()
    return dict(r) if r else None
