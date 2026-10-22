"""El clic del metronomo, para la mezcla que se guarda con clic.

Es el mismo que suena en el modo estudio: la tabla de sonidos y la cuenta
por muestra son las de `engine/src/click.rs`, y esto las usa a traves del
crate (analysis/src/click.rs). Antes estaban copiadas aqui, con numpy, y si se
cambiaba una habia que cambiar la otra.
"""

import danplay_core as _rust
import numpy as np

RATE = 44100

# Los sonidos, en el orden en que se ofrecen (los nombres que manda la
# interfaz; uno que no esta es el clasico).
SOUNDS = tuple(_rust.click_sounds())
# Hasta donde sube el clic: el triple de como viene.
MAX_VOLUME = 3.0


def _sound(sound) -> str:
    return sound if isinstance(sound, str) else ""


def hit(sound: str, accent: bool, rate: int = RATE) -> np.ndarray:
    """Un golpe entero, muestra a muestra como `click_sample` en Rust."""
    return _rust.click_hit(_sound(sound), bool(accent), int(rate))


def track(
    beats: list[float],
    accents: list[bool],
    length: float,
    sound: str = "clasico",
    volume: float = 1.0,
    rate: int = RATE,
) -> np.ndarray:
    """La pista del clic, en mono: un golpe en cada pulso (en segundos de la
    pista), acentuado donde diga `accents`, y silencio entre medias."""
    pairs = [(float(t), bool(a)) for t, a in zip(beats, accents, strict=True)]
    return _rust.click_track(pairs, float(length), _sound(sound), float(volume), int(rate))
