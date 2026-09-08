#!/usr/bin/env bash
# Construye DanPlay para Windows DESDE LINUX, en versión portátil.
#
#   ./scripts/build-windows-cross.sh            construye dist/windows/
#   ./scripts/build-windows-cross.sh --zip      además, el .zip para repartir
#
# Sale una carpeta que se copia a un Windows y se ejecuta: no es un instalador
# (para eso está .github/workflows/windows.yml o scripts/build-windows.ps1),
# pero permite probar sin tener una máquina Windows delante ni esperar a la
# integración continua.
#
# Por qué NO se usa PyInstaller aquí: no compila para otro sistema. En su
# lugar se coge el Python embebido oficial de Windows y se le meten las
# dependencias como ruedas `win_amd64`, que sí se pueden descargar desde
# cualquier sitio. Un lanzador diminuto (packaging/launcher.c) hace de
# `danplay-core.exe`, que es lo que la aplicación busca.
#
# Todo lo que hace falta se descarga a una caché y no toca el sistema: no se
# necesita ser administrador.
set -euo pipefail
cd "$(dirname "$0")/.."
RAIZ=$(pwd)
CACHE="${DANPLAY_CACHE_WIN:-$HOME/.cache/danplay-cross}"
DESTINO="$RAIZ/dist/windows"
TRIPLE=x86_64-pc-windows-gnu
PY_VERSION="${DANPLAY_PY_WIN:-3.13.5}"
PY_TAG=313

paso () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
aviso () { printf '\033[33m%s\033[0m\n' "$1"; }
morir () { printf '\n\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

mkdir -p "$CACHE"

# --------------------------------------------------------------- herramientas
# mingw-w64 (enlazador y compilador de recursos) y zig (compila el lanzador).
# Se extraen de sus paquetes sin instalarlos: el enlazador de zig no sabe usar
# las bibliotecas de importación de Windows que trae Rust, así que hace falta
# el de mingw de verdad.
SYSROOT="$CACHE/mingw"
if [ ! -x "$SYSROOT/usr/bin/x86_64-w64-mingw32-gcc-win32" ]; then
    paso "herramientas de Windows (una vez)"
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    ( cd "$tmp" && apt-get download \
        binutils-mingw-w64-x86-64 mingw-w64-x86-64-dev \
        gcc-mingw-w64-base gcc-mingw-w64-x86-64-win32-runtime \
        gcc-mingw-w64-x86-64-win32 >/dev/null ) \
      || morir "no pude descargar mingw-w64. ¿Hay red y apt configurado?"
    mkdir -p "$SYSROOT"
    for d in "$tmp"/*.deb; do dpkg-deb -x "$d" "$SYSROOT"; done
    ln -sf x86_64-w64-mingw32-gcc-win32 "$SYSROOT/usr/bin/x86_64-w64-mingw32-gcc"
    rm -rf "$tmp"; trap - EXIT
fi
export PATH="$SYSROOT/usr/bin:$PATH"

ZIG=$(ls -d "$CACHE"/zig-*/zig 2>/dev/null | head -1 || true)
if [ -z "$ZIG" ]; then
    ZIG=$(command -v zig || true)
fi
if [ -z "$ZIG" ]; then
    paso "zig (una vez, para el lanzador)"
    version=$(curl -s https://ziglang.org/download/index.json |
      python3 -c "import json,sys;d=json.load(sys.stdin);print(next(k for k in d if k!='master'))")
    url=$(curl -s https://ziglang.org/download/index.json |
      python3 -c "import json,sys;d=json.load(sys.stdin);k=next(k for k in d if k!='master');print(d[k]['x86_64-linux']['tarball'])")
    curl -sL -o "$CACHE/zig.tar.xz" "$url" || morir "no pude descargar zig ($version)"
    tar xf "$CACHE/zig.tar.xz" -C "$CACHE" && rm "$CACHE/zig.tar.xz"
    ZIG=$(ls -d "$CACHE"/zig-*/zig | head -1)
fi

rustup target list --installed | grep -qx "$TRIPLE" \
  || rustup target add "$TRIPLE" \
  || morir "no pude añadir el destino $TRIPLE"

# ----------------------------------------------------------------- interfaz
paso "1/5  interfaz (Vite)"
( cd desktop && npx vite build )

# ------------------------------------------------------------ nucleo Python
paso "2/5  núcleo (Python embebido de Windows + ruedas win_amd64)"
NUCLEO="$CACHE/core-$PY_VERSION"
if [ ! -f "$NUCLEO/python/python.exe" ]; then
    rm -rf "$NUCLEO"; mkdir -p "$NUCLEO"
    curl -sL -o "$NUCLEO/python.zip" \
      "https://www.python.org/ftp/python/$PY_VERSION/python-$PY_VERSION-embed-amd64.zip" \
      || morir "no pude descargar el Python embebido $PY_VERSION"
    unzip -oq "$NUCLEO/python.zip" -d "$NUCLEO/python" && rm "$NUCLEO/python.zip"
    # Sin `import site`, el Python embebido no mira site-packages y no
    # encuentra ninguna dependencia.
    cat > "$NUCLEO/python/python$PY_TAG._pth" <<EOF
python$PY_TAG.zip
.
site-packages

