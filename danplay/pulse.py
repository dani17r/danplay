"""El pulso y el «1» de una cancion con Beat This! (CPJKU, MIT), en ONNX Runtime.

Beat This! es una red que mira el espectrograma de la cancion y dice, cada
50 avos de segundo, lo probable que es que ahi haya un pulso y que ese pulso
sea el «1». Medido en GTZAN (que la red no vio al entrenar), acierta mucho
mas que el analisis de siempre (`beats.rs` en la app de escritorio), sobre
todo el «1»: ver scripts/evaluar-pulso.py y docs/ARQUITECTURA.md.

Como el separador, la red va en un grafo ONNX (`danplay/data/pulso/`) y lo
de alrededor se hace aqui con numpy, igual que en su codigo:

- el espectrograma: mel de 128 bandas (30 Hz a 11 kHz, escala de Slaney) a
  22.050 Hz, ventanas de 1024 cada 441 muestras (50 por segundo), en
  magnitud y con log(1 + 1000·x);
- la cancion en trozos de 30 s (1500 tramas) que se solapan 6 tramas por
  cada lado; en lo que se pisan manda el primero;
- los pulsos: los maximos de la probabilidad en ±70 ms que pasan del 50 %,
  y cada «1» llevado al pulso mas cercano.

Y lo que la app necesita encima (`grid`), medido canción a canción sobre
una biblioteca de alabanza (`scripts/revisar-metronomo.py`) y contra GTZAN:

- un solo nivel de pulso de principio a fin. La red, por tramos, oye el pulso
  al doble (el «y» de cada tiempo) o a la mitad, y el clic se aceleraba y
  frenaba dentro de la canción. Entre los candidatos de la red se elige la
  cadena de tempo estable (programacion dinamica) y lo que falta se rellena;
- cada pulso afinado entre tramas (una parabola sobre la red: la red mira
  cada 20 ms y el clic bailaba ±10 ms) y suavizado con el tempo de alrededor;
- el «1» con un modelo de compas: un compas irregular solo si los «1» de la
  red lo piden de verdad (antes salian compases de 1, 2, 5 y 8 tiempos);
- sin clic donde no hay pulso: antes del primero y despues del ultimo (una
  intro libre, un final que se apaga) y en un hueco que no es un numero
  entero de tiempos (una parte libre). Un hueco a tempo se rellena siguiendo
  a la red (no en linea recta);
- desde 1.28, los **saltos de fase**: la red, en vivo sobre todo, se pasa a
  veces medio pulso durante unos compases (oye el «y» con toda seguridad) y
  la cadena volvia a empezar ahi. Eso eran a la vez los silencios de un
  compas y los tramos con el clic a contratiempo. Cada trozo de la cadena se
  vuelve a poner en la fase de la mayoria (por lo que dura, por el
  «sandwich» —si lo de antes y lo de despues casan entre si, lo de en medio
  es lo raro— y por donde golpean el bombo y el bajo);
- las canciones grabadas a clic (estudio, secuencias) se reconocen (un
  tempo que no se mueve en toda la cancion) y van a tempo fijo exacto;
- con la bateria separada, la red se pasa tambien por ella y manda la mas
  segura de las dos en cada instante.

Lo que dice la red se guarda (`raw`): volver a analizar un tramo, o toda la
cancion de otra manera («Re-analizar el clic»), tarda milisegundos.

Corre en un proceso aparte (`main`), con prioridad baja, como el separador:
son unos segundos de todos los nucleos.
"""

import contextlib
import hashlib
import json
import math
import os
import subprocess
import sys
import zipfile

import numpy as np

RATE = 22050
NFFT = 1024
HOP = 441
FPS = RATE / HOP  # 50 tramas por segundo
MELS = 128
F_MIN = 30.0
F_MAX = 11000.0
CHUNK = 1500
BORDER = 6

WINDOW = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(NFFT) / NFFT)).astype(np.float64)


# ------------------------------------------------------------ espectrograma


def _hz_to_mel(f):
    """Escala de Slaney: lineal hasta 1 kHz y logaritmica por encima (librosa,
    torchaudio con mel_scale="slaney")."""
    f = np.asarray(f, dtype=np.float64)
    f_sp = 200.0 / 3
    mels = f / f_sp
    min_log_hz = 1000.0
    min_log_mel = min_log_hz / f_sp
    logstep = math.log(6.4) / 27.0
    return np.where(
        f >= min_log_hz, min_log_mel + np.log(np.maximum(f, 1e-10) / min_log_hz) / logstep, mels
    )


def _mel_to_hz(m):
    m = np.asarray(m, dtype=np.float64)
    f_sp = 200.0 / 3
    freqs = f_sp * m
    min_log_hz = 1000.0
    min_log_mel = min_log_hz / f_sp
    logstep = math.log(6.4) / 27.0
    return np.where(m >= min_log_mel, min_log_hz * np.exp(logstep * (m - min_log_mel)), freqs)


