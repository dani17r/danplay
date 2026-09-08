#!/bin/sh
# Construye DanPlay entero, en orden. Son cuatro capas y el orden importa:
# si te saltas una, la app arranca con la version vieja y no se nota.
#
#   ./scripts/build.sh            interfaz + nucleo + app
#   ./scripts/build.sh --package  ademas, genera el .deb y el .AppImage
#
# Todo lo que se genera acaba en dist/. Los directorios de trabajo intermedios
# se borran al terminar: no queda un build/ suelto en la raiz.
set -eu
cd "$(dirname "$0")/.."
RAIZ=$(pwd)
PY="$RAIZ/.venv/bin/python"
TRABAJO=$(mktemp -d)
trap 'rm -rf "$TRABAJO"' EXIT

paso () { printf '\n\033[36m== %s\033[0m\n' "$1"; }

mkdir -p dist/core dist/installers

paso "1/4  interfaz (Vite)"
( cd desktop && npx vite build )

paso "2/4  nucleo empaquetado (PyInstaller)"
# --workpath a un temporal: asi no se crea build/ en la raiz del proyecto
"$PY" -m PyInstaller --noconfirm --clean \
    --workpath "$TRABAJO/work" --distpath dist/core \
    packaging/core.spec

paso "3/4  copiar el nucleo junto a la app"
# La carpeta no esta en git (pesa 40 MB y se genera aqui mismo), asi que en un
# clon recien hecho hay que crearla antes de copiar.
#
# Tauri quiere el sidecar con el triple del destino en el nombre. Se pregunta
# a rustc en vez de escribirlo a mano: asi vale para cualquier maquina.
TRIPLE=$(rustc -vV | awk '/^host:/{print $2}')
mkdir -p desktop/src-tauri/binaries
cp dist/core/danplay-core "desktop/src-tauri/binaries/danplay-core-$TRIPLE"

paso "4/4  app de escritorio (Tauri)"
( cd desktop/src-tauri && cargo build --release )

if [ "${1:-}" = "--package" ]; then
    paso "extra  paquetes .deb y .AppImage"
    # Sin `|| true`: si el empaquetado falla hay que enterarse. Antes se lo
    # tragaba y luego se copiaban los paquetes VIEJOS con fecha nueva, que es
    # la peor forma de fallar: parece que funciono y arrancas la app de ayer.
    ( cd desktop && npx tauri build )

    BUNDLE=desktop/src-tauri/target/release/bundle
    # Fuera los paquetes de la version anterior. El nombre lleva el numero
    # dentro, asi que al subirla los viejos NO se sobreescriben: se quedaban
    # al lado, y abrir el que no toca te devuelve la app de antes sin que
    # nada lo advierta. Es el error que mas tiempo cuesta encontrar.
    rm -f dist/installers/*.deb dist/installers/*.AppImage

    for f in $(find "$BUNDLE" -type f \( -name '*.deb' -o -name '*.AppImage' \)); do
        # Se desenlaza antes de copiar. Si tienes el AppImage abierto, el
        # kernel no deja SOBRESCRIBIRLO ("Text file busy") y el build moria
        # en el ultimo paso dejandote el paquete de antes; desenlazarlo si
        # deja, y el que ya esta corriendo sigue con su copia tan tranquilo.
        rm -f "dist/installers/$(basename "$f")"
        cp "$f" dist/installers/
        printf '  %s\n' "$(basename "$f")"
    done
fi

printf '\n\033[32mLISTO\033[0m\n'
printf '  app     : %s\n' "desktop/src-tauri/target/release/danplay-app"
printf '  nucleo  : %s\n' "dist/core/danplay-core"
[ "${1:-}" = "--package" ] && printf '  paquetes: %s\n' "dist/installers/"
printf '\nArrancala con  ./danplay-app.sh\n'