import site
EOF
    mkdir -p "$NUCLEO/wheels" "$NUCLEO/python/site-packages"
    "$RAIZ/.venv/bin/python" -m pip download --quiet \
        --platform win_amd64 --python-version "$PY_TAG" --only-binary=:all: \
        --dest "$NUCLEO/wheels" -r requirements.txt \
      || morir "alguna dependencia no tiene rueda para Windows"
    for w in "$NUCLEO"/wheels/*.whl; do unzip -oq "$w" -d "$NUCLEO/python/site-packages"; done
    rm -rf "$NUCLEO"/python/site-packages/*.dist-info
fi

# el código propio se copia siempre: cambia en cada compilación
rm -rf "$NUCLEO/python/danplay"
cp -r danplay "$NUCLEO/python/danplay"
find "$NUCLEO/python/danplay" -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true

"$ZIG" cc -target x86_64-windows-gnu -O2 -municode \
    -o "$NUCLEO/danplay-core.exe" packaging/launcher.c \
  || morir "no pude compilar el lanzador"

# ----------------------------------------------------------- app de escritorio
paso "3/5  aplicación (Rust, $TRIPLE)"
# El empaquetador comprueba que el sidecar existe antes de compilar.
mkdir -p desktop/src-tauri/binaries
cp "$NUCLEO/danplay-core.exe" "desktop/src-tauri/binaries/danplay-core-$TRIPLE.exe"
( cd desktop/src-tauri && cargo build --release --target "$TRIPLE" )

# ------------------------------------------------------------------- montaje
paso "4/5  montar dist/windows"
rm -rf "$DESTINO"; mkdir -p "$DESTINO"
APP=desktop/src-tauri/target/$TRIPLE/release
cp "$APP/danplay-app.exe" "$DESTINO/"
# WebView2Loader.dll: la aplicación lo carga al arrancar y sin él no abre
cp "$APP/WebView2Loader.dll" "$DESTINO/" 2>/dev/null \
  || morir "falta WebView2Loader.dll junto al ejecutable"
cp "$NUCLEO/danplay-core.exe" "$DESTINO/"
cp -r "$NUCLEO/python" "$DESTINO/python"
mkdir -p "$DESTINO/tools"
cp desktop/src-tauri/tools/LEEME.txt "$DESTINO/tools/" 2>/dev/null || true

cat > "$DESTINO/LEEME.txt" <<'EOF'
DanPlay para Windows (versión portátil)
=======================================

No hay que instalar nada: ejecuta danplay-app.exe.

Qué hay aquí
  danplay-app.exe      la aplicación
  danplay-core.exe     arranca el núcleo que está en python\
  python\              Python con lo que el núcleo necesita
  WebView2Loader.dll   lo carga la aplicación al abrir
  tools\               si pones aquí ffmpeg.exe, ffprobe.exe y fpcalc.exe,
                       se activan la conversión de formatos y la
                       identificación de canciones por su sonido

Hace falta WebView2, que Windows 10 y 11 actualizados ya traen. Si la ventana
sale en blanco, instálalo desde
https://developer.microsoft.com/microsoft-edge/webview2/

Esta versión se construyó desde Linux y NO se ha ejecutado en Windows todavía:
si algo falla, es información útil. El instalador de verdad (con su desinstalador
y su acceso directo) sale de la integración continua del proyecto.
EOF

# ------------------------------------------------------------ comprobaciones
paso "5/5  comprobar lo que ha salido"
fallos=0
comprobar () {
    if [ -e "$DESTINO/$1" ]; then printf '  ok    %s\n' "$1"
    else printf '  FALTA %s\n' "$1"; fallos=$((fallos+1)); fi
}
comprobar danplay-app.exe
comprobar danplay-core.exe
comprobar WebView2Loader.dll
comprobar python/python.exe
comprobar python/danplay/cli.py
comprobar python/site-packages/fastapi
comprobar python/site-packages/mutagen

for exe in danplay-app.exe danplay-core.exe; do
    tipo=$(file -b "$DESTINO/$exe")
    case "$tipo" in
        PE32+*) printf '  ok    %s es un ejecutable de Windows\n' "$exe" ;;
        *) printf '  FALLA %s no es PE32+: %s\n' "$exe" "$tipo"; fallos=$((fallos+1)) ;;
    esac
done

# que el núcleo pueda importarse: si falta una dependencia, aquí se ve
if ! "$RAIZ/.venv/bin/python" - "$DESTINO/python/site-packages" <<'EOF'
import sys, pathlib
faltan = [m for m in ("fastapi", "uvicorn", "mutagen", "rapidfuzz", "yt_dlp",
                      "acoustid", "dotenv", "platformdirs", "send2trash",
                      "openai", "pydantic")
          if not list(pathlib.Path(sys.argv[1]).glob(m + "*"))]
if faltan:
    print("  FALLA faltan dependencias:", ", ".join(faltan)); sys.exit(1)
print("  ok    están todas las dependencias")
EOF
then fallos=$((fallos+1)); fi

[ "$fallos" -eq 0 ] || morir "$fallos comprobación(es) fallida(s)"

if [ "${1:-}" = "--zip" ]; then
    paso "extra  comprimir"
    # con python: `zip` no siempre esta instalado y esto ya lo tenemos
    ( cd dist && rm -f danplay-windows.zip && python3 -c "
import shutil; shutil.make_archive('danplay-windows', 'zip', '.', 'windows')" )
    printf '  %s\n' "dist/danplay-windows.zip ($(du -h dist/danplay-windows.zip | cut -f1))"
fi

printf '\n\033[32mLISTO\033[0m  %s  (%s)\n' "$DESTINO" "$(du -sh "$DESTINO" | cut -f1)"
aviso 'Sin probar en Windows: cópialo a uno y ejecuta danplay-app.exe.'