def mel_filters() -> np.ndarray:
    """(513, 128): el banco de filtros de torchaudio.functional.melscale_fbanks
    con esos parametros, sin normalizar."""
    freqs = np.linspace(0, RATE // 2, NFFT // 2 + 1)
    m_pts = np.linspace(_hz_to_mel(F_MIN), _hz_to_mel(F_MAX), MELS + 2)
    f_pts = _mel_to_hz(m_pts)
    f_diff = f_pts[1:] - f_pts[:-1]
    slopes = f_pts[None, :] - freqs[:, None]
    down = -slopes[:, :-2] / f_diff[:-1]
    up = slopes[:, 2:] / f_diff[1:]
    return np.maximum(0.0, np.minimum(down, up)).astype(np.float32)


_FILTERS: np.ndarray | None = None


def spectrogram(mono: np.ndarray) -> np.ndarray:
    """(tramas, 128): el log-mel de Beat This! (`LogMelSpect`) para `mono` a 22.050 Hz."""
    global _FILTERS
    if _FILTERS is None:
        _FILTERS = mel_filters()
    x = np.asarray(mono, dtype=np.float32)
    pad = NFFT // 2
    if x.size <= pad:
        x = np.pad(x, (0, pad + 1 - x.size))
    x = np.pad(x, (pad, pad), mode="reflect")
    frames = 1 + (x.size - NFFT) // HOP
    out = np.empty((frames, MELS), dtype=np.float32)
    # normalized="frame_length" de torchaudio: torch.stft(normalized=True),
    # que divide por la raiz del tamaño de la ventana
    norm = math.sqrt(NFFT)
    # por bloques: una cancion de diez minutos son 30.000 tramas
    step = 2048
    for a in range(0, frames, step):
        b = min(frames, a + step)
        idx = np.arange(NFFT)[None, :] + HOP * np.arange(a, b)[:, None]
        spec = np.abs(np.fft.rfft(x[idx] * WINDOW, axis=-1)) / norm
        out[a:b] = spec.astype(np.float32) @ _FILTERS
    return np.log1p(1000.0 * out)


# ------------------------------------------------------------ la red


def _starts(frames: int) -> list[int]:
    """Donde empieza cada trozo (`split_piece` con avoid_short_end)."""
    starts = list(range(-BORDER, frames - BORDER, CHUNK - 2 * BORDER))
    if frames > CHUNK - 2 * BORDER:
        starts[-1] = frames - (CHUNK - BORDER)
    return starts


def logits(session, spect: np.ndarray, progress=None) -> tuple[np.ndarray, np.ndarray]:
    """La probabilidad (en logits) de pulso y de «1» en cada trama.
    `progress(hechos, total)`, si se da, tras cada trozo."""
    frames = spect.shape[0]
    beat = np.full(frames, -1000.0, dtype=np.float32)
    down = np.full(frames, -1000.0, dtype=np.float32)
    starts = _starts(frames)
    # en lo que se pisan manda el primero: se escriben del ultimo al primero
    for done, start in enumerate(reversed(starts), 1):
        lo, hi = max(start, 0), min(start + CHUNK, frames)
        chunk = spect[lo:hi]
        chunk = np.pad(chunk, [(lo - start, max(0, min(BORDER, start + CHUNK - frames))), (0, 0)])
        b, d = session.run(None, {"spect": chunk[None].astype(np.float32)})
        b, d = b[0][BORDER:-BORDER], d[0][BORDER:-BORDER]
        at, n = start + BORDER, len(b)
        keep = slice(max(0, -at), min(n, frames - at))
        beat[at + keep.start : at + keep.stop] = b[keep]
        down[at + keep.start : at + keep.stop] = d[keep]
        if progress:
            progress(done, len(starts))
    return beat, down


def _peaks(values: np.ndarray) -> np.ndarray:
    """Las tramas con un maximo en ±3 tramas (±60 ms) por encima del 50 %,
    con las pegadas juntadas en su media (el `postp_minimal` de Beat This!)."""
    padded = np.pad(values, 3, constant_values=-np.inf)
    window = np.lib.stride_tricks.sliding_window_view(padded, 7).max(axis=1)
    frames = np.flatnonzero((values == window) & (values > 0))
    out: list[float] = []
    group: list[int] = []
    for f in frames:
        if group and f - group[-1] > 1:
            out.append(float(np.mean(group)))
            group = []
        group.append(int(f))
    if group:
        out.append(float(np.mean(group)))
    return np.array(out) / FPS


def beats_of(beat_logits: np.ndarray, down_logits: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Los pulsos y los «1» en segundos; cada «1», en el pulso mas cercano."""
    beats = _peaks(beat_logits)
    downs = _peaks(down_logits)
    if beats.size and downs.size:
        idx = np.abs(beats[None, :] - downs[:, None]).argmin(axis=1)
        downs = np.unique(beats[idx])
    return beats, downs


# ------------------------------------------------------------ la rejilla

# Cuanto cuesta, al elegir la cadena de pulsos: apartarse del tempo de
# alrededor (por el cuadrado del log2 del cociente), un pulso que falta y
# empezar otra cadena despues de un hueco. Medidos sobre GTZAN y sobre una
# biblioteca de alabanza: con menos, vuelven los tramos al doble.
TEMPO_COST = 240.0
MISSING_COST = 0.6
RESTART_COST = 3.0
# A partir de que probabilidad un maximo de la red es candidato a pulso: por
# debajo del 50 % (el umbral de `beats_of`) para no perder los flojos de una
# parte suave; la cadena ya descarta los que no cuadran.
CANDIDATE = 0.2
# Un hueco se rellena si cabe un numero entero de pulsos (± esto de pulso).
WHOLE = 0.25
# Cuantos vecinos a cada lado alisan cada pulso.
SMOOTH = 4
# Lo que cuesta un compas irregular (cortado o con un tiempo de mas).
IRREGULAR_BAR = 0.004


def _sigmoid(x) -> np.ndarray:
    return 1.0 / (1.0 + np.exp(-np.asarray(x, dtype=np.float64)))


def candidates(beat_logits: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Los maximos (±3 tramas) de la probabilidad de pulso que pasan de
    `CANDIDATE`, cada uno en su instante afinado entre tramas (el vertice de
    la parabola por la trama y sus dos vecinas), con su probabilidad."""
    lg = np.asarray(beat_logits, dtype=np.float64)
    p = _sigmoid(lg)
    padded = np.pad(lg, 3, constant_values=-np.inf)
    window = np.lib.stride_tricks.sliding_window_view(padded, 7).max(axis=1)
    frames: list[int] = []
    for f in np.flatnonzero((lg == window) & (p > CANDIDATE)):
        if not frames or f - frames[-1] > 1:  # una meseta: uno
            frames.append(int(f))
    idx = np.array(frames, dtype=np.int64)
    times = idx.astype(np.float64)
    inner = (idx > 0) & (idx < lg.size - 1)
    a, b, c = lg[idx[inner] - 1], lg[idx[inner]], lg[idx[inner] + 1]
    den = a - 2 * b + c
    frac = np.divide(0.5 * (a - c), den, out=np.zeros_like(den), where=den != 0)
    times[inner] += np.clip(frac, -0.5, 0.5)
    return times / FPS, p[idx]


def _dominant_period(times: np.ndarray, probs: np.ndarray) -> float:
    """El periodo que mas se repite entre los pulsos claros: cada intervalo,
    un voto, como la mediana de antes (ante la duda, el nivel rapido)."""
    strong = times[probs > 0.5]
    ibi = np.diff(strong if strong.size >= 4 else times)
    ibi = ibi[(ibi > 0.15) & (ibi < 3.0)]
    if ibi.size == 0:
        raise ValueError("la cancion no tiene pulso claro")
    hist, edges = np.histogram(np.log2(ibi), bins=np.linspace(np.log2(0.15), np.log2(3.0), 160))
    k = int(np.argmax(np.convolve(hist, [1, 2, 1], mode="same")))
    guess = 2 ** ((edges[k] + edges[k + 1]) / 2)
    near = ibi[np.abs(np.log2(ibi / guess)) < 0.1]
    return float(np.median(near)) if near.size else float(guess)


def _local_period(beats: np.ndarray, period: float, half: int = 7):
    """El periodo de alrededor de cada instante, en el nivel del dominante:
    un intervalo al doble o a la mitad cuenta como el suyo. Asi una cancion
    que acelera o un popurri cambian de tempo, pero no de nivel. Hasta un 28 %
    del dominante (un popurri de 87 a 111): mas lejos, a un tercio de otro
    nivel, es la red confundiendo un tresillo, no la cancion."""
    ibi = np.diff(beats)
    mid = (beats[1:] + beats[:-1]) / 2
    folded = ibi * 2.0 ** np.clip(np.round(np.log2(period / ibi)), -2, 2)
    ok = np.abs(np.log2(folded / period)) < 0.36
    folded, mid = folded[ok], mid[ok]
    if folded.size < 3:
        return lambda t: np.full_like(np.asarray(t, dtype=np.float64), period)
    smooth = np.array(
        [np.median(folded[max(0, i - half) : i + half + 1]) for i in range(folded.size)]
    )
    return lambda t: np.interp(t, mid, smooth)


def _chain(times: np.ndarray, probs: np.ndarray, local) -> np.ndarray:
    """Los candidatos que forman la cadena de pulsos con mas probabilidad y
    menos tirones de tempo (programacion dinamica). Saltarse uno sale gratis
    (no suma); uno que falta en medio cuesta `MISSING_COST`; despues de un
    hueco largo se puede empezar otra cadena por `RESTART_COST`."""
    n = times.size
    score = np.full(n, -np.inf)
    back = np.full(n, -1, dtype=np.int64)
    best = np.full(n, -np.inf)  # la mejor cadena que acaba en <= i
    best_at = np.full(n, -1, dtype=np.int64)
    for j in range(n):
        tj, pl = times[j], float(local(times[j]))
        far = int(np.searchsorted(times, tj - 4.5 * pl))
        near = int(np.searchsorted(times, tj - 0.55 * pl))
        # empezar aqui: el primero de la cancion no paga; despues de un hueco, si
        s_j, b_j = probs[j], -1
        if far > 0 and best[far - 1] > -np.inf:
            s_j, b_j = best[far - 1] + probs[j] - RESTART_COST, int(best_at[far - 1])
            if s_j < probs[j] - RESTART_COST:
                s_j, b_j = probs[j], -1
        for i in range(far, near):
            d = tj - times[i]
            k = max(1, round(d / pl))
            dev = math.log2(d / (k * pl))
            s = score[i] + probs[j] - TEMPO_COST * dev * dev - MISSING_COST * (k - 1)
            if s > s_j:
                s_j, b_j = s, i
        score[j], back[j] = s_j, b_j
        if j > 0 and best[j - 1] >= s_j:
            best[j], best_at[j] = best[j - 1], best_at[j - 1]
        else:
            best[j], best_at[j] = s_j, j
    chain = []
    at = int(best_at[n - 1])
    while at != -1:
        chain.append(at)
        at = int(back[at])
    return np.array(chain[::-1], dtype=np.int64)


def _fill(beats: np.ndarray, local, beat_logits=None) -> np.ndarray:
    """Los pulsos que faltan dentro de la cadena: un hueco en el que cabe un
    numero entero de pulsos (±`WHOLE`, sea largo o corto) se reparte; uno que
    no (una parte libre, un ritardando) se queda sin clic. Con lo que dice la
    red (`beat_logits`), un hueco de cuatro pulsos o mas se rellena siguiendo
    sus maximos (`anchored`), no en linea recta: antes una intro en vivo de
    un minuto salia con un clic de metronomo que no iba con nadie."""
    out = [float(beats[0])]
    for b in beats[1:]:
        gap = b - out[-1]
        n = gap / float(local((b + out[-1]) / 2))
        k = round(n)
        if k >= 2 and abs(n - k) <= WHOLE:
            if beat_logits is not None and k >= 4:
                out += [float(t) for t in anchored(out[-1], b, k, beat_logits)]
            else:
                out += [out[-1] + gap * i / k for i in range(1, k)]
        out.append(float(b))
    return np.array(out)


def anchored(a: float, b: float, k: int, beat_logits, slack=0.15, tempo_w=40.0) -> np.ndarray:
    """Los `k - 1` pulsos que van entre `a` y `b` (que no se mueven): donde
    la red dice que hay pulso, sin tirones de tempo (programacion dinamica
    por pulso y trama). Para rellenar un hueco sin descoser los extremos."""
    lp = np.log(np.clip(_sigmoid(beat_logits), 1e-3, 1.0))
    fa, fb = a * FPS, b * FPS
    p = (fb - fa) / k
    prev_frames = np.array([fa])
    prev_score = np.array([0.0])
    prev_ibi = np.array([p])
    steps: list[tuple[np.ndarray, np.ndarray]] = []  # (candidatos, de cual vienen)
    for i in range(1, k):
        c = fa + i * p
        w = max(2, math.ceil(min(i, k - i) * slack * p))
        cand = np.arange(int(c) - w, int(c) + w + 1).astype(np.float64)
        cand = cand[(cand > fa) & (cand < fb)]
        if cand.size == 0:
            cand = np.array([c])
        d = cand[:, None] - prev_frames[None, :]
        ok = (d > 0.6 * p) & (d < 1.4 * p)
        tempo = -tempo_w * np.log2(np.where(ok, d, p) / prev_ibi[None, :]) ** 2
        sc = np.where(ok, prev_score[None, :] + tempo, -np.inf)
        j = np.argmax(sc, axis=1)
        best = sc[np.arange(cand.size), j] + lp[np.clip(cand.astype(int), 0, lp.size - 1)]
        steps.append((cand, j))
        prev_ibi = np.where(np.isfinite(best), d[np.arange(cand.size), j], p)
        prev_frames, prev_score = cand, best
    if not steps:
        return np.array([])
    d = fb - prev_frames
    final = prev_score - tempo_w * np.log2(np.clip(d, 1e-6, None) / prev_ibi) ** 2
    final[(d <= 0.6 * p) | (d >= 1.4 * p)] = -np.inf
    if not np.isfinite(final).any():
        return a + (b - a) * np.arange(1, k) / k  # nada cuadra: en linea recta
    at = int(np.argmax(final))
    path = []
    for cand, back in reversed(steps):
        path.append(cand[at])
        at = int(back[at])
    return np.array(path[::-1]) / FPS


# ------------------------------------------------------------ los saltos de fase

# Un salto de fase: un hueco en el que no cabe un numero entero de pulsos.
PHASE_JUMP = 0.25


def _segments(beats: np.ndarray, local) -> tuple[list[np.ndarray], list[float]]:
    """La cadena partida en los saltos de fase, y lo que salta en cada uno
    (en pulsos, de -0,5 a 0,5)."""
    cuts, fracs = [], []
    for i in range(beats.size - 1):
        p = float(local((beats[i] + beats[i + 1]) / 2))
        n = (beats[i + 1] - beats[i]) / p
        if n >= 1.5 and abs(n - round(n)) > PHASE_JUMP:
            cuts.append(i + 1)
            fracs.append(n - round(n))
    return np.split(beats, cuts), fracs


def _low_vote(seg: np.ndarray, local, lowband) -> float:
    """Lo que dicen el bombo y el bajo de un trozo: cuanto golpean en sus
    pulsos frente a medio pulso mas alla (>1: en fase; <1: a contratiempo).
    `lowband`: (envolvente de ataques por debajo de 150 Hz, tramas por s)."""
    env, fps = lowband
    if env is None or seg.size < 4:
        return 1.0

    def at(t):
        c = np.clip(np.round(np.asarray(t) * fps).astype(int), 3, env.size - 4)
        return np.maximum.reduce([env[c + o] for o in range(-3, 4)])

    own = float(np.mean(at(seg)))
    half = float(np.mean(at(seg + local(seg) / 2)))
    return own / max(1e-9, half)


def _wrap(x: float) -> float:
    """Una fase en pulsos, de -0,5 a 0,5."""
    return (x + 0.5) % 1.0 - 0.5


def repair_flips(beats: np.ndarray, local, lowband=None) -> tuple[np.ndarray, int]:
    """Los trozos de la cadena que se fueron de fase, de vuelta a su sitio.

    La firma del fallo de la red es una **excursion**: la fase salta y unos
    compases despues vuelve (lo de antes y lo de despues casan entre si, y lo
    de en medio va medio pulso, o un tercio, corrido). Eso se corrige siempre:
    cada trozo de la excursion se corre lo que se fue, por su propio tempo de
    alrededor (no por el intervalo de al lado: con un hueco al doble dentro,
    se rompia). Un salto que no vuelve puede ser una parte libre de verdad (y
    se deja: sin clic en el hueco); solo se corrige si el bombo y el bajo
    dicen claramente que lo de un lado golpea a contratiempo y es lo corto.
    Devuelve los pulsos y cuantos trozos se movieron."""
    segs, fracs = _segments(beats, local)
    if len(segs) < 2:
        return beats, 0
    n = len(segs)
    off = np.concatenate([[0.0], np.cumsum(fracs)])  # fase de cada trozo, desde el primero
    shift = np.zeros(n)
    good = 0  # el ultimo trozo en la fase buena
    i = 1
    while i < n:
        # ¿vuelve a la fase de `good` mas adelante?
        back = next(
            (j for j in range(i + 1, n) if abs(_wrap(off[j] - off[good])) < PHASE_JUMP),
            None,
        )
        if abs(_wrap(off[i] - off[good])) < PHASE_JUMP:
            good = i
            i += 1
            continue
        if back is None:
            break
        for k in range(i, back):
            shift[k] = _wrap(off[k] - off[good])
        good = back
        i = back + 1
    # lo que queda al final (o al principio) en otra fase sin volver: solo con
    # el bombo y el bajo, y si es lo corto
    if lowband is not None:
        total = sum(s.size for s in segs)
        rest = [
            k for k in range(n) if abs(_wrap(off[k] - off[good])) >= PHASE_JUMP and not shift[k]
        ]
        size = sum(segs[k].size for k in rest)
        if rest and size < 0.3 * total:
            votes = [_low_vote(segs[k], local, lowband) for k in rest]
            if np.median(votes) < 0.8:
                for k in rest:
                    shift[k] = _wrap(off[k] - off[good])
    out, moved = [], 0
    for s, d in zip(segs, shift, strict=True):
        if d and s.size >= 2:
            out.append(s - d * local(s))
            moved += 1
        else:
            out.append(s)
    fixed = np.sort(np.concatenate(out))
    keep = np.concatenate([[True], np.diff(fixed) > 0.05])
    return fixed[keep], moved


# ------------------------------------------------------------ tempo fijo

# Lo que tiene que cumplir una cancion para darla por grabada a clic: el
# tempo de un periodograma de sus pulsos explica casi todos (R), y en ningun
# medio minuto se va mas de esto.
STEADY_R = 0.9
STEADY_DRIFT = 0.015
STEADY_FLIPPED = 0.15


def steady_fit(beats: np.ndarray, weights: np.ndarray, p0: float, span=0.03):
    """El tempo fijo que mejor explica los pulsos (periodograma de Rayleigh:
    aguanta trozos a contratiempo, pulsos de mas y de menos) y su fase.
    Devuelve (periodo, fase, R, lo que se aparta por medio minuto (s),
    la parte a contratiempo)."""
    t = np.asarray(beats, dtype=np.float64)
    w = np.asarray(weights, dtype=np.float64)

    def best(periods):
        z = (w[None, :] * np.exp(2j * np.pi * t[None, :] / periods[:, None])).sum(1)
        k = int(np.argmax(np.abs(z)))
        return periods[k], z[k]

    period, _ = best(p0 * np.exp(np.linspace(-span, span, 2001)))
    period, z = best(period * np.exp(np.linspace(-span / 500, span / 500, 801)))
    phase = (np.angle(z) / (2 * np.pi)) * period % period
    r = float(np.abs(z) / max(1e-9, w.sum()))
    res = (t - phase + period / 2) % period - period / 2
    flipped = np.abs(res) > 0.25 * period
    drift = 0.0
    for a in range(0, int(t[-1]) + 1, 30):
        m = (~flipped) & (t >= a) & (t < a + 30)
        if m.sum() > 10:
            drift = max(drift, abs(float(np.median(res[m]))))
    return float(period), float(phase), r, drift, float(flipped.mean())


def steady_beats(period: float, phase: float, t0: float, t1: float) -> np.ndarray:
    """Los pulsos a tempo fijo de `t0` a `t1`."""
    first = math.ceil((t0 - phase) / period - 1e-9)
    last = math.floor((t1 - phase) / period + 1e-9)
    return phase + period * np.arange(first, last + 1)


def extend_edges(beats: np.ndarray, local, t0: float, t1: float, beat_logits=None) -> np.ndarray:
    """La intro y el final, con clic: hacia atras desde el primer pulso y
    hacia delante desde el ultimo, a su tempo de alrededor (cada uno pegado
    al maximo de la red de al lado si lo hay, a menos de 60 ms)."""
    p_first = float(local(beats[0]))
    p_last = float(local(beats[-1]))
    before = beats[0] - p_first * np.arange(1, int((beats[0] - t0) / p_first) + 1)
    after = beats[-1] + p_last * np.arange(1, int((t1 - beats[-1]) / p_last) + 1)
    extra = np.concatenate([before, after])
    if beat_logits is not None and extra.size:
        times, _ = candidates(beat_logits)
        if times.size:
            j = np.clip(np.searchsorted(times, extra), 1, times.size - 1)
            near = np.where(
                np.abs(times[j] - extra) < np.abs(times[j - 1] - extra), times[j], times[j - 1]
            )
            close = np.abs(near - extra) < 0.06
            extra = np.where(close, near, extra)
    out = np.sort(np.concatenate([beats, extra[(extra >= t0) & (extra <= t1)]]))
    keep = np.concatenate([[True], np.diff(out) > 0.05])
    return out[keep]


def _smooth(beats: np.ndarray) -> np.ndarray:
    """Cada pulso en la recta de sus `SMOOTH` vecinos de cada lado, si el
    tramo va regular; donde el tempo cambia de golpe, como estaba."""
    out = beats.copy()
    for i in range(beats.size):
        lo, hi = max(0, i - SMOOTH), min(beats.size, i + SMOOTH + 1)
        seg = beats[lo:hi]
        ibi = np.diff(seg)
        if seg.size < 5:
            continue
        med = np.median(ibi)
        if ibi.max() > 1.25 * med or ibi.min() < 0.8 * med:
            continue
        idx = np.arange(lo, hi)
        slope, icept = np.polyfit(idx, seg, 1)
        out[i] = slope * i + icept
    for i in range(1, out.size):  # sin cruzarse nunca
        if out[i] <= out[i - 1]:
            out[i] = beats[i]
    return out


def _bars(down_p: np.ndarray, meter: int) -> tuple[np.ndarray, float]:
    """El «1» de cada compas (Viterbi): los estados son el tiempo del compas
    (0..meter-1) y uno mas para un compas con un tiempo de mas; lo normal es
    pasar al siguiente, y cortar o alargar un compas cuesta `IRREGULAR_BAR`.
    Devuelve los indices de los «1» y lo bien que cuadra."""
    m, n = meter, down_p.size
    states = m + 1
    lp = np.log(np.clip(down_p, 1e-4, 1 - 1e-4))
    lq = np.log(np.clip(1 - down_p, 1e-4, 1 - 1e-4))
    emit = np.empty((n, states))
    emit[:, 0] = lp
    emit[:, 1:] = lq[:, None]
    trans = np.full((states, states), -np.inf)
    odd, keep = math.log(IRREGULAR_BAR), math.log(1 - 2 * IRREGULAR_BAR)
    for s in range(m):
        trans[s, (s + 1) % m] = keep
        if 1 <= s < m - 1:
            trans[s, 0] = odd  # compas cortado
    trans[m - 1, m] = odd  # un tiempo de mas
    trans[m, 0] = 0.0
    delta = np.full(states, -np.inf)
    delta[:m] = emit[0, :m] - math.log(m)
    psi = np.zeros((n, states), dtype=np.int64)
    for t in range(1, n):
        cand = delta[:, None] + trans
        psi[t] = np.argmax(cand, axis=0)
        delta = cand[psi[t], np.arange(states)] + emit[t]
    state = int(np.argmax(delta))
    fit = float(delta[state])
    path = np.empty(n, dtype=np.int64)
    for t in range(n - 1, -1, -1):
        path[t] = state
        state = int(psi[t, state])
    return np.flatnonzero(path == 0), fit


def combine(beat_logits: np.ndarray, *others: np.ndarray) -> np.ndarray:
    """La red sobre la cancion y sobre sus pistas (la bateria): en cada trama,
    la mas segura (el maximo de las probabilidades), otra vez como logits.
    El maximo y no la media: la media perdia la intro (la bateria calla)."""
    p = _sigmoid(beat_logits)
    for o in others:
        q = _sigmoid(o)
        n = min(p.size, q.size)
        p = np.concatenate([np.maximum(p[:n], q[:n]), p[n:]])
    p = np.clip(p, 1e-6, 1 - 1e-6)
    return np.log(p / (1 - p))


MODES = ("auto", "repair", "steady", "fill", "follow")


def grid(
    beat_logits: np.ndarray,
    down_logits: np.ndarray,
    *,
    lowband=None,
    mode: str = "auto",
    edges: bool = False,
    duration: float | None = None,
    bpm: float | None = None,
) -> dict:
    """La rejilla que usa la app (`BeatGrid` de beats.rs) a partir de lo que
    dice la red trama a trama (ver el principio del archivo).

    `mode`: 'auto' (reparar los saltos de fase; si la cancion va a clic,
    tempo fijo), 'repair' (lo mismo, sin pasar a tempo fijo), 'steady'
    (tempo fijo siempre; `bpm`, si se da), 'fill' (como 'auto' y ademas con
    clic en la intro y el final: `edges`) o 'follow' (como antes de 1.28:
    sin reparar nada). `lowband`: la envolvente del bombo y el bajo, para
    decidir la fase. `closed`: acaba en su ultimo pulso; despues no hay clic.
    `report` dice lo que se arreglo."""
    times, probs = candidates(beat_logits)
    if times.size < 4:
        raise ValueError("la cancion no tiene pulso claro")
    period = _dominant_period(times, probs)
    strong = times[probs > 0.5]
    local = _local_period(strong if strong.size >= 4 else times, period)
    # dos pasadas: la primera cadena ya va en un solo nivel, y el tempo de
    # alrededor que sale de ella (sin los golpes sueltos de la red) es fiable
    first = times[_chain(times, probs, local)]
    local = _local_period(first, period, half=4)
    chain_idx = _chain(times, probs, local)
    chained = times[chain_idx]
    report = {"mode": mode, "flips": 0, "holes": 0, "steady": False}
    _, jumps = _segments(chained, local)
    report["holes"] = len(jumps)
    if mode != "follow":
        chained, report["flips"] = repair_flips(chained, local, lowband)
    end = float(duration) if duration else float(beat_logits.size / FPS)
    steady = None
    if mode in ("auto", "steady", "fill"):
        w = probs[np.clip(np.searchsorted(times, chained), 0, probs.size - 1)]
        p0 = 60.0 / bpm if bpm else float(local(np.median(chained)))
        fit = steady_fit(chained, w, p0, span=0.01 if bpm else 0.03)
        looks = fit[2] >= STEADY_R and fit[3] <= STEADY_DRIFT and fit[4] <= STEADY_FLIPPED
        report["regular"] = round(fit[2], 3)
        report["drift_ms"] = round(fit[3] * 1000, 1)
        if mode == "steady" or looks:
            steady = fit
    if steady is not None:
        lo, hi = (0.0, end) if edges else (float(chained[0]), float(chained[-1]))
        beats = steady_beats(steady[0], steady[1], lo, hi)
        report["steady"] = True
        report["bpm"] = round(60.0 / steady[0], 3)
    else:
        beats = _smooth(_fill(chained, local, beat_logits if mode != "follow" else None))
        if edges:
            beats = extend_edges(beats, local, 0.0, end, beat_logits)
    if beats.size < 4:
        raise ValueError("la cancion no tiene pulso claro")
    # los huecos sin clic que quedan (partes libres de verdad)
    gaps = np.diff(beats) / np.maximum(1e-6, local((beats[1:] + beats[:-1]) / 2))
    report["silent"] = round(float(np.sum(np.diff(beats)[gaps > 1.5])), 1)
    return finish(beats, down_logits, report)


def finish(
    beats: np.ndarray, down_logits: np.ndarray, report: dict, meter: int | None = None
) -> dict:
    """La rejilla con sus pulsos ya puestos: el «1» de cada compas (con el
    compas que se diga, o el que cuadre: 4 salvo que el 3 cuadre claramente
    mejor), el tempo y lo demas que espera la app."""
    beats = np.asarray(beats, dtype=np.float64)
    # la probabilidad de «1» en cada pulso (la mejor de su trama y las de al lado)
    dp = _sigmoid(down_logits)
    f = np.clip(np.round(beats * FPS).astype(int), 0, dp.size - 1)
    down_p = np.maximum.reduce([dp[np.clip(f + o, 0, dp.size - 1)] for o in (-1, 0, 1)])
    fits = {m: _bars(down_p, m) for m in (3, 4)}
    if meter not in (3, 4):
        # 4 salvo que el 3 cuadre claramente mejor
        meter = 3 if fits[3][1] > fits[4][1] + 0.05 * beats.size else 4
    bars = fits[meter][0]
    phases = {
        m: int(np.bincount(fits[m][0] % m, minlength=m).argmax()) if fits[m][0].size else 0
        for m in (3, 4)
    }
    return {
        "bpm": round(float(60.0 / np.median(np.diff(beats))), 2),
        "meter": meter,
        "beats": [round(float(t), 4) for t in beats],
        "first_downbeat": int(bars[0]) if bars.size else 0,
        "phase3": phases[3],
        "phase4": phases[4],
        "confidence": round(float(np.mean(down_p[bars])) if bars.size else 0.0, 3),
        "bars": [int(b) for b in bars],
        "closed": True,
        "report": report,
    }


def lowband(mono: np.ndarray, fmax: float = 150.0) -> tuple[np.ndarray, float]:
    """Donde golpean el bombo y el bajo: el flujo espectral por debajo de
    `fmax` Hz, cada 10 ms (para decidir la fase de un trozo: el flujo de toda
    la banda no distingue, los platos van a corcheas)."""
    n, hop = 2048, 220
    win = np.hanning(n).astype(np.float32)
    k = int(fmax * n / RATE) + 1
    frames = 1 + max(0, (mono.size - n) // hop)
    out = np.zeros(frames, np.float32)
    prev = None
    for a in range(0, frames, 4096):
        b = min(frames, a + 4096)
        idx = np.arange(n)[None, :] + hop * np.arange(a, b)[:, None]
        mag = np.log1p(100 * np.abs(np.fft.rfft(mono[idx] * win, axis=-1)[:, 1:k]))
        d = np.diff(mag, axis=0, prepend=(mag[:1] if prev is None else prev[None]))
        prev = mag[-1]
        out[a:b] = np.maximum(d, 0).sum(1)
    # centrado en la ventana: el instante de cada trama es el de su mitad
    shift = round((n / 2) / hop)
    out = np.concatenate([np.zeros(shift, np.float32), out])
    return out, RATE / hop


# ------------------------------------------------------------ la cancion


def _decode(ffmpeg: str, path: str) -> np.ndarray:
    """La cancion a mono y 22.050 Hz."""
    r = subprocess.run(
        [
            *(ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-i", path, "-vn"),
            *("-f", "f32le", "-ac", "1", "-ar", str(RATE), "-"),
        ],
        capture_output=True,
        check=False,
    )
    if r.returncode != 0 or len(r.stdout) < 4:
        detail = r.stderr.decode("utf-8", "replace").strip().splitlines()
        raise RuntimeError("ffmpeg no pudo leer la cancion" + (f": {detail[-1]}" if detail else ""))
    return np.frombuffer(r.stdout[: len(r.stdout) // 4 * 4], dtype=np.float32)


def open_session(graph):
    import onnxruntime as ort

    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    # cuatro hilos: con mas no va mas rapido (medido: 11,7 s con 4 y 13,3 con
    # todos en una cancion de 4 min) y se deja sitio a la musica y a la app
    options.intra_op_num_threads = min(4, os.cpu_count() or 1)
    return ort.InferenceSession(str(graph), options, providers=["CPUExecutionProvider"])


# Cuanto del total es leer la cancion; el resto, la red (medido: 2,5 s de
# ~18 en una de 4 min, leida de un disco duro).
READ_SHARE = 0.15


def analyze(ffmpeg: str, path: str, graph, progress=None, raw=None) -> dict:
    """La rejilla de un archivo, de principio a fin. `progress(0..1)`, si se
    da, segun avanza: la app enseña una barra mientras espera. Con `raw` (un
    dict), deja ahi lo que dijo la red y la envolvente del bombo y el bajo,
    para guardarlo."""
    mono = _decode(ffmpeg, path)
    session = open_session(graph)
    if progress:
        progress(READ_SHARE)
    step = (
        (lambda done, total: progress(READ_SHARE + (1 - READ_SHARE) * done / total))
        if progress
        else None
    )
    beat, down = logits(session, spectrogram(mono), step)
    low = lowband(mono)
    if raw is not None:
        raw.update(beat=beat, down=down, low=low[0], low_fps=low[1], seconds=mono.size / RATE)
    return grid(beat, down, lowband=low, duration=mono.size / RATE)


def save_raw(path, raw: dict) -> None:
    """Lo que dijo la red, en float16 (unos 100 KB para una de 4 min). A un
    temporal y a su sitio al final: cortado a medias, no queda roto."""
    path = str(path)
    tmp = path + ".part"
    with open(tmp, "wb") as f:
        np.savez_compressed(
            f,
            beat=np.asarray(raw["beat"], np.float16),
            down=np.asarray(raw["down"], np.float16),
            low=np.asarray(raw["low"], np.float16),
            low_fps=np.float64(raw["low_fps"]),
            seconds=np.float64(raw["seconds"]),
        )
    os.replace(tmp, path)


def load_raw(path) -> dict | None:
    """Lo guardado por `save_raw`, o None. Uno dañado (de antes de guardarse
    asi) se borra: se vuelve a calcular."""
    try:
        with np.load(path) as z:
            return {
                "beat": z["beat"].astype(np.float32),
                "down": z["down"].astype(np.float32),
                "low": z["low"].astype(np.float32),
                "low_fps": float(z["low_fps"]),
                "seconds": float(z["seconds"]),
            }
    except FileNotFoundError:
        return None
    except (OSError, ValueError, KeyError, EOFError, zipfile.BadZipFile):
        with contextlib.suppress(OSError):
            os.remove(path)
        return None


def key_of(path, st=None) -> str:
    """Como se reconoce un archivo en la cache: ruta, tamaño y fecha (los de
    `st`, si se dan: los de antes de reescribir su etiqueta)."""
    st = st or os.stat(path)
    raw = f"{os.path.abspath(path)}|{st.st_size}|{int(st.st_mtime)}"
    return hashlib.sha1(raw.encode("utf-8")).hexdigest()  # noqa: S324 - no es seguridad


def main(argv=None) -> int:
    """El proceso hijo: `python -m danplay.pulse GRAFO FFMPEG ARCHIVO [CRUDO]`
    -> JSON. Con CRUDO (un .npz), deja ahi lo que dijo la red."""
    args = sys.argv[1:] if argv is None else argv
    if len(args) not in (3, 4):
        sys.stderr.write("uso: danplay.pulse GRAFO FFMPEG ARCHIVO [CRUDO]\n")
        return 2
    if hasattr(os, "nice"):
        try:
            os.nice(10)
        except OSError:
            pass
    graph, ffmpeg, path = args[:3]
    raw_path = args[3] if len(args) == 4 else None

    def progress(part: float) -> None:
        # una linea por paso: el padre las lee segun llegan
        sys.stdout.write(json.dumps({"progress": round(part, 3)}) + "\n")
        sys.stdout.flush()

    try:
        raw: dict = {}
        result = analyze(ffmpeg, path, graph, progress, raw)
        if raw_path:
            save_raw(raw_path, raw)
    except Exception as e:  # noqa: BLE001 - se cuenta al proceso padre
        sys.stdout.write(json.dumps({"error": str(e) or e.__class__.__name__}) + "\n")
        return 1
    sys.stdout.write(json.dumps({"ok": True, **result}) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
