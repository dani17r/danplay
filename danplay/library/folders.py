"""Las carpetas que DanPlay gestiona, lo que se excluye de ellas y que archivos
de audio hay dentro. Tambien una carpeta que se movio entera y vuelve."""

import json
import logging
import os
import time
from fnmatch import fnmatch
from pathlib import Path

from .. import config
from . import db
from .db import _SCAN_LOCK, _insert_row, _to_missing, _touch

log = logging.getLogger(__name__)


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
    except ValueError:  # unidades distintas, o relativa y absoluta
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
            return {
                "kind": "inside",
                "other": other,
                "message": f"esta dentro de «{other}», que ya esta indexada",
            }
        if _inside(other, path):
            return {
                "kind": "contains",
                "other": other,
                "message": f"contiene a «{other}», que ya esta indexada",
            }

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
    with db.connect() as conn:
        placeholders = ",".join("?" * len(sample))
        rows = conn.execute(
            f"SELECT file, size, root FROM songs WHERE file IN ({placeholders})",  # noqa: S608
            list(sample),
        ).fetchall()
    by_root = {}
    for f in rows:
        if sample.get(f["file"]) == f["size"]:
            by_root[f["root"]] = by_root.get(f["root"], 0) + 1
    if not by_root:
        return None
    other, matches = max(by_root.items(), key=lambda x: x[1])
    percent = round(100 * matches / len(sample))
    if percent >= 60:
        return {
            "kind": "copy",
            "other": other,
            "percent": percent,
            "message": (
                f"el {percent}% de sus archivos ya estan indexados desde "
                f"«{other}»: parecen la misma musica duplicada"
            ),
        }
    return None


def add_folder(path, label="", role="library") -> bool:
    path = str(Path(path).expanduser().resolve())
    if not os.path.isdir(path):
        return False
    with db.connect() as conn:
        conn.execute(
            "INSERT OR REPLACE INTO folders VALUES (?,?,?,1,?)",
            (path, label or os.path.basename(path), role, time.time()),
        )
    return True


def remove_folder(path) -> None:
    """Deja de gestionar la carpeta. Sus canciones se apartan, no se borran:
    si se vuelve a añadir (se quito sin querer, era un disco que volvera),
    cada una recupera su id, sus listas y sus notas."""
    path = str(Path(path).expanduser().resolve())
    with _SCAN_LOCK, db.connect() as conn:
        conn.execute("DELETE FROM folders WHERE path=?", (path,))
        _to_missing(
            conn, [r["path"] for r in conn.execute("SELECT path FROM songs WHERE root=?", (path,))]
        )
    _touch()


def list_folders() -> list[dict]:
    with db.connect() as conn:
        rows = conn.execute(
            "SELECT c.*, (SELECT COUNT(*) FROM songs s WHERE s.root=c.path) n "
            "FROM folders c ORDER BY c.label"
        ).fetchall()
    # `exists`: la carpeta sigue en su sitio. Si no (se movio, o es un disco
    # sin montar), sus canciones estan apartadas y la interfaz lo dice.
    return [{**dict(f), "exists": os.path.isdir(f["path"])} for f in rows]


def missing_roots() -> list[str]:
    """Las carpetas gestionadas que ya no estan donde estaban."""
    return [r for r in _roots() if not os.path.isdir(r)]


def missing_root_of(path) -> str | None:
    """La carpeta gestionada que ya no esta y de la que cuelga `path`, si la hay."""
    return next((r for r in missing_roots() if _inside(path, r)), None)


class FolderGone(OSError):
    """La carpeta cuelga de una carpeta gestionada que ya no esta."""


def ensure_folder(path) -> Path:
    """Crea `path` (Entrada/, Listas/...) si hace falta, salvo dentro de una
    carpeta gestionada que ya no esta: entonces `FolderGone`.

    Crearla ahi con `parents=True` hacia «volver» la carpeta movida, vacia: la
    app ya no decia «No encuentro tu musica», elegir la carpeta en su sitio
    nuevo no se reconocia como la misma, y quedaba una carpeta fantasma en el
    disco. Pasaba solo con abrir la app, que pregunta por la Entrada.
    """
    path = Path(path)
    if path.is_dir():
        return path
    gone = missing_root_of(path)
    if gone:
        raise FolderGone(f"la carpeta «{gone}» ya no esta donde estaba: ¿la moviste?")
    path.mkdir(parents=True, exist_ok=True)
    return path


