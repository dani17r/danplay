#!/usr/bin/env python3
"""Regenera el grafo del separador de maxima calidad (danplay/data/separador/mega53.*).

Es el «MVSep Mega 53 Stems» de ZFTurbo (BS-RoFormer, MIT, release v1.0.21
de Music-Source-Separation-Training): una sola red que oye 53 instrumentos.
DanPlay usa solo las cabezas que reparte (guitarra electrica y acustica,
teclados, cuerdas, metales, percusion y congas...): el cuerpo es comun y
cada cabeza es un estimador de mascara aparte, asi que exportar unas pocas
da exactamente lo mismo que el modelo entero para esas pistas.

Como el otro exportador: el STFT y su inversa los hace `danplay.separation`
con numpy, y aqui se exporta solo la red, SIN los pesos. La receta
(`mega53.json`) dice de que tensor del checkpoint oficial sale cada peso;
la app baja ese archivo del autor (1,4 GB, con su sha256) y lo lee sin
PyTorch (`danplay/torchzip.py`).

La atencion va por bloques de filas: con trozos de 10 s, la del tiempo
seria de 62 bandas x 8 cabezas x 862 x 862 (1,5 GB) de una vez.

Para regenerarlo hace falta PyTorch (la app no lo usa):

    uv venv .dev/torchenv && VIRTUAL_ENV=.dev/torchenv uv pip install \\
        --index-url https://download.pytorch.org/whl/cpu --extra-index-url https://pypi.org/simple \\
        --index-strategy unsafe-best-match \\
        torch einops beartype rotary-embedding-torch onnx onnxscript onnxruntime pyyaml numpy
    .dev/torchenv/bin/python scripts/exportar-mega53.py

Baja a .dev/mega53/ el checkpoint, su yaml y el codigo del modelo (de la
misma release), y comprueba que el grafo, con los pesos puestos como los
pondra la app, da lo mismo que el modelo original.
"""

import hashlib
import importlib.util
import json
import re
import sys
import types
import urllib.request
from pathlib import Path

import numpy as np
import onnx
import onnxscript.optimizer
import torch
import yaml
from einops import pack, rearrange, unpack

ROOT = Path(__file__).resolve().parent.parent
DEV = ROOT / ".dev" / "mega53"
OUT = ROOT / "danplay" / "data" / "separador"
sys.path.insert(0, str(ROOT))
from danplay import torchzip  # noqa: E402

TAG = "v1.0.21"
RELEASE = f"https://github.com/ZFTurbo/Music-Source-Separation-Training/releases/download/{TAG}"
RAW = f"https://raw.githubusercontent.com/ZFTurbo/Music-Source-Separation-Training/{TAG}"
CKPT = "mvsep_mega_model_bs_roformer_53_stems_v1.ckpt"
CKPT_SHA = "c62820893bbf86d4e734f966bd142d9157cfc8bb8e79e9d8f9ea553f3ff3519f"
YAML = "mvsep_mega_model_bs_roformer_53_stems.yaml"
YAML_SHA = "7e198062a251587088adb91215a4f44ab59e67bd62fcc805cf54d6e7dfc51103"

# Las cabezas que se usan (ver `danplay.separation.LEAVES`): lo que se reparte
# y lo que decide si un instrumento esta.
HEADS = (
    "drums",
    "percussion",
    "congas",
    "tambourine",
    "lead-vocal",
    "back-vocal",
    "electric-guitar",
    "acoustic-guitar",
    "piano",
    "keys",
    "synth",
    "organ",
    "strings",
    "brass",
    "wind",
)

# Los trozos: 5 s. Los de su entrenamiento son 10 s, pero la atencion cuesta
# el cuadrado de lo largo: con 10 s la red pasaba de 8 GB y un portatil de 14
# se quedaba sin memoria; con 5, la mitad, y los trozos se solapan mas.
CHUNK = 220500
HOP = 512
FRAMES = CHUNK // HOP + 1
# Filas de la atencion que se calculan de una vez.
BLOCK = 128


