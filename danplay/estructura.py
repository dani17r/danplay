"""La guia automatica: las partes de la cancion (intro, estrofas, coros,
puente, solos, final), sacadas del audio, y una guia hablada justo antes de
cada una («Estrofa 1», «Coro 1», «Puente», «Solo de guitarra»…), para
estudiarla sin mirar la pantalla.

Como se escuchan las partes, solo con numpy:

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
`plan_cues`): se prueban sin audio.
"""

import itertools
import logging
import math
import subprocess
from collections import Counter
from collections.abc import Callable, Mapping, Sequence
from typing import Any

import numpy as np

from . import convert, cues, stems
from .api import jobs

log = logging.getLogger(__name__)

AUTO_JOB = "guia-automatica"

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


class EstructuraError(jobs.JobError):
    """No se pudo hacer la guia automatica (por que, en castellano)."""


# ------------------------------------------------------------ escuchar


def _decode(path: str, rate: int = RATE, longest: float = MAX_SECONDS) -> np.ndarray:
    """El audio a mono, a esa frecuencia y hasta `longest` segundos."""
    ffmpeg = convert.tool("ffmpeg")
    if not ffmpeg:
        raise EstructuraError("hace falta ffmpeg para escuchar la canción")
    try:
        r = subprocess.run(
            [
                ffmpeg,
                "-v",
                "error",
                "-nostdin",
                "-i",
                path,
                "-t",
                str(longest),
                "-ac",
                "1",
                "-ar",
                str(rate),
                "-f",
                "f32le",
                "-",
            ],
            capture_output=True,
            timeout=600,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        raise EstructuraError("no se pudo leer la canción") from e
    if r.returncode != 0 or not r.stdout:
        raise EstructuraError("no se pudo leer la canción")
    return np.frombuffer(r.stdout, np.float32)


def _dct(n: int, k: int) -> np.ndarray:
    """La DCT-II (sus `k` primeras filas) de `n` valores."""
    i = np.arange(n)
    return np.cos(np.pi / n * (i[None, :] + 0.5) * np.arange(k)[:, None])


def frames(x: np.ndarray, rate: int = RATE) -> dict[str, np.ndarray]:
    """Lo que suena en cada trozo (ventana de Hann de 4096, cada 2048): su
    centro en segundos (`times`), lo fuerte (`rms`), el croma (12 notas,
    comprimido y de largo 1) y el timbre (12 coeficientes de 24 bandas,
    normalizados en toda la cancion)."""
    x = np.asarray(x, np.float32)
    if x.size < WINDOW:
        x = np.pad(x, (0, WINDOW - x.size))
    view = np.lib.stride_tricks.sliding_window_view(x, WINDOW)[::HOP]
    m = view.shape[0]
    hann = np.hanning(WINDOW).astype(np.float32)
    freqs = np.fft.rfftfreq(WINDOW, 1 / rate)
    # cada frecuencia de 55 Hz a 5 kHz, a su nota (do = 0)
    pitched = np.flatnonzero((freqs >= 55) & (freqs <= 5000))
    notes = np.round(69 + 12 * np.log2(freqs[pitched] / 440)).astype(int) % 12
    to_chroma = np.zeros((freqs.size, 12), np.float32)
    to_chroma[pitched, notes] = 1
    # las bandas, repartidas como el oido (logaritmicas)
    edges = np.geomspace(40.0, min(10000.0, 0.95 * rate / 2), BANDS + 1)
    to_bands = np.zeros((freqs.size, BANDS), np.float32)
    for b in range(BANDS):
        inside = (freqs >= edges[b]) & (freqs < edges[b + 1])
        if not inside.any():
            inside[np.argmin(np.abs(freqs - math.sqrt(edges[b] * edges[b + 1])))] = True
        to_bands[inside, b] = 1
    rms = np.empty(m, np.float32)
    chroma = np.empty((m, 12), np.float32)
    bands = np.empty((m, BANDS), np.float32)
    for a in range(0, m, CHUNK):
        block = view[a : a + CHUNK]
        b = a + len(block)
        rms[a:b] = np.sqrt(np.mean(np.square(block), axis=1))
        spec = np.abs(np.fft.rfft(block * hann, axis=1)).astype(np.float32)
        c = np.log1p(spec @ to_chroma)
        chroma[a:b] = c / np.maximum(np.linalg.norm(c, axis=1, keepdims=True), 1e-9)
        bands[a:b] = np.log(np.square(spec) @ to_bands + 1e-10)
    timbre = bands @ _dct(BANDS, COEFS).T
    timbre = (timbre - timbre.mean(axis=0)) / (timbre.std(axis=0) + 1e-9)
    times = (np.arange(m) * HOP + WINDOW / 2) / rate
    return {"times": times, "rms": rms, "chroma": chroma, "timbre": timbre.astype(np.float32)}


def units_from(
    bars: Sequence[float] | None, duration: float
) -> tuple[list[tuple[float, float]], int | None]:
    """Los trozos en que se mira la cancion: (inicio, fin) de cada uno, y
    cual es el primer compas (None si no son compases).

    Con al menos 8 compases, los compases; lo de antes del primero y lo de
    despues del ultimo (o un hueco entre dos), en trozos de un compas. Si
    no, trozos de 2 s."""
    marks = sorted({round(float(b), 3) for b in bars or [] if 0 <= float(b) < duration})
    if len(marks) < MIN_BARS:
        n = max(1, round(duration / WINDOW_UNIT))
        edges = np.linspace(0.0, duration, n + 1)
        return [(float(a), float(b)) for a, b in itertools.pairwise(edges)], None
    size = float(np.median(np.diff(marks)))
    first_bar = marks[0]
    if first_bar < 0.25 * size:
        marks[0] = 0.0  # lo de antes del primero, con el primero
    edges = ([0.0] if marks[0] > 0 else []) + marks + [duration]
    out: list[tuple[float, float]] = []
    for a, b in itertools.pairwise(edges):
        if b - a <= 1e-3:
            continue
        if out and b - a < 0.25 * size:
            out[-1] = (out[-1][0], b)  # un resto: con el de antes
            continue
        k = max(1, round((b - a) / size))
        out += [(a + (b - a) * i / k, a + (b - a) * (i + 1) / k) for i in range(k)]
    starts = np.array([u[0] for u in out])
    return out, int(np.argmin(np.abs(starts - marks[0])))


def pool(times: np.ndarray, values: np.ndarray, units: Sequence[tuple[float, float]]):
    """La media de lo de cada trozo de audio en cada unidad (el mas cercano
    si una unidad es mas corta que un trozo)."""
    values = np.asarray(values, np.float64)
    if values.ndim == 1:
        values = values[:, None]
    out = np.zeros((len(units), values.shape[1]))
    for i, (a, b) in enumerate(units):
        lo, hi = np.searchsorted(times, a), np.searchsorted(times, b)
        if hi <= lo:
            near = int(np.clip(np.searchsorted(times, (a + b) / 2), 0, len(times) - 1))
            lo, hi = near, near + 1
        out[i] = values[lo:hi].mean(axis=0)
    return out


def unit_rms(x: np.ndarray, rate: int, units: Sequence[tuple[float, float]]) -> np.ndarray:
    """Lo fuerte que suena el audio en cada unidad (RMS)."""
    energy = np.concatenate([[0.0], np.cumsum(np.square(np.asarray(x, np.float64)))])
    n = len(x)
    a = np.clip((np.array([u[0] for u in units]) * rate).astype(int), 0, max(0, n - 1))
    b = np.clip((np.array([u[1] for u in units]) * rate).astype(int), a + 1, max(1, n))
    return np.sqrt(np.maximum(energy[b] - energy[a], 0) / np.maximum(b - a, 1))


# ------------------------------------------------------------ las partes


def _prepared(block) -> np.ndarray:
    """Cada unidad sin lo que suena en toda la cancion, y de largo 1: el
    producto de dos es su coseno."""
    f = np.asarray(block, np.float64)
    f = f - f.mean(axis=0)
    return f / np.maximum(np.linalg.norm(f, axis=1, keepdims=True), 1e-9)


def _novelty(similar: np.ndarray, half: int = KERNEL) -> np.ndarray:
    """Cuanto cambia lo que suena al empezar cada unidad: el nucleo de
    tablero de ajedrez (con forma de campana) por la diagonal. Lo parecido
    de cada lado suma y lo parecido de un lado al otro resta; cerca de los
    bordes, con lo que haya (cada mitad, con su media)."""
    n = len(similar)
    offs = np.arange(-half, half)
    weight = np.exp(-0.5 * ((offs + 0.5) / (0.5 * half)) ** 2)
    side = np.where(offs < 0, -1.0, 1.0)
    kernel = np.outer(side * weight, side * weight)
    np.fill_diagonal(kernel, 0.0)  # cada unidad se parece a si misma: no cuenta
    out = np.zeros(n)
    for t in range(1, n):
        idx = t + offs
        ok = (idx >= 0) & (idx < n)
        k = kernel[np.ix_(ok, ok)]
        s = similar[np.ix_(idx[ok], idx[ok])]
        same, cross = k > 0, k < 0
        if not same.any() or not cross.any():
            continue
        out[t] = (k[same] * s[same]).sum() / k[same].sum() - (k[cross] * s[cross]).sum() / k[
            cross
        ].sum()
    return out


def _peaks(novelty: np.ndarray, phase: int | None) -> list[int]:
    """Los cambios de parte: los picos de la novedad por encima de su media
    mas media desviacion, separados al menos MIN_GAP. Entre dos cercanos,
    mejor el que cae en un multiplo de cuatro compases desde el primero."""
    n = len(novelty)
    if n < 3:
        return []
    inner = novelty[1:]
    floor = float(inner.mean() + 0.5 * inner.std())
    bonus = 0.5 * float(inner.std())
    candidates = []
    for t in range(1, n):
        v = novelty[t]
        if v <= floor or v < novelty[t - 1] or (t + 1 < n and v < novelty[t + 1]):
            continue
        aligned = phase is not None and (t - phase) % 4 == 0
        candidates.append((v + (bonus if aligned else 0.0), t))
    chosen: list[int] = []
    for _, t in sorted(candidates, reverse=True):
        if all(abs(t - c) >= MIN_GAP for c in chosen):
            chosen.append(t)
    return sorted(chosen)


def _runs(flags: np.ndarray) -> list[tuple[int, int]]:
    """Los tramos seguidos con el mismo valor: (desde, hasta)."""
    cuts = [0, *(np.flatnonzero(flags[1:] != flags[:-1]) + 1).tolist(), len(flags)]
    return list(itertools.pairwise(cuts))


def voiced(vocals: Sequence[float] | np.ndarray) -> np.ndarray:
    """En que unidades se canta (sin los tramos de menos de MIN_UNITS: un
    respiro o una frase suelta no cambian la parte)."""
    v = np.asarray(vocals, np.float64)
    top = float(np.percentile(v, 95)) if v.size else 0.0
    flags = v > VOICE_FLOOR * top if top > 1e-6 else np.zeros(v.size, bool)
    while True:
        runs = _runs(flags)
        if len(runs) <= 1:
            return flags
        a, b = min(runs, key=lambda r: r[1] - r[0])
        if b - a >= MIN_UNITS:
            return flags
        flags[a:b] = not flags[a]


def _similarity(a: Sequence[np.ndarray], b: Sequence[np.ndarray]) -> float:
    """Lo que se parecen dos partes: la media del coseno de cada rasgo."""
    out = []
    for x, y in zip(a, b, strict=True):
        norm = float(np.linalg.norm(x) * np.linalg.norm(y))
        out.append(float(x @ y) / norm if norm > 1e-12 else 0.0)
    return float(np.mean(out))


def _merge_short(spans: list[tuple[int, int]], blocks: list[np.ndarray]) -> list[tuple[int, int]]:
    """Las partes de menos de MIN_UNITS, con la vecina que mas se les parece."""
    spans = list(spans)

    def mean(span):
        return [blk[span[0] : span[1]].mean(axis=0) for blk in blocks]

    while len(spans) > 1:
        i = min(range(len(spans)), key=lambda j: spans[j][1] - spans[j][0])
        if spans[i][1] - spans[i][0] >= MIN_UNITS:
            break
        sides = [j for j in (i - 1, i + 1) if 0 <= j < len(spans)]
        j = max(sides, key=lambda k: _similarity(mean(spans[i]), mean(spans[k])))
        a, b = min(i, j), max(i, j)
        spans[a : b + 1] = [(spans[a][0], spans[b][1])]
    return spans


def _clusters(means: list[list[np.ndarray]]) -> list[int]:
    """Las partes que suenan igual, juntas: cada una con el grupo al que mas
    se parece de media (si llega a SAME), o en uno nuevo."""
    groups: list[list[int]] = []
    out = []
    for i, m in enumerate(means):
        best, score = -1, SAME
        for g, members in enumerate(groups):
            s = float(np.mean([_similarity(m, means[j]) for j in members]))
            if s >= score:
                best, score = g, s
        if best < 0:
            groups.append([i])
            out.append(len(groups) - 1)
        else:
            groups[best].append(i)
            out.append(best)
    return out


def _letter(k: int) -> str:
    return chr(ord("A") + k) if k < 26 else f"Z{k}"


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
    rms = np.asarray(rms, np.float64)
    n = len(rms)
    if units is None:
        units = [(float(i), float(i + 1)) for i in range(n)]
    if n == 0:
        return []
    blocks = [_prepared(f) for f in features]
    similar = sum((b @ b.T for b in blocks), start=np.zeros((n, n))) / max(1, len(blocks))
    bounds = set(_peaks(_novelty(similar), phase))
    presence = None
    if vocals is not None:
        presence = voiced(vocals)
        # donde entra o sale la voz (y se queda), tambien es un cambio
        for a, _ in _runs(presence)[1:]:
            if all(abs(a - b) > 1 for b in bounds):
                bounds.add(a)
    edges = sorted({0, n, *bounds})
    spans = _merge_short(list(itertools.pairwise(edges)), blocks)
    groups = _clusters([[blk[a:b].mean(axis=0) for blk in blocks] for a, b in spans])
    median = float(np.median(rms)) if n else 0.0
    levels = {k: np.asarray(v, np.float64) for k, v in (sources or {}).items() if k not in VOCALS}
    song_total = sum(float(v.mean()) for v in levels.values())
    out = []
    for (a, b), g in zip(spans, groups, strict=True):
        loud = float(rms[a:b].mean())
        part: dict[str, Any] = {
            "first": a,
            "last": b,
            "start": float(units[a][0]),
            "end": float(units[b - 1][1]),
            "cluster": _letter(g),
            "rms": loud,
            "quiet": loud < QUIET * median,
        }
        if presence is not None:
            part["vocal"] = float(presence[a:b].mean())
        if levels and song_total > 1e-9:
            here = {k: float(v[a:b].mean()) for k, v in levels.items()}
            total = sum(here.values())
            named = [k for k in here if k in SOLOS]
            if total > 1e-9 and named:
                top = max(named, key=lambda k: here[k])
                share = here[top] / total
                usual = float(levels[top].mean()) / song_total
                if share >= SOLO_SHARE and share >= SOLO_OVER * usual:
                    part["lead"] = top
        out.append(part)
    return out


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
    n = len(segments)
    if not n:
        return []
    names = [""] * n
    known = any("vocal" in s for s in segments)
    sung = [float(s.get("vocal", 0.0)) >= 0.5 for s in segments]
    if known and any(sung):
        first = sung.index(True)
        last = n - 1 - sung[::-1].index(True)
        for i in range(first):
            names[i] = "Intro"
        for i in range(last + 1, n):
            names[i] = "Final"
        for i in range(first, last + 1):
            if not sung[i]:
                lead = segments[i].get("lead")
                names[i] = SOLOS.get(str(lead), "Instrumental") if lead else "Instrumental"
    elif n > 1:
        if segments[0].get("quiet"):
            names[0] = "Intro"
        if segments[-1].get("quiet"):
            names[-1] = "Final"
    middle = [i for i in range(n) if not names[i]]
    counts = Counter(segments[i]["cluster"] for i in middle)
    repeated = [c for c, k in counts.items() if k >= 2]
    chorus = None
    if repeated:

        def energy(c):
            return float(
                np.mean([segments[i]["rms"] for i in middle if segments[i]["cluster"] == c])
            )

        chorus = max(repeated, key=energy)
        others = [c for c in repeated if c != chorus]
        verse = None
        if others:
            verse = min(
                others, key=lambda c: next(i for i in middle if segments[i]["cluster"] == c)
            )
        for i in middle:
            if segments[i]["cluster"] == chorus:
                names[i] = "Coro"
            elif segments[i]["cluster"] == verse:
                names[i] = "Estrofa"
    for i in middle:
        short = segments[i]["end"] - segments[i]["start"] <= 4 * bar + 1e-6
        before_chorus = i + 1 < n and names[i + 1] == "Coro"
        if not names[i] and short and before_chorus and segments[i]["cluster"] != chorus:
            names[i] = "Pre coro"
    choruses = [i for i in range(n) if names[i] == "Coro"]
    for i in middle:
        if names[i]:
            continue
        if not choruses:
            names[i] = "Parte"  # sin coro no hay estrofa ni puente que valgan
        elif i < choruses[0]:
            names[i] = "Estrofa"
        else:
            names[i] = "Puente"
    merged: list[dict] = []
    for seg, name in zip(segments, names, strict=True):
        if merged and merged[-1]["label"] == name and name != "Parte":
            merged[-1]["end"] = round(float(seg["end"]), 2)
        else:
            merged.append(
                {
                    "start": round(float(seg["start"]), 2),
                    "end": round(float(seg["end"]), 2),
                    "label": name,
                }
            )
    times = Counter(m["label"] for m in merged)
    seen: Counter[str] = Counter()
    for m in merged:
        if times[m["label"]] > 1:
            seen[m["label"]] += 1
            m["label"] = f"{m['label']} {seen[m['label']]}"
    return merged


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
    air = float(beat) if beat and beat > 0 else AIR
    out: list[dict] = []
    end = None
    for s in sections:
        start, label = float(s["start"]), str(s["label"])
        clip = clip_seconds.get(label)
        if start < FIRST or clip is None:
            continue
        t = max(0.0, start - clip - air)
        if end is not None and t < end + GAP:
            t = max(end + GAP, start - clip)
        t = round(t, 2)
        if t + clip > start + 0.01:
            continue  # ya no cabe antes de su parte
        out.append({"t": t, "text": label})
        end = t + clip
    return out


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
        except EstructuraError:
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
        raise EstructuraError("la canción es demasiado corta para buscarle partes")
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
    seen: dict[float, str] = {}
    for s in sections:
        start = round(max(0.0, float(s["start"])), 2)
        label = str(s.get("label") or "").strip()[:80]
        if label and (not duration or start < duration) and start not in seen:
            seen[start] = label
    starts = sorted(seen)
    out = []
    for i, start in enumerate(starts):
        end = starts[i + 1] if i + 1 < len(starts) else max(start, round(duration, 2))
        out.append({"start": start, "end": end, "label": seen[start]})
    return out


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
            raise EstructuraError(f"no se pudo decir «{label}»: {e}") from e
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
