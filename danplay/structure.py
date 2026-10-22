"""La guia automatica: las partes de la cancion (intro, estrofas, coros,
puente, solos, final), sacadas del audio, y una guia hablada justo antes de
cada una («Estrofa 1», «Coro 1», «Puente», «Solo de guitarra»…), para
estudiarla sin mirar la pantalla.

Como se escuchan las partes (lo hace Rust, en analysis/src/structure.rs,
con las cuentas de numpy numero a numero):

1. la cancion a mono y a 22 050 Hz (con ffmpeg), hasta veinte minutos;
2. por cada trozo de 4096 muestras (cada 2048): lo fuerte que suena (RMS),
   que notas suenan (el croma: las doce notas, de 55 Hz a 5 kHz) y su color
   (el timbre: la energia en 24 bandas, resumida en 12 numeros con una DCT);
3. todo eso por compas (los que enseña la app) o, si no hay compases, cada
   2 s;
4. cuanto se parece cada compas a cada otro. Donde acaba un bloque de
   compases parecidos y empieza otro distinto (la novedad, con un nucleo de
   tablero de ajedrez) hay un cambio de parte; mejor si cae en un multiplo
   de cuatro compases, como las frases;
5. con las pistas separadas, ademas, donde entra y donde sale la voz, y que
   instrumento manda cuando no se canta (un solo);
6. las partes que se parecen se agrupan (A, B, C…) y se nombran: lo de
   antes de la voz es la intro; lo de despues, el final; lo que se repite y
   suena mas fuerte, el coro; lo otro que se repite, la estrofa; lo corto
   justo antes del coro, el pre coro; lo cantado una sola vez despues de un
   coro, el puente.

Las cuentas van en funciones que reciben arrays (`segment`, `name_sections`,
`plan_cues`): se prueban sin audio. Aqui queda lo de alrededor: leer las
pistas separadas (`stems`), decir las guias (`cues`) y el trabajo de fondo.
"""

import logging
from collections.abc import Callable, Mapping, Sequence
from typing import Any

import danplay_core as _rust
import numpy as np

from . import convert, cues, stems
from .api import jobs

log = logging.getLogger(__name__)

AUTO_JOB = "auto-cue"

RATE = 22050
WINDOW = 4096
HOP = 2048
# Lo que se escucha como mucho de una cancion.
MAX_SECONDS = 20 * 60.0
# Las pistas separadas solo hacen falta para saber lo fuerte que suenan.
STEM_RATE = 8000
BANDS = 24
COEFS = 12
CHUNK = 256  # trozos que se pasan a la vez por la FFT

# Sin compases (o con menos de estos), trozos de 2 s.
MIN_BARS = 8
WINDOW_UNIT = 2.0
# La mitad del nucleo de la novedad, en compases (o trozos).
KERNEL = 4
# Entre dos cambios de parte, al menos esto.
MIN_GAP = 4
# Una parte de menos de esto se junta con la vecina que mas se le parece.
MIN_UNITS = 2
# Lo que se tienen que parecer dos partes para ser «la misma»: el coseno de
# lo que suenan de media, ya sin lo que suena en toda la cancion. Medido con
# partes sinteticas: dos distintas no pasan de 0,4 y dos iguales con mucho
# ruido bajan a 0,75.
SAME = 0.7
# Hay voz en un compas si suena a mas de esto de lo que suena cantando (su
# percentil 95: asi no cuenta lo que se cuela de los otros instrumentos).
VOICE_FLOOR = 0.12
# Un solo: un instrumento que se lleva al menos esto de lo que suena, y
# bastante mas que en el resto de la cancion.
SOLO_SHARE = 0.35
SOLO_OVER = 1.25
# Una intro o un final sin voz conocida: bastante mas flojos que la cancion.
QUIET = 0.7
# Las guias: la primera parte que se anuncia, el aire que se deja entre una
# y otra, y el que se deja antes de la parte si no se sabe el pulso.
FIRST = 1.0
GAP = 0.15
AIR = 0.5

