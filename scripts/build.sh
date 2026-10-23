#!/bin/sh
# Construye DanPlay entero, en orden. Son tres capas y el orden importa: si
# te saltas una, la app arranca con la version vieja y no se nota.
#
#   ./scripts/build.sh            interfaz + datos + app
#   ./scripts/build.sh --package  ademas, genera el .deb y el .AppImage
#
# Sin Python ni PyInstaller: el nucleo va dentro de la app (el crate
# `server`). Lo que viaja con ella son sus datos (desktop/src-tauri/data:
# los modelos, las voces, la foto del catalogo y ONNX Runtime) y, en los
# paquetes, sus programas (desktop/src-tauri/tools: ffmpeg, fpcalc, QuickJS y
# yt-dlp).
#
# Los paquetes acaban en dist/installers/. Respeta CARGO_TARGET_DIR.
set -eu
cd "$(dirname "$0")/.."
TARGET="${CARGO_TARGET_DIR:-target}"
APP="$TARGET/release/danplay-app"

step () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
die () { printf '\n\033[31mERROR\033[0m  %s\n' "$1" >&2; exit 1; }

mkdir -p dist/installers

step "1/3  interfaz (Vite)"
( cd desktop && npx vite build )

step "2/3  datos que viajan con la app (ONNX Runtime y danplay/data)"
./scripts/data.sh
# el nucleo de Python empaquetado de hasta la 1.30 (core/) y el de un solo
# archivo de antes (binaries/), si quedaron: ya no viajan
rm -rf desktop/src-tauri/core desktop/src-tauri/binaries

step "3/3  app de escritorio (Tauri), con el nucleo dentro"
if [ "${1:-}" = "--package" ]; then
    # ffmpeg, fpcalc, QuickJS y yt-dlp dentro de los paquetes
    # (desktop/src-tauri/tools)
    ./scripts/tools-linux.sh
    # La compila `tauri build` (con su entorno: compilarla tambien con cargo
    # era hacerlo dos veces), pero sin empaquetar todavia: antes se saca con
    # ella la foto del catalogo (abajo).
    ( cd desktop && npx tauri build --no-bundle )
else
    # workspace de Cargo: sale en target/ de la raiz
    cargo build --release -p danplay-app
fi

if [ "${1:-}" = "--package" ]; then
    step "extra  la foto del catalogo de modelos, la de hoy"
    # Para que ningun paquete salga con una lista de hace meses, la saca la
    # propia app recien compilada (lo que hacia scripts/update-models.py), y
    # en data/, que es lo que se empaqueta: la del repositorio no se toca.
    # Sin internet viaja la del repositorio.
    "$APP" --models-snapshot desktop/src-tauri/data/models-snapshot.json \
      || echo "  sin red: viaja la del repositorio"

    step "extra  paquetes .deb y .AppImage"
    BUNDLE="$TARGET/release/bundle"

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
    ( cd desktop && npx tauri bundle --bundles deb,appimage )

    # Y fuera tambien los de la version anterior en el destino, por lo mismo.
    rm -f dist/installers/*.deb dist/installers/*.AppImage

    # La version que se acaba de construir, para comprobar lo que sale
    # (la misma en todas partes: la prueba de versiones lo vigila).
    VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)

    for f in $(find "$BUNDLE" -type f \( -name '*.deb' -o -name '*.AppImage' \)); do
        # Cinturon: si por lo que sea aparece un paquete que no es de esta
        # version, se para en vez de copiarlo.
        case "$(basename "$f")" in
            *_"$VERSION"_*) ;;
            *) die "$(basename "$f") no es de la version $VERSION" ;;
        esac
        # Se desenlaza antes de copiar. Si tienes el AppImage abierto, el
        # kernel no deja SOBRESCRIBIRLO ("Text file busy") y el build moria
        # en el ultimo paso dejandote el paquete de antes; desenlazarlo si
        # deja, y el que ya esta corriendo sigue con su copia tan tranquilo.
        rm -f "dist/installers/$(basename "$f")"
        cp "$f" dist/installers/
        printf '  %s  (%s)\n' "$(basename "$f")" "$(du -h "$f" | cut -f1)"
    done

    # Lo que tiene que llevar el .deb, y lo que no: ni rastro de Python.
    DEB=$(ls dist/installers/*_"$VERSION"_*.deb)
    CONTENTS=$(dpkg-deb -c "$DEB")
    for wanted in usr/bin/danplay-app usr/lib/DanPlay/data/libonnxruntime.so \
                  usr/lib/DanPlay/data/pulse/beat_this.onnx usr/lib/DanPlay/data/models-snapshot.json \
                  usr/lib/DanPlay/tools/ffmpeg usr/lib/DanPlay/tools/yt-dlp; do
        printf '%s\n' "$CONTENTS" | grep -q "$wanted" || die "el .deb no lleva $wanted"
    done
    if printf '%s\n' "$CONTENTS" | grep -iqE 'python|\.pyc|_internal|libpython'; then
        die "el .deb lleva algo de Python: $(printf '%s\n' "$CONTENTS" | grep -iE 'python|\.pyc|_internal|libpython' | head -3)"
    fi
    echo "  el .deb lleva el nucleo, sus datos y sus programas, y nada de Python"
fi

printf '\n\033[32mLISTO\033[0m\n'
printf '  app     : %s\n' "$APP"
[ "${1:-}" = "--package" ] && printf '  paquetes: %s\n' "dist/installers/"
printf '\nArrancala con  ./danplay-app.sh\n'
