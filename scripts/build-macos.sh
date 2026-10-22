#!/usr/bin/env bash
# El paquete de macOS, que se construye en la integración continua de GitHub
# (desde Linux no se puede: hace falta el SDK de Apple y un Mac).
#
#   ./scripts/build-macos.sh            lanza .github/workflows/macos.yml con
#                                       lo subido de esta rama, espera y lo baja
#   ./scripts/build-macos.sh ID         baja el de una ejecución ya terminada
#
# Los dos .dmg quedan en dist/installers/, junto al .deb, el .AppImage y los
# de Windows, con nombres que dicen para qué Mac es cada uno:
#   DanPlay-X.Y.Z-mac-apple-silicon.dmg   M1 en adelante
#   DanPlay-X.Y.Z-mac-intel.dmg           los de antes
#
# Es un atajo de scripts/github.sh (`packages macos` y `download ID`), que hace
# lo mismo con Linux y Windows y con las pruebas.
set -euo pipefail
cd "$(dirname "$0")"

if [ -n "${1:-}" ]; then
    exec ./github.sh download "$1"
fi
exec ./github.sh packages macos
