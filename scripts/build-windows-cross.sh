#!/usr/bin/env bash
# Construye DanPlay para Windows DESDE LINUX: portátil e instalador.
#
#   ./scripts/build-windows-cross.sh                 construye dist/windows/
#   ./scripts/build-windows-cross.sh --zip           además, el .zip portátil
#   ./scripts/build-windows-cross.sh --instalador    además, el .exe instalador
#   ./scripts/build-windows-cross.sh --herramientas  mete ffmpeg y fpcalc dentro
#
# Las dos formas salen de LA MISMA carpeta, así que no pueden desincronizarse:
# el instalador es esa carpeta comprimida con NSIS más los accesos directos,
# las asociaciones de archivo y el desinstalador.
#
# `--instalador` implica `--herramientas`: un instalador que se llama «con
# todo» y luego no sabe convertir formatos ni reproducir .opus no es con todo.
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

ZIP=0; INSTALADOR=0; HERRAMIENTAS=0
for arg in "$@"; do
    case "$arg" in
        --zip)          ZIP=1 ;;
        --instalador)   INSTALADOR=1; HERRAMIENTAS=1 ;;
        --herramientas) HERRAMIENTAS=1 ;;
        *) printf 'no conozco la opción %s\n' "$arg" >&2; exit 2 ;;
    esac
done

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
cargo build --release -p danplay-app --target "$TRIPLE"

# ------------------------------------------------------------------- montaje
paso "4/5  montar dist/windows"
rm -rf "$DESTINO"; mkdir -p "$DESTINO"
APP=target/$TRIPLE/release
cp "$APP/danplay-app.exe" "$DESTINO/"
# WebView2Loader.dll: la aplicación lo carga al arrancar y sin él no abre
cp "$APP/WebView2Loader.dll" "$DESTINO/" 2>/dev/null \
  || morir "falta WebView2Loader.dll junto al ejecutable"
cp "$NUCLEO/danplay-core.exe" "$DESTINO/"
cp -r "$NUCLEO/python" "$DESTINO/python"
mkdir -p "$DESTINO/tools"
cp desktop/src-tauri/tools/LEEME.txt "$DESTINO/tools/" 2>/dev/null || true