VOCALS = ("vocals", "lead", "backing")
SOLOS = {
    "guitar": "Solo de guitarra",
    "eguitar": "Solo de guitarra",
    "aguitar": "Solo de guitarra",
    "piano": "Solo de piano",
    "keys": "Solo de teclado",
    "organ": "Solo de órgano",
    "drums": "Solo de batería",
    "percussion": "Solo de percusión",
    "bass": "Solo de bajo",
    "strings": "Solo de cuerdas",
    "brass": "Solo de metales",
    "wind": "Solo de vientos",
}

Progress = Callable[..., None]


class StructureError(jobs.JobError):
    """No se pudo hacer la guia automatica (por que, en castellano)."""


# ------------------------------------------------------------ escuchar


def _decode(path: str, rate: int = RATE, longest: float = MAX_SECONDS) -> np.ndarray:
    """El audio a mono, a esa frecuencia y hasta `longest` segundos."""
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise StructureError("hace falta ffmpeg para escuchar la canción")
    try:
        return _rust.audio_decode_mono(ffmpeg, path, int(rate), float(longest), 600.0)
    except OSError as e:
        raise StructureError("no se pudo leer la canción") from e


def frames(x: np.ndarray, rate: int = RATE) -> dict[str, np.ndarray]:
    """Lo que suena en cada trozo (ventana de Hann de 4096, cada 2048): su
    centro en segundos (`times`), lo fuerte (`rms`), el croma (12 notas,
    comprimido y de largo 1) y el timbre (12 coeficientes de 24 bandas,
    normalizados en toda la cancion)."""
    return _rust.structure_frames(np.ascontiguousarray(x, np.float32), int(rate))


def units_from(
    bars: Sequence[float] | None, duration: float
) -> tuple[list[tuple[float, float]], int | None]:
    """Los trozos en que se mira la cancion: (inicio, fin) de cada uno, y
    cual es el primer compas (None si no son compases).

    Con al menos 8 compases, los compases; lo de antes del primero y lo de
    despues del ultimo (o un hueco entre dos), en trozos de un compas. Si
    no, trozos de 2 s."""
    return _rust.structure_units_from([float(b) for b in bars or []], float(duration))


def _units(units) -> list[tuple[float, float]]:
    return [(float(a), float(b)) for a, b in units]


def _matrix(values) -> np.ndarray:
    values = np.asarray(values, np.float64)
    if values.ndim == 1:
        values = values[:, None]
    return np.ascontiguousarray(values)


def pool(times: np.ndarray, values: np.ndarray, units: Sequence[tuple[float, float]]):
    """La media de lo de cada trozo de audio en cada unidad (el mas cercano
    si una unidad es mas corta que un trozo)."""
    return _rust.structure_pool(
        np.ascontiguousarray(times, np.float64), _matrix(values), _units(units)
    )


def unit_rms(x: np.ndarray, rate: int, units: Sequence[tuple[float, float]]) -> np.ndarray:
    """Lo fuerte que suena el audio en cada unidad (RMS)."""
    return _rust.structure_unit_rms(np.ascontiguousarray(x, np.float32), int(rate), _units(units))


# ------------------------------------------------------------ las partes


def _prepared(block) -> np.ndarray:
    """Cada unidad sin lo que suena en toda la cancion, y de largo 1: el
    producto de dos es su coseno."""
    return _rust.structure_prepared(_matrix(block))


def _merge_short(spans: list[tuple[int, int]], blocks: list[np.ndarray]) -> list[tuple[int, int]]:
    """Las partes de menos de MIN_UNITS, con la vecina que mas se les parece."""
    return _rust.structure_merge_short(
        [(int(a), int(b)) for a, b in spans], [_matrix(b) for b in blocks]
    )


