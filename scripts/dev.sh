#!/bin/sh
# DanPlay en modo desarrollo: la app entera (Tauri + Vite en caliente) con el
# nucleo Python del propio proyecto, no el empaquetado. Lo que cambies en la
# interfaz se ve al momento; lo de Python, al volver a abrirla.
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
# identificador, su socket y su nombre en MPRIS y en la bandeja.
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)

missing () { printf '\033[31mfalta\033[0m %s\n  %s\n' "$1" "$2" >&2; exit 1; }
[ -x .venv/bin/python ] || missing ".venv" "uv sync   (o: python3 -m venv .venv && .venv/bin/python -m pip install -r requirements-dev.txt)"
[ -d desktop/node_modules ] || missing "desktop/node_modules" "cd desktop && npm ci"
command -v cargo >/dev/null 2>&1 || missing "Rust" "https://rustup.rs"

if [ "${1:-}" = "--sandbox" ]; then
    shift
    SANDBOX="$ROOT/.dev/sandbox"
    if [ "${1:-}" = "--clean" ]; then
        shift
        rm -rf "$SANDBOX"
    fi
    mkdir -p "$SANDBOX/config" "$SANDBOX/data" "$SANDBOX/cache" "$SANDBOX/Musica"
    # platformdirs (Python) y Tauri leen estas: todo lo de la app queda dentro
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
            .venv/bin/python - "$SANDBOX/Musica" <<'EOF'
import pathlib, sys
sys.path.insert(0, "tests")
from conftest import make_mp3
lib = pathlib.Path(sys.argv[1])
for artist, title, tone in (("Barak", "Mi Gozo", 440), ("Barak", "Sera Llena La Tierra", 330),
                            ("New Wine", "Shekinah", 262), ("New Wine", "A Una Voz", 392)):
    make_mp3(lib / "Artistas" / artist / f"{artist} - {title}.mp3", artist=artist,
             title=title, seconds=20, tone=tone)
EOF
        fi
    fi
    printf '\033[36mprueba\033[0m datos en %s\n       biblioteca: %s (eligela en la bienvenida)\n' \
        "$SANDBOX" "$SANDBOX/Musica"
fi

cd desktop
exec npm run app
