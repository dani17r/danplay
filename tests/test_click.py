"""El clic de la mezcla que se guarda (danplay/click.py): el mismo que suena
en el modo estudio (motor/src/click.rs). Los numeros de aqui son los mismos que
comprueba la prueba de Rust `every_sound_matches_the_one_in_the_saved_mix`:
si cambia uno, tiene que cambiar el otro."""

import numpy as np
import pytest

from danplay import click

REFERENCE = [
    ("clasico", True, 2646, [0.244754, -0.173007, 0.481330]),
    ("clasico", False, 2205, [0.367322, 0.329463, 0.012853]),
    ("madera", True, 3528, [0.221040, -0.714910, 0.415429]),
    ("madera", False, 3087, [0.213124, 0.374582, -0.242670]),
    ("baqueta", True, 1764, [0.045442, -0.141499, -0.065701]),
    ("baqueta", False, 1455, [0.032436, -0.090391, -0.035205]),
    ("cencerro", True, 11025, [0.261085, 0.141134, -0.069276]),
    ("cencerro", False, 8820, [0.216180, -0.003905, 0.078483]),
    ("claves", True, 5292, [-0.176218, -0.686703, -0.600713]),
    ("claves", False, 4410, [0.042177, -0.107441, -0.143851]),
    ("hihat", True, 3969, [-0.038458, 0.417596, -0.106221]),
    ("hihat", False, 2646, [-0.030294, 0.312369, -0.063259]),
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
        click.MAX_VOLUME * np.abs(click.hit("clasico", True)).max(), rel=1e-6
    )