def segment(
    features: Sequence[np.ndarray],
    rms: Sequence[float] | np.ndarray,
    vocals: Sequence[float] | np.ndarray | None = None,
    sources: Mapping[str, Sequence[float] | np.ndarray] | None = None,
    units: Sequence[tuple[float, float]] | None = None,
    phase: int | None = 0,
) -> list[dict]:
    """Las partes de la cancion, por unidades (compases o trozos de 2 s).

    `features`: un array (unidades x rasgos) por cada tipo de rasgo (el
    croma, el timbre): lo que se parecen dos unidades es la media de lo que
    se parecen en cada uno. `rms`: lo fuerte de cada unidad. `vocals`: lo
    fuerte de la voz (de las pistas separadas), si se sabe. `sources`: lo
    fuerte de cada instrumento separado, para los solos. `units`: (inicio,
    fin) de cada una en segundos (sin ellas, un segundo cada una). `phase`:
    la unidad del primer compas (None si no son compases).

    Cada parte: {first, last (unidades, la ultima fuera), start, end
    (segundos), cluster («A», «B»…), rms, quiet, vocal (cuanto se canta, si
    se sabe), lead (el instrumento que manda, si es un solo)}."""

    def vector(v):
        return np.ascontiguousarray(np.asarray(v, np.float64).reshape(-1))

    return _rust.structure_segment(
        [_matrix(f) for f in features],
        vector(rms),
        None if vocals is None else vector(vocals),
        [(str(k), vector(v)) for k, v in (sources or {}).items()],
        None if units is None else _units(units),
        phase,
    )


def name_sections(segments: Sequence[Mapping[str, Any]], bar: float = 2.0) -> list[dict]:
    """El nombre de cada parte, en castellano: [{start, end, label}].

    Por orden: lo de antes de la primera parte cantada es la intro y lo de
    despues de la ultima, el final (sin saber de la voz, la primera o la
    ultima si suenan bastante mas flojas que la cancion). Entre medias, lo
    que no se canta es un solo (si un instrumento manda) o un instrumental;
    el grupo que se repite y suena mas fuerte, el coro; otro que se repite,
    la estrofa; lo corto (hasta cuatro compases de `bar` segundos) justo
    antes de un coro, el pre coro; lo cantado una sola vez despues de un
    coro, el puente. Lo que quede: estrofa antes del primer coro, puente
    despues. Las seguidas con el mismo nombre se juntan, y lo que se repite
    se numera («Coro 1», «Coro 2»)."""
    return _rust.structure_name_sections(list(segments), float(bar))


def plan_cues(
    sections: Sequence[Mapping[str, Any]],
    clip_seconds: Mapping[str, float],
    beat: float | None = None,
) -> list[dict]:
    """Cuando suena la guia de cada parte: [{t, text}].

    Acaba un pulso (`beat`, o medio segundo) antes de que empiece su parte;
    nunca antes del principio ni encima de la anterior (entonces, justo
    detras de ella, y como mucho acabando cuando empieza su parte; si ni
    asi cabe, esa parte no se anuncia). La que empieza en el primer segundo
    (la intro) no hace falta anunciarla."""
    return _rust.structure_plan_cues(
        [(float(s["start"]), str(s["label"])) for s in sections],
        [(str(k), float(v)) for k, v in clip_seconds.items()],
        float(beat) if beat else None,
    )


# ------------------------------------------------------------ la cancion