def fetch(url: str, target: Path, sha: str | None = None) -> Path:
    if not target.is_file():
        target.parent.mkdir(parents=True, exist_ok=True)
        print(f"bajando {url}")
        tmp = target.with_suffix(target.suffix + ".part")
        with urllib.request.urlopen(url) as r, open(tmp, "wb") as f:
            while block := r.read(1 << 22):
                f.write(block)
        tmp.replace(target)
    if sha:
        digest = hashlib.sha256()
        with open(target, "rb") as f:
            for block in iter(lambda: f.read(1 << 22), b""):
                digest.update(block)
        assert digest.hexdigest() == sha, f"{target.name}: sha256 distinto"
    return target


def load_model():
    """El modelo de la release, con su codigo (MIT) tal cual."""
    code = DEV / "code" / "models" / "bs_roformer"
    for name in ("bs_roformer.py", "attend.py", "__init__.py"):
        if name == "__init__.py":
            code.mkdir(parents=True, exist_ok=True)
            (code / name).touch()
            (code.parent / name).touch()
            continue
        fetch(f"{RAW}/models/bs_roformer/{name}", code / name)
    sys.path.insert(0, str(DEV / "code"))
    spec = importlib.util.spec_from_file_location(
        "models.bs_roformer.bs_roformer", code / "bs_roformer.py"
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)

    config = load_config()
    model = module.BSRoformer(**config["model"]).eval()
    ckpt = fetch(f"{RELEASE}/{CKPT}", DEV / CKPT, CKPT_SHA)
    state = torch.load(ckpt, map_location="cpu", weights_only=True)
    state = state.get("state_dict", state) if isinstance(state, dict) else state
    model.load_state_dict(state)
    return model.float(), config


def blocked_attend(self, q, k, v):
    """softmax(q·kᵀ)·v por bloques de filas: lo mismo, sin la matriz entera."""
    scale = self.scale if self.scale is not None else q.shape[-1] ** -0.5
    kt = k.transpose(-1, -2)
    outs = [torch.softmax((qc @ kt) * scale, dim=-1) @ v for qc in q.split(BLOCK, dim=-2)]
    return torch.cat(outs, dim=-2)


class Core(torch.nn.Module):
    """BSRoformer.forward sin el STFT ni su inversa, con unas cabezas.

    Entra el espectrograma de un trozo en el orden de la red: (1, T, F·2·2)
    con la frecuencia y el canal intercalados ('b s f t c -> b t (f s c)').
    Salen las mascaras complejas de cada cabeza: (1, N, T, F·2·2).
    """

    def __init__(self, m, heads):
        super().__init__()
        self.m = m
        self.heads = torch.nn.ModuleList(heads)

    def forward(self, x):
        m = self.m
        x = m.band_split(x)
        for block in m.layers:
            time_transformer, freq_transformer = block[-2], block[-1]
            x = rearrange(x, "b t f d -> b f t d")
            x, ps = pack([x], "* t d")
            x = time_transformer(x)
            (x,) = unpack(x, ps, "* t d")
            x = rearrange(x, "b f t d -> b t f d")
            x, ps = pack([x], "* f d")
            x = freq_transformer(x)
            (x,) = unpack(x, ps, "* f d")
        x = m.final_norm(x)
        return torch.stack([h(x) for h in self.heads], dim=1)


def _same(w: np.ndarray, value: np.ndarray) -> str | None:
    """Como sale `value` de `w`: tal cual, traspuesto o con otra forma."""
    if w.size != value.size:
        return None
    w = w.astype(np.float32) if w.dtype == np.float16 and value.dtype == np.float32 else w
    if w.dtype != value.dtype:
        return None
    if w.shape == value.shape and np.array_equal(w, value):
        return ""
    if w.ndim == 2 and w.T.shape == value.shape and np.array_equal(w.T, value):
        return "T"
    if np.array_equal(np.ascontiguousarray(w).reshape(value.shape), value):
        return "R"
    return None


def candidates(name: str, index: list[int]) -> list[str]:
    """Los nombres del checkpoint que puede ser un peso del grafo, por su
    nombre: el exportador conserva el de cada parametro («m.layers.0…»,
    «heads.3.to_freqs…»), a veces con un prefijo o sufijo suyo."""
    out = []
    for m in re.finditer(r"(?:^|[._/])(m\.|heads\.)", name):
        tail = name[m.start(1) :]
        if tail.startswith("m."):
            out.append(tail[2:])
        else:
            head = re.match(r"heads\.(\d+)\.(.*)", tail)
            if head:
                out.append(f"mask_estimators.{index[int(head[1])]}.{head[2]}")
    return out


