"""Punto de entrada del nucleo empaquetado.

Se compila con PyInstaller y viaja dentro del .deb / .AppImage como sidecar,
para que la app no dependa de que haya Python instalado.
"""
import sys, multiprocessing

def main():
    multiprocessing.freeze_support()
    from danplay.api import serve
    uds = None
    args = sys.argv[1:]
    if "--uds" in args:
        uds = args[args.index("--uds") + 1]
    serve(uds=uds)

if __name__ == "__main__":
    main()
