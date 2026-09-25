"""Las carpetas gestionadas, el escaneo, la busqueda y lo que se hace con la
biblioteca entera: importar, convertir, duplicados."""

import logging
import os
from pathlib import Path
from typing import Annotated

from fastapi import APIRouter, Body, HTTPException, Query
from fastapi.responses import JSONResponse

from ... import config, convert, duplicates, external, ingest, library, watcher
from .. import jobs
from ..common import _relative, _started
from ..models import (
    ConvertIn,
    ExclusionIn,
    FolderIn,
    IdsIn,
    ImportIn,
    PathIn,
    RelocateIn,
    ResolveIn,
)

log = logging.getLogger(__name__)

router = APIRouter()


@router.get("/api/folders")
def folders():
    return {
        "folders": library.list_folders(),
        "exclusions": library.list_exclusions(),
        "always_excluded": library.DEFAULT_EXCLUDES,
    }


@router.post("/api/check-folder")
def check_folder(body: Annotated[PathIn, Body()]):
    """Mira si la carpeta repite musica ya indexada, antes de añadirla."""
    path = body.path
    if not os.path.isdir(os.path.expanduser(path)):
        raise HTTPException(400, "esa ruta no existe")
    return {"notice": library.check_overlap(path)}


@router.post("/api/folders")
def add_folder(body: Annotated[FolderIn, Body()]):
    """Añade una carpeta. Es idempotente: repetir la misma no duplica nada.

    accion:
      ya_estaba   -> ya indexada (o cubierta por otra); no se toca nada
      reemplaza   -> la nueva contiene a otra ya indexada; sustituye a la interna
      confirmar   -> parece una copia de otra distinta; hace falta force
      agregada    -> añadida
    """
    path = body.path
    if not os.path.isdir(os.path.expanduser(path)):
        raise HTTPException(400, "esa ruta no existe")

    # ¿Es una carpeta gestionada que se movio? Entonces no es una nueva: es
    # la misma en otro sitio, y sus canciones vuelven con su id, sus listas y
    # sus notas. Es lo que pasa cuando alguien mueve su musica y la «vuelve a
    # importar» desde la bienvenida.
    moved_from = library.relocation_for(path)
    if moved_from:
        r = library.relocate_folder(moved_from, path)
        watcher.folders_changed()
        notice = {
            "kind": "relocated",
            "other": moved_from,
            "back": r["back"],
            "message": f"es «{moved_from}», que se habia movido: sus canciones "
            "vuelven con sus listas y sus notas",
        }
        return {"action": "relocated", "notice": notice, **folders()}

    notice = library.check_overlap(path)
    kind = (notice or {}).get("kind")

    if kind in ("same", "inside"):
        return {"action": "already_there", "notice": notice, **folders()}

    if kind == "contains" and notice:
        library.remove_folder(notice["other"])
        library.add_folder(path, body.label)
        watcher.folders_changed()
        return {"action": "replaced", "notice": notice, **folders()}

    if kind == "copy" and not body.force:
        return {"action": "confirm", "notice": notice, **folders()}

    if not library.add_folder(path, body.label):
        raise HTTPException(400, "esa ruta no existe")
    watcher.folders_changed()
    return {"action": "added", "notice": notice, **folders()}


@router.delete("/api/folders")
def remove_folder(path: Annotated[str, Query()]):
    library.remove_folder(path)
    watcher.folders_changed()
    return folders()


@router.post("/api/folders/relocate")
def relocate_folder(body: Annotated[RelocateIn, Body()]):
    """Una carpeta gestionada que ya no esta, en su sitio nuevo. Sus canciones
    vuelven con su id; despues conviene un escaneo para lo que haya cambiado."""
    if not os.path.isdir(os.path.expanduser(body.new)):
        raise HTTPException(400, "esa ruta no existe")
    try:
        r = library.relocate_folder(body.old, body.new)
    except ValueError as e:
        raise HTTPException(400, str(e)) from e
    watcher.folders_changed()
    return {**r, **folders()}


@router.post("/api/exclusions")
def add_exclusion(body: Annotated[ExclusionIn, Body()]):
    library.add_exclusion(body.pattern, body.kind, body.note)
    return folders()


@router.delete("/api/exclusions")
def remove_exclusion(pattern: Annotated[str, Query()]):
    library.remove_exclusion(pattern)
    return folders()


