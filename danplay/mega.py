"""La separacion de maxima calidad: mas instrumentos, cada uno en su pista.

Sobre las pistas buenas de siempre (bateria y bajo de los especialistas,
las voces), el Mega53 (BS-RoFormer de ZFTurbo, MIT) oye que instrumento
suena en cada punto del espectrograma, y con eso se reparte:

- la bateria, en el kit y la percusion de aparte (congas, pandereta,
  shakers…: lo que en directo toca el percusionista);
- las voces, en la principal y los coros;
- lo melodico que queda (la cancion menos bateria, bajo y voces), entre
  guitarra electrica, guitarra acustica, piano, organo, teclados (con los
  sintes), cuerdas, metales y vientos.

La bateria y las voces se reparten en proporcion (una mascara suave: cuanto
de cada cosa hay en cada punto, segun el Mega53), asi que el kit y la
percusion suman exactamente la bateria, y la voz y los coros las voces. Lo
melodico, con lo que oye cada cabeza sin pasar de lo que hay; lo que ningun
instrumento reclama queda en «Otros», que sigue siendo la cancion menos todo
lo demas. Asi lo fuerte de los especialistas se conserva y el
Mega53 solo decide como repartir.

El Mega53 trabaja con su propio espectrograma (2048 puntos, salto de 512,
sin normalizar), en trozos de 10 s (los de su entrenamiento) que se solapan
y se funden. No importa nada de DanPlay: corre en el proceso de separar.
"""

import math
from collections.abc import Callable

import numpy as np

RATE = 44100
NFFT = 2048
HOP = 512
FREQS = NFFT // 2 + 1
WINDOW = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(NFFT) / NFFT)).astype(np.float32)
EPS = 1e-10

# Lo que se reparte, y con que cabezas del Mega53 (varias: la mas fuerte en
# cada punto, porque se solapan: «piano» y «digital-piano» oyen el mismo
# piano).
DRUM_SPLIT = {"percussion": ("congas", "tambourine", "percussion"), "drums": ("drums",)}
VOICE_SPLIT = {"lead": ("lead-vocal",), "backing": ("back-vocal",)}
MELODIC = {
    "eguitar": ("electric-guitar",),
    "aguitar": ("acoustic-guitar",),
    "piano": ("piano",),
    "organ": ("organ",),
    "keys": ("keys", "synth"),
    "strings": ("strings",),
    "brass": ("brass",),
    "wind": ("wind",),
}


def stft(x: np.ndarray) -> np.ndarray:
    """torch.stft(n_fft=2048, hop=512, hann, center, reflect, sin normalizar):
    (..., N) -> (..., 1025, frames) complejo."""
    pad = NFFT // 2
    x = np.pad(x, [(0, 0)] * (x.ndim - 1) + [(pad, pad)], mode="reflect")
    frames = 1 + (x.shape[-1] - NFFT) // HOP
    idx = np.arange(NFFT)[None, :] + HOP * np.arange(frames)[:, None]
    z = np.fft.rfft(x[..., idx] * WINDOW, axis=-1)
    return np.swapaxes(z, -1, -2).astype(np.complex64)


def istft(z: np.ndarray, length: int) -> np.ndarray:
    """torch.istft con los mismos parametros, recortada a `length`."""
    frames = z.shape[-1]
    fr = np.fft.irfft(np.swapaxes(z, -1, -2), n=NFFT, axis=-1).astype(np.float32)
    fr *= WINDOW
    k = NFFT // HOP
    fr = fr.reshape((*fr.shape[:-1], k, HOP))
    out = np.zeros((*z.shape[:-2], frames + k - 1, HOP), dtype=np.float32)
    square = (WINDOW**2).reshape(k, HOP)
    env = np.zeros((frames + k - 1, HOP), dtype=np.float32)
    for j in range(k):
        out[..., j : j + frames, :] += fr[..., :, j, :]
        env[j : j + frames] += square[j]
    start = NFFT // 2
    out = out.reshape((*out.shape[:-2], -1))[..., start : start + length]
    env = env.reshape(-1)[start : start + length]
    return out / np.where(env > 1e-11, env, np.float32(1))


def to_net(z: np.ndarray) -> np.ndarray:
    """(2, F, T) complejo -> (1, T, F·2·2) como lo espera la red: por cada
    frecuencia, cada canal, y de cada uno la parte real y la imaginaria."""
    parts = np.stack([np.real(z), np.imag(z)], axis=-1)  # (2, F, T, 2)
    return np.ascontiguousarray(parts.transpose(2, 1, 0, 3).reshape(1, z.shape[-1], -1), np.float32)


def from_net(masks: np.ndarray) -> np.ndarray:
    """(1, N, T, F·2·2) -> (N, 2, F, T) complejo."""
    n, t = masks.shape[1], masks.shape[2]
    m = masks[0].reshape(n, t, FREQS, 2, 2)
    return (m[..., 0] + 1j * m[..., 1]).transpose(0, 3, 2, 1).astype(np.complex64)


def _power(estimates: dict[str, np.ndarray], heads: tuple[str, ...]) -> np.ndarray:
    """Lo que oyen esas cabezas, en energia (la mas fuerte en cada punto)."""
    out = None
    for h in heads:
        p = np.abs(estimates[h]) ** 2
        out = p if out is None else np.maximum(out, p)
    assert out is not None
    return out


