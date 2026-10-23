#!/usr/bin/env bash
# Construye DanPlay para Windows DESDE LINUX: portátil e instalador.
#
#   ./scripts/build-windows-cross.sh                 construye dist/windows/
#   ./scripts/build-windows-cross.sh --zip           además, el .zip portátil
#   ./scripts/build-windows-cross.sh --installer     además, el .exe instalador
#   ./scripts/build-windows-cross.sh --with-tools    mete ffmpeg, fpcalc, QuickJS y yt-dlp dentro
#
# El .zip y el .exe quedan en dist/installers/, con la versión en el nombre,
# junto al .deb, el .AppImage y los .dmg de Mac: todo lo que se entrega, en
# una sola carpeta.
#
# Las dos formas salen de LA MISMA carpeta, así que no pueden desincronizarse:
# el instalador es esa carpeta comprimida con NSIS más los accesos directos,
# las asociaciones de archivo y el desinstalador.
#
# `--installer` implica `--with-tools`: un instalador que se llama «con
# todo» y luego no sabe convertir formatos ni reproducir .opus no es con todo.
#
# Sin Python: el núcleo va dentro de la app (el crate `server`), que se
# compila para Windows con mingw-w64. Lo que viaja con ella son sus datos
# (data/: los modelos, las voces, la foto del catálogo del repositorio y ONNX
# Runtime, de scripts/data.sh) y sus programas (tools/). El instalador de
# verdad (MSVC y el NSIS de Tauri) sale de la integración continua
# (windows.yml).
#
# Todo lo que hace falta se descarga a una caché (~/.cache/danplay-windows) y
# no toca el sistema: no se necesita ser administrador.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
CACHE="${DANPLAY_CACHE_WIN:-$HOME/.cache/danplay-windows}"
DEST="$ROOT/dist/windows"
TRIPLE=x86_64-pc-windows-gnu
TARGET="${CARGO_TARGET_DIR:-target}"

ZIP=0; INSTALLER=0; WITH_TOOLS=0
for arg in "$@"; do
    case "$arg" in
        --zip)          ZIP=1 ;;
        --installer)    INSTALLER=1; WITH_TOOLS=1 ;;
        --with-tools)   WITH_TOOLS=1 ;;
        *) printf 'no conozco la opción %s\n' "$arg" >&2; exit 2 ;;
    esac
done

step () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
warn () { printf '\033[33m%s\033[0m\n' "$1"; }
die () { printf '\n\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

mkdir -p "$CACHE"

# --------------------------------------------------------------- herramientas
# mingw-w64 (enlazador, compiladores de C y C++ —el motor de audio lleva el
# estirado de Signalsmith, en C++— y compilador de recursos). Se extraen de
# sus paquetes sin instalarlos.
SYSROOT="$CACHE/mingw"
if [ ! -x "$SYSROOT/usr/bin/x86_64-w64-mingw32-g++-win32" ]; then
    step "herramientas de Windows (una vez)"
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
    ( cd "$tmp" && apt-get download \
        binutils-mingw-w64-x86-64 mingw-w64-x86-64-dev mingw-w64-common \
        gcc-mingw-w64-base gcc-mingw-w64-x86-64-win32-runtime \
        gcc-mingw-w64-x86-64-win32 g++-mingw-w64-x86-64-win32 >/dev/null ) \
      || die "no pude descargar mingw-w64. ¿Hay red y apt configurado?"
    mkdir -p "$SYSROOT"
    for d in "$tmp"/*.deb; do dpkg-deb -x "$d" "$SYSROOT"; done
    ln -sf x86_64-w64-mingw32-gcc-win32 "$SYSROOT/usr/bin/x86_64-w64-mingw32-gcc"
    ln -sf x86_64-w64-mingw32-g++-win32 "$SYSROOT/usr/bin/x86_64-w64-mingw32-g++"
    rm -rf "$tmp"; trap - EXIT
fi
export PATH="$SYSROOT/usr/bin:$PATH"

rustup target list --installed | grep -qx "$TRIPLE" \
  || rustup target add "$TRIPLE" \
  || die "no pude añadir el destino $TRIPLE"

# ----------------------------------------------------------------- interfaz
step "1/4  interfaz (Vite)"
( cd desktop && npx vite build )

# ---------------------------------------------------------------- los datos
step "2/4  datos: ONNX Runtime de Windows y lo de danplay/data"
DANPLAY_CACHE_WINDOWS="$CACHE" ./scripts/data.sh windows

# ----------------------------------------------------------- app de escritorio
step "3/4  aplicación (Rust, $TRIPLE), con el núcleo dentro"
cargo build --release -p danplay-app --target "$TRIPLE"

# ------------------------------------------------------------------- montaje
step "4/4  montar dist/windows"
rm -rf "$DEST"; mkdir -p "$DEST"
APP=$TARGET/$TRIPLE/release
cp "$APP/danplay-app.exe" "$DEST/"
# WebView2Loader.dll: la aplicación lo carga al arrancar y sin él no abre
cp "$APP/WebView2Loader.dll" "$DEST/" 2>/dev/null \
  || die "falta WebView2Loader.dll junto al ejecutable"
cp -r desktop/src-tauri/data "$DEST/data"
mkdir -p "$DEST/tools"
cp desktop/src-tauri/tools/LEEME.txt "$DEST/tools/" 2>/dev/null || true

# ffmpeg, fpcalc, QuickJS y yt-dlp: en Windows no hay `apt install`, así que
# o van dentro o no hay conversión de formatos, ni huella acústica, ni
# .opus/.wma sonando, ni YouTube.
#
# Con versión fija y su SHA-256, las mismas que el instalador de la CI
# (.github/workflows/windows.yml): antes se bajaba «la última» sin comprobar
# nada. Para subir una, se cambian la URL y la suma aquí y allí.
FFMPEG_URL=https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip
FFMPEG_SHA256=60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba
FPCALC_URL=https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-windows-x86_64.zip
FPCALC_SHA256=735d6182b38e9f364b84ce6f4ccd682c75e2851de89735711d6b762d12b92a4e
QJS_URL=https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0/qjs-windows-x86_64.exe
QJS_SHA256=2aeabf0092c3262d6b2609824418f7dd7ed1f1df939f73b2b15645230cac0d77
YTDLP_URL=https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe
YTDLP_SHA256=66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a
download () {  # URL SHA256 DEST
    curl -sfL -o "$3" "$1" || die "no pude descargar $1"
    echo "$2  $3" | sha256sum -c --status - || die "$(basename "$1") no es lo esperado (sha256)"
}
if [ "$WITH_TOOLS" = 1 ]; then
    TOOLS="$CACHE/tools-win-ffmpeg9.0.2-fpcalc1.6.1-qjs0.17.0-ytdlp2026.08.19"
    if [ ! -f "$TOOLS/yt-dlp.exe" ]; then
        step "herramientas de Windows: ffmpeg, fpcalc, QuickJS y yt-dlp (una vez, ~150 MB)"
        rm -rf "$TOOLS"; mkdir -p "$TOOLS"
        tmp=$(mktemp -d -p "$CACHE")
        download "$FFMPEG_URL" "$FFMPEG_SHA256" "$tmp/ffmpeg.zip"
        unzip -oq "$tmp/ffmpeg.zip" -d "$tmp/ffmpeg"
        find "$tmp/ffmpeg" -name 'ffmpeg.exe' -exec cp {} "$TOOLS/" \;
        download "$FPCALC_URL" "$FPCALC_SHA256" "$tmp/fpcalc.zip"
        unzip -oq "$tmp/fpcalc.zip" -d "$tmp/fpcalc"
        find "$tmp/fpcalc" -name 'fpcalc.exe' -exec cp {} "$TOOLS/" \;
        rm -rf "$tmp"
        [ -f "$TOOLS/ffmpeg.exe" ] || die "el zip de ffmpeg no traía ffmpeg.exe"
        download "$QJS_URL" "$QJS_SHA256" "$TOOLS/qjs.exe"
        # el último: es el que dice que la caché está completa
        download "$YTDLP_URL" "$YTDLP_SHA256" "$TOOLS/yt-dlp.exe"
    fi
    cp "$TOOLS/ffmpeg.exe" "$TOOLS/fpcalc.exe" "$TOOLS/qjs.exe" "$TOOLS/yt-dlp.exe" "$DEST/tools/"
    # el LEEME de la carpeta vacia diria que aqui no hay nada
    cat > "$DEST/tools/LEEME.txt" <<'EOF'
ffmpeg, fpcalc, QuickJS y yt-dlp, incluidos.

En Windows no hay gestor de paquetes que los ponga, asi que viajan aqui. Son
los que hacen que funcionen la conversion de formatos, la reproduccion de
.opus y .wma, el encogido de portadas, la identificacion de canciones por su
sonido y las descargas de YouTube (yt-dlp.exe, con QuickJS, qjs.exe, el motor
de JavaScript que pide). La aplicacion los busca en esta carpeta antes que en
el PATH.

No los borres. Si faltan, DanPlay sigue abriendo y avisa de lo que no puede
hacer.
EOF
fi

cat > "$DEST/LEEME.txt" <<'EOF'
DanPlay para Windows (versión portátil)
=======================================

No hay que instalar nada: ejecuta danplay-app.exe.

Qué hay aquí
  danplay-app.exe      la aplicación, con el núcleo dentro
  WebView2Loader.dll   lo carga la aplicación al abrir
  data\                los modelos (el pulso, el separador), las voces de
                       las guías y ONNX Runtime, que los mueve
  tools\               ffmpeg.exe, fpcalc.exe, qjs.exe y yt-dlp.exe:
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

DanPlay es obra de Daniel Rangel (https://t.me/Dani17r).
EOF

# ------------------------------------------------------------ comprobaciones
step "comprobar lo que ha salido"
failures=0
check () {
    if [ -e "$DEST/$1" ]; then printf '  ok    %s\n' "$1"
    else printf '  FALTA %s\n' "$1"; failures=$((failures+1)); fi
}
check danplay-app.exe
check WebView2Loader.dll
check data/onnxruntime.dll
check data/pulse/beat_this.onnx
check data/voices.json
check data/models-snapshot.json
if [ "$WITH_TOOLS" = 1 ]; then
    check tools/ffmpeg.exe
    check tools/fpcalc.exe
    check tools/qjs.exe
    check tools/yt-dlp.exe
fi

for exe in danplay-app.exe data/onnxruntime.dll; do
    kind=$(file -b "$DEST/$exe")
    case "$kind" in
        PE32+*) printf '  ok    %s es de Windows (64 bits)\n' "$exe" ;;
        *) printf '  FALLA %s no es PE32+: %s\n' "$exe" "$kind"; failures=$((failures+1)) ;;
    esac
done

# ni rastro del núcleo de Python de antes
if find "$DEST" | grep -iE 'python|\.pyc|_internal|danplay-core'; then
    printf '  FALLA lleva algo del núcleo de Python\n'; failures=$((failures+1))
else
    printf '  ok    sin nada de Python\n'
fi

[ "$failures" -eq 0 ] || die "$failures comprobación(es) fallida(s)"

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
PORTABLE="dist/installers/DanPlay-$VERSION-windows-portatil.zip"

if [ "$ZIP" = 1 ]; then
    step "extra  comprimir el portátil"
    mkdir -p dist/installers
    # los de versiones anteriores fuera, como hace build.sh con el .deb
    rm -f dist/installers/DanPlay-*-windows-portatil.zip
    if command -v zip >/dev/null 2>&1; then
        ( cd dist && zip -qr "../$PORTABLE" windows )
    elif command -v 7z >/dev/null 2>&1; then
        ( cd dist && 7z a -tzip -bd "../$PORTABLE" windows >/dev/null )
    else
        die "hace falta zip o 7z para comprimir el portátil"
    fi
    printf '  %s\n' "$PORTABLE ($(du -h "$PORTABLE" | cut -f1))"
fi

# ---------------------------------------------------------------- instalador
if [ "$INSTALLER" = 1 ]; then
    step "extra  instalador (NSIS)"
    # NSIS también se saca de sus paquetes sin instalarlo, igual que mingw.
    NSIS="$CACHE/nsis"
    if [ ! -x "$NSIS/usr/bin/makensis" ]; then
        tmp=$(mktemp -d)
        ( cd "$tmp" && apt-get download nsis nsis-common >/dev/null ) \
          || die "no pude descargar NSIS. ¿Hay red y apt configurado?"
        mkdir -p "$NSIS"
        for d in "$tmp"/*.deb; do dpkg-deb -x "$d" "$NSIS"; done
        rm -rf "$tmp"
    fi

    mkdir -p dist/installers
    OUTPUT="$ROOT/dist/installers/DanPlay-$VERSION-instalador.exe"
    rm -f dist/installers/DanPlay-*-instalador.exe

    NSISDIR="$NSIS/usr/share/nsis" "$NSIS/usr/bin/makensis" -V2 -NOCD \
        -DVERSION="$VERSION" \
        -DSOURCE_DIR="$DEST" \
        -DOUT_FILE="$OUTPUT" \
        -DAPP_ICON="$ROOT/desktop/src-tauri/icons/icon.ico" \
        packaging/windows/installer.nsi \
      || die "makensis no pudo construir el instalador"

    [ -f "$OUTPUT" ] || die "makensis dijo que si pero no dejo el .exe"
    # Que dentro este TODO lo de la carpeta: si NSIS se salta archivos, la
    # aplicacion instalada arranca y falla luego, que es peor que no arrancar.
    packed=$(7z l "$OUTPUT" 2>/dev/null | grep -c '^20[0-9][0-9]-' || echo 0)
    on_disk=$(find "$DEST" -type f | wc -l)
    printf '  %s  (%s)\n' "$(basename "$OUTPUT")" "$(du -h "$OUTPUT" | cut -f1)"
    if [ "$packed" -gt 0 ] && [ "$packed" -lt "$on_disk" ]; then
        warn "  ojo: $packed archivos dentro y $on_disk en la carpeta"
    fi
fi

printf '\n\033[32mLISTO\033[0m\n'
printf '  portátil    : %s  (%s)\n' "$DEST" "$(du -sh "$DEST" | cut -f1)"
[ "$ZIP" = 1 ] && printf '  comprimido  : %s\n' "$PORTABLE"
[ "$INSTALLER" = 1 ] && printf '  instalador  : %s\n' "dist/installers/DanPlay-$VERSION-instalador.exe"
warn 'Sin probar en Windows: cópialo a uno y ejecútalo.'
