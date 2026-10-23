#!/bin/sh
# La linea de ordenes del nucleo de Python (escanear, el estado, `serve`...).
# La app ya no lo usa: sigue en el repositorio para sus pruebas y para los
# diferenciales con el de Rust.
exec "$(dirname "$0")/.venv/bin/python" -m danplay.cli "$@"
