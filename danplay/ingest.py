"""Tuberia de importacion.

Orden de resolucion, de mas fiable a menos:
  1. etiquetas ID3 ya presentes
  2. huella acustica (AcoustID -> MusicBrainz)
  3. heuristico contra los artistas que ya existen en la biblioteca
  4. IA (DeepInfra)
  5. si nada funciona -> carpeta Revisar/

Lo hace el crate de Rust (library/src/ingest.rs, con el puente en
core/src/ingest.rs), paso a paso como antes. Aqui quedan el `Result`, lo que
`process` toma de `config` si no se le dice y la puerta de cada funcion, con
los mismos nombres. Rust busca en este modulo, en el momento de usarlo, lo de
fuera (`tags`, `fingerprint`, `ai`, `convert`, `names`, `shutil`, `config`,
`Path`, `log`) y los pasos (`_from_tags`, `_from_fingerprint`,
`_from_heuristic`, `_from_ai`, `resolve`, `process`): lo que las pruebas
cambian se sigue usando.
"""

import logging
import shutil  # noqa: F401  (Rust lo busca aqui: mover cada archivo)
import sys
from dataclasses import dataclass, field
from pathlib import Path

import danplay_core as _rust

# los busca Rust aqui, en el momento de usarlos (las pruebas los cambian)
from . import ai, config, convert, fingerprint, names, tags  # noqa: F401

log = logging.getLogger(__name__)

HEURISTIC_THRESHOLD, AI_THRESHOLD, _categories = _rust.ingest_constants()

CATEGORY_FOLDER = dict(_categories)

# este modulo: lo de fuera que usa Rust se busca en el cada vez
_SELF = sys.modules[__name__]


@dataclass
class Result:
    source_path: Path
    target: Path | None = None
    artist: str = ""
    title: str = ""
    feat: str = ""
    extra: str = ""
    source: str = ""
    confidence: float = 0.0
    action: str = ""  # movido | simulado | revisar | error
    note: str = ""
    warnings: list = field(default_factory=list)


def _from_tags(path) -> dict | None:
    return _rust.ingest_from_tags(_SELF, path)


def _from_fingerprint(path) -> dict | None:
    return _rust.ingest_from_fingerprint(_SELF, path)


def _from_heuristic(path, vocab) -> dict | None:
    return _rust.ingest_from_heuristic(_SELF, path, vocab)


def _from_ai(path, vocab) -> dict | None:
    return _rust.ingest_from_ai(_SELF, path, vocab)


def _safe(step, *args):
    """Un escalon que falla no debe tumbar la cascada entera.

    Paso esto de verdad: AcoustID devolvia un error de servicio, la excepcion
    subia hasta arriba y NADA se podia importar. Un escalon roto solo significa
    que decide el siguiente. (`resolve`, en Rust, hace lo mismo con cada uno.)
    """
    try:
        return step(*args)
    except Exception:  # noqa: BLE001
        return None


def resolve(path, vocab) -> dict:
    """Aplica la cascada y devuelve el mejor dato disponible."""
    return _rust.ingest_resolve(_SELF, path, vocab)


def process(
    path, vocab=None, dry_run=False, write_tags=None, convert_mp3=None, known=None
) -> Result:
    """Identifica, renombra y archiva un archivo.

    `known` salta la cascada: es {artist, title, feat, source} ya decidido
    por quien llama. Lo usan las descargas, donde el nombre lo pone YouTube
    (ver `names.from_video`): la huella acustica archivaba una «Drum Cam» de
    una cancion bajo el artista original, que no es quien la toca.
    """
    path = Path(path)
    convert_mp3 = config.CONVERT_TO_MP3 if convert_mp3 is None else convert_mp3
    vocab = vocab if vocab is not None else names.vocabulary(config.ARTISTS_DIR)
    write_tags = config.WRITE_TAGS if write_tags is None else write_tags
    done = _rust.ingest_process(_SELF, path, vocab, dry_run, write_tags, convert_mp3, known)
    return Result(**done)


def process_inbox(dry_run=False, limit=None, convert_mp3=None, progress=None) -> list[Result]:
    """Procesa todo lo que haya en la carpeta Entrada/.

    `progress(done, total, name)` cuenta el avance (el trabajo «importacion»)."""
    return _rust.ingest_process_inbox(_SELF, dry_run, limit, convert_mp3, progress)
