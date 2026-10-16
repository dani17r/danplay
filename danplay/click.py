"""El clic del metronomo, para la mezcla que se guarda con clic.

Es el mismo que suena en el modo estudio (`metronome.rs`): la misma tabla de
sonidos y la misma cuenta por muestra, aqui con numpy. Si se cambia uno, se
cambia el otro; las pruebas de los dos comparan con los mismos numeros.
"""

import math

import numpy as np

RATE = 44100

# Cada sonido, para el «1» (acento) y para los demas: sus parciales (hz,
# amplitud, caida en segundos), lo que dura y lo fuerte que va. Desde 1.28
# suenan mas (caidas algo mas largas) y hay dos nuevos: claves y charles.
_HAT = [
    (6312.0, 0.22, 0.030),
    (7140.0, 0.20, 0.028),
    (8256.0, 0.18, 0.025),
    (9433.0, 0.16, 0.022),
    (10601.0, 0.14, 0.020),
    (12105.0, 0.10, 0.018),
]
_HAT_SHORT = [
    (6312.0, 0.22, 0.018),
    (7140.0, 0.20, 0.017),
    (8256.0, 0.18, 0.015),
    (9433.0, 0.16, 0.013),
    (10601.0, 0.14, 0.012),
    (12105.0, 0.10, 0.011),
]
SOUNDS = {
    "clasico": {
        True: ([(1568.0, 1.0, 0.012), (3136.0, 0.25, 0.006)], 0.06, 1.0),
        False: ([(1046.5, 1.0, 0.010), (2093.0, 0.25, 0.005)], 0.05, 0.8),
    },
    "madera": {
        True: ([(1250.0, 0.75, 0.016), (3450.0, 0.25, 0.006)], 0.08, 1.0),
        False: ([(950.0, 0.75, 0.014), (2620.0, 0.25, 0.005)], 0.07, 0.8),
    },
    "baqueta": {
        True: (
            [
                (430.0, 0.2, 0.008),
                (2150.0, 0.35, 0.0033),
                (3720.0, 0.28, 0.0024),
                (5310.0, 0.17, 0.0016),
            ],
            0.04,
            1.5,
        ),
        False: (
            [
                (430.0, 0.2, 0.0065),
                (2150.0, 0.35, 0.0026),
                (3720.0, 0.28, 0.002),
                (5310.0, 0.17, 0.0013),
            ],
            0.033,
            1.1,
        ),
    },
    "cencerro": {
        True: ([(587.0, 0.35, 0.08), (871.0, 0.45, 0.08), (2610.0, 0.2, 0.02)], 0.25, 1.0),
        False: ([(540.0, 0.35, 0.06), (800.0, 0.45, 0.06), (2400.0, 0.2, 0.015)], 0.2, 0.8),
    },
    "claves": {
        True: ([(2500.0, 0.85, 0.035), (6830.0, 0.15, 0.008)], 0.12, 1.0),
        False: ([(2200.0, 0.85, 0.030), (6010.0, 0.15, 0.007)], 0.1, 0.8),
    },
    "hihat": {
        True: (_HAT, 0.09, 1.2),
        False: (_HAT_SHORT, 0.06, 0.95),
    },
}
# Hasta donde sube el clic: el triple de como viene.
MAX_VOLUME = 3.0


def hit(sound: str, accent: bool, rate: int = RATE) -> np.ndarray:
    """Un golpe entero, muestra a muestra como `click_sample` en Rust."""
    partials, length, gain = SOUNDS.get(sound, SOUNDS["clasico"])[accent]
    n = int(length * rate)
    pos = np.arange(n, dtype=np.float64)
    t = pos / rate
    # un ataque de medio milisegundo, para que no chasque
    attack = np.minimum(pos / (0.0005 * rate), 1.0)
    v = np.zeros(n)
    for hz, amp, decay in partials:
        v += amp * np.exp(-t / decay) * np.sin(2 * math.pi * hz * t)
    return (gain * attack * v).astype(np.float32)


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
    out = np.zeros(max(0, math.ceil(length * rate)), dtype=np.float32)
    cache = {a: hit(sound, a, rate) for a in (True, False)}
    for t, accent in zip(beats, accents, strict=True):
        start = round(t * rate)
        if start < 0 or start >= out.size:
            continue
        h = cache[bool(accent)]
        end = min(out.size, start + h.size)
        out[start:end] += h[: end - start]
    return out * np.float32(max(0.0, min(MAX_VOLUME, volume)))
