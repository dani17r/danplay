"""Punto de entrada del nucleo empaquetado.

Se compila con PyInstaller y viaja dentro del .deb / .AppImage / instalador,
para que la aplicacion no dependa de que haya Python instalado.

Los argumentos son los mismos que `danplay serve`:

    danplay-core --uds RUTA          socket Unix (Linux y macOS)
    danplay-core --host H --port N   loopback (Windows), con DANPLAY_TOKEN
"""

import argparse
import multiprocessing
import os
import sys


def without_console():
    """En Windows el nucleo va sin consola (no abre una ventana negra detras
    de la app) y Python deja `sys.stdout` y `sys.stderr` en None. uvicorn
    pregunta `sys.stdout.isatty()` al configurar sus avisos, asi que el nucleo
    se caia antes de escuchar, tanto lanzado por la app como a mano. Lo que se
    escriba ahi no va a ninguna parte; los avisos van al registro
    (DATA_DIR/logs)."""
    for name in ("stdout", "stderr"):
        if getattr(sys, name) is None:
            setattr(sys, name, open(os.devnull, "w", encoding="utf-8"))  # noqa: SIM115


def main():
    without_console()
    multiprocessing.freeze_support()
    parser = argparse.ArgumentParser(prog="danplay-core", add_help=True)
    parser.add_argument("--uds", help="socket Unix (sin puerto TCP)")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8730)
    # Antes se leia `sys.argv` a mano y `--uds` sin valor reventaba con un
    # IndexError antes de llegar a decir nada util.
    args = parser.parse_args()

    from danplay.api import serve

    serve(host=args.host, port=args.port, uds=args.uds)


if __name__ == "__main__":
    sys.exit(main())
