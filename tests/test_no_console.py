"""En Windows, los procesos que lanza el nucleo no abren ventanas negras."""

import subprocess

from danplay import no_console


def test_se_añade_la_marca_salvo_si_se_pide_consola_a_proposito():
    assert no_console.hidden(None) == no_console.CREATE_NO_WINDOW
    assert no_console.hidden(0x200) == 0x200 | no_console.CREATE_NO_WINDOW
    new_console, detached = 0x10, 0x08
    assert no_console.hidden(new_console) == new_console
    assert no_console.hidden(detached) == detached


def _fake_popen():
    class Popen:
        def __init__(self, args, **kwargs):
            self.args, self.kwargs = args, kwargs

    class Child(Popen):  # como el Popen de yt-dlp
        def __init__(self, args, **kwargs):
            super().__init__(args, **kwargs)

    return Popen, Child


def test_en_windows_todos_los_procesos_van_sin_ventana():
    Popen, Child = _fake_popen()
    assert no_console.install("win32", Popen)
    assert Popen(["ffmpeg"]).kwargs["creationflags"] == no_console.CREATE_NO_WINDOW
    assert Child(["yt-dlp"]).kwargs["creationflags"] == no_console.CREATE_NO_WINDOW
    assert Popen(["x"], creationflags=0x10).kwargs["creationflags"] == 0x10
    assert not no_console.install("win32", Popen), "una sola vez, sin envolverlo dos veces"


def test_en_linux_no_se_toca_nada():
    Popen, _ = _fake_popen()
    assert not no_console.install("linux", Popen)
    assert "creationflags" not in Popen(["ffmpeg"]).kwargs
    assert not getattr(subprocess.Popen, "_danplay_no_console", False)
