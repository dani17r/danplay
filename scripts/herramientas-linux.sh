#!/bin/sh
# Las herramientas que viajan DENTRO de los paquetes de Linux (.deb y
# .AppImage), en desktop/src-tauri/tools/: ffmpeg (ondas, clic, separar,
# convertir), fpcalc (identificar por el sonido) y QuickJS-NG (`qjs`, el
# motor de JavaScript que YouTube exige para descargar). ffprobe ya no: lo
# poco que medía (una carátula, la duración de una secuencia) sale de las
# cabeceras. Deno tampoco: hacía lo mismo que qjs en 96 MB en vez de 2,5
# (más rápido, sí: unos 5 s menos por descarga).
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
# fpcalc 1.6.1 (estático), de las versiones de Chromaprint
FPCALC_URL=https://github.com/acoustid/chromaprint/releases/download/v1.6.1/chromaprint-fpcalc-1.6.1-linux-x86_64.tar.gz
FPCALC_SHA256=fc16cd37a70168040bc9ceb45f1d4d1216f5a75bc4c9cf8564bea70ac6a45733
# QuickJS-NG 0.17.0, el mismo que el de Windows y macOS (un binario suelto)
QJS_URL=https://github.com/quickjs-ng/quickjs/releases/download/v0.17.0/qjs-linux-x86_64
QJS_SHA256=0bfc02511a9f549c28b53880d988fc7cd5d361e90c5e8afdfcd7dc6774ceace5

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
bajar "$FPCALC_URL" "$FPCALC_SHA256" fpcalc-1.6.1.tar.gz
bajar "$QJS_URL" "$QJS_SHA256" qjs-0.17.0

unzip -o -q -j "$CACHE/ffmpeg-9.0.2.zip" ffmpeg -d "$DESTINO"
cp "$CACHE/qjs-0.17.0" "$DESTINO/qjs"
tar xzf "$CACHE/fpcalc-1.6.1.tar.gz" -C "$DESTINO" --strip-components=1 \
    chromaprint-fpcalc-1.6.1-linux-x86_64/fpcalc
# el ffprobe y el Deno de una versión anterior, si quedaron ahí: ya no viajan
rm -f "$DESTINO/ffprobe" "$DESTINO/deno"
chmod 755 "$DESTINO/ffmpeg" "$DESTINO/fpcalc" "$DESTINO/qjs"

# Que arrancan de verdad en esta máquina (la de la CI es la más vieja que se
# admite: Ubuntu 22.04)
"$DESTINO/ffmpeg" -hide_banner -version | head -1
"$DESTINO/fpcalc" -version
"$DESTINO/qjs" --help 2>&1 | head -1
