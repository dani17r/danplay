"""Leer pesos de un checkpoint de PyTorch sin PyTorch (danplay/torchzip.py).
Los dos archivos de prueba los hizo `torch.save` (2.14): uno de verdad y uno
con un pickle que ejecutaria una orden al abrirlo."""

from pathlib import Path

import numpy as np
import pytest

FIXTURES = Path(__file__).parent / "fixtures"


def test_the_weights_come_out_as_torch_saved_them():
    from danplay import torchzip

    w = torchzip.load(FIXTURES / "pesos.ckpt")
    assert set(w) == {"a", "b", "c", "d", "e", "layer.weight"}, "sin el «epoch» de fuera"
    a = np.array([[1.540996, -0.293429, -2.178789], [0.568431, -1.084522, -1.398595]], np.float32)
    np.testing.assert_allclose(w["a"], a, atol=1e-6)
    # una vista traspuesta del mismo almacen
    np.testing.assert_array_equal(w["c"], w["a"].T)
    assert w["b"].dtype == np.float16
    np.testing.assert_allclose(w["b"], [0.4033, 0.8379, -0.7192, -0.4033], atol=1e-3)
    np.testing.assert_allclose(w["d"], [-0.59765625, 0.181640625, -0.85546875])
    assert w["e"] == 7 and w["layer.weight"].tolist() == [[0, 1, 2], [3, 4, 5]]
    assert not w["a"].flags.writeable, "mapeado del archivo, sin copiar"


def test_a_checkpoint_that_runs_code_is_refused():
    from danplay import torchzip

    with pytest.raises(torchzip.CheckpointError, match="no es un peso"):
        torchzip.load(FIXTURES / "trampa.ckpt")
    with pytest.raises(torchzip.CheckpointError):
        torchzip.load(Path(__file__))
