#!/bin/sh
# La instalacion limpia: instala el .deb en sistemas recien hechos (Docker)
# y comprueba que no le falte nada para abrirse y sonar.
#
#   ./scripts/test-install.sh                 el .deb mas nuevo de dist/installers
#   ./scripts/test-install.sh RUTA.deb        ese
#   DANPLAY_INSTALL_IMAGES="debian:13" ...    en que sistemas (por defecto,
#                                             Debian 13 y Ubuntu 24.04)
#
# Lo que mira esta en `scripts/install-check.sh`. Tarda unos minutos por
# sistema (baja WebKit y compañia). No entra en la CI: pide Docker y red.
set -eu
cd "$(dirname "$0")/.."
die () { printf '\n\033[31mERROR\033[0m  %s\n' "$1" >&2; exit 1; }

command -v docker >/dev/null 2>&1 || die "hace falta Docker"
DEB="${1:-$(ls -t dist/installers/*.deb 2>/dev/null | head -1)}"
[ -n "$DEB" ] && [ -f "$DEB" ] || die "no hay ningun .deb: ./scripts/build.sh --package"
DEB=$(realpath "$DEB")
CHECK=$(realpath scripts/install-check.sh)
IMAGES="${DANPLAY_INSTALL_IMAGES:-debian:13 ubuntu:24.04}"

FAILED=""
for image in $IMAGES; do
    printf '\n\033[36m== %s en %s\033[0m\n' "$(basename "$DEB")" "$image"
    if docker run --rm \
        -v "$DEB":/tmp/danplay.deb:ro -v "$CHECK":/tmp/install-check.sh:ro \
        "$image" sh /tmp/install-check.sh; then
        printf '\033[32mBIEN\033[0m   %s\n' "$image"
    else
        printf '\033[31mMAL\033[0m    %s\n' "$image"
        FAILED="$FAILED $image"
    fi
done

[ -z "$FAILED" ] || die "la instalacion limpia falla en:$FAILED"
printf '\n\033[32mLa instalacion limpia pasa en: %s\033[0m\n' "$IMAGES"
