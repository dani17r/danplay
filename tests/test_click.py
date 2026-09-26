"""El clic de la mezcla que se guarda (danplay/click.py): el mismo que suena
en el modo estudio (metronome.rs). Los numeros de aqui son los mismos que
comprueba la prueba de Rust `every_sound_matches_the_one_in_the_saved_mix`:
si cambia uno, tiene que cambiar el otro."""

import numpy as np
import pytest

from danplay import click

REFERENCE = [
    ("clasico", True, 1543, [0.347106, -0.255526, 0.306821]),
    ("clasico", False, 970, [0.280529, 0.292624, 0.005068]),
    ("madera", True, 2646, [0.220936, -0.653449, 0.304936]),
    ("madera", False, 2425, [0.185483, 0.308987, -0.159989]),
    ("baqueta", True, 1323, [0.038463, -0.108205, -0.035979]),
    ("baqueta", False, 1102, [0.023202, -0.056385, -0.015836]),
    ("cencerro", True, 11025, [0.261085, 0.141134, -0.069276]),
    ("cencerro", False, 8820, [0.189158, -0.003417, 0.068673]),
]


@pytest.mark.parametrize(("sound", "accent", "length", "values"), REFERENCE)
def test_every_sound_is_the_one_of_the_study(sound, accent, length, values):
    h = click.hit(sound, accent)
    assert h.size == length
    np.testing.assert_allclose(h[[10, 100, 400]], values, atol=1e-5)


def test_the_click_track_puts_a_hit_on_every_beat():
    beats = [0.5, 1.0, 1.5, 2.0]
    accents = [True, False, False, False]
    t = click.track(beats, accents, 3.0, "madera", volume=1.0)
    assert t.size == 3 * click.RATE
    starts = [i for i in range(1, t.size) if t[i] != 0 and t[i - 1] == 0]
    assert [round(s / click.RATE, 2) for s in starts] == beats
    peaks = [np.abs(t[s : s + 2000]).max() for s in starts]
    assert peaks[0] > peaks[1] * 1.3, "el «1» suena mas fuerte"
    # un sonido que no existe es el clasico; el volumen tiene tope
    np.testing.assert_array_equal(
        click.track([0.1], [False], 1, "gong"), click.track([0.1], [False], 1)
    )
    loud = click.track([0.1], [True], 1, volume=9)
    assert np.abs(loud).max() == pytest.approx(
        2 * np.abs(click.hit("clasico", True)).max(), rel=1e-6
    )
