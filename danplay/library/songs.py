"""Una cancion del indice: leerla, cambiarla (en el indice y en su archivo),
mandarla a la papelera o sacarla del indice."""

import logging
import os
import shutil
import subprocess
import sys
from typing import cast

from .. import config, tags
from . import db
from .db import SCAN_BATCH, Song, _to_missing, _touch

log = logging.getLogger(__name__)


def by_id(cid: int) -> Song | None:
    """La cancion de la biblioteca con ese id (la fila entera), o None."""
    with db.connect() as conn:
        f = conn.execute("SELECT * FROM songs WHERE id=?", (cid,)).fetchone()
    return cast(Song, dict(f)) if f else None


def update(cid: int, **fields) -> int:
    """Cambia columnas del indice (solo el indice). Devuelve filas tocadas."""
    allowed = {
        "artist",
        "title",
        "album",
        "year",
        "genre",
        "feat",
        "key",
        "bpm",
        "cover",
        "lyrics",
        "lyrics_synced",
        "chords",
        "analyzed",
        "stars",
        "favorite",
        "blur",
        "study",
    }
    fields = {k: v for k, v in fields.items() if k in allowed}
    if not fields:
        return 0
    with db.connect() as conn:
        cur = conn.execute(
            f"UPDATE songs SET {','.join(f'{k}=?' for k in fields)} WHERE id=?",  # noqa: S608
            [*list(fields.values()), cid],
        )
    _touch()
    return cur.rowcount or 0


_METADATA_FIELDS = ("artist", "title", "album", "year", "genre")


def edit(cid: int, **fields) -> Song | None:
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
            written = (
                tags.write(
                    c["path"],
                    artist=fields.get("artist", c["artist"]),
                    title=fields.get("title", c["title"]),
                    album=fields.get("album", c["album"]),
                    year=str(fields.get("year", c["year"])),
                    genre=fields.get("genre", c["genre"]),
                )
                and written
            )
        if "key" in fields or "bpm" in fields:
            written = (
                tags.write_analysis(
                    c["path"], fields.get("key", c["key"]), fields.get("bpm", c["bpm"])
                )
                and written
            )
        # la letra vive en el USLT del propio archivo, no solo en el indice
        if "lyrics" in fields:
            written = tags.write_lyrics(c["path"], fields["lyrics"] or "") and written
    out = by_id(cid)
    if out is not None:
        out["tags_written"] = bool(written)
    return out


def set_blur(cid: int, value=True) -> Song | None:
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
    except Exception as e:
        log.warning("send2trash no pudo con %s", path, exc_info=True)
        reason = str(e)[:120]
    if sys.platform.startswith("linux") and shutil.which("gio"):
        r = subprocess.run(["gio", "trash", path], capture_output=True, text=True, check=False)
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
    return {
        "ok": True,
        "trashed": True,
        "path": path,
        "name": os.path.basename(path),
        "artist": c["artist"],
        "title": c["title"],
    }


def forget(cid: int) -> None:
    """Quita la cancion del indice. No toca el archivo.

    Se aparta, no se borra (`_to_missing`): si el archivo vuelve —se restaura
    de la papelera— recupera su id y sus listas. La forma de onda cacheada se
    borra aqui.
    """
    from .. import thumbnails as _thumbnails
    from .. import waveform as _waveform

    with db.connect() as conn:
        row = conn.execute("SELECT path FROM songs WHERE id=?", (cid,)).fetchone()
        if row:
            _to_missing(conn, [row["path"]])
    if row:
        _waveform.forget(row["path"])
        _thumbnails.forget(row["path"])
    _touch()


def forget_path(path: str) -> int:
    """Saca del indice el archivo que estaba en esa ruta. No toca el disco.

    Para cuando el archivo ya no esta ahi (se borro o se renombro) y solo hay
    que ponerse al dia. Se aparta, como en `forget`. Devuelve cuantas filas
    se quitaron.
    """
    from .. import thumbnails as _thumbnails
    from .. import waveform as _waveform

    with db.connect() as conn:
        n = _to_missing(conn, [os.path.abspath(path)])
    _waveform.forget(os.path.abspath(path))
    _thumbnails.forget(os.path.abspath(path))
    _touch()
    return n


