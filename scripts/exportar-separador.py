#!/usr/bin/env python3
"""Regenera los grafos del separador de pistas (danplay/data/separador/).

El separador es Demucs v4 (HTDemucs, de Meta, licencia MIT) ejecutado con
ONNX Runtime, sin PyTorch. Aqui se exporta **solo la red**: el espectrograma
y su inversa los hace `danplay.separation` con numpy, igual que Demucs.

Lo que sale, por modelo, son dos archivos pequeños que viajan con la app:

- `<modelo>.onnx`: el grafo SIN los pesos. Cada peso es una referencia
  externa que la app rellena al cargarlo (`add_external_initializers`).
- `<modelo>.json`: de donde salen esos pesos. El archivo oficial del autor
  (safetensors, en HuggingFace) con su sha256, y para cada peso del grafo
  el tensor del archivo del que sale: tal cual o traspuesto (el exportador
  guarda traspuestos los de las capas lineales).

Asi la app no reparte los pesos (84 MB y 55 MB): los baja del autor la
primera vez que se separa algo, y se comprueba que no cambiaron.

Para regenerarlos hace falta PyTorch y Demucs, que la app no usa:

    uv run --no-project -p 3.13 \\
        --index-url https://download.pytorch.org/whl/cpu --index-strategy unsafe-best-match \\
        --with torch --with demucs==4.1.0 --with onnx --with onnxscript --with onnxruntime \\
        python scripts/exportar-separador.py

Antes de guardar se comprueba que el grafo, con los pesos puestos como los
pondra la app, da lo mismo que el modelo original.
"""

import importlib.util
import json
import sys
import types
from pathlib import Path

import numpy as np
import onnx
import onnxscript.optimizer
import torch
import torch.nn.functional as F
from demucs.pretrained import get_model
from einops import rearrange
from huggingface_hub import hf_hub_download

ROOT = Path(__file__).resolve().parent.parent
# el motor tal cual, sin el paquete (que tira de la configuracion de la app)
_spec = importlib.util.spec_from_file_location("engine", ROOT / "danplay" / "separation.py")
engine = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(engine)

OUT = ROOT / "danplay" / "data" / "separador"

# nombre en Demucs -> (repositorio de HuggingFace, firma del archivo de pesos)
MODELS = {
    "htdemucs_6s": ("adefossez/HTDemucs-6s", "5c90dfd2"),
    "htdemucs": ("adefossez/HTDemucs", "955717e8"),
}

# Cuantas filas de la atencion se calculan de una vez. ONNX Runtime guarda
# entera la matriz de la atencion (8 cabezas x 2688 x 2688, 230 MB) donde
# PyTorch no: por bloques da exactamente lo mismo y la memoria baja.
CHUNK = 512