def _stem_levels(
    song: Mapping[str, Any], units: Sequence[tuple[float, float]]
) -> tuple[np.ndarray | None, dict[str, np.ndarray] | None]:
    """Lo fuerte de la voz y de cada instrumento en cada unidad, si la
    cancion tiene sus pistas separadas enteras. (None, None) si no."""
    data = stems.info(song) if song.get("stems") else None
    if not data or not data.get("complete"):
        return None, None
    levels = {}
    for t in data["tracks"]:
        if t["source"] not in stems.SOURCES or t.get("role"):
            continue
        try:
            levels[t["source"]] = unit_rms(_decode(t["path"], STEM_RATE), STEM_RATE, units)
        except StructureError:
            log.warning("no se pudo leer la pista %s", t["path"])
            return None, None
    sung = [levels[k] for k in VOCALS if k in levels]
    if sung:
        vocals = np.sqrt(np.sum(np.square(sung), axis=0))
    elif any(k in VOCALS for k in data.get("dropped") or []):
        vocals = np.zeros(len(units))  # la red no oyo voz: es instrumental
    else:
        return None, None
    return vocals, {k: v for k, v in levels.items() if k not in VOCALS}


def analyse(
    song: Mapping[str, Any],
    bars: Sequence[float] | None = None,
    beat: float | None = None,
    progress: Progress | None = None,
) -> list[dict]:
    """Las partes de la cancion, con su nombre: [{start, end, label}]."""
    progress = progress or (lambda *a, **k: None)
    progress(message="Escuchando la canción…")
    x = _decode(str(song["path"]))
    duration = x.size / RATE
    if duration < 4 * WINDOW_UNIT:
        raise StructureError("la canción es demasiado corta para buscarle partes")
    progress(message="Buscando sus partes…")
    f = frames(x)
    del x
    units, phase = units_from(bars, duration)
    rms = pool(f["times"], f["rms"], units)[:, 0]
    chroma = pool(f["times"], f["chroma"], units)
    timbre = pool(f["times"], f["timbre"], units)
    vocals, sources = _stem_levels(song, units)
    parts = segment([chroma, timbre], rms, vocals, sources, units, phase)
    if phase is not None:
        bar = float(np.median([b - a for a, b in units]))
    else:
        bar = 4 * float(beat) if beat else WINDOW_UNIT
    return name_sections(parts, bar)


def _given(sections: Sequence[Mapping[str, Any]], duration: float) -> list[dict]:
    """Las partes que ya tienen nombre (los marcadores), tal cual: en orden,
    cada una hasta la siguiente y la ultima hasta el final."""
    return _rust.structure_given(
        [(float(s["start"]), str(s.get("label") or "").strip()[:80]) for s in sections],
        float(duration),
    )


def auto_guide(
    song: Mapping[str, Any],
    voice: str,
    bars: Sequence[float] | None = None,
    beat: float | None = None,
    sections: Sequence[Mapping[str, Any]] | None = None,
    progress: Progress | None = None,
) -> dict:
    """La guia automatica de una cancion: sus partes (las que se le
    encuentran, o `sections` tal cual) y la guia hablada de cada una, dicha
    con esa voz y puesta justo antes. {sections: [{start, end, label}],
    cues: [{t, text, voice, file, seconds}]}."""
    progress = progress or (lambda *a, **k: None)
    if sections:
        duration = float(song.get("duration") or 0.0)
        if not duration:
            duration = _decode(str(song["path"])).size / RATE
        found = _given(sections, duration)
    else:
        found = analyse(song, bars, beat, progress)
    labels = list(dict.fromkeys(s["label"] for s in found if s["start"] >= FIRST))
    said: dict[str, dict] = {}
    for i, label in enumerate(labels):
        progress(i, len(labels), f"Diciendo «{label}»…")
        try:
            said[label] = cues.make(label, voice)
        except cues.CueError as e:
            raise StructureError(f"no se pudo decir «{label}»: {e}") from e
    progress(len(labels), len(labels))
    plan = plan_cues(found, {k: float(v["seconds"]) for k, v in said.items()}, beat)
    return {
        "sections": found,
        "cues": [
            {
                "t": p["t"],
                "text": p["text"],
                "voice": voice,
                "file": said[p["text"]]["file"],
                "seconds": said[p["text"]]["seconds"],
            }
            for p in plan
        ],
    }
