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
# La foto del catalogo de modelos de IA se pone al dia en cada compilacion,
# para que ningun paquete salga con una lista de hace meses. Sin internet se
# queda la que hay (el script lo dice y no falla).
"$PY" scripts/actualizar-modelos.py || true
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
    BUNDLE=desktop/src-tauri/target/release/bundle

    # Se vacia ANTES de empaquetar. Tauri no limpia lo suyo: el nombre lleva
    # la version dentro, asi que al subirla el paquete nuevo se pone AL LADO
    # del viejo en vez de sustituirlo, y esta carpeta acaba con una version de
    # cada dia. Como luego se copia todo lo que haya, en dist/installers/
    # aparecian tres .deb con fecha de hoy y el de arriba no era el de hoy.
    # Vaciando antes, lo que quede aqui es exactamente lo que se acaba de
    # construir. (No se pierde nada: es todo salida de compilacion.)
    rm -rf "$BUNDLE/deb" "$BUNDLE/appimage" "$BUNDLE/rpm"

    # Sin `|| true`: si el empaquetado falla hay que enterarse. Antes se lo
    # tragaba y luego se copiaban los paquetes VIEJOS con fecha nueva, que es
    # la peor forma de fallar: parece que funciono y arrancas la app de ayer.
    ( cd desktop && npx tauri build )

    # Y fuera tambien los de la version anterior en el destino, por lo mismo.
    rm -f dist/installers/*.deb dist/installers/*.AppImage

    # La version que se acaba de construir, para comprobar lo que sale.
    VERSION=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' \
              desktop/src-tauri/tauri.conf.json | head -1)

    for f in $(find "$BUNDLE" -type f \( -name '*.deb' -o -name '*.AppImage' \)); do
        # Cinturon: si por lo que sea aparece un paquete que no es de esta
        # version, se para en vez de copiarlo.
        case "$(basename "$f")" in
            *_"$VERSION"_*) ;;
            *) printf '\n\033[31mERROR\033[0m  %s no es de la version %s\n' \
                   "$(basename "$f")" "$VERSION" >&2; exit 1 ;;
        esac
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