def bias_names(proto) -> dict[str, str]:
    """Para cada peso de una capa lineal que el exportador renombro
    («val_123», ya traspuesto): el nombre de su sesgo, que si conserva el
    suyo. Se ve en el grafo: MatMul(x, peso) -> Add(·, sesgo)."""
    consumers: dict[str, list] = {}
    for node in proto.graph.node:
        for i in node.input:
            consumers.setdefault(i, []).append(node)
    named = {t.name for t in proto.graph.initializer if t.name.endswith(".bias")}
    out = {}
    for node in proto.graph.node:
        if node.op_type not in ("MatMul", "Gemm"):
            continue
        weight = next((i for i in node.input[1:2]), None)
        if not weight:
            continue
        if node.op_type == "Gemm" and len(node.input) > 2 and node.input[2] in named:
            out[weight] = node.input[2]
            continue
        for after in consumers.get(node.output[0], []):
            if after.op_type == "Add":
                bias = next((i for i in after.input if i in named), None)
                if bias:
                    out[weight] = bias
    return out


def origin_of(
    name: str, value: np.ndarray, weights: dict, by_size: dict, index: list[int], biases: dict
):
    """De que tensor del checkpoint sale este valor: (nombre, "" | "T" | "R").
    Por su nombre, o por el de su sesgo; si no, entre los del cuerpo del
    mismo tamaño (las cabezas que no se usan no cuentan)."""
    names = candidates(name, index)
    if name in biases:
        names += [k.rsplit(".", 1)[0] + ".weight" for k in candidates(biases[name], index)]
    for key in names:
        for k in (key, key.rsplit(".", 1)[0] + ".weight", key.rsplit(".", 1)[0] + ".bias"):
            if k in weights:
                op = _same(weights[k], value)
                if op is not None:
                    return k, op
    used_heads = tuple(f"mask_estimators.{i}." for i in index)
    for key in by_size.get(value.size, []):
        if key.startswith("mask_estimators.") and not key.startswith(used_heads):
            continue
        op = _same(weights[key], value)
        if op is not None:
            return key, op
    return None


def load_config():
    class Loader(yaml.SafeLoader):
        pass

    Loader.add_constructor(
        "tag:yaml.org,2002:python/tuple", lambda l, n: tuple(l.construct_sequence(n))
    )
    # un SafeLoader que ademas entiende las tuplas del yaml de la release
    return yaml.load(fetch(f"{RELEASE}/{YAML}", DEV / YAML, YAML_SHA).read_text(), Loader=Loader)  # noqa: S506


def export_once(index: list[int], width: int) -> None:
    """Exporta con PyTorch (tarda: unos 20 min) y guarda el grafo con sus
    pesos y una entrada con su salida, para comprobar sin volver a cargar el
    modelo (no caben los dos a la vez en un portatil de 14 GB)."""
    model, _ = load_model()
    for module in model.modules():
        if type(module).__name__ == "Attend":
            module.forward = types.MethodType(blocked_attend, module)
    core = Core(model, [model.mask_estimators[i] for i in index]).eval()
    torch.manual_seed(0)
    x = torch.randn(1, FRAMES, width) * 0.5
    with torch.no_grad():
        want = core(x)
    np.savez(DEV / f"comprobar-{FRAMES}.npz", x=x.numpy(), want=want.numpy())
    cached = DEV / f"mega53-{FRAMES}.onnx"
    if not cached.is_file():
        program = torch.onnx.export(
            core, (x,), dynamo=True, input_names=["spec"], output_names=["masks"], optimize=False
        )
        proto = onnxscript.optimizer.optimize(
            program.model_proto, input_size_limit=1024, output_size_limit=65536
        )
        onnx.save_model(
            proto, str(cached), save_as_external_data=True, location=cached.name + ".data"
        )
        print("exportado", flush=True)