def hide_missing_roots() -> int:
    """Aparta las canciones de las carpetas que ya no estan. Devuelve cuantas.

    Es rapido a proposito (no recorre nada: solo mira si cada carpeta existe)
    porque es lo primero que hace el nucleo al arrancar, antes de que la
    interfaz pregunte: asi nunca enseña canciones de una carpeta que se movio.
    El escaneo completo, que viene despues, pone al dia el resto.
    """
    with _SCAN_LOCK:
        gone = missing_roots()
        if not gone:
            return 0
        with db.connect() as conn:
            marks = ",".join("?" * len(gone))
            sql = f"SELECT path FROM songs WHERE root IN ({marks})"  # noqa: S608
            n = _to_missing(conn, [r["path"] for r in conn.execute(sql, gone)])
    if n:
        _touch()
    return n


def _songs_of_root(conn, root, limit=None) -> list[str]:
    """Las rutas que tenia una carpeta: las del indice y las apartadas."""
    sql = "SELECT path FROM songs WHERE root=? UNION SELECT path FROM songs_missing WHERE root=?"
    if limit:
        sql += f" LIMIT {int(limit)}"
    return [r[0] for r in conn.execute(sql, (root, root))]


def relocation_for(path) -> str | None:
    """Si `path` es una carpeta que se movio, la ruta donde estaba antes.

    Se mira cada carpeta gestionada que ya no esta: si al menos la mitad de
    una muestra de sus canciones aparece en `path` con la misma ruta relativa
    (Artistas/Barak/…), es la misma carpeta en otro sitio. Es lo que permite
    «volver a importar» sin perder nada: quien la elige en la bienvenida no
    tiene por que saber que es otra cosa que añadir una carpeta.
    """
    new = os.path.realpath(os.path.expanduser(str(path)))
    if not os.path.isdir(new):
        return None
    candidates = missing_roots()
    if not candidates:
        return None
    with db.connect() as conn:
        for old in candidates:
            sample = _songs_of_root(conn, old, limit=40)
            if not sample:
                continue
            hits = sum(os.path.isfile(os.path.join(new, os.path.relpath(p, old))) for p in sample)
            if hits * 2 >= len(sample):
                return old
    return None


def relocate_folder(old, new) -> dict:
    """La carpeta `old` se movio a `new`: la misma entrada, en su sitio nuevo.

    Cada cancion que esta en `new` con la misma ruta relativa vuelve con su
    id (y sus listas, notas, acordes…) sin releer el archivo; si el archivo
    cambio por el camino, se marca para que el siguiente escaneo lo relea.
    Lo que no aparezca sigue apartado, por si vuelve.
    """
    old = os.path.normpath(str(old))
    new = os.path.realpath(os.path.expanduser(str(new)))
    if not os.path.isdir(new):
        raise ValueError("esa carpeta no existe")
    if os.path.isdir(old):
        raise ValueError("la carpeta de antes sigue en su sitio")
    with _SCAN_LOCK, db.connect() as conn:
        # lo que el indice aun enseñaba de ella: su archivo ya no esta ahi
        _to_missing(
            conn, [r["path"] for r in conn.execute("SELECT path FROM songs WHERE root=?", (old,))]
        )
        if conn.execute("SELECT 1 FROM folders WHERE path=?", (new,)).fetchone():
            conn.execute("DELETE FROM folders WHERE path=?", (old,))
        else:
            conn.execute(
                "UPDATE folders SET path=?, label=CASE WHEN label=? THEN ? "
                "ELSE label END WHERE path=?",
                (new, os.path.basename(old), os.path.basename(new), old),
            )
        back = 0
        for r in conn.execute("SELECT * FROM songs_missing WHERE root=?", (old,)).fetchall():
            target = os.path.join(new, os.path.relpath(r["path"], old))
            try:
                st = os.stat(target)
            except OSError:
                continue
            if conn.execute("SELECT 1 FROM songs WHERE path=?", (target,)).fetchone():
                continue
            data = json.loads(r["data"])
            data.update(path=target, root=new, folder=os.path.relpath(os.path.dirname(target), new))
            if st.st_size != data.get("size") or st.st_mtime != data.get("mtime"):
                data["mtime"] = -1  # cambio por el camino: que se relea
            _insert_row(conn, data)
            conn.execute("DELETE FROM songs_missing WHERE id=?", (r["id"],))
            back += 1
    _touch()
    return {"from": old, "to": new, "back": back}


