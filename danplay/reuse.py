"""Aprovechar lo ya hecho de una cancion que vuelve en otro archivo.

Pasaba asi: se cambia la carpeta de musica de sitio, o se vuelve a bajar una
cancion que ya estaba, y DanPlay la trataba como nueva: separar las pistas
otra vez (minutos), el clic otra vez (15-20 s), sin sus marcadores ni sus
guias. Lo hecho seguia en el disco, atado al archivo de antes.

Ahora, cuando entra una cancion, se busca su «gemela»: la misma (artista y
titulo, o el nombre del archivo, normalizados con `names.match_key`) y con la
misma duracion (dos segundos arriba o abajo), en la biblioteca, en las
apartadas (las de una carpeta que se movio) o en los manifiestos de las
pistas separadas. Si la hay, se mide cuanto esta corrido un archivo respecto
al otro (otra descarga puede traer unos milisegundos, o unos segundos de
intro, de mas o de menos) comparando como suenan, y se adopta lo suyo ya en
su sitio: las pistas separadas, la rejilla del clic y lo del modo estudio
(tramos, marcadores, guias, el clic propio). Si no suenan igual (otra
version con el mismo nombre), no se adopta nada.
"""

import contextlib
import json
import logging
import os
import shutil
import subprocess
import threading
import time
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import danplay_core as _rust

from . import config, convert

log = logging.getLogger(__name__)

# Lo que puede diferir la duracion de dos archivos de la misma cancion.
DURATION_TOL = 2.0
# Lo que se compara para medir el desfase (lo de delante de la cancion).
WINDOW = 90.0
# El desfase mas grande que se busca (una intro de mas o de menos).
MAX_LAG = 12.0
# Por debajo de esto, el desfase no se corrige (no se oye).
SMALL_LAG = 0.003
# La correlacion minima para creer que suenan igual.
MIN_SCORE = 0.5
RATE = 8000


def ident(row) -> str:
    """Como se reconoce una cancion sin mirar el archivo: su artista y su
    titulo (o el nombre del archivo, si no los tiene), normalizados."""
    return _rust.scan_ident(dict(row))


def _close(a, b, tol=DURATION_TOL) -> bool:
    try:
        a, b = float(a or 0), float(b or 0)
    except (TypeError, ValueError):
        return False
    return a > 0 and b > 0 and abs(a - b) <= tol


def twins(song) -> list[dict]:
    """Las otras filas que son esta misma cancion: de la biblioteca y de las
    apartadas (con `missing`: True), de la duracion mas parecida a la menos.
    Las busca Rust (library/src/reuse.rs)."""
    from .library import db

    with db.connect() as conn:
        return _rust.reuse_twins(conn, dict(song))


def with_twins(song_ids) -> list[int]:
    """De esas canciones (recien llegadas), las que tienen gemela en la
    biblioteca o pistas separadas de otro archivo: solo esas merecen el
    trabajo de `settle`. Una consulta para todas."""
    from .library import db

    wanted = [int(i) for i in song_ids]
    if not wanted:
        return []
    manifests = {k: [d for d, _ in v] for k, v in _manifest_index().items()}
    with db.connect() as conn:
        return _rust.reuse_with_twins(conn, wanted, manifests)


# ------------------------------------------------------------ el desfase


def _decode(path: str, seconds: float = WINDOW, rate: int = RATE):
    """Lo de delante de un archivo, en mono a `rate`."""
    import numpy as np

    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        return None
    r = subprocess.run(
        [
            ffmpeg,
            "-v",
            "error",
            "-nostdin",
            "-i",
            path,
            "-t",
            str(seconds),
            "-ac",
            "1",
            "-ar",
            str(rate),
            "-f",
            "f32le",
            "-",
        ],
        capture_output=True,
        timeout=120,
        check=False,
    )
    if r.returncode != 0 or len(r.stdout) < rate * 4:
        return None
    return np.frombuffer(r.stdout, np.float32).copy()


def _mix(paths: list[str], seconds: float = WINDOW):
    """Las pistas separadas sumadas: suenan como la cancion."""
    total = None
    for p in paths:
        x = _decode(p, seconds)
        if x is None:
            continue
        if total is None:
            total = x
        else:
            n = min(total.size, x.size)
            total = total[:n] + x[:n]
    return total