def split(
    estimates: dict[str, np.ndarray], spectra: dict[str, np.ndarray]
) -> dict[str, np.ndarray]:
    """El reparto de un trozo. `estimates`: lo que el Mega53 oye de cada
    cabeza (mascara por la mezcla); `spectra`: los espectrogramas de la
    bateria (`drums`), las voces (`vocals`) y lo melodico (`melodic`).
    Devuelve el espectrograma de cada pista nueva."""
    out: dict[str, np.ndarray] = {}
    if "drums" in spectra:
        perc = sum(_power(estimates, (h,)) for h in DRUM_SPLIT["percussion"])
        kit = _power(estimates, DRUM_SPLIT["drums"])
        share = perc / (perc + kit + EPS)
        out["percussion"] = share * spectra["drums"]
        out["drums"] = spectra["drums"] - out["percussion"]
    if "vocals" in spectra:
        lead = _power(estimates, VOICE_SPLIT["lead"])
        back = _power(estimates, VOICE_SPLIT["backing"])
        share = back / (lead + back + EPS)
        out["backing"] = share * spectra["vocals"]
        out["lead"] = spectra["vocals"] - out["backing"]
    if "melodic" in spectra:
        # Cada instrumento, lo que oye su cabeza (con su fase), sin pasar en
        # ningun punto de lo que hay en lo melodico. Medido contra la verdad
        # de una secuencia (las pistas de MultiTracks): repartir lo melodico
        # en proporcion daba 1,3 dB en las guitarras y 4,5 en los teclados;
        # asi, 2,2 y 5,3. Lo que ningun instrumento reclama, a «Otros».
        limit = np.abs(spectra["melodic"])
        for k, heads in MELODIC.items():
            heard = sum(estimates[h] for h in heads)
            out[k] = heard * np.minimum(1.0, limit / (np.abs(heard) + EPS))
    return out


def _fade(size: int, overlap: int) -> np.ndarray:
    """El peso de un trozo: sube y baja en lo que se solapa con sus vecinos."""
    w = np.ones(size, np.float32)
    if overlap > 0:
        ramp = np.linspace(0, 1, overlap + 2, dtype=np.float32)[1:-1]
        w[:overlap] = ramp
        w[-overlap:] = ramp[::-1]
    return w


Progress = Callable[[int, int], None]


def separate(
    session,
    heads: list[str],
    chunk: int,
    mix: np.ndarray,
    tracks: dict[str, np.ndarray],
    emit: Callable[[dict[str, np.ndarray]], None],
    progress: Progress | None = None,
    cancelled: Callable[[], bool] | None = None,
    overlap: float = 0.25,
) -> None:
    """Pasa la cancion por el Mega53 en trozos y reparte cada uno. `mix`,
    (2, N), es la cancion; `tracks`, las pistas de las que se reparte
    (`drums`, `vocals`, `melodic`), del mismo largo. Va dando las pistas
    nuevas a `emit` por tramos, en orden ({pista: (2, n)}), fundiendo los
    trozos donde se solapan."""
    length = mix.shape[-1]
    lap = int(chunk * overlap)
    step = chunk - lap
    starts = list(range(0, max(1, length - lap), step)) or [0]
    weight = _fade(chunk, lap)
    acc: dict[str, np.ndarray] = {}
    acc_w = np.zeros(chunk + step, np.float32)
    base = 0
    for n, start in enumerate(starts):
        if cancelled and cancelled():
            break
        end = min(length, start + chunk)
        size = end - start

        def piece(x, _s=start, _e=end, _size=size):
            seg = x[:, _s:_e]
            return np.pad(seg, [(0, 0), (0, chunk - _size)]) if _size < chunk else seg

        x = piece(mix)
        z = stft(x)
        (masks,) = session.run(None, {"spec": to_net(z)})
        m = from_net(masks)
        estimates = {h: m[i] * z for i, h in enumerate(heads)}
        spectra = {k: stft(piece(v)) for k, v in tracks.items()}
        parts = {k: istft(v, chunk) for k, v in split(estimates, spectra).items()}
        last = n == len(starts) - 1
        w = weight.copy()
        if n == 0:
            w[:lap] = 1  # el principio de la cancion no se funde con nada
        if last:
            w[lap if n else 0 :] = 1  # ni el final
            w[size:] = 0  # lo que se relleno detras no cuenta
        at = start - base
        for k, v in parts.items():
            if k not in acc:
                acc[k] = np.zeros((2, chunk + step), np.float32)
            acc[k][:, at : at + chunk] += v * w
        acc_w[at : at + chunk] += w
        ready = (end if last else starts[n + 1]) - base
        norm = np.where(acc_w[:ready] > 1e-6, acc_w[:ready], 1)
        emit({k: v[:, :ready] / norm for k, v in acc.items()})
        for k in acc:
            acc[k] = np.concatenate([acc[k][:, ready:], np.zeros((2, ready), np.float32)], axis=1)
        acc_w[:] = np.concatenate([acc_w[ready:], np.zeros(ready, np.float32)])
        base += ready
        if progress:
            progress(n + 1, len(starts))


def frames_for(chunk: int) -> int:
    return chunk // HOP + 1


def chunks_for(length: int, chunk: int, overlap: float = 0.25) -> int:
    lap = int(chunk * overlap)
    return max(1, math.ceil(max(1, length - lap) / (chunk - lap)))
