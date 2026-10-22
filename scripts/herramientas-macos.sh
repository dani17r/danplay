#!/bin/sh
# Las herramientas que viajan DENTRO del paquete de macOS (la .app), en
# desktop/src-tauri/tools/: ffmpeg (ondas, clic, separar, convertir), fpcalc
# (identificar por el sonido) y Deno (el motor de JavaScript que YouTube exige
# para descargar), para el procesador de este Mac: Apple Silicon o Intel.
#
#   ./scripts/herramientas-macos.sh                 lo que viaja en la .app
#   ./scripts/herramientas-macos.sh --con-ffprobe   y ffprobe, que la app ya
#                                                   no usa pero las pruebas sí,
#                                                   para mirar lo que sale
#
# Lo llama la CI (.github/workflows/macos.yml), y vale igual en un Mac. En
# macOS no hay gestor de paquetes que ponga ffmpeg, y una app abierta desde el
# Finder no ve el PATH de Homebrew: por eso van dentro. Como en
# scripts/herramientas-linux.sh, las mismas versiones que en Linux y Windows,
# fijadas con su sha256, y en una caché: la segunda vez no se baja nada.
set -eu
cd "$(dirname "$0")/.."

DESTINO=desktop/src-tauri/tools
CACHE="${DANPLAY_CACHE_MACOS:-$HOME/.cache/danplay-macos}"
CON_FFPROBE=0
[ "${1:-}" = --con-ffprobe ] && CON_FFPROBE=1

morir () { printf '\033[31m%s\033[0m\n' "$1" >&2; exit 1; }
[ "$(uname -s)" = Darwin ] || morir "las herramientas de macOS son para un Mac (esto es $(uname -s))"

# ffmpeg 9.0.2 de martin-riedl.de (el mismo que el de Linux) y Deno 2.9.7
ARCH=$(uname -m)
case "$ARCH" in
    arm64)
        FFMPEG_URL=https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2
        FFMPEG_SHA256=c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924
        FFPROBE_SHA256=fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6
        DENO=deno-aarch64-apple-darwin
        DENO_SHA256=5cd46d6268f6f78f5d88bdc7159d20bd44cdaa4b3303474839f87ec6fe7ae25c
        ;;
    x86_64)
        FFMPEG_URL=https://ffmpeg.martin-riedl.de/download/macos/amd64/1789931006_9.0.2
        FFMPEG_SHA256=7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4
        FFPROBE_SHA256=2322438ed2f6319a691291b247d09c69dcaa3a982460d1f269a7e1af335cfdfd
        DENO=deno-x86_64-apple-darwin
        DENO_SHA256=95daaff11c116a52ad54785e7914c8e9c9cdcaba793c5ed929c74ca2d8e6259a
        ;;
    *) morir "no hay herramientas de macOS para $ARCH" ;;
esac
DENO_URL=https://github.com/denoland/deno/releases/download/v2.9.7/$DENO.zip
# fpcalc 1.6.1: un solo binario para los dos procesadores
FPCALC_URL=https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-macos-universal.tar.gz
FPCALC_SHA256=240aeb5a8c8205af458e3625cb7487b826b711a999e491ef00111f3cebd76f00

mkdir -p "$CACHE" "$DESTINO"

# Baja `url` a la caché como `nombre`, si no está ya, y comprueba su sha256.
bajar () {
    url=$1 sha=$2 nombre=$3
    archivo="$CACHE/$nombre"
    if [ -f "$archivo" ] && echo "$sha  $archivo" | shasum -a 256 -c --status -; then
        return 0
    fi
    printf '  bajando %s\n' "$nombre"
    curl -sfL --retry 3 -o "$archivo.part" "$url" || morir "no pude bajar $url"
    echo "$sha  $archivo.part" | shasum -a 256 -c --status - \
      || { rm -f "$archivo.part"; morir "$nombre no es el que se esperaba (sha256)"; }
    mv "$archivo.part" "$archivo"
}

bajar "$FFMPEG_URL/ffmpeg.zip" "$FFMPEG_SHA256" "ffmpeg-9.0.2-$ARCH.zip"
bajar "$DENO_URL" "$DENO_SHA256" "deno-2.9.7-$ARCH.zip"
bajar "$FPCALC_URL" "$FPCALC_SHA256" fpcalc-1.6.1-universal.tar.gz

unzip -o -q "$CACHE/ffmpeg-9.0.2-$ARCH.zip" -d "$DESTINO"
unzip -o -q "$CACHE/deno-2.9.7-$ARCH.zip" -d "$DESTINO"
tar xzf "$CACHE/fpcalc-1.6.1-universal.tar.gz" -C "$DESTINO" --strip-components=1 \
    chromaprint-fpcalc-1.6.1-macos-universal/fpcalc
if [ "$CON_FFPROBE" = 1 ]; then
    bajar "$FFMPEG_URL/ffprobe.zip" "$FFPROBE_SHA256" "ffprobe-9.0.2-$ARCH.zip"
    unzip -o -q "$CACHE/ffprobe-9.0.2-$ARCH.zip" -d "$DESTINO"
    chmod 755 "$DESTINO/ffprobe"
else
    # el de una vez anterior con --con-ffprobe: en la .app no viaja
    rm -f "$DESTINO/ffprobe"
fi
chmod 755 "$DESTINO/ffmpeg" "$DESTINO/fpcalc" "$DESTINO/deno"

# Que arrancan de verdad en este Mac
"$DESTINO/ffmpeg" -hide_banner -version | head -1
"$DESTINO/fpcalc" -version
"$DESTINO/deno" --version | head -1