def _envelope(x, hop: int):
    """Donde empiezan los golpes: lo que sube la energia, trama a trama."""
    import numpy as np

    n = x.size // hop
    e = np.log1p(1000 * np.mean(x[: n * hop].reshape(n, hop) ** 2, axis=1))
    d = np.maximum(np.diff(e, prepend=e[:1]), 0)
    d = d - d.mean()
    return d / (np.linalg.norm(d) + 1e-9)


def _xcorr(a, b, max_lag: int):
    """La correlacion de `a` contra `b` corrido de -max_lag a max_lag:
    (desfases, valores). Un desfase positivo: lo de `b` llega antes."""
    import numpy as np

    n = 1 << int(np.ceil(np.log2(a.size + b.size)))
    c = np.fft.irfft(np.fft.rfft(a, n) * np.conj(np.fft.rfft(b, n)), n)
    lags = np.arange(-max_lag, max_lag + 1)
    return lags, c[lags % n]


def _peak(lags, values) -> tuple[float, float]:
    """El maximo, afinado entre muestras (parabola)."""
    import numpy as np

    i = int(np.argmax(values))
    best = float(lags[i])
    if 0 < i < values.size - 1:
        y0, y1, y2 = values[i - 1], values[i], values[i + 1]
        den = y0 - 2 * y1 + y2
        if abs(den) > 1e-12:
            best += float(0.5 * (y0 - y2) / den)
    return best, float(values[i])


def offset(new, ref, rate: int = RATE) -> tuple[float, float] | None:
    """Cuanto llega mas tarde lo de `new` que lo de `ref` (segundos) y lo
    que se parecen (0-1). Primero por los golpes (cada 10 ms), en hasta
    `MAX_LAG` s; despues, muestra a muestra cerca de ahi."""
    import numpy as np

    if new is None or ref is None or min(new.size, ref.size) < rate * 5:
        return None
    hop = rate // 100
    a, b = _envelope(new, hop), _envelope(ref, hop)
    lags, values = _xcorr(a, b, min(int(MAX_LAG * 100), max(a.size, b.size) - 1))
    coarse, score = _peak(lags, values)
    # el parecido, sin lo que se pierde por los bordes
    score = float(min(1.0, score / max(1e-9, np.sqrt(min(a.size, b.size) / max(a.size, b.size)))))
    # afinar: la señal entera, ±15 ms alrededor
    shift = round(coarse * hop)
    span = int(0.015 * rate)
    seg = slice(rate * 2, min(new.size, ref.size) - rate * 2 - abs(shift))
    if seg.stop - seg.start < rate * 3:
        return coarse / 100, score
    hp_new = np.diff(new)
    hp_ref = np.diff(ref)
    best, best_v = None, -np.inf
    values = []
    for d in range(shift - span, shift + span + 1):
        s0 = seg.start + d
        if s0 < 0 or seg.stop + d > hp_new.size:
            values.append(-np.inf)
            continue
        v = float(np.dot(hp_new[s0 : seg.stop + d], hp_ref[seg]))
        values.append(v)
        if v > best_v:
            best, best_v = d, v
    if best is None:
        return coarse / 100, score
    fine, _ = _peak(np.arange(shift - span, shift + span + 1), np.array(values))
    return fine / rate, score


def lag_between(new_path: str, ref_paths: list[str]) -> tuple[float, float] | None:
    """El desfase entre un archivo y otro (o sus pistas separadas sumadas)."""
    new = _decode(new_path)
    ref = _decode(ref_paths[0]) if len(ref_paths) == 1 else _mix(ref_paths)
    return offset(new, ref)


def lag_from_lowband(new_path: str, raw: dict) -> tuple[float, float] | None:
    """El desfase con lo que dijo la red del archivo de antes (su bombo y
    bajo, cada 10 ms), cuando el archivo ya no esta ni hay pistas."""
    import numpy as np

    from . import pulse

    x = _decode(new_path, WINDOW, pulse.RATE)
    if x is None:
        return None
    low, fps = pulse.lowband(x)
    old = np.asarray(raw["low"], np.float32)[: low.size + int(MAX_LAG * fps)]
    if fps != raw.get("low_fps") or old.size < fps * 5:
        return None

    def norm(v):
        v = v - v.mean()
        return v / (np.linalg.norm(v) + 1e-9)

    a, b = norm(low), norm(old)
    lags, values = _xcorr(a, b, min(int(MAX_LAG * fps), max(a.size, b.size) - 1))
    best, score = _peak(lags, values)
    return best / fps, float(min(1.0, score))


# ------------------------------------------------------------ adoptar


