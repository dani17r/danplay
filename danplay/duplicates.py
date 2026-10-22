"""Deteccion de duplicados: identicos byte a byte y misma cancion en otra version.

Los grupos los hace el crate de Rust (biblioteca/src/duplicados.rs, con las
mismas puntuaciones de rapidfuzz que se usaban); resolverlos, que toca la
papelera y las carpetas de la biblioteca, sigue aqui.
"""

import logging
import os

import danplay_core as _rust

from . import config

log = logging.getLogger(__name__)

# El crate es obligatorio (antes era opcional, ~9x mas rapido que Python).
RUST = True


def partial_hash(path, n=1024 * 1024) -> str:
    """md5 de los primeros `n` bytes, o "" si no se puede leer (para
    comparar archivos, no para seguridad)."""
    return _rust.partial_hash(path, n) or ""


def hashes(paths, n=1024 * 1024) -> dict:
    """Hash de muchos archivos a la vez, en paralelo."""
    return {r: h for r, h in _rust.hashes(list(paths), n) if h}


def identical(paths) -> list[list[str]]:
    """Grupos de archivos byte a byte iguales (mismo tamaño + mismo hash)."""
    paths = list(paths)
    return [[paths[i] for i in g] for g in _rust.duplicates_identical(paths)]


def same_song(paths, threshold=0.88) -> list[list[str]]:
    """Grupos que parecen la misma cancion aunque el archivo sea distinto.

    Primero los de la misma clave (`names.match_key`), luego los parecidos
    (`fuzz.token_set_ratio` si comparten palabras, `fuzz.ratio` si no); ver
    `misma_cancion` en duplicados.rs. Dentro de cada grupo, en cualquier orden.
    """
    paths = list(paths)
    return [[paths[i] for i in g] for g in _rust.duplicates_same_song(paths, float(threshold))]


# ------------------------------------------------------------- resolucion


def name_without_suffix(name: str) -> str:
    """'Barak - Mi Gozo - r2.mp3' -> 'Barak - Mi Gozo.mp3'"""
    return _rust.duplicates_name_without_suffix(name)


def _rejection(path: str) -> str:
    """Por que una ruta no se puede tocar desde aqui, o cadena vacia."""
    from . import library

    if os.path.splitext(path)[1].lower() not in config.EXTENSIONS:
        return f"«{os.path.basename(path)}» no es un archivo de audio"
    if not library.within_roots(path):
        return f"«{os.path.basename(path)}» esta fuera de las carpetas de la biblioteca"
    return ""


def resolve(keep: str, remove: list[str], dry_run=True) -> dict:
    """Se queda con una de las copias y manda las demas a la papelera.

    Si la que se conserva llevaba el sufijo ' - r' (el que se pone al detectar
    duplicados), se lo quita, siempre que el nombre limpio quede libre.

    Todas las rutas tienen que ser audio y estar dentro de las carpetas
    gestionadas: esto lo llama la API con rutas que vienen de fuera, y sin
    la comprobacion borraba cualquier archivo del disco. `dry_run` es True
    por defecto por lo mismo: borrar solo cuando se pide expresamente.
    """
    from . import library

    keep = os.path.abspath(keep)
    remove = [os.path.abspath(b) for b in remove if os.path.abspath(b) != keep]
    if not os.path.isfile(keep):
        return {"ok": False, "reason": "el archivo a conservar no existe"}
    for path in [keep, *remove]:
        why = _rejection(path)
        if why:
            return {"ok": False, "reason": why}

    deleted, failures = [], []
    if not dry_run:
        for b in remove:
            if not os.path.isfile(b):
                continue
            r = library.trash_path(b)
            if r.get("ok"):
                deleted.append(b)
            else:
                failures.append({"path": b, "reason": r.get("error", "")})
    else:
        deleted = [b for b in remove if os.path.isfile(b)]

    # quitarle el sufijo al que se queda
    folder = os.path.dirname(keep)
    actual = os.path.basename(keep)
    clean_name = name_without_suffix(actual)
    target = keep
    renamed = False
    if clean_name != actual:
        candidate = os.path.join(folder, clean_name)
        if not os.path.exists(candidate) or os.path.abspath(candidate) in deleted:
            if not dry_run:
                try:
                    os.rename(keep, candidate)
                    target, renamed = candidate, True
                except OSError as e:
                    failures.append({"path": keep, "reason": str(e)})
            else:
                target, renamed = candidate, True

    return {
        "ok": not failures,
        "kept": target,
        "renamed": renamed,
        "final_name": os.path.basename(target),
        "deleted": deleted,
        "failures": failures,
        "dry_run": dry_run,
    }