def _roots() -> list[str]:
    with db.connect() as conn:
        rows = conn.execute("SELECT path FROM folders WHERE active=1").fetchall()
    # sin carpetas configuradas no se indexa nada: la app pide elegirlas primero
    return [f["path"] for f in rows]


def roots() -> list[str]:
    """Carpetas gestionadas activas. Para pasarselas a `index_file` en bucle
    y no consultar la base por cada archivo."""
    return _roots()


# ------------------------------------------------------------- escaneo

# ------------------------------------------------------------- exclusiones

DEFAULT_EXCLUDES = [
    ".*",
    "@eaDir",
    "#recycle",
    "$RECYCLE.BIN",
    "System Volume Information",
    "__MACOSX",
]


def add_exclusion(pattern, kind="glob", note="") -> None:
    with db.connect() as conn:
        conn.execute(
            "INSERT OR REPLACE INTO exclusions VALUES (?,?,?)", (pattern.rstrip("/"), kind, note)
        )


def remove_exclusion(pattern) -> None:
    with db.connect() as conn:
        conn.execute("DELETE FROM exclusions WHERE pattern=?", (pattern.rstrip("/"),))


def list_exclusions() -> list[dict]:
    with db.connect() as conn:
        rows = conn.execute("SELECT * FROM exclusions ORDER BY pattern").fetchall()
    return [dict(f) for f in rows]


def _excluded(dir_path, dir_name, exclusions) -> bool:
    """True si esta carpeta debe saltarse.

    Los patrones NO distinguen mayusculas: escribir «secuencias» tiene que
    funcionar aunque la carpeta se llame «Secuencias».
    """
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
        elif (
            fnmatch(name_lc, p)
            or fnmatch(full_lc, p)
            or fnmatch(full_lc, "*/" + p.strip("*/") + "/*")
        ):
            return True
    return False


# El archivo que marca una carpeta de pistas separadas (ver danplay/stems.py).
STEMS_MANIFEST = ".danplay-pistas.json"


def _stems_folder(dir_path, dir_name, found) -> bool:
    """True si es una carpeta de pistas separadas: sus pistas son de una
    cancion, no canciones sueltas, y el escaneo no las mete en la biblioteca.
    Si se pasa `found`, se apunta su manifiesto para unirla con su cancion."""
    folder = os.path.join(dir_path, dir_name)
    manifest = os.path.join(folder, STEMS_MANIFEST)
    if not os.path.isfile(manifest):
        return False
    if found is not None:
        try:
            with open(manifest, encoding="utf-8") as f:
                data = json.load(f)
            if isinstance(data, dict):
                found[folder] = data
        except (OSError, ValueError):
            log.warning("no se pudo leer %s", manifest)
    return True


def audio_files(root, exclusions=None, stems=None):
    """Los archivos de audio de la carpeta, sin las excluidas ni las de pistas
    separadas. En `stems` (un dict) se apuntan estas ultimas."""
    exclusions = list_exclusions() if exclusions is None else exclusions
    for dp, dn, fns in os.walk(root):
        dn[:] = [
            d for d in dn if not _excluded(dp, d, exclusions) and not _stems_folder(dp, d, stems)
        ]
        for fn in fns:
            if Path(fn).suffix.lower() in config.EXTENSIONS:
                path = os.path.join(dp, fn)
                if _storable(path):
                    yield path


# Rutas ya avisadas: el escaneo corre solo cada pocos minutos, y repetir el
# mismo aviso cada vez llenaria el registro.
_UNSTORABLE: set = set()


def _storable(path: str) -> bool:
    """False si la ruta no se puede guardar en la base.

    Un nombre que no es UTF-8 valido (un disco que viene de Windows o de un
    FAT viejo) llega de `os.walk` con caracteres sustitutos, y SQLite los
    rechaza: uno solo tumbaba el escaneo entero y no se indexaba nada.
    """
    try:
        path.encode("utf-8")
        return True
    except UnicodeEncodeError:
        if path not in _UNSTORABLE:
            _UNSTORABLE.add(path)
            log.warning(
                "se salta %r: el nombre no es UTF-8 valido; renombralo para que DanPlay lo vea",
                path,
            )
        return False