def shift_study(study: dict, lag: float) -> dict:
    """Lo del estudio corrido `lag` segundos (lo que pasaba en t pasa en
    t + lag). Lo que quedaria antes del principio, fuera."""
    if not study or abs(lag) < SMALL_LAG:
        return study
    return _rust.reuse_shift_study(dict(study), float(lag))


def _stems_folder(row) -> Path | None:
    """La carpeta de pistas de esa fila, si sigue en el disco."""
    from . import stems

    with contextlib.suppress(ValueError, TypeError, AttributeError):
        rel = json.loads(row.get("stems") or "{}").get("folder")
        if rel:
            folder = Path(row.get("root") or config.LIBRARY) / rel
            if (folder / stems.MANIFEST).is_file():
                return folder
    return None


_MANIFESTS: dict = {"at": 0.0, "index": {}}


def _manifest_index() -> dict:
    """Las carpetas de pistas de todas las carpetas gestionadas, por la clave
    de su cancion: {clave: [(duracion, carpeta)]}. Se rehace cada minuto
    (tras un escaneo llegan muchas canciones seguidas)."""
    from . import stems
    from .library.folders import _roots

    if time.monotonic() - _MANIFESTS["at"] < 60:
        return _MANIFESTS["index"]
    index: dict = {}
    for root in _roots():
        with contextlib.suppress(OSError):
            for folder in (Path(root) / stems.FOLDER).iterdir():
                src = (stems.read_manifest(folder) or {}).get("source") or {}
                key = ident(src) if src else ""
                if key and float(src.get("duration") or 0) > 0:
                    index.setdefault(key, []).append((float(src["duration"]), folder))
    _MANIFESTS.update(at=time.monotonic(), index=index)
    return index


def _manifest_twin(song) -> Path | None:
    """Una carpeta de pistas, en cualquier carpeta gestionada, cuyo manifiesto
    diga que es de esta cancion (aunque ninguna fila apunte a ella)."""
    from . import stems

    for duration, folder in _manifest_index().get(ident(song), ()):
        if _close(duration, song.get("duration")) and (folder / stems.MANIFEST).is_file():
            return folder
    return None


def _encode_shifted(ffmpeg: str, src: Path, dest: Path, lag: float) -> None:
    """Una pista corrida `lag` segundos (con silencio delante, o sin lo de
    delante), en FLAC."""
    if lag > 0:
        flt = f"adelay=delays={lag * 1000:.3f}:all=1"
    else:
        flt = f"atrim=start={-lag:.6f},asetpts=PTS-STARTPTS"
    r = subprocess.run(
        [
            ffmpeg,
            "-v",
            "error",
            "-nostdin",
            "-y",
            "-i",
            str(src),
            "-af",
            flt,
            "-c:a",
            "flac",
            str(dest),
        ],
        capture_output=True,
        timeout=600,
        check=False,
    )
    if r.returncode != 0:
        raise OSError(r.stderr.decode("utf-8", "replace")[-300:])


def adopt_stems(song, source: Path, lag: float, *, move: bool) -> Path | None:
    """Las pistas de `source` para esta cancion, en su carpeta de pistas
    (movidas si ya no son de nadie, copiadas si siguen siendo de otra) y
    corridas `lag` segundos si hace falta. Devuelve la carpeta."""
    from . import library, stems

    manifest = stems.read_manifest(source)
    if manifest is None:
        return None
    target = stems._target_for(song)
    if target.exists() and target.resolve() != source.resolve():
        return None  # ya tiene otra carpeta suya
    target.parent.mkdir(parents=True, exist_ok=True)
    work = target.with_name(f".adoptando-{target.name}-{int(time.time())}")
    try:
        if abs(lag) < SMALL_LAG:
            if move and source.resolve() != target.resolve():
                shutil.move(str(source), str(work))
            elif source.resolve() == target.resolve():
                work = target
            else:
                shutil.copytree(source, work)
        else:
            ffmpeg = convert.tool("ffmpeg")
            if not ffmpeg:
                return None
            work.mkdir()
            tracks = []
            for t in manifest.get("tracks") or []:
                name = Path(str(t.get("file") or "")).stem + ".flac"
                _encode_shifted(ffmpeg, source / str(t["file"]), work / name, lag)
                tracks.append({**t, "file": name})
            manifest = {**manifest, "tracks": tracks}
            if move:
                shutil.rmtree(source, ignore_errors=True)
        manifest["source"] = {
            "file": song["file"],
            "folder": song.get("folder") or "",
            "size": song.get("size") or 0,
            "duration": song.get("duration") or 0,
            "artist": song.get("artist") or "",
            "title": song.get("title") or "",
        }
        if abs(lag) >= SMALL_LAG:
            manifest["shifted"] = round(lag, 4)
        (work / stems.MANIFEST).write_text(
            json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
        )
        if work != target:
            os.replace(work, target)
    except (OSError, shutil.Error):
        log.warning("no pude adoptar las pistas de %s", source, exc_info=True)
        if work != target:
            shutil.rmtree(work, ignore_errors=True)
        return None
    rel = os.path.relpath(target, song["root"])
    data = {
        "folder": rel,
        "model": manifest.get("model", ""),
        "quality": manifest.get("quality", ""),
    }
    library.update(song["id"], stems=json.dumps(data))
    return target


