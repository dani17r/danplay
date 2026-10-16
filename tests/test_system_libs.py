"""El nucleo empaquetado de Linux lanza ffmpeg y los demas con las
bibliotecas del sistema, no con las del paquete (danplay/system_libs.py)."""

from danplay import system_libs


def test_the_original_library_path_comes_back():
    env = {"LD_LIBRARY_PATH": "/tmp/_MEI123:/opt/x", "LD_LIBRARY_PATH_ORIG": "/opt/x"}
    assert system_libs.restore(env, frozen=True, platform="linux")
    assert env["LD_LIBRARY_PATH"] == "/opt/x"


def test_without_one_before_it_goes_away():
    """Lo normal: no habia ninguna, y la del paquete no se queda."""
    env = {"LD_LIBRARY_PATH": "/tmp/_MEI123", "PATH": "/usr/bin"}
    assert system_libs.restore(env, frozen=True, platform="linux")
    assert "LD_LIBRARY_PATH" not in env and env["PATH"] == "/usr/bin"


def test_outside_the_package_or_linux_nothing_changes():
    env = {"LD_LIBRARY_PATH": "/mio"}
    assert not system_libs.restore(env, frozen=False, platform="linux")
    assert not system_libs.restore(env, frozen=True, platform="win32")
    assert not system_libs.restore(env, frozen=True, platform="darwin")
    assert env == {"LD_LIBRARY_PATH": "/mio"}
    assert not system_libs.restore({}, frozen=True, platform="linux")


def test_a_real_ffmpeg_child_gets_the_system_libraries(tmp_path, monkeypatch):
    """Como en el paquete: el hijo ya no ve la carpeta del paquete."""
    import subprocess
    import sys

    monkeypatch.setenv("LD_LIBRARY_PATH", str(tmp_path))
    monkeypatch.delenv("LD_LIBRARY_PATH_ORIG", raising=False)
    system_libs.restore(frozen=True, platform="linux")
    out = subprocess.run(
        [sys.executable, "-c", "import os; print(os.environ.get('LD_LIBRARY_PATH', '-'))"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    assert out == "-"
