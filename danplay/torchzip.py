"""Leer los pesos de un checkpoint de PyTorch (`.ckpt`, `.pth`) sin PyTorch.

Un checkpoint moderno es un zip: `data.pkl` (un pickle que dice que tensor es
cual) y `data/<n>` (los numeros de cada almacen, en crudo). Abrir el pickle
con `pickle.load` sin mas ejecutaria lo que trajera dentro: lo lee el crate
(analysis/src/weights.rs), con una maquina de pickle propia que solo conoce lo
que un diccionario de pesos necesita (reconstruir un tensor desde su almacen,
un OrderedDict) y rechaza todo lo demas. Rust dice donde esta cada tensor en
el archivo y aqui se mapean con numpy, sin copiarlos (los checkpoints van sin
comprimir).

Para el separador de maxima calidad: los pesos del Mega53 de ZFTurbo (MIT)
se publican asi, y DanPlay los baja de su autor como los demas.
"""

from pathlib import Path

import danplay_core as _rust
import numpy as np

# El archivo no es un checkpoint que se sepa leer sin riesgo (un ValueError).
CheckpointError = _rust.CheckpointError


def _bfloat16(raw: np.ndarray) -> np.ndarray:
    """bfloat16 a float32: los 16 bits altos de un float32."""
    return (raw.view(np.uint16).astype(np.uint32) << 16).view(np.float32)


def load(path, key: str | None = None) -> dict[str, np.ndarray]:
    """Los tensores del checkpoint: {nombre: array} (de solo lectura,
    mapeados del archivo). Si el checkpoint guarda el modelo dentro de otra
    clave (`state_dict`, `model`), se dice con `key` o se busca solo."""
    path = Path(path)
    tensors = _rust.torchzip_tensors(path, key)
    mapped = np.memmap(path, dtype=np.uint8, mode="r")
    # un almacen sirve a varios tensores (una vista traspuesta, por ejemplo)
    storages: dict[str, np.ndarray] = {}
    out: dict[str, np.ndarray] = {}
    for name, dtype, storage, start, count, offset, shape, strides in tensors:
        data = storages.get(storage)
        if data is None:
            raw_type = np.dtype(np.uint16 if dtype == "bfloat16" else dtype)
            data = mapped[start : start + count * raw_type.itemsize].view(raw_type)[:count]
            storages[storage] = data
        if dtype == "bfloat16":
            data = _bfloat16(data)
        if not shape:
            out[name] = np.array(data[offset], copy=True)
            continue
        itemsize = data.dtype.itemsize
        out[name] = np.lib.stride_tricks.as_strided(
            data[offset:],
            shape=tuple(shape),
            strides=tuple(s * itemsize for s in strides),
            writeable=False,
        )
    return out