def main() -> None:
    config = load_config()
    names = config["training"]["instruments"]
    index = [names.index(h) for h in HEADS]
    width = sum(config["model"]["freqs_per_bands"]) * 2 * 2
    cached = DEV / f"mega53-{FRAMES}.onnx"
    check = DEV / f"comprobar-{FRAMES}.npz"
    if not cached.is_file() or not check.is_file():
        export_once(index, width)
    # el grafo sin sus pesos; los pesos, mapeados del disco (a mano: el
    # comprobador de onnx no acepta un disco NTFS montado, «not regular file»)
    proto = onnx.load(str(cached), load_external_data=False)
    data = np.memmap(DEV / (cached.name + ".data"), dtype=np.uint8, mode="r")

    def value_of(tensor) -> np.ndarray:
        if tensor.data_location != onnx.TensorProto.EXTERNAL:
            return onnx.numpy_helper.to_array(tensor)
        info = {e.key: e.value for e in tensor.external_data}
        start, size = int(info.get("offset", 0)), int(info["length"])
        dtype = onnx.helper.tensor_dtype_to_np_dtype(tensor.data_type)
        return np.frombuffer(data[start : start + size], dtype=dtype).reshape(tuple(tensor.dims))

    weights = torchzip.load(DEV / CKPT)
    by_size: dict = {}
    for key, value in weights.items():
        by_size.setdefault(value.size, []).append(key)
    biases = bias_names(proto)
    recipe, inside = [], 0
    for tensor in proto.graph.initializer:
        value = value_of(tensor)
        found = (
            origin_of(tensor.name, value, weights, by_size, index, biases)
            if value.dtype.kind == "f" and value.size > 64
            else None
        )
        if found is None:
            # una constante del grafo: viaja dentro
            inside += value.nbytes
            if tensor.data_location == onnx.TensorProto.EXTERNAL:
                tensor.raw_data = value.tobytes()
                tensor.data_location = onnx.TensorProto.DEFAULT
                del tensor.external_data[:]
            continue
        source, op = found
        item = {
            "name": tensor.name,
            "source": source,
            "op": op,
            "dtype": str(value.dtype),
            "shape": list(weights[source].shape),
        }
        if op == "R":
            item["reshape"] = list(value.shape)
        recipe.append(item)
        tensor.ClearField("raw_data")
        tensor.ClearField("float_data")
        tensor.data_location = onnx.TensorProto.EXTERNAL
        del tensor.external_data[:]
        for key, val in (
            ("location", "pesos-aparte"),
            ("offset", "0"),
            ("length", str(value.nbytes)),
        ):
            entry = tensor.external_data.add()
            entry.key, entry.value = key, val
    assert inside < 8_000_000, f"demasiadas constantes dentro del grafo: {inside} bytes"
    for node in proto.graph.node:
        del node.metadata_props[:]
        node.doc_string = ""
    proto.doc_string = (
        "MVSep Mega 53 Stems (BS-RoFormer de ZFTurbo, MIT), solo unas cabezas. Pesos aparte."
    )
    manifest = {
        "model": "mega53",
        # una red grande: la app la abre sin la reserva de memoria de ORT
        "lean": True,
        "heads": list(HEADS),
        "chunk": CHUNK,
        "stft": {"n_fft": 2048, "hop": HOP, "normalized": False},
        "bands": list(config["model"]["freqs_per_bands"]),
        "weights": {
            "file": CKPT,
            "url": f"{RELEASE}/{CKPT}",
            "sha256": CKPT_SHA,
            "bytes": (DEV / CKPT).stat().st_size,
            "format": "ckpt",
        },
        "initializers": recipe,
    }
    OUT.mkdir(parents=True, exist_ok=True)
    graph = OUT / "mega53.onnx"
    onnx.save_model(proto, graph)
    (OUT / "mega53.json").write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")

    # la prueba de verdad: con los pesos puestos como los pone la app
    sys.path.insert(0, str(ROOT))
    from danplay import separation

    del weights, by_size
    probe = np.load(check)
    session = separation.open_session(graph, manifest, DEV / CKPT)
    (got,) = session.run(None, {"spec": probe["x"]})
    diff = float(np.abs(got - probe["want"]).max())
    print(
        f"mega53: {graph.stat().st_size // 1024} KB de grafo, {len(recipe)} pesos aparte, {inside // 1024} KB dentro, diferencia {diff:.1e}"
    )
    assert diff < 2e-3, f"el grafo no da lo mismo que el modelo: {diff}"


if __name__ == "__main__":
    main()
