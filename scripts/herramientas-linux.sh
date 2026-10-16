#!/bin/sh
# Las herramientas que viajan DENTRO de los paquetes de Linux (.deb y
# .AppImage), en desktop/src-tauri/tools/: ffmpeg y ffprobe (ondas, clic,
# separar, convertir), fpcalc (identificar por el sonido) y Deno (el motor de
# JavaScript que YouTube exige para descargar).
#
#   ./scripts/herramientas-linux.sh
#
# Lo llaman `scripts/build.sh --package` y la CI (.github/workflows/linux.yml).
# La app las encuentra ahí y se las pasa al núcleo (DANPLAY_TOOLS_DIR), que
# las usa antes que las del sistema: así no hace falta instalar nada aparte,
# y no dependen de las bibliotecas que traiga cada distribución.
#
# Las mismas versiones que los paquetes de Windows y macOS, fijadas con su
# sha256. Quedan en una caché: la segunda vez no se baja nada.
set -eu
cd "$(dirname "$0")/.."

DESTINO=desktop/src-tauri/tools
CACHE="${DANPLAY_CACHE_LINUX:-$HOME/.cache/danplay-linux}"

# ffmpeg 9.0.2 de martin-riedl.de (el mismo que el paquete de macOS)
FFMPEG_URL=https://ffmpeg.martin-riedl.de/download/linux/amd64/1789931100_9.0.2
FFMPEG_SHA256=fa8ecf4abbd290d98f7d188b8649cc6b391ae209a98452be955a15aab1909d7f
FFPROBE_SHA256=3f428c49070be3d24ec338602b76d412e401ffcb8a5641ef0e729181a232fc32
# fpcalc 1.6.1 (estático), de las versiones de Chromaprint
FPCALC_URL=https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-linux-x86_64.tar.gz
FPCALC_SHA256=fc16cd37a70168040bc9ceb45f1d4d1216f5a75bc4c9cf8564bea70ac6a45733
# Deno 2.9.7, el mismo que el de Windows
DENO_URL=https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-unknown-linux-gnu.zip
DENO_SHA256=c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490

morir () { printf '\033[31m%s\033[0m\n' "$1" >&2; exit 1; }
[ "$(uname -m)" = x86_64 ] || morir "las herramientas de Linux son para x86_64 (esto es $(uname -m))"
mkdir -p "$CACHE" "$DESTINO"

# Baja `url` a la caché como `nombre`, si no está ya, y comprueba su sha256.
bajar () {
    url=$1 sha=$2 nombre=$3
    archivo="$CACHE/$nombre"
    if [ -f "$archivo" ] && echo "$sha  $archivo" | sha256sum -c --status -; then
        return 0
    fi
    printf '  bajando %s\n' "$nombre"
    curl -sfL --retry 3 -o "$archivo.part" "$url" || morir "no pude bajar $url"
    echo "$sha  $archivo.part" | sha256sum -c --status - \
      || { rm -f "$archivo.part"; morir "$nombre no es el que se esperaba (sha256)"; }
    mv "$archivo.part" "$archivo"
}

bajar "$FFMPEG_URL/ffmpeg.zip" "$FFMPEG_SHA256" ffmpeg-9.0.2.zip
bajar "$FFMPEG_URL/ffprobe.zip" "$FFPROBE_SHA256" ffprobe-9.0.2.zip
bajar "$FPCALC_URL" "$FPCALC_SHA256" fpcalc-1.6.1.tar.gz
bajar "$DENO_URL" "$DENO_SHA256" deno-2.9.7.zip

unzip -o -q -j "$CACHE/ffmpeg-9.0.2.zip" ffmpeg -d "$DESTINO"
unzip -o -q -j "$CACHE/ffprobe-9.0.2.zip" ffprobe -d "$DESTINO"
unzip -o -q -j "$CACHE/deno-2.9.7.zip" deno -d "$DESTINO"
tar xzf "$CACHE/fpcalc-1.6.1.tar.gz" -C "$DESTINO" --strip-components=1 \
    chromaprint-fpcalc-1.6.1-linux-x86_64/fpcalc
chmod 755 "$DESTINO/ffmpeg" "$DESTINO/ffprobe" "$DESTINO/fpcalc" "$DESTINO/deno"

# Que arrancan de verdad en esta máquina (la de la CI es la más vieja que se
# admite: Ubuntu 22.04)
"$DESTINO/ffmpeg" -hide_banner -version | head -1
"$DESTINO/ffprobe" -hide_banner -version | head -1
"$DESTINO/fpcalc" -version
"$DESTINO/deno" --version | head -1