def paths_of(ids) -> dict[int, str | None]:
    """Donde esta ahora cada una de esas canciones de la biblioteca, o None si
    su archivo no esta (ya no es del indice, o se fue y aun no se escaneo).

    Lo usa la cola de Rust para ponerse al dia de una vez cuando cambia la
    biblioteca: una cancion movida sigue sonando desde su sitio nuevo, y la
    que ya no esta sale de la cola.
    """
    ids = [int(i) for i in ids if int(i) > 0]
    out: dict[int, str | None] = dict.fromkeys(ids)
    if not ids:
        return out
    with db.connect() as conn:
        for i in range(0, len(ids), SCAN_BATCH):
            chunk = ids[i : i + SCAN_BATCH]
            marks = ",".join("?" * len(chunk))
            for r in conn.execute(f"SELECT id, path FROM songs WHERE id IN ({marks})", chunk):  # noqa: S608
                out[r["id"]] = r["path"] if os.path.isfile(r["path"]) else None
    return out


# Lo que se guarda del modo estudio y sus limites: un JSON pequeño y con
# forma conocida, no lo que mande cualquiera.
STUDY_KEYS = ("loop", "speed", "pitch", "metronome", "markers", "notes")


def set_study(cid: int, study: dict | None) -> Song | None:
    """Guarda el modo estudio de una cancion (indice y etiqueta del archivo).
    Con None o vacio, lo quita. Devuelve la ficha."""
    import json as _json

    c = by_id(cid)
    if not c:
        return None
    clean: dict = {}
    if study:
        loop = study.get("loop")
        if isinstance(loop, (list, tuple)) and len(loop) == 2:
            try:
                a, b = float(loop[0]), float(loop[1])
                if 0 <= a < b:
                    clean["loop"] = [round(a, 2), round(b, 2)]
            except (TypeError, ValueError):
                pass
        try:
            speed = float(study.get("speed") or 1.0)
            if 0.25 <= speed <= 3.0 and abs(speed - 1.0) > 1e-3:
                clean["speed"] = round(speed, 2)
        except (TypeError, ValueError):
            pass
        # el tono corrido, en semitonos; 0 no se guarda
        try:
            pitch = int(study.get("pitch") or 0)
            if pitch and -12 <= pitch <= 12:
                clean["pitch"] = pitch
        except (TypeError, ValueError):
            pass
        # el metronomo: solo lo que uno ajusto a mano sobre lo detectado
        metro = study.get("metronome")
        if isinstance(metro, dict):
            m: dict = {}
            try:
                if metro.get("bpm") is not None:
                    bpm = float(metro["bpm"])
                    if 20 <= bpm <= 300:
                        m["bpm"] = round(bpm, 1)
            except (TypeError, ValueError):
                pass
            if metro.get("meter") in (3, 4):
                m["meter"] = int(metro["meter"])
            try:
                shift = int(metro.get("shift") or 0)
                if shift:
                    m["shift"] = max(-12, min(12, shift))
            except (TypeError, ValueError):
                pass
            if metro.get("mult") in (-1, 1):
                m["mult"] = int(metro["mult"])
            if m:
                clean["metronome"] = m
        # Un marcador es un tramo con nombre ({t, end, label}) y, si se quiere,
        # sus notas; sin `end` es un instante suelto. Un `end` que no vaya
        # detras de `t` se descarta y el marcador queda como instante.
        markers = []
        for m in (study.get("markers") or [])[:50]:
            if not isinstance(m, dict) or m.get("t") is None:
                continue
            try:
                t = float(m["t"])
            except (TypeError, ValueError):
                continue
            if t < 0:
                continue
            item = {"t": round(t, 2), "label": str(m.get("label") or "").strip()[:60]}
            raw_end = m.get("end")
            try:
                end = float(raw_end) if raw_end is not None else None
            except (TypeError, ValueError):
                end = None
            if end is not None and end > t:
                item["end"] = round(end, 2)
            notes = str(m.get("notes") or "").strip()[:2000]
            if notes:
                item["notes"] = notes
            markers.append(item)
        if markers:
            clean["markers"] = sorted(markers, key=lambda m: m["t"])
        notes = str(study.get("notes") or "").strip()[:4000]
        if notes:
            clean["notes"] = notes
    raw = _json.dumps(clean, ensure_ascii=False) if clean else ""
    update(cid, study=raw)
    if config.WRITE_TAGS and os.path.exists(c["path"]):
        tags.write_study(c["path"], raw)
    return by_id(cid)
