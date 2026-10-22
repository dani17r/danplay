"""Las carpetas que DanPlay gestiona, lo que se excluye de ellas y que archivos
de audio hay dentro. Tambien una carpeta que se movio entera y vuelve.

Lo hace Rust (library/src/index/folders.rs); esto es la puerta de
Python, con los mismos nombres. El cerrojo del escaneo (`_SCAN_LOCK`) y el
aviso a la interfaz (`_touch`) siguen aqui, alrededor de cada llamada.
"""

import logging
import os
from pathlib import Path

import danplay_core as _rust

from .. import config
from . import db
from .db import _SCAN_LOCK, _touch

log = logging.getLogger(__name__)


def _s(path) -> str:
    return os.fspath(path) if not isinstance(path, str) else path


# ------------------------------------------------------------- carpetas


def _inside(path, root) -> bool:
    """True si `path` es `root` o cuelga de el.

    Sobre rutas reales (enlaces simbolicos y «..» resueltos) y por
    componentes, no pegando «/» a una cadena: asi `../../x` o un enlace que
    apunte fuera no cuentan como «dentro», y en Windows tampoco importa si
    la unidad viene en mayuscula o minuscula.
    """
    return _rust.folders_inside(_s(path), _s(root))


def within_roots(path) -> bool:
    """Si la ruta esta dentro de alguna carpeta gestionada (activa).

    Es la barrera de todo lo que borra o mueve: nada que venga de la API o
    del asistente debe tocar un archivo fuera de la biblioteca.
    """
    with db.connect() as conn:
        return _rust.folders_within_roots(conn, _s(path))


def check_overlap(path) -> dict | None:
    """Avisa si la carpeta nueva repite musica que ya esta indexada.

    Dos casos: que una contenga a la otra, o que sean copias distintas del
    mismo contenido (por ejemplo el original y su respaldo).
    """
    with db.connect() as conn:
        return _rust.folders_check_overlap(conn, _s(path), sorted(config.EXTENSIONS))


def add_folder(path, label="", role="library") -> bool:
    with db.connect() as conn:
        return _rust.folders_add(conn, _s(path), label or "", role)


def remove_folder(path) -> None:
    """Deja de gestionar la carpeta. Sus canciones se apartan, no se borran:
    si se vuelve a añadir (se quito sin querer, era un disco que volvera),
    cada una recupera su id, sus listas y sus notas."""
    with _SCAN_LOCK, db.connect() as conn:
        _rust.folders_remove(conn, _s(path))
    _touch()


def list_folders() -> list[dict]:
    with db.connect() as conn:
        return _rust.folders_list(conn)


def missing_roots() -> list[str]:
    """Las carpetas gestionadas que ya no estan donde estaban."""
    with db.connect() as conn:
        return _rust.folders_missing_roots(conn)


def missing_root_of(path) -> str | None:
    """La carpeta gestionada que ya no esta y de la que cuelga `path`, si la hay."""
    with db.connect() as conn:
        return _rust.folders_missing_root_of(conn, _s(path))


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
    with _SCAN_LOCK, db.connect() as conn:
        n = _rust.folders_hide_missing_roots(conn)
    if n:
        _touch()
    return n


def relocation_for(path) -> str | None:
    """Si `path` es una carpeta que se movio, la ruta donde estaba antes.

    Se mira cada carpeta gestionada que ya no esta: si al menos la mitad de
    una muestra de sus canciones aparece en `path` con la misma ruta relativa
    (Artistas/Barak/…), es la misma carpeta en otro sitio. Es lo que permite
    «volver a importar» sin perder nada: quien la elige en la bienvenida no
    tiene por que saber que es otra cosa que añadir una carpeta.
    """
    with db.connect() as conn:
        return _rust.folders_relocation_for(conn, _s(path))


def relocate_folder(old, new) -> dict:
    """La carpeta `old` se movio a `new`: la misma entrada, en su sitio nuevo.

    Cada cancion que esta en `new` con la misma ruta relativa vuelve con su
    id (y sus listas, notas, acordes…) sin releer el archivo; si el archivo
    cambio por el camino, se marca para que el siguiente escaneo lo relea.
    Lo que no aparezca sigue apartado, por si vuelve. `ValueError` si `new`
    no existe o `old` sigue en su sitio.
    """
    with _SCAN_LOCK, db.connect() as conn:
        out = _rust.folders_relocate(conn, _s(old), _s(new))
    _touch()
    return out


def _roots() -> list[str]:
    # sin carpetas configuradas no se indexa nada: la app pide elegirlas primero
    with db.connect() as conn:
        return _rust.folders_roots(conn)


def roots() -> list[str]:
    """Carpetas gestionadas activas. Para pasarselas a `index_file` en bucle
    y no consultar la base por cada archivo."""
    return _roots()


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
        _rust.folders_add_exclusion(conn, pattern, kind, note)


def remove_exclusion(pattern) -> None:
    with db.connect() as conn:
        _rust.folders_remove_exclusion(conn, pattern)


def list_exclusions() -> list[dict]:
    with db.connect() as conn:
        return _rust.folders_list_exclusions(conn)


def _excluded(dir_path, dir_name, exclusions) -> bool:
    """True si esta carpeta debe saltarse.

    Los patrones NO distinguen mayusculas: escribir «secuencias» tiene que
    funcionar aunque la carpeta se llame «Secuencias».
    """
    return _rust.folders_excluded(_s(dir_path), dir_name, exclusions)


# El archivo que marca una carpeta de pistas separadas (ver danplay/stems.py).
STEMS_MANIFEST = ".danplay-pistas.json"


def audio_files(root, exclusions=None, stems=None):
    """Los archivos de audio de la carpeta, sin las excluidas ni las de pistas
    separadas. En `stems` (un dict) se apuntan estas ultimas."""
    exclusions = list_exclusions() if exclusions is None else exclusions
    paths, found, unreadable, unstorable = _rust.folders_audio_files(
        _s(root), exclusions, sorted(config.EXTENSIONS)
    )
    for manifest in unreadable:
        log.warning("no se pudo leer %s", manifest)
    if stems is not None:
        stems.update(found)
    for path in unstorable:
        _warn_unstorable(path)
    return iter(paths)


# Rutas ya avisadas: el escaneo corre solo cada pocos minutos, y repetir el
# mismo aviso cada vez llenaria el registro.
_UNSTORABLE: set = set()


def _warn_unstorable(path: str) -> None:
    """Un nombre que no es UTF-8 valido (un disco que viene de Windows o de un
    FAT viejo) no se puede guardar en la base: SQLite lo rechaza, y uno solo
    tumbaba el escaneo entero. Se salta, avisando una vez."""
    if path not in _UNSTORABLE:
        _UNSTORABLE.add(path)
        log.warning(
            "se salta %r: el nombre no es UTF-8 valido; renombralo para que DanPlay lo vea",
            path,
        )