class Core(torch.nn.Module):
    """HTDemucs.forward sin el espectrograma ni su inversa.

    Entra la mezcla (1, 2, L) y su espectrograma complejo en canales
    (1, 4, 2048, T); salen el espectrograma de cada fuente (1, S, 4, 2048,
    T), ya desnormalizado, y la rama temporal (1, S, 2, L).
    """

    def __init__(self, m):
        super().__init__()
        self.m = m

    def forward(self, mix, mag):
        m = self.m
        length = mix.shape[-1]
        x = mag
        B, _, Fq, T = x.shape
        mean = x.mean(dim=(1, 2, 3), keepdim=True)
        std = x.std(dim=(1, 2, 3), keepdim=True)
        x = (x - mean) / (1e-5 + std)
        xt = mix
        meant = xt.mean(dim=(1, 2), keepdim=True)
        stdt = xt.std(dim=(1, 2), keepdim=True)
        xt = (xt - meant) / (1e-5 + stdt)
        saved, saved_t, lengths, lengths_t = [], [], [], []
        for idx, encode in enumerate(m.encoder):
            lengths.append(x.shape[-1])
            inject = None
            if idx < len(m.tencoder):
                lengths_t.append(xt.shape[-1])
                tenc = m.tencoder[idx]
                xt = tenc(xt)
                if not tenc.empty:
                    saved_t.append(xt)
                else:
                    inject = xt
            x = encode(x, inject)
            if idx == 0 and m.freq_emb is not None:
                frs = torch.arange(x.shape[-2], device=x.device)
                emb = m.freq_emb(frs).t()[None, :, :, None].expand_as(x)
                x = x + m.freq_emb_scale * emb
            saved.append(x)
        if m.crosstransformer:
            if m.bottom_channels:
                _, _, f, _ = x.shape
                x = rearrange(x, "b c f t-> b c (f t)")
                x = m.channel_upsampler(x)
                x = rearrange(x, "b c (f t)-> b c f t", f=f)
                xt = m.channel_upsampler_t(xt)
            x, xt = m.crosstransformer(x, xt)
            if m.bottom_channels:
                x = rearrange(x, "b c f t-> b c (f t)")
                x = m.channel_downsampler(x)
                x = rearrange(x, "b c (f t)-> b c f t", f=f)
                xt = m.channel_downsampler_t(xt)
        for idx, decode in enumerate(m.decoder):
            skip = saved.pop(-1)
            x, pre = decode(x, skip, lengths.pop(-1))
            offset = m.depth - len(m.tdecoder)
            if idx >= offset:
                tdec = m.tdecoder[idx - offset]
                length_t = lengths_t.pop(-1)
                if tdec.empty:
                    pre = pre[:, :, 0]
                    xt, _ = tdec(pre, None, length_t)
                else:
                    skip = saved_t.pop(-1)
                    xt, _ = tdec(xt, skip, length_t)
        S = len(m.sources)
        x = x.view(B, S, -1, Fq, T)
        x = x * std[:, None] + mean[:, None]
        xt = xt.view(B, S, -1, length)
        xt = xt * stdt[:, None] + meant[:, None]
        return x, xt


def chunked_attention(
    self,
    query,
    key,
    value,
    key_padding_mask=None,
    need_weights=True,
    attn_mask=None,
    average_attn_weights=True,
    is_causal=False,
):
    """nn.MultiheadAttention (batch_first, sin mascaras) por bloques de filas."""
    assert self.batch_first and attn_mask is None and key_padding_mask is None
    E, H = self.embed_dim, self.num_heads
    d = E // H
    w, b = self.in_proj_weight, self.in_proj_bias
    q = F.linear(query, w[:E], b[:E])
    k = F.linear(key, w[E : 2 * E], b[E : 2 * E])
    v = F.linear(value, w[2 * E :], b[2 * E :])
    B, L, _ = q.shape
    S = k.shape[1]
    q = q.view(B, L, H, d).transpose(1, 2) * (d**-0.5)
    kt = k.view(B, S, H, d).transpose(1, 2).transpose(-1, -2)
    v = v.view(B, S, H, d).transpose(1, 2)
    outs = [torch.softmax(qc @ kt, dim=-1) @ v for qc in q.split(CHUNK, dim=2)]
    o = torch.cat(outs, dim=2).transpose(1, 2).reshape(B, L, E)
    return F.linear(o, self.out_proj.weight, self.out_proj.bias), None


def origin_of(value: np.ndarray, weights: dict, by_size: dict) -> tuple[str, str] | None:
    """De que tensor oficial sale este valor: (nombre, "" | "T"), o None."""
    for key in by_size.get(value.size, []):
        w = weights[key]
        # los pesos oficiales van en float16 y la red los usa en float32: el
        # paso de uno a otro es exacto, y es el que hace la app al cargarlos
        if w.dtype == np.float16 and value.dtype == np.float32:
            w = w.astype(np.float32)
        elif w.dtype != value.dtype:
            continue
        if w.shape == value.shape and np.array_equal(w, value):
            return key, ""
        if w.ndim == 2 and w.T.shape == value.shape and np.array_equal(w.T, value):
            return key, "T"
    return None


