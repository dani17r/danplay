"""Leer los pesos de un checkpoint de PyTorch (`.ckpt`, `.pth`) sin PyTorch.

Un checkpoint moderno es un zip: `data.pkl` (un pickle que dice que tensor es
cual) y `data/<n>` (los numeros de cada almacen, en crudo). Abrir el pickle
con `pickle.load` sin mas ejecutaria lo que trajera dentro: aqui se lee con
un `Unpickler` que solo conoce lo que un diccionario de pesos necesita
(reconstruir un tensor desde su almacen, un OrderedDict) y rechaza todo lo
demas. Los numeros se mapean del zip sin copiarlos (los checkpoints van sin
comprimir).

Para el separador de maxima calidad: los pesos del Mega53 de ZFTurbo (MIT)
se publican asi, y DanPlay los baja de su autor como los demas.
"""

import collections
import pickle
import zipfile
from pathlib import Path

import numpy as np

_STORAGES = {
    "FloatStorage": np.float32,
    "HalfStorage": np.float16,
    "DoubleStorage": np.float64,
    "LongStorage": np.int64,
    "IntStorage": np.int32,
    "ShortStorage": np.int16,
    "CharStorage": np.int8,
    "ByteStorage": np.uint8,
    "BoolStorage": np.bool_,
    "BFloat16Storage": "bfloat16",
}


class CheckpointError(ValueError):
    """El archivo no es un checkpoint que se sepa leer sin riesgo."""


class _Storage:
    def __init__(self, dtype, data: np.ndarray):
        self.dtype = dtype
        self.data = data


def _bfloat16(raw: np.ndarray) -> np.ndarray:
    """bfloat16 a float32: los 16 bits altos de un float32."""
    return (raw.view(np.uint16).astype(np.uint32) << 16).view(np.float32)


def _rebuild_tensor(storage, offset, size, stride, *_rest):
    data = storage.data
    if storage.dtype == "bfloat16":
        data = _bfloat16(data)
    if not size:
        return np.array(data[offset], copy=True)
    itemsize = data.dtype.itemsize
    return np.lib.stride_tricks.as_strided(
        data[offset:],
        shape=tuple(size),
        strides=tuple(s * itemsize for s in stride),
        writeable=False,
    )


def _rebuild_parameter(data, *_rest):
    return data


class _Unpickler(pickle.Unpickler):
    def __init__(self, file, archive: zipfile.ZipFile, prefix: str, mapped: np.ndarray):
        super().__init__(file)
        self.archive = archive
        self.prefix = prefix
        self.mapped = mapped
        self.cache: dict[str, _Storage] = {}

    def find_class(self, module, name):
        if module == "collections" and name == "OrderedDict":
            return collections.OrderedDict
        if module == "torch._utils" and name in ("_rebuild_tensor_v2", "_rebuild_tensor"):
            return _rebuild_tensor
        if module == "torch._utils" and name == "_rebuild_parameter":
            return _rebuild_parameter
        if module == "torch" and name in _STORAGES:
            return name
        if module == "torch" and name == "Size":
            return tuple
        raise CheckpointError(f"el checkpoint trae algo que no es un peso: {module}.{name}")

    def persistent_load(self, pid):
        # ('storage', tipo, clave, dispositivo, cuantos)
        if not isinstance(pid, tuple) or len(pid) < 5 or pid[0] != "storage":
            raise CheckpointError("almacen desconocido en el checkpoint")
        kind, key, numel = pid[1], str(pid[2]), int(pid[4])
        if key in self.cache:
            return self.cache[key]
        name = kind if isinstance(kind, str) else getattr(kind, "__name__", "")
        dtype = _STORAGES.get(name)
        if dtype is None:
            raise CheckpointError(f"tipo de almacen desconocido: {name}")
        info = self.archive.getinfo(f"{self.prefix}data/{key}")
        if info.compress_type != zipfile.ZIP_STORED:
            raise CheckpointError("el checkpoint va comprimido")
        start = _data_offset(self.archive, info)
        raw_type = np.uint16 if dtype == "bfloat16" else dtype
        count = numel if numel else info.file_size // np.dtype(raw_type).itemsize
        data = self.mapped[start : start + info.file_size].view(raw_type)[:count]
        storage = _Storage(dtype, data)
        self.cache[key] = storage
        return storage


def _data_offset(archive: zipfile.ZipFile, info: zipfile.ZipInfo) -> int:
    """Donde empiezan los datos de un miembro sin comprimir: tras su
    cabecera local (que puede traer extras distintos de los del directorio)."""
    fp = archive.fp
    assert fp is not None
    fp.seek(info.header_offset)
    header = fp.read(30)
    if header[:4] != b"PK\x03\x04":
        raise CheckpointError("zip dañado")
    name_len = int.from_bytes(header[26:28], "little")
    extra_len = int.from_bytes(header[28:30], "little")
    return info.header_offset + 30 + name_len + extra_len


def load(path, key: str | None = None) -> dict[str, np.ndarray]:
    """Los tensores del checkpoint: {nombre: array} (de solo lectura,
    mapeados del archivo). Si el checkpoint guarda el modelo dentro de otra
    clave (`state_dict`, `model`), se dice con `key` o se busca solo."""
    path = Path(path)
    try:
        archive = zipfile.ZipFile(path)
    except zipfile.BadZipFile as e:
        raise CheckpointError(f"{path.name} no es un checkpoint de PyTorch moderno") from e
    with archive:
        pkl = next(
            (n for n in archive.namelist() if n.endswith("/data.pkl") or n == "data.pkl"), None
        )
        if pkl is None:
            raise CheckpointError(f"{path.name} no trae data.pkl")
        prefix = pkl[: -len("data.pkl")]
        mapped = np.memmap(path, dtype=np.uint8, mode="r")
        with archive.open(pkl) as f:
            obj = _Unpickler(f, archive, prefix, mapped).load()
    if key is not None:
        obj = obj[key]
    elif isinstance(obj, dict) and not any(isinstance(v, np.ndarray) for v in obj.values()):
        for candidate in ("state_dict", "model", "model_state_dict"):
            if isinstance(obj.get(candidate), dict):
                obj = obj[candidate]
                break
    if not isinstance(obj, dict):
        raise CheckpointError(f"{path.name} no es un diccionario de pesos")
    return {str(k): v for k, v in obj.items() if isinstance(v, np.ndarray)}