# ffmpeg, fpcalc y Deno: en Windows no hay `apt install`, así que o van dentro
# o no hay conversión de formatos, ni huella acústica, ni .opus/.wma sonando,
# ni YouTube (yt-dlp necesita un motor de JavaScript).
#
# Con versión fija y su SHA-256, las mismas que el instalador de la CI
# (.github/workflows/windows.yml): antes se bajaba «la última» sin comprobar
# nada. Para subir una, se cambian la URL y la suma aquí y allí.
FFMPEG_URL=https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip
FFMPEG_SHA256=60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba
FPCALC_URL=https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-windows-x86_64.zip
FPCALC_SHA256=735d6182b38e9f364b84ce6f4ccd682c75e2851de89735711d6b762d12b92a4e
DENO_URL=https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-pc-windows-msvc.zip
DENO_SHA256=a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238
bajar () {  # URL SHA256 DESTINO
    curl -sfL -o "$3" "$1" || morir "no pude descargar $1"
    echo "$2  $3" | sha256sum -c --status - || morir "$(basename "$1") no es lo esperado (sha256)"
}
if [ "$HERRAMIENTAS" = 1 ]; then
    TOOLS="$CACHE/tools-win-ffmpeg9.0.2-fpcalc1.6.1-deno2.9.7"
    if [ ! -f "$TOOLS/deno.exe" ]; then
        paso "herramientas de Windows: ffmpeg, fpcalc y Deno (una vez, ~160 MB)"
        rm -rf "$TOOLS"; mkdir -p "$TOOLS"
        tmp=$(mktemp -d)
        bajar "$FFMPEG_URL" "$FFMPEG_SHA256" "$tmp/ffmpeg.zip"
        unzip -oq "$tmp/ffmpeg.zip" -d "$tmp/ffmpeg"
        find "$tmp/ffmpeg" -name 'ffmpeg.exe' -o -name 'ffprobe.exe' \
          | while read -r f; do cp "$f" "$TOOLS/"; done
        bajar "$FPCALC_URL" "$FPCALC_SHA256" "$tmp/fpcalc.zip"
        unzip -oq "$tmp/fpcalc.zip" -d "$tmp/fpcalc"
        find "$tmp/fpcalc" -name 'fpcalc.exe' -exec cp {} "$TOOLS/" \;
        bajar "$DENO_URL" "$DENO_SHA256" "$tmp/deno.zip"
        unzip -oq "$tmp/deno.zip" -d "$tmp/deno"
        find "$tmp/deno" -name 'deno.exe' -exec cp {} "$TOOLS/" \;
        rm -rf "$tmp"
        [ -f "$TOOLS/ffmpeg.exe" ] || morir "el zip de ffmpeg no traía ffmpeg.exe"
        [ -f "$TOOLS/deno.exe" ] || morir "el zip de Deno no traía deno.exe"
    fi
    cp "$TOOLS"/*.exe "$DESTINO/tools/"
    # el LEEME de la carpeta vacia diria que aqui no hay nada
    cat > "$DESTINO/tools/LEEME.txt" <<'EOF'
ffmpeg, ffprobe, fpcalc y Deno, incluidos.

En Windows no hay gestor de paquetes que los ponga, asi que viajan aqui. Son
los que hacen que funcionen la conversion de formatos, la reproduccion de
.opus y .wma, el encogido de portadas, la identificacion de canciones por su
sonido y las descargas de YouTube (Deno es el motor de JavaScript que pide
yt-dlp). La aplicacion los busca en esta carpeta antes que en el PATH.

No los borres. Si faltan, DanPlay sigue abriendo y avisa de lo que no puede
hacer.
EOF
fi

cat > "$DESTINO/LEEME.txt" <<'EOF'
DanPlay para Windows (versión portátil)
=======================================

No hay que instalar nada: ejecuta danplay-app.exe.

Qué hay aquí
  danplay-app.exe      la aplicación
  danplay-core.exe     arranca el núcleo que está en python\
  python\              Python con lo que el núcleo necesita
  WebView2Loader.dll   lo carga la aplicación al abrir
  tools\               ffmpeg.exe, ffprobe.exe, fpcalc.exe y deno.exe:
                       conversión de formatos, reproducción de .opus y .wma,
                       identificación de canciones por su sonido y descargas
                       de YouTube. Si la carpeta está vacía, eso no funciona
                       y la aplicación lo dice

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
if [ "$HERRAMIENTAS" = 1 ]; then
    comprobar tools/ffmpeg.exe
    comprobar tools/ffprobe.exe
    comprobar tools/fpcalc.exe
    comprobar tools/deno.exe
fi

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

if [ "$ZIP" = 1 ]; then
    paso "extra  comprimir el portátil"
    # con python: `zip` no siempre esta instalado y esto ya lo tenemos
    ( cd dist && rm -f danplay-windows.zip && python3 -c "
import shutil; shutil.make_archive('danplay-windows', 'zip', '.', 'windows')" )
    printf '  %s\n' "dist/danplay-windows.zip ($(du -h dist/danplay-windows.zip | cut -f1))"
fi

# ---------------------------------------------------------------- instalador
if [ "$INSTALADOR" = 1 ]; then
    paso "extra  instalador (NSIS)"
    # NSIS también se saca de sus paquetes sin instalarlo, igual que mingw.
    NSIS="$CACHE/nsis"
    if [ ! -x "$NSIS/usr/bin/makensis" ]; then
        tmp=$(mktemp -d)
        ( cd "$tmp" && apt-get download nsis nsis-common >/dev/null ) \
          || morir "no pude descargar NSIS. ¿Hay red y apt configurado?"
        mkdir -p "$NSIS"
        for d in "$tmp"/*.deb; do dpkg-deb -x "$d" "$NSIS"; done
        rm -rf "$tmp"
    fi

    VERSION=$(sed -n 's/^__version__ = "\([^"]*\)"/\1/p' danplay/__init__.py)
    mkdir -p dist/installers
    SALIDA="$RAIZ/dist/installers/DanPlay-$VERSION-instalador.exe"
    rm -f "$SALIDA"

    NSISDIR="$NSIS/usr/share/nsis" "$NSIS/usr/bin/makensis" -V2 -NOCD \
        -DVERSION="$VERSION" \
        -DORIGEN="$DESTINO" \
        -DSALIDA="$SALIDA" \
        -DICONO="$RAIZ/desktop/src-tauri/icons/icon.ico" \
        packaging/windows/installer.nsi \
      || morir "makensis no pudo construir el instalador"

    [ -f "$SALIDA" ] || morir "makensis dijo que si pero no dejo el .exe"
    # Que dentro este TODO lo de la carpeta: si NSIS se salta archivos, la
    # aplicacion instalada arranca y falla luego, que es peor que no arrancar.
    dentro=$(7z l "$SALIDA" 2>/dev/null | grep -c '^20[0-9][0-9]-' || echo 0)
    fuera=$(find "$DESTINO" -type f | wc -l)
    printf '  %s  (%s)\n' "$(basename "$SALIDA")" "$(du -h "$SALIDA" | cut -f1)"
    if [ "$dentro" -gt 0 ] && [ "$dentro" -lt "$fuera" ]; then
        aviso "  ojo: $dentro archivos dentro y $fuera en la carpeta"
    fi
fi

printf '\n\033[32mLISTO\033[0m\n'
printf '  portátil    : %s  (%s)\n' "$DESTINO" "$(du -sh "$DESTINO" | cut -f1)"
[ "$ZIP" = 1 ] && printf '  comprimido  : %s\n' "dist/danplay-windows.zip"
[ "$INSTALADOR" = 1 ] && printf '  instalador  : %s\n' "dist/installers/DanPlay-$VERSION-instalador.exe"
aviso 'Sin probar en Windows: cópialo a uno y ejecútalo.'