def _old_key(row) -> str | None:
    """La clave con que se guardo la rejilla del archivo de esa fila (aunque
    el archivo ya no este: su ruta, tamaño y fecha estan en la fila)."""
    from .pulse import key_of

    try:
        st = SimpleNamespace(st_size=int(row["size"]), st_mtime=float(row["mtime"]))
        return key_of(row["path"], st)
    except (KeyError, TypeError, ValueError):
        return None


def adopt_grid(path: str, twin: dict, lag: float) -> bool:
    """La rejilla del clic (y lo que dijo la red) de la gemela, corrida
    `lag` segundos, para este archivo."""
    import numpy as np

    from . import beatgrid, pulse

    key = _old_key(twin)
    if not key or beatgrid.cached(path):
        return False
    folder = config.DATA_DIR / "beats"
    old_json, old_raw = folder / f"{key}.json", folder / f"{key}.npz"
    try:
        saved = json.loads(old_json.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        saved = None
    raw = pulse.load_raw(old_raw) if old_raw.is_file() else None
    if raw is not None:
        # lo que dijo la red, corrido: tramas de delante con silencio, o fuera
        for name, fps in (("beat", pulse.FPS), ("down", pulse.FPS), ("low", raw["low_fps"])):
            frames = round(lag * fps)
            arr = raw[name]
            fill = -10.0 if name != "low" else 0.0
            if frames > 0:
                arr = np.concatenate([np.full(frames, fill, np.float32), arr])
            elif frames < 0:
                arr = arr[-frames:]
            raw[name] = arr
        raw["seconds"] = float(raw["seconds"]) + lag
        new_raw = beatgrid._raw_file(path)
        new_raw.parent.mkdir(parents=True, exist_ok=True)
        pulse.save_raw(new_raw, raw)
    if not saved or not saved.get("grid") or saved.get("version") != beatgrid.VERSION:
        # sin rejilla de esta version: con lo que dijo la red, se saca al momento
        return raw is not None
    grid = dict(saved["grid"])
    beats = [round(t + lag, 4) for t in grid.get("beats") or []]
    keep = [i for i, t in enumerate(beats) if t >= 0]
    index = {old: new for new, old in enumerate(keep)}
    grid["beats"] = [beats[i] for i in keep]
    grid["bars"] = [index[b] for b in grid.get("bars") or [] if b in index]
    grid["first_downbeat"] = grid["bars"][0] if grid["bars"] else 0
    edits = []
    for e in saved.get("edits") or []:
        e = dict(e)
        if e.get("start") is not None:
            e["start"], e["end"] = round(e["start"] + lag, 3), round(e["end"] + lag, 3)
        edits.append(e)
    beatgrid._save(path, grid, edits)
    return True


def _worth(row) -> int:
    """Lo que hay que aprovechar de una fila: sus pistas valen mas que su
    clic, y su clic mas que su estudio."""
    worth = 4 if _stems_folder(row) else 0
    key = _old_key(row)
    folder = config.DATA_DIR / "beats"
    if key and ((folder / f"{key}.npz").is_file() or (folder / f"{key}.json").is_file()):
        worth += 2
    if row.get("study"):
        worth += 1
    return worth


def settle(
    song_id: int, *, twin_of_missing: dict | None = None, candidates: list | None = None
) -> dict:
    """Lo que se aprovecha para esa cancion recien llegada. `twin_of_missing`:
    la fila apartada con la que el escaneo ya la junto (mismo id: sus listas
    y su estudio ya son suyos); si no, se busca una gemela. Devuelve lo que
    se hizo: {twin, lag, score, stems, grid, study}."""
    from . import library, stems

    row = library.by_id(song_id)
    done: dict = {
        "twin": None,
        "lag": 0.0,
        "score": None,
        "stems": False,
        "grid": False,
        "study": False,
    }
    if not row or not os.path.isfile(row["path"]):
        return done
    song: dict[str, Any] = dict(row)
    has_stems = stems.info(song) is not None
    if twin_of_missing:
        candidates = [twin_of_missing]
    elif candidates is None:
        candidates = twins(song)
    # la que tenga mas que aprovechar (a igualdad, la de duracion mas
    # parecida: vienen en ese orden)
    twin = None
    best = 0
    for c in candidates:
        worth = _worth(c) if c else 0
        if worth > best:
            twin, best = c, worth
    if twin is None:
        folder = None if has_stems else _manifest_twin(song)
        if folder is None:
            return done
        twin = {"id": None, "path": "", "stems": "", "root": "", "manifest_folder": str(folder)}
    done["twin"] = twin.get("id")
    folder = Path(twin["manifest_folder"]) if twin.get("manifest_folder") else _stems_folder(twin)
    # cuanto esta corrido el archivo nuevo: contra el de antes si sigue, sus
    # pistas sumadas, o lo que oyo la red
    measured = None
    if twin.get("path") and os.path.isfile(twin["path"]) and twin["path"] != song["path"]:
        measured = lag_between(song["path"], [twin["path"]])
    elif folder is not None:
        manifest = stems.read_manifest(folder) or {}
        paths = [
            str(folder / t["file"])
            for t in manifest.get("tracks") or []
            if (folder / t["file"]).is_file()
        ]
        if paths:
            measured = lag_between(song["path"], paths)
    else:
        from . import beatgrid

        key = _old_key(twin)
        raw = beatgrid.raw_by_key(key) if key else None
        if raw is not None:
            measured = lag_from_lowband(song["path"], raw)
    if measured is not None:
        done["lag"], done["score"] = round(measured[0], 4), round(measured[1], 3)
        if measured[1] < MIN_SCORE:
            log.info(
                "%s: la gemela no suena igual (%.2f): no se aprovecha", song["file"], measured[1]
            )
            if twin_of_missing and has_stems and folder is None:
                library.update(song_id, stems="")
            return done
    elif not _close(twin.get("duration"), song.get("duration"), 0.5) and not twin_of_missing:
        return done  # sin poder comprobarlo y con otra duracion: mejor nada
    lag = done["lag"] if abs(done["lag"]) >= SMALL_LAG else 0.0
    # las pistas
    if folder is not None:
        same = stems.info(song)
        here = same and Path(same["folder"]).resolve() == folder.resolve()
        if not (here and lag == 0.0):
            move = bool(twin.get("missing") or twin_of_missing or twin.get("manifest_folder"))
            done["stems"] = adopt_stems(song, folder, lag, move=move) is not None
        else:
            done["stems"] = True
    # el clic
    if twin.get("path"):
        with contextlib.suppress(OSError, ValueError, KeyError):
            done["grid"] = adopt_grid(song["path"], twin, lag)
    # el estudio: el de la apartada ya es suyo (mismo id), pero corrido
    with contextlib.suppress(ValueError, TypeError):
        own = json.loads(song.get("study") or "{}")
        theirs = json.loads(twin.get("study") or "{}") if not twin_of_missing else own
        if twin_of_missing and own and lag:
            library.set_study(song_id, shift_study(own, lag))
            done["study"] = True
        elif not own and theirs:
            library.set_study(song_id, shift_study(theirs, lag))
            done["study"] = True
    if done["stems"] or done["grid"] or done["study"]:
        log.info("%s: aprovechado de su gemela %s", song["file"], done)
    return done


def merge_missing(song_id: int, twin: dict) -> int:
    """La apartada que era esta cancion (se cambio la carpeta de musica y la
    cancion se volvio a bajar antes de esto): sus listas y sus recientes
    pasan a la fila nueva, y la apartada se olvida. Devuelve en cuantas
    listas vuelve a estar."""
    from . import playlists
    from .library import db

    old = twin.get("id")
    if not old or old == song_id:
        return 0
    with db.connect() as conn:
        tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        n = 0
        if "playlist_songs" in tables:
            n = conn.execute(
                "UPDATE OR IGNORE playlist_songs SET song_id=? WHERE song_id=?", (song_id, old)
            ).rowcount
            conn.execute("DELETE FROM playlist_songs WHERE song_id=?", (old,))
        if "recent" in tables:
            conn.execute("UPDATE OR IGNORE recent SET song_id=? WHERE song_id=?", (song_id, old))
            conn.execute("DELETE FROM recent WHERE song_id=?", (old,))
        conn.execute("DELETE FROM songs_missing WHERE id=?", (old,))
    if n:
        # las listas tambien viajan en el archivo
        with contextlib.suppress(Exception):
            playlists._stamp_playlists_into_files([song_id])
    return n


def sweep(progress=None) -> dict:
    """Una pasada por toda la biblioteca: a cada cancion sin pistas o sin
    clic se le busca una gemela de la que aprovecharlos, y las que se
    perdieron de las listas al mover la carpeta de musica vuelven a ellas.
    Para lo que paso antes de que DanPlay supiera hacerlo solo."""
    from . import beatgrid, library, stems
    from .library import db

    with db.connect() as conn:
        rows = [dict(r) for r in conn.execute("SELECT id, path, duration FROM songs ORDER BY id")]
    told = {"songs": len(rows), "stems": 0, "grid": 0, "study": 0, "lists": 0}
    for i, r in enumerate(rows):
        if progress:
            progress(i, len(rows))
        row = library.by_id(r["id"])
        if not row or not os.path.isfile(row["path"]):
            continue
        song: dict[str, Any] = dict(row)
        candidates = twins(song)
        for t in [t for t in candidates if t.get("missing")]:
            told["lists"] += merge_missing(song["id"], t)
        needs = (
            stems.info(song) is None or not beatgrid.cached(song["path"]) or not song.get("study")
        )
        if not needs or not (candidates or _manifest_twin(song)):
            continue
        try:
            done = settle(song["id"], candidates=candidates)
        except Exception:
            log.warning("no se pudo aprovechar lo de %s", song["path"], exc_info=True)
            continue
        for k in ("stems", "grid", "study"):
            told[k] += int(bool(done.get(k)))
    if progress:
        progress(len(rows), len(rows))
    return told


# La pasada de `sweep` se hace sola una vez (al arrancar, cuando el
# escaneo ya puso el indice al dia); con otro numero, otra vez.
SWEEP_VERSION = 1


def sweep_once_in_background(delay: float = 60.0) -> bool:
    """`sweep` una sola vez por equipo, en segundo plano y pasado un rato
    (lo de este arranque primero). True si se lanzo."""
    marker = config.DATA_DIR / "aprovechar.json"
    with contextlib.suppress(OSError, ValueError):
        if json.loads(marker.read_text(encoding="utf-8")).get("version") == SWEEP_VERSION:
            return False

    def run() -> None:
        time.sleep(delay)
        try:
            told = sweep()
        except Exception:
            log.warning("la pasada de aprovechar fallo", exc_info=True)
            return
        log.info("aprovechado de otras copias: %s", told)
        with contextlib.suppress(OSError):
            marker.parent.mkdir(parents=True, exist_ok=True)
            marker.write_text(
                json.dumps({"version": SWEEP_VERSION, "at": round(time.time()), **told}),
                encoding="utf-8",
            )

    threading.Thread(target=run, name="danplay-aprovechar-todo", daemon=True).start()
    return True


# ------------------------------------------------------------ en segundo plano

_LOCK = threading.Lock()
_QUEUE: list[tuple[int, dict | None]] = []
_WORKER: list[threading.Thread] = []


def later(song_id: int, twin_of_missing: dict | None = None) -> None:
    """`settle` en segundo plano (decodificar y medir tarda unos segundos):
    el escaneo y las descargas no esperan."""
    with _LOCK:
        _QUEUE.append((song_id, twin_of_missing))
        if _WORKER and _WORKER[0].is_alive():
            return
        t = threading.Thread(target=_work, name="danplay-aprovechar", daemon=True)
        _WORKER[:] = [t]
        t.start()


def _work() -> None:
    while True:
        with _LOCK:
            if not _QUEUE:
                _WORKER.clear()
                return
            song_id, twin = _QUEUE.pop(0)
        try:
            settle(song_id, twin_of_missing=twin)
        except Exception:
            log.warning("no se pudo aprovechar lo de la cancion %s", song_id, exc_info=True)


def idle(timeout: float = 30.0) -> bool:
    """Espera a que no quede nada por aprovechar (para las pruebas)."""
    limit = time.monotonic() + timeout
    while time.monotonic() < limit:
        with _LOCK:
            if not _QUEUE and not (_WORKER and _WORKER[0].is_alive()):
                return True
        time.sleep(0.02)
    return False
