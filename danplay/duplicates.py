# -*- coding: utf-8 -*-
"""Deteccion de duplicados: identicos byte a byte y misma cancion en otra version."""
import hashlib, os, re
from collections import defaultdict
from . import names

try:
    import danplay_core as _rust          # crate en Rust: ~9x mas rapido
    RUST = True
except ImportError:
    _rust, RUST = None, False


def partial_hash(path, n=1024 * 1024) -> str:
    h = hashlib.md5()
    try:
        with open(path, "rb") as f:
            h.update(f.read(n))
    except OSError:
        return ""
    return h.hexdigest()


def hashes(paths, n=1024 * 1024) -> dict:
    """Hash de muchos archivos a la vez. Usa Rust en paralelo si esta compilado."""
    if RUST:
        return {r: h for r, h in _rust.hashes(list(paths), n) if h}
    return {r: h for r in paths if (h := partial_hash(r, n))}


def identical(paths) -> list[list[str]]:
    """Grupos de archivos byte a byte iguales (mismo tamaño + mismo hash)."""
    by_size = defaultdict(list)
    for r in paths:
        try:
            by_size[os.path.getsize(r)].append(r)
        except OSError:
            pass
    suspects = [r for size_of, rs in by_size.items() if len(rs) > 1 and size_of for r in rs]
    tabla = hashes(suspects)
    groups = []
    for size_of, rs in by_size.items():
        if len(rs) < 2 or size_of == 0:
            continue
        by_hash = defaultdict(list)
        for r in rs:
            if tabla.get(r):
                by_hash[tabla[r]].append(r)
        groups += [g for g in by_hash.values() if len(g) > 1]
    return groups


def same_song(paths, umbral=0.88) -> list[list[str]]:
    """Grupos que parecen la misma cancion aunque el archivo sea distinto."""
    from rapidfuzz import fuzz
    match_keys = {r: names.match_key(os.path.basename(r)) for r in paths}

    # primero exactos por clave normalizada
    by_match_key = defaultdict(list)
    for r, k in match_keys.items():
        if k:
            by_match_key[k].append(r)
    groups = [g for g in by_match_key.values() if len(g) > 1]
    ya = {r for g in groups for r in g}

    # Luego los parecidos. La comparacion es de todos contra todos, pero
    # `token_set_ratio` cuesta unas diez veces mas que una comparacion simple
    # porque parte las dos cadenas en palabras CADA VEZ. Se evita en casi
    # todas las parejas, sin cambiar ni un resultado, apoyandose en como esta
    # definida la puntuacion:
    #
    #   - Si las dos claves comparten alguna palabra, hay que calcularla
    #     entera. Un indice invertido palabra -> claves da esas parejas
    #     directamente, y son pocas.
    #   - Si no comparten NINGUNA palabra, la interseccion es vacia, los dos
    #     primeros terminos de la formula valen cero y la puntuacion se reduce
    #     a comparar las cadenas tal cual. Como `match_key` ya las devuelve
    #     ordenadas y sin repetir, esa comparacion simple da exactamente el
    #     mismo numero. Comprobado sobre cientos de miles de parejas.
    #
    # Los grupos que salen son identicos; lo unico que cambia es cuanto tarda.
    remaining = [r for r in paths if r not in ya and match_keys[r]]
    if len(remaining) > 1:
        from rapidfuzz import process
        keys = [match_keys[r] for r in remaining]
        words = [frozenset(k.split()) for k in keys]
        by_word = defaultdict(list)
        for j, ws in enumerate(words):
            for w in ws:
                by_word[w].append(j)

        minimo = umbral * 100
        used = set()
        for i, a in enumerate(remaining):
            if a in used:
                continue
            group = [a]

            # 1) comparten alguna palabra: puntuacion completa, pocas parejas
            share = set()
            for w in words[i]:
                share.update(j for j in by_word[w] if j > i)
            for j in share:
                if remaining[j] not in used and \
                        fuzz.token_set_ratio(keys[i], keys[j]) >= minimo:
                    group.append(remaining[j])

            # 2) sin ninguna palabra en comun: comparacion simple, en C
            for _, _, j in process.extract(keys[i], keys, scorer=fuzz.ratio,
                                           score_cutoff=minimo, limit=None):
                if j > i and j not in share and not (words[i] & words[j]) \
                        and remaining[j] not in used:
                    group.append(remaining[j])

            if len(group) > 1:
                groups.append(group)
                used.update(group)
    return groups


# ------------------------------------------------------------- resolucion

DUP_SUFFIX = re.compile(r"\s*-\s*r\d*$", re.IGNORECASE)


def name_without_suffix(name: str) -> str:
    """'Barak - Mi Gozo - r2.mp3' -> 'Barak - Mi Gozo.mp3'"""
    stem, ext = os.path.splitext(name)
    clean_name = DUP_SUFFIX.sub("", stem).strip()
    return (clean_name or stem) + ext


def resolve(keep: str, remove: list[str], dry_run=False) -> dict:
    """Se queda con una de las copias y elimina las demas.

    Si la que se conserva llevaba el sufijo ' - r' (el que se pone al detectar
    duplicados), se lo quita, siempre que el nombre limpio quede libre.
    """
    keep = os.path.abspath(keep)
    remove = [os.path.abspath(b) for b in remove if os.path.abspath(b) != keep]
    if not os.path.isfile(keep):
        return {"ok": False, "reason": "el archivo a conservar no existe"}

    deleted, failures = [], []
    if not dry_run:
        for b in remove:
            try:
                if os.path.isfile(b):
                    os.remove(b)
                    deleted.append(b)
            except OSError as e:
                failures.append({"path": b, "reason": str(e)})
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

    return {"ok": not failures, "kept": target, "renamed": renamed,
            "final_name": os.path.basename(target),
            "deleted": deleted, "failures": failures, "dry_run": dry_run}