@router.post("/api/scan", status_code=202)
def scan():
    """Arranca el escaneo y contesta al momento; el resultado, en
    GET /api/jobs/escaneo. El puente de Rust corta a los 60 s y con una
    biblioteca grande el escaneo tarda mas."""

    def work(progress):
        r = library.scan(progress=lambda n, total=None: progress(n, total))
        progress(message=f"{r['total']} canciones ({r['added_count']} nuevas)")
        return {**r, "stats": library.stats_of()}

    return _started(*jobs.start("escaneo", work, failure="el escaneo fallo"))


# ---------------------------------------------------------------- busqueda


@router.get("/api/search")
def search(
    q: str = "",
    sort: str = "artist",
    desc: bool = False,
    limit: Annotated[int, Query(ge=1, le=20000)] = 200,
    from_key: Annotated[int, Query(ge=0)] = 0,
    artist: str = "",
    album: str = "",
    genre: str = "",
    folder: str = "",
    only_favorites: bool = False,
    min_stars: int = 0,
):
    """Una pagina de la busqueda, en filas ligeras (sin letra, acordes ni
    estudio: van en GET /api/song/{id}), y `count`: cuantas cumplen la
    busqueda en total, para que la interfaz cargue el resto por detras en vez
    de cortarse en silencio en la primera pagina."""
    filters = {
        k: v
        for k, v in {"artist": artist, "album": album, "genre": genre, "folder": folder}.items()
        if v
    }
    rows = library.search(
        q,
        filters,
        sort,
        limit,
        from_key,
        only_favorites=only_favorites,
        min_stars=min_stars,
        desc=desc,
        light=True,
    )
    count = library.count(q, filters, only_favorites=only_favorites, min_stars=min_stars)
    return {"total": len(rows), "count": count, "songs": rows}


@router.get("/api/facets")
def facets():
    """Valores disponibles para los filtros, y por que se puede ordenar.

    La lista de campos sale del nucleo para que la interfaz no tenga que
    repetirla: si aqui se añade uno, aparece solo en el buscador avanzado.
    """
    return {
        **library.facets(),
        "sorts": library.sort_options(),
        "filters": sorted(set(library.FILTER_FIELDS.values())),
        "numeric": sorted(set(library.NUMERIC_FIELDS.values())),
    }


# ---------------------------------------------------------------- importar


@router.get("/api/inbox")
def inbox():
    # Consultar no crea nada: la interfaz pregunta al arrancar, y crear aqui
    # la Entrada (con sus carpetas de mas arriba) hacia reaparecer, vacia, una
    # carpeta de musica que se acababa de mover (ver library.ensure_folder).
    if not config.INBOX.is_dir():
        return {"files": [], "total": 0}
    files = [
        {"name": p.name, "bytes": p.stat().st_size, "convertible": convert.needs_convert(str(p))}
        for p in sorted(config.INBOX.iterdir())
        if p.is_file() and p.suffix.lower() in config.EXTENSIONS
    ]
    return {"files": files, "total": len(files)}


@router.post("/api/import", status_code=202)
def run_import(body: Annotated[ImportIn | None, Body()] = None):
    """Procesa Entrada/ (tambien en simulacion) como trabajo «importacion»:
    identificar puede tardar (huella, IA) y se contesta al momento."""
    body = body or ImportIn()
    dry_run, convert_mp3 = body.dry_run, body.convert

    def work(progress):
        rs = ingest.process_inbox(dry_run=dry_run, convert_mp3=convert_mp3, progress=progress)
        roots = library.roots()
        for r in rs:  # que aparezcan sin reescanear todo
            if r.target and r.action not in ("dry_run", "error"):
                library.index_file(str(r.target), roots=roots)
        return {
            "results": [
                {
                    "source_path": r.source_path.name,
                    "target": _relative(r.target) if r.target else "",
                    "artist": r.artist,
                    "title": r.title,
                    "source": r.source,
                    "confidence": r.confidence,
                    "action": r.action,
                    "warnings": r.warnings,
                }
                for r in rs
            ]
        }

    return _started(*jobs.start("importacion", work, failure="la importacion fallo"))


@router.get("/api/convertible")
def convertible_files():
    c = convert.candidates()
    return {
        "total": len(c),
        "files": [str(Path(x).relative_to(config.LIBRARY)) for x in c[:200]],
        "protected": sorted(config.NEVER_CONVERT),
    }


@router.post("/api/convert")
def convert_batch(body: Annotated[ConvertIn | None, Body()] = None):
    """Con `dry_run` (lo normal), que se convertiria, al momento. Sin el, el
    trabajo «conversion»: convertir una biblioteca son minutos."""
    body = body or ConvertIn()
    quality = body.quality or config.MP3_QUALITY
    if body.dry_run:
        return convert.convert_batch(quality=quality, keep_original=body.keep, dry_run=True)
    keep = body.keep

    def work(progress):
        return convert.convert_batch(
            quality=quality, keep_original=keep, dry_run=False, progress=progress
        )

    snap, already = jobs.start("conversion", work, failure="la conversion fallo")
    return JSONResponse(_started(snap, already), status_code=202)


