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

Lo hace el crate (analysis/src/mega.rs, cuenta a cuenta lo que antes estaba
aqui con numpy: el espectrograma propio del Mega53, el reparto por mascaras
y los trozos de 10 s que se solapan y se funden). Esto es la puerta de
Python, con los mismos nombres. No importa nada de DanPlay: corre en el
proceso de separar.
"""

from collections.abc import Callable

import danplay_core as _rust
import numpy as np

RATE = 44100
NFFT = 2048
HOP = 512
FREQS = NFFT // 2 + 1
WINDOW = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(NFFT) / NFFT)).astype(np.float32)
EPS = 1e-10

# Lo que se reparte, y con que cabezas del Mega53 (varias: la mas fuerte en
# cada punto, porque se solapan: «piano» y «digital-piano» oyen el mismo
# piano). Las tablas son las del crate.
_drums, _voices, _melodic = _rust.mega_tables()
DRUM_SPLIT = {k: tuple(v) for k, v in _drums.items()}
VOICE_SPLIT = {k: tuple(v) for k, v in _voices.items()}
MELODIC = {k: tuple(v) for k, v in _melodic.items()}


def stft(x: np.ndarray) -> np.ndarray:
    """torch.stft(n_fft=2048, hop=512, hann, center, reflect, sin normalizar):
    (..., N) -> (..., 1025, frames) complejo."""
    return _rust.mega_stft(np.ascontiguousarray(x, dtype=np.float32))


def istft(z: np.ndarray, length: int) -> np.ndarray:
    """torch.istft con los mismos parametros, recortada a `length`."""
    return _rust.mega_istft(np.ascontiguousarray(z, dtype=np.complex64), int(length))


def to_net(z: np.ndarray) -> np.ndarray:
    """(2, F, T) complejo -> (1, T, F·2·2) como lo espera la red: por cada
    frecuencia, cada canal, y de cada uno la parte real y la imaginaria."""
    return _rust.mega_to_net(np.ascontiguousarray(z, dtype=np.complex64))


def from_net(masks: np.ndarray) -> np.ndarray:
    """(1, N, T, F·2·2) -> (N, 2, F, T) complejo."""
    return _rust.mega_from_net(masks)


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
    trozos donde se solapan. `session`, la de `separation.open_session`
    (todo en Rust) o cualquiera con `run` como las de onnxruntime."""
    _rust.mega_separate(
        session,
        list(heads),
        int(chunk),
        np.ascontiguousarray(mix, dtype=np.float32),
        dict(tracks),
        emit,
        progress,
        cancelled,
        float(overlap),
    )


def frames_for(chunk: int) -> int:
    return _rust.mega_frames_for(int(chunk))


def chunks_for(length: int, chunk: int, overlap: float = 0.25) -> int:
    return _rust.mega_chunks_for(int(length), int(chunk), float(overlap))
