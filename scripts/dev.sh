#!/bin/sh
# DanPlay en modo desarrollo: la app entera (Tauri + Vite en caliente) con el
# nucleo dentro, compilado del codigo, y los datos de danplay/data. Lo que
# cambies en la interfaz se ve al momento; lo de Rust, cuando tauri dev lo
# vuelve a compilar (solo, al guardar). No hace falta Python.
#
#   ./scripts/dev.sh                  con tus datos de siempre (~/.config/danplay,
#                                     ~/.local/share/danplay): tu biblioteca real
#   ./scripts/dev.sh --sandbox [DIR]  con datos aparte, en .dev/sandbox/: ajustes,
#                                     base e indice propios y una biblioteca que
#                                     es una COPIA de las canciones de DIR (o unas
#                                     generadas con ffmpeg si no das ninguna).
#                                     Ni tu musica ni tus ajustes se tocan.
#   ./scripts/dev.sh --sandbox --clean [DIR]   lo mismo, empezando de cero
#
# Convive con DanPlay instalado: la compilacion de desarrollo usa su propio
# identificador y su nombre en MPRIS y en la bandeja.
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)

missing () { printf '\033[31mfalta\033[0m %s\n  %s\n' "$1" "$2" >&2; exit 1; }
# la interfaz, con Vite+ (vp) y Bun, que vp baja solo si no lo tienes
command -v vp >/dev/null 2>&1 || missing "Vite+ (vp)" "curl -fsSL https://vite.plus | bash"
[ -d desktop/node_modules ] || missing "desktop/node_modules" "cd desktop && vp install"
command -v cargo >/dev/null 2>&1 || missing "Rust" "https://rustup.rs"
# ONNX Runtime (el pulso, el separador, las voces): el que viaja con la app,
# una vez (queda en la cache); la compilacion lo deja junto al ejecutable
ls desktop/src-tauri/data/libonnxruntime.* >/dev/null 2>&1 || ./scripts/data.sh

if [ "${1:-}" = "--sandbox" ]; then
    shift
    SANDBOX="$ROOT/.dev/sandbox"
    if [ "${1:-}" = "--clean" ]; then
        shift
        rm -rf "$SANDBOX"
    fi
    mkdir -p "$SANDBOX/config" "$SANDBOX/data" "$SANDBOX/cache" "$SANDBOX/Musica"
    # el nucleo y Tauri leen estas: todo lo de la app queda dentro
    export XDG_CONFIG_HOME="$SANDBOX/config"
    export XDG_DATA_HOME="$SANDBOX/data"
    export XDG_CACHE_HOME="$SANDBOX/cache"
    export DANPLAY_LIBRARY="$SANDBOX/Musica"
    # ni el .env del proyecto (puede llevar tus claves de IA) ni su base
    export DANPLAY_PROJECT_ENV=0
    if [ -z "$(ls -A "$SANDBOX/Musica")" ]; then
        if [ -n "${1:-}" ]; then
            # una copia: la app escribe etiquetas y la importacion mueve archivos
            find "$1" -maxdepth 2 -type f \( -iname '*.mp3' -o -iname '*.flac' -o -iname '*.ogg' \
                -o -iname '*.opus' -o -iname '*.m4a' -o -iname '*.wav' -o -iname '*.aac' \
                -o -iname '*.wma' \) -exec cp {} "$SANDBOX/Musica/" \;
        else
            command -v ffmpeg >/dev/null 2>&1 || missing "ffmpeg" "para generar las canciones de prueba"
            # cuatro tonos de 20 s con sus etiquetas, como los de las pruebas
            for song in "Barak|Mi Gozo|440" "Barak|Sera Llena La Tierra|330" \
                        "New Wine|Shekinah|262" "New Wine|A Una Voz|392"; do
                artist=${song%%|*}; rest=${song#*|}; title=${rest%|*}; tone=${rest##*|}
                mkdir -p "$SANDBOX/Musica/Artistas/$artist"
                ffmpeg -loglevel error -f lavfi -i "sine=frequency=$tone:duration=20" \
                    -c:a libmp3lame -b:a 128k -metadata artist="$artist" -metadata title="$title" \
                    "$SANDBOX/Musica/Artistas/$artist/$artist - $title.mp3"
            done
        fi
    fi
    printf '\033[36mprueba\033[0m datos en %s\n       biblioteca: %s (eligela en la bienvenida)\n' \
        "$SANDBOX" "$SANDBOX/Musica"
fi

cd desktop
exec vp run app
