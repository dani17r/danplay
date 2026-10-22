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
ROOT=$(pwd)
PY="$ROOT/.venv/bin/python"
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

step () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
die () { printf '\n\033[31mERROR\033[0m  %s\n' "$1" >&2; exit 1; }

mkdir -p dist/core dist/installers

step "1/4  interfaz (Vite)"
( cd desktop && npx vite build )

step "2/4  nucleo empaquetado (PyInstaller)"
# El crate de Rust del nucleo (las etiquetas, con lofty), recompilado con lo
# de ahora: el nucleo empaquetado lo lleva dentro, y sin el no hay etiquetas.
uv sync --locked --reinstall-package danplay-core --quiet
# La foto del catalogo de modelos de IA se saca al dia en cada compilacion,
# para que ningun paquete salga con una lista de hace meses, pero en el
# temporal: la del repositorio no se toca (antes se reescribia y cada
# compilacion dejaba el arbol sucio). Sin internet viaja la del repositorio.
"$PY" scripts/update-models.py "$WORK_DIR/models-snapshot.json" || true
# --workpath a un temporal: asi no se crea build/ en la raiz del proyecto
DANPLAY_SNAPSHOT="$WORK_DIR/models-snapshot.json" "$PY" -m PyInstaller --noconfirm --clean \
    --workpath "$WORK_DIR/work" --distpath dist/core \
    packaging/core.spec
# lo que tiene que llevar dentro, y lo que no (mutagen es GPL: ver core.spec)
[ -d dist/core/danplay-core/_internal/danplay_core ] \
  || die "el nucleo empaquetado no lleva el crate de Rust (danplay_core)"
[ ! -e dist/core/danplay-core/_internal/mutagen ] \
  || die "se colo mutagen en el nucleo empaquetado (ver packaging/core.spec)"
# la biblioteca de ONNX Runtime que carga el pulso desde Rust
ls dist/core/danplay-core/_internal/onnxruntime/capi/ | grep -qE '^(libonnxruntime\.|onnxruntime\.dll)' \
  || die "el nucleo empaquetado no lleva libonnxruntime (ver packaging/core.spec)"

step "3/4  copiar el nucleo junto a la app"
# Una carpeta entera (onedir, ver packaging/core.spec) entre los recursos de
# Tauri: desktop/src-tauri/core/, que en git solo tiene su LEEME. Se vacia
# antes, para que no quede nada de una compilacion anterior.
find desktop/src-tauri/core -mindepth 1 -maxdepth 1 ! -name LEEME.txt -exec rm -rf {} +
cp -a dist/core/danplay-core/. desktop/src-tauri/core/
# el nucleo de un solo archivo de antes (externalBin), si quedo: ya no se usa
rm -rf desktop/src-tauri/binaries

step "4/4  app de escritorio (Tauri)"
if [ "${1:-}" = "--package" ]; then
    # la compila `tauri build` al empaquetar, mas abajo: compilarla tambien
    # aqui era hacerlo dos veces (Tauri la recompila con su entorno)
    echo "  la compila el empaquetado"
else
    # workspace de Cargo: sale en target/ de la raiz
    cargo build --release -p danplay-app
fi

if [ "${1:-}" = "--package" ]; then
    step "extra  paquetes .deb y .AppImage"
    BUNDLE=target/release/bundle

    # Se vacia ANTES de empaquetar. Tauri no limpia lo suyo: el nombre lleva
    # la version dentro, asi que al subirla el paquete nuevo se pone AL LADO
    # del viejo en vez de sustituirlo, y esta carpeta acaba con una version de
    # cada dia. Como luego se copia todo lo que haya, en dist/installers/
    # aparecian tres .deb con fecha de hoy y el de arriba no era el de hoy.
    # Vaciando antes, lo que quede aqui es exactamente lo que se acaba de
    # construir. (No se pierde nada: es todo salida de compilacion.)
    rm -rf "$BUNDLE/deb" "$BUNDLE/appimage" "$BUNDLE/rpm"

    # ffmpeg, fpcalc y QuickJS dentro de los paquetes (desktop/src-tauri/tools)
    ./scripts/tools-linux.sh

    # Sin `|| true`: si el empaquetado falla hay que enterarse. Antes se lo
    # tragaba y luego se copiaban los paquetes VIEJOS con fecha nueva, que es
    # la peor forma de fallar: parece que funciono y arrancas la app de ayer.
    ( cd desktop && npx tauri build )

    # Y fuera tambien los de la version anterior en el destino, por lo mismo.
    rm -f dist/installers/*.deb dist/installers/*.AppImage

    # La version que se acaba de construir, para comprobar lo que sale
    # (la misma en todas partes: la prueba de versiones lo vigila).
    VERSION=$(sed -n 's/^__version__ = "\([^"]*\)"/\1/p' danplay/__init__.py)

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
printf '  app     : %s\n' "target/release/danplay-app"
printf '  nucleo  : %s\n' "dist/core/danplay-core/"
[ "${1:-}" = "--package" ] && printf '  paquetes: %s\n' "dist/installers/"
printf '\nArrancala con  ./danplay-app.sh\n'