@router.post("/api/duplicates/resolve")
def resolve_duplicate(body: Annotated[ResolveIn, Body()]):
    """Conserva una copia y manda las demas a la papelera. Reindexa despues.

    Las rutas vienen de fuera, asi que `duplicates.resolve` comprueba que
    esten dentro de las carpetas gestionadas y que sean audio. Aqui solo se
    completan las relativas.
    """
    base = str(config.LIBRARY)

    def full_path(r):
        return r if os.path.isabs(r) else os.path.join(base, r)

    keep = full_path(body.keep)
    remove = [full_path(b) for b in body.remove]
    dry_run = body.dry_run

    def work():
        r = duplicates.resolve(keep, remove, dry_run)
        if r["ok"] and not dry_run:
            # Solo se pone al dia lo que ha cambiado. Antes esto lanzaba un
            # escaneo COMPLETO —releer las etiquetas de toda la biblioteca—
            # para reflejar que se han borrado una o dos copias.
            for gone in r["deleted"]:
                library.forget_path(gone)
            if r["kept"] != keep:  # se le quito el sufijo ' - r'
                library.forget_path(keep)
            library.index_file(r["kept"])
        return r

    r = work()
    if not r["ok"] and r.get("reason"):
        raise HTTPException(400, r["reason"])
    if r["ok"] and not dry_run:
        r["stats"] = library.stats_of()
    return r


@router.post("/api/duplicates/scan", status_code=202)
def duplicates_scan():
    """Busca duplicados como trabajo «duplicados»: con miles de canciones la
    comparacion de todos contra todos pasa del minuto."""

    def work(progress):
        progress(0, 2, "buscando copias identicas y versiones de la misma cancion")
        report = duplicates_report()
        progress(
            2, 2, f"{len(report['identical'])} grupos identicos, {len(report['similar'])} parecidos"
        )
        return report

    return _started(*jobs.start("duplicados", work, failure="la busqueda de duplicados fallo"))


def duplicates_report():
    """Grupos de duplicados, con los datos de cada cancion para poder oirlas."""

    def work():
        index = library.brief_by_path()
        paths = list(index)

        def enrich(groups):
            out = []
            for g in groups:
                items = []
                for r in sorted(g):
                    c = index.get(r)
                    items.append(
                        {
                            "id": c["id"] if c else None,
                            "path": r,
                            "relative": os.path.relpath(r, config.LIBRARY),
                            "file": os.path.basename(r),
                            "artist": (c or {}).get("artist", ""),
                            "title": (c or {}).get("title", ""),
                            "duration": (c or {}).get("duration", 0),
                            "bitrate": (c or {}).get("bitrate", 0),
                            "size": (c or {}).get("size", 0),
                            "stars": (c or {}).get("stars", 0),
                            "favorite": (c or {}).get("favorite", 0),
                            "has_suffix": duplicates.name_without_suffix(os.path.basename(r))
                            != os.path.basename(r),
                        }
                    )
                # se sugiere la de mejor calidad como candidata a conservar
                # a igualdad de calidad, gana la que ya tiene el nombre limpio
                best = max(items, key=lambda t: (t["bitrate"], t["size"], not t["has_suffix"]))
                out.append({"items": items, "suggested": best["path"]})
            return out

        return {
            "identical": enrich(duplicates.identical(paths)),
            "similar": enrich(duplicates.same_song(paths, config.DUPLICATE_THRESHOLD)),
        }

    return work()


@router.post("/api/songs/locate")
def locate_songs(body: Annotated[IdsIn, Body()]):
    """Donde esta ahora cada cancion: `{paths: {id: ruta | null}}`.

    La usa la cola de Rust cuando cambia la biblioteca, de una vez para toda
    la cola: una cancion movida sigue sonando desde su sitio nuevo y la que ya
    no esta (null) sale de la cola. Los ids negativos son archivos abiertos
    desde fuera de la biblioteca.
    """
    found = library.paths_of(i for i in body.ids if i > 0)
    for cid in (i for i in body.ids if i < 0):
        c = external.resolve(cid)
        found[cid] = c["path"] if c and os.path.isfile(c["path"]) else None
    return {"paths": {str(k): v for k, v in found.items()}}
