#!/bin/sh
# Los datos que viajan DENTRO de los paquetes, en desktop/src-tauri/data/ (el
# recurso «data/» de tauri.conf.json; la app se los da al núcleo):
#
#   - lo de data/: el detector de pulso (pulse/), los grafos del
#     separador (separator/), el catálogo de voces (voices.json) y la foto del
#     catálogo de modelos de IA (models-snapshot.json, la del repositorio: la
#     de hoy la saca la propia app con --models-snapshot una vez compilada);
#   - ONNX Runtime, la biblioteca oficial de github.com/microsoft/onnxruntime
#     para este sistema, con su licencia: la cargan el pulso, el separador y
#     las voces. Antes salía del paquete de Python `onnxruntime`, dentro del
#     núcleo de PyInstaller.
#
#   ./scripts/data.sh            los de este sistema (Linux o macOS)
#   ./scripts/data.sh windows    los de Windows, desde Linux
#                                (scripts/build-windows-cross.sh)
#
# Lo llaman scripts/build.sh, scripts/build-windows-cross.sh y la CI de Linux
# y macOS. En Windows hacen lo mismo, en PowerShell, scripts/build-windows.ps1
# y .github/workflows/windows.yml: para subir una versión de ONNX Runtime, se
# cambian el nombre y la suma aquí y allí.
#
# Como scripts/tools-linux.sh con ffmpeg: versión fija, comprobada con su
# sha256, y en una caché (~/.cache/danplay-<sistema>/): la segunda vez no se
# baja nada.
set -eu
cd "$(dirname "$0")/.."

DEST=desktop/src-tauri/data
RELEASES=https://github.com/microsoft/onnxruntime/releases/download

die () { printf '\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

SYSTEM=${1:-}
if [ -z "$SYSTEM" ]; then
    case "$(uname -s)" in
        Linux) SYSTEM=linux ;;
        Darwin) SYSTEM=macos ;;
        *) die "no sé qué sistema es $(uname -s): dilo (linux, macos o windows)" ;;
    esac
fi

# La 1.31.0 (la misma que pedía el núcleo de Python) y, en los Mac con Intel,
# la 1.23.2: Microsoft dejó de publicarla para ellos y es la última que hay.
# Vale igual: ort pide la API 23 de ONNX Runtime, que la 1.23 ya tiene.
case "$SYSTEM" in
    linux)
        [ "$(uname -m)" = x86_64 ] || die "ONNX Runtime de Linux, para x86_64 (esto es $(uname -m))"
        VERSION=1.31.0
        ARCHIVE=onnxruntime-linux-x64-$VERSION.tgz
        SHA256=cc5c72baf5ae5c8238a6841f0897227be2d02826b9cf98eaf02fdefaeeb03a57
        LIB=libonnxruntime.so.$VERSION
        CACHE="${DANPLAY_CACHE_LINUX:-$HOME/.cache/danplay-linux}"
        ;;
    macos)
        [ "$(uname -s)" = Darwin ] || die "ONNX Runtime de macOS, en un Mac (esto es $(uname -s))"
        case "$(uname -m)" in
            arm64)
                VERSION=1.31.0
                ARCHIVE=onnxruntime-osx-arm64-$VERSION.tgz
                SHA256=1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc
                ;;
            x86_64)
                VERSION=1.23.2
                ARCHIVE=onnxruntime-osx-x86_64-$VERSION.tgz
                SHA256=d10359e16347b57d9959f7e80a225a5b4a66ed7d7e007274a15cae86836485a6
                ;;
            *) die "no hay ONNX Runtime de macOS para $(uname -m)" ;;
        esac
        LIB=libonnxruntime.$VERSION.dylib
        CACHE="${DANPLAY_CACHE_MACOS:-$HOME/.cache/danplay-macos}"
        ;;
    windows)
        VERSION=1.31.0
        ARCHIVE=onnxruntime-win-x64-$VERSION.zip
        SHA256=3958d8a44984160692c1b4ffd0de3cb48f8c39f9d34bb31e433db06e30b82195
        LIB=onnxruntime.dll
        CACHE="${DANPLAY_CACHE_WINDOWS:-$HOME/.cache/danplay-windows}"
        ;;
    *) die "no conozco el sistema «$SYSTEM» (linux, macos o windows)" ;;
esac
TOP=${ARCHIVE%.*}

# el sha256 de un archivo, con lo que haya (sha256sum en Linux, shasum en macOS)
sha256 () {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

mkdir -p "$CACHE" "$DEST"
file="$CACHE/$ARCHIVE"
if [ ! -f "$file" ] || [ "$(sha256 "$file")" != "$SHA256" ]; then
    printf '  bajando %s\n' "$ARCHIVE"
    curl -sfL --retry 3 -o "$file.part" "$RELEASES/v$VERSION/$ARCHIVE" \
      || { rm -f "$file.part"; die "no pude bajar $ARCHIVE"; }
    [ "$(sha256 "$file.part")" = "$SHA256" ] \
      || { rm -f "$file.part"; die "$ARCHIVE no es el que se esperaba (sha256)"; }
    mv "$file.part" "$file"
fi

# lo de una vez anterior fuera (una ONNX Runtime de otra versión se quedaría
# al lado de la nueva), salvo el LEEME
find "$DEST" -mindepth 1 -maxdepth 1 ! -name LEEME.txt -exec rm -rf {} +

# Se saca en una carpeta de la caché, no en /tmp (que suele estar en la RAM):
# el de Mac trae la biblioteca dos veces y sus símbolos, unos 140 MB. Del de
# Windows, solo lo que hace falta: con sus símbolos son 450.
work="$CACHE/onnxruntime-$$"
rm -rf "$work"
mkdir -p "$work"
trap 'rm -rf "$work"' EXIT
case "$ARCHIVE" in
    *.zip) unzip -q -o "$file" "$TOP/lib/$LIB" "$TOP/LICENSE" "$TOP/ThirdPartyNotices.txt" -d "$work" ;;
    *) tar xzf "$file" -C "$work" ;;
esac
[ -f "$work/$TOP/lib/$LIB" ] || die "$ARCHIVE no trae lib/$LIB"
cp "$work/$TOP/lib/$LIB" "$DEST/$LIB"
# Su licencia (MIT) y la de lo que lleva dentro: van con ella
cp "$work/$TOP/LICENSE" "$DEST/LICENSE-onnxruntime.txt"
cp "$work/$TOP/ThirdPartyNotices.txt" "$DEST/ThirdPartyNotices-onnxruntime.txt"
chmod 644 "$DEST/$LIB"

# lo de data/, tal cual (sin pesos del separador: se bajan al usarlo)
for item in pulse separator voices.json models-snapshot.json; do
    [ -e "data/$item" ] || die "falta data/$item"
    cp -R "data/$item" "$DEST/"
done

printf '  ONNX Runtime %s (%s) y lo de data/ en %s: %s\n' \
    "$VERSION" "$LIB" "$DEST" "$(du -sh "$DEST" | cut -f1)"
