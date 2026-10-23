#!/bin/sh
# Las herramientas que viajan DENTRO del paquete de macOS (la .app), en
# desktop/src-tauri/tools/: ffmpeg (ondas, clic, separar, convertir), fpcalc
# (identificar por el sonido), yt-dlp (bajar de YouTube: su ejecutable
# oficial, sin Python) y QuickJS-NG (`qjs`, el motor de JavaScript que YouTube
# exige a yt-dlp), para el procesador de este Mac: Apple Silicon o Intel.
#
#   ./scripts/tools-macos.sh                 lo que viaja en la .app
#   ./scripts/tools-macos.sh --with-ffprobe  y ffprobe, que la app ya
#                                                   no usa pero las pruebas sí,
#                                                   para mirar lo que sale
#
# Lo llama la CI (.github/workflows/macos.yml), y vale igual en un Mac. En
# macOS no hay gestor de paquetes que ponga ffmpeg, y una app abierta desde el
# Finder no ve el PATH de Homebrew: por eso van dentro. Como en
# scripts/tools-linux.sh, las mismas versiones que en Linux y Windows,
# fijadas con su sha256, y en una caché: la segunda vez no se baja nada.
set -eu
cd "$(dirname "$0")/.."

DEST=desktop/src-tauri/tools
CACHE="${DANPLAY_CACHE_MACOS:-$HOME/.cache/danplay-macos}"
WITH_FFPROBE=0
[ "${1:-}" = --with-ffprobe ] && WITH_FFPROBE=1

die () { printf '\033[31m%s\033[0m\n' "$1" >&2; exit 1; }
[ "$(uname -s)" = Darwin ] || die "las herramientas de macOS son para un Mac (esto es $(uname -s))"

# ffmpeg 9.0.2 de martin-riedl.de (el mismo que el de Linux) y QuickJS-NG 0.17.0
ARCH=$(uname -m)
case "$ARCH" in
    arm64)
        FFMPEG_URL=https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2
        FFMPEG_SHA256=c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924
        FFPROBE_SHA256=fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6
        QJS=qjs-darwin-arm64
        QJS_SHA256=8be3ddfe3397d2e692e4e1e8972ee9d032a0a580505d2f8b4ea528cf1b651c11
        ;;
    x86_64)
        FFMPEG_URL=https://ffmpeg.martin-riedl.de/download/macos/amd64/1789931006_9.0.2
        FFMPEG_SHA256=7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4
        FFPROBE_SHA256=2322438ed2f6319a691291b247d09c69dcaa3a982460d1f269a7e1af335cfdfd
        QJS=qjs-darwin-x86_64
        QJS_SHA256=9e5e101b4fd13cda3204222ca9f8be35412c41dcdef3745829633b7a67245412
        ;;
    *) die "no hay herramientas de macOS para $ARCH" ;;
esac
QJS_URL=https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0/$QJS
# fpcalc 1.6.1: un solo binario para los dos procesadores
FPCALC_URL=https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-macos-universal.tar.gz
FPCALC_SHA256=240aeb5a8c8205af458e3625cb7487b826b711a999e491ef00111f3cebd76f00
# yt-dlp 2026.08.19 (la de Linux y Windows): también uno para los dos
YTDLP_URL=https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos
YTDLP_SHA256=0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202

mkdir -p "$CACHE" "$DEST"

# Baja `url` a la caché como `name`, si no está ya, y comprueba su sha256.
download () {
    url=$1 sha=$2 name=$3
    file="$CACHE/$name"
    if [ -f "$file" ] && echo "$sha  $file" | shasum -a 256 -c --status -; then
        return 0
    fi
    printf '  bajando %s\n' "$name"
    curl -sfL --retry 3 -o "$file.part" "$url" || die "no pude bajar $url"
    echo "$sha  $file.part" | shasum -a 256 -c --status - \
      || { rm -f "$file.part"; die "$name no es el que se esperaba (sha256)"; }
    mv "$file.part" "$file"
}

download "$FFMPEG_URL/ffmpeg.zip" "$FFMPEG_SHA256" "ffmpeg-9.0.2-$ARCH.zip"
download "$QJS_URL" "$QJS_SHA256" "qjs-0.17.0-$ARCH"
download "$FPCALC_URL" "$FPCALC_SHA256" fpcalc-1.6.1-universal.tar.gz
download "$YTDLP_URL" "$YTDLP_SHA256" yt-dlp-2026.08.19

unzip -o -q "$CACHE/ffmpeg-9.0.2-$ARCH.zip" -d "$DEST"
cp "$CACHE/qjs-0.17.0-$ARCH" "$DEST/qjs"
cp "$CACHE/yt-dlp-2026.08.19" "$DEST/yt-dlp"
tar xzf "$CACHE/fpcalc-1.6.1-universal.tar.gz" -C "$DEST" --strip-components=1 \
    chromaprint-fpcalc-1.6.1-macos-universal/fpcalc
if [ "$WITH_FFPROBE" = 1 ]; then
    download "$FFMPEG_URL/ffprobe.zip" "$FFPROBE_SHA256" "ffprobe-9.0.2-$ARCH.zip"
    unzip -o -q "$CACHE/ffprobe-9.0.2-$ARCH.zip" -d "$DEST"
    chmod 755 "$DEST/ffprobe"
else
    # el de una vez anterior con --with-ffprobe: en la .app no viaja
    rm -f "$DEST/ffprobe"
fi
rm -f "$DEST/deno"  # el de antes: ya no viaja
chmod 755 "$DEST/ffmpeg" "$DEST/fpcalc" "$DEST/qjs" "$DEST/yt-dlp"

# Que arrancan de verdad en este Mac
"$DEST/ffmpeg" -hide_banner -version | head -1
"$DEST/fpcalc" -version
"$DEST/qjs" --help 2>&1 | head -1
printf 'yt-dlp %s\n' "$("$DEST/yt-dlp" --ignore-config --version)"