def export(name: str) -> None:
    repo, sig = MODELS[name]
    weights_file = Path(hf_hub_download(repo, f"{sig}.safetensors"))
    weights = engine.read_safetensors(weights_file)
    by_size: dict = {}
    for key, value in weights.items():
        by_size.setdefault(value.size, []).append(key)

    model = get_model(name).models[0].eval()
    assert model.samplerate == engine.RATE and model.nfft == engine.NFFT
    assert model.hop_length == engine.HOP and model.cac
    segment = int(model.segment * model.samplerate)
    assert segment == engine.SEGMENT, segment
    for module in model.modules():
        if isinstance(module, torch.nn.MultiheadAttention):
            module.forward = types.MethodType(chunked_attention, module)
    core = Core(model).eval()

    mix = torch.randn(1, 2, segment) * 0.1
    with torch.no_grad():
        mag = model._magnitude(model._spec(mix))
    program = torch.onnx.export(
        core,
        (mix, mag),
        dynamo=True,
        input_names=["mix", "mag"],
        output_names=["spec", "wave"],
        optimize=False,
    )
    # Plegar solo lo pequeño (formas, rangos): lo grande se queda como esta,
    # para que cada peso siga siendo un tensor del archivo oficial.
    proto = onnxscript.optimizer.optimize(
        program.model_proto, input_size_limit=1024, output_size_limit=65536
    )

    recipe, inside = [], 0
    for tensor in proto.graph.initializer:
        value = onnx.numpy_helper.to_array(tensor)
        found = origin_of(value, weights, by_size) if value.size > 64 else None
        if found is None:
            inside += value.nbytes  # una constante del grafo: viaja dentro
            continue
        source, op = found
        recipe.append(
            {
                "name": tensor.name,
                "source": source,
                "op": op,
                "dtype": str(value.dtype),
                "shape": list(weights[source].shape),
            }
        )
        # sin datos: una referencia externa que la app rellena al cargar
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
    assert inside < 2_000_000, f"demasiadas constantes dentro del grafo: {inside} bytes"
    for node in proto.graph.node:
        del node.metadata_props[:]
        node.doc_string = ""
    proto.doc_string = f"HTDemucs ({name}), Demucs v4 de Meta (MIT). Pesos aparte."

    digest = engine.sha256_of(weights_file)
    manifest = {
        "model": name,
        "sources": list(model.sources),
        "weights": {
            "file": f"{sig}.safetensors",
            "url": f"https://huggingface.co/{repo}/resolve/main/{sig}.safetensors",
            "sha256": digest,
            "bytes": weights_file.stat().st_size,
        },
        "initializers": recipe,
    }
    OUT.mkdir(parents=True, exist_ok=True)
    graph = OUT / f"{name}.onnx"
    onnx.save_model(proto, graph)
    (OUT / f"{name}.json").write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")

    # la prueba de verdad: con los pesos puestos como los pone la app
    session = engine.open_session(graph, manifest, weights_file)
    with torch.no_grad():
        want_x, want_xt = core(mix, mag)
    got_x, got_xt = session.run(None, {"mix": mix.numpy(), "mag": mag.numpy()})
    diff = max(
        float(np.abs(got_x - want_x.numpy()).max()), float(np.abs(got_xt - want_xt.numpy()).max())
    )
    assert diff < 1e-3, f"el grafo no da lo mismo que el modelo: {diff}"
    print(
        f"{name}: {graph.stat().st_size // 1024} KB de grafo, {len(recipe)} pesos aparte "
        f"({inside // 1024} KB de constantes dentro), diferencia {diff:.1e}"
    )


if __name__ == "__main__":
    for model_name in sys.argv[1:] or MODELS:
        export(model_name)
