"""El clic del metronomo, para la mezcla que se guarda con clic.

Es el mismo que suena en el modo estudio (`metronome.rs`): la misma tabla de
sonidos y la misma cuenta por muestra, aqui con numpy. Si se cambia uno, se
cambia el otro; las pruebas de los dos comparan con los mismos numeros.
"""

import math

import numpy as np

RATE = 44100

# Cada sonido, para el «1» (acento) y para los demas: sus parciales (hz,
# amplitud, caida en segundos), lo que dura y lo fuerte que va.
SOUNDS = {
    "clasico": {
        True: ([(1568.0, 1.0, 0.035 / 4.5)], 0.035, 1.0),
        False: ([(1046.5, 1.0, 0.022 / 4.5)], 0.022, 0.65),
    },
    "madera": {
        True: ([(1250.0, 0.75, 0.011), (3450.0, 0.25, 0.004)], 0.06, 1.0),
        False: ([(950.0, 0.75, 0.010), (2620.0, 0.25, 0.004)], 0.055, 0.7),
    },
    "baqueta": {
        True: (
            [
                (430.0, 0.2, 0.006),
                (2150.0, 0.35, 0.0025),
                (3720.0, 0.28, 0.0018),
                (5310.0, 0.17, 0.0012),
            ],
            0.03,
            1.3,
        ),
        False: (
            [
                (430.0, 0.2, 0.005),
                (2150.0, 0.35, 0.002),
                (3720.0, 0.28, 0.0015),
                (5310.0, 0.17, 0.001),
            ],
            0.025,
            0.8,
        ),
    },
    "cencerro": {
        True: ([(587.0, 0.35, 0.08), (871.0, 0.45, 0.08), (2610.0, 0.2, 0.02)], 0.25, 1.0),
        False: ([(540.0, 0.35, 0.06), (800.0, 0.45, 0.06), (2400.0, 0.2, 0.015)], 0.2, 0.7),
    },
}


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
    volume: float = 0.8,
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
    return out * np.float32(max(0.0, min(2.0, volume)))
