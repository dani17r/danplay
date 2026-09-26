#!/usr/bin/env python3
"""Regenera el grafo del detector de pulso (danplay/data/pulso/).

El detector es Beat This! (Foscarin, Schlüter y Widmer, CPJKU, 2024; codigo
y pesos con licencia MIT), ejecutado con ONNX Runtime, sin PyTorch. Aqui se
exporta la red con sus pesos; el espectrograma, los trozos y los pulsos los
hace `danplay.pulse` con numpy, igual que su codigo.

Para regenerarlo hace falta PyTorch, torchaudio y beat_this, que la app no
usa (torchaudio se quedo en la 2.11: con un torch mas nuevo no carga):

    uv run --no-project -p 3.13 \\
        --index-url https://download.pytorch.org/whl/cpu --extra-index-url https://pypi.org/simple \\
        --index-strategy unsafe-best-match \\
        --with "torch==2.11.*" --with "torchaudio==2.11.*" --with soundfile \\
        --with "beat_this @ git+https://github.com/CPJKU/beat_this.git" \\
        --with onnx --with onnxruntime \\
        python scripts/exportar-pulso.py [final0|small0]

Antes de guardar se comprueba que el grafo da lo mismo que la red en
PyTorch (con trozos de varias longitudes), y que el espectrograma de numpy
es el de torchaudio. Los pesos se guardan en float16 dentro del grafo, cada
uno con una conversion a float32 que ONNX Runtime hace al cargar (la mitad
de tamaño, y el calculo en float32 como la original).
"""

import hashlib
import importlib.util
import json
import sys
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
import torch
from onnx import helper, numpy_helper

ROOT = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location("pulse", ROOT / "danplay" / "pulse.py")
assert _spec and _spec.loader
pulse = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pulse)

OUT = ROOT / "danplay" / "data" / "pulso"
CHECKPOINTS = "https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp"


class Core(torch.nn.Module):
    """BeatThis sin el diccionario: entra el espectrograma (1, T, 128) y salen
    los logits de pulso y de «1» (1, T)."""

    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, spect):
        p = self.m(spect)
        return p["beat"], p["downbeat"]


def unwrap_autocast() -> None:
    """Los candados de autocast (que en CPU no hacen nada) no se exportan."""
    import rotary_embedding_torch.rotary_embedding_torch as rot

    torch.amp.is_autocast_available = lambda *_: False
    rot.apply_rotary_emb = rot.apply_rotary_emb.__wrapped__
    rot.RotaryEmbedding.forward = rot.RotaryEmbedding.forward.__wrapped__


def half_weights(model: onnx.ModelProto) -> int:
    """Cada peso float32 grande pasa a float16, con una conversion a float32
    detras (ONNX Runtime la hace una vez, al cargar). Devuelve cuantos."""
    graph = model.graph
    n = 0
    keep, casts = [], []
    for init in graph.initializer:
        value = numpy_helper.to_array(init)
        if value.dtype != np.float32 or value.size < 1024:
            keep.append(init)
            continue
        half = numpy_helper.from_array(value.astype(np.float16), init.name + "_f16")
        keep.append(half)
        casts.append(helper.make_node("Cast", [half.name], [init.name], to=onnx.TensorProto.FLOAT))
        n += 1
    del graph.initializer[:]
    graph.initializer.extend(keep)
    for node in reversed(casts):
        graph.node.insert(0, node)
    return n


def check_frontend() -> float:
    """El espectrograma de numpy frente al de Beat This! (torchaudio)."""
    from beat_this.preprocessing import LogMelSpect

    rng = np.random.default_rng(0)
    t = np.arange(10 * pulse.RATE) / pulse.RATE
    signal = 0.3 * np.sin(2 * np.pi * 220 * t) + 0.05 * rng.standard_normal(t.size)
    want = LogMelSpect()(torch.tensor(signal, dtype=torch.float32)).numpy()
    got = pulse.spectrogram(signal.astype(np.float32))
    assert want.shape == got.shape, (want.shape, got.shape)
    diff = float(np.abs(want - got).max())
    assert diff < 1e-3, f"el espectrograma no es el de Beat This!: {diff}"
    return diff


def export(name: str) -> None:
    from beat_this.inference import load_checkpoint, load_model

    unwrap_autocast()
    model = load_model(name, "cpu")
    core = Core(model).eval()
    params = sum(p.numel() for p in model.parameters())
    OUT.mkdir(parents=True, exist_ok=True)
    graph = OUT / "beat_this.onnx"
    x = torch.randn(1, pulse.CHUNK, 128)
    torch.onnx.export(
        core,
        (x,),
        str(graph),
        dynamo=False,
        input_names=["spect"],
        output_names=["beat", "downbeat"],
        dynamic_axes={"spect": {1: "T"}, "beat": {1: "T"}, "downbeat": {1: "T"}},
        opset_version=18,
    )
    proto = onnx.load(str(graph))
    halved = half_weights(proto)
    proto.doc_string = f"Beat This! ({name}), CPJKU, MIT. Pesos en float16."
    onnx.save_model(proto, str(graph))

    # la prueba de verdad: trozos de varias longitudes, contra PyTorch con
    # los mismos pesos redondeados a float16 que lleva el grafo; y cuanto se
    # aparta eso de la red original, en float32
    inputs = [torch.randn(1, n, 128) * 2 + 3 for n in (pulse.CHUNK, 700, 64)]
    with torch.no_grad():
        original = [core(xi) for xi in inputs]
        for p in model.parameters():
            if p.numel() >= 1024:
                p.data = p.data.half().float()
        rounded = [core(xi) for xi in inputs]
    session = ort.InferenceSession(str(graph), providers=["CPUExecutionProvider"])
    worst = drift = 0.0
    for xi, (ob, od), (wb, wd) in zip(inputs, original, rounded, strict=True):
        gb, gd = session.run(None, {"spect": xi.numpy()})
        worst = max(
            worst, float(np.abs(gb - wb.numpy()).max()), float(np.abs(gd - wd.numpy()).max())
        )
        drift = max(
            drift, float(np.abs(gb - ob.numpy()).max()), float(np.abs(gd - od.numpy()).max())
        )
    assert worst < 1e-3, f"el grafo no da lo mismo que la red: {worst}"
    # en logits (del orden de ±10): nada que cambie donde cae un pulso
    assert drift < 0.05, f"los pesos en float16 cambian demasiado la red: {drift}"
    frontend = check_frontend()

    ckpt = Path(torch.hub.get_dir()) / "checkpoints" / f"beat_this-{name}.ckpt"
    load_checkpoint(name, "cpu")  # que este bajado
    digest = hashlib.sha256(ckpt.read_bytes()).hexdigest()
    info = {
        "model": name,
        "source": f"{CHECKPOINTS}/{name}.ckpt",
        "sha256": digest,
        "parameters": params,
        "weights": "float16",
        "fps": pulse.FPS,
        "chunk": pulse.CHUNK,
        "border": pulse.BORDER,
    }
    (OUT / "beat_this.json").write_text(json.dumps(info, indent=1) + "\n", encoding="utf-8")
    print(
        f"{name}: {graph.stat().st_size / 1e6:.1f} MB de grafo, {params} parametros "
        f"({halved} pesos en float16), diferencia {worst:.1e} (con la original en float32, "
        f"{drift:.1e}); espectrograma {frontend:.1e}"
    )


if __name__ == "__main__":
    export(sys.argv[1] if len(sys.argv) > 1 else "final0")
