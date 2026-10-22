#!/usr/bin/env bash
# La app del celular para Android (mobile/), lista para mandar a la banda.
#
#   ./scripts/build-android.sh            el APK firmado para teléfonos
#                                         (arm64 y armv7: casi todos)
#   ./scripts/build-android.sh --emulator el de depuración para el emulador
#                                         (x86_64), sin firma de entrega
#
# El APK queda en dist/installers/, junto a los de la PC:
#   DanPlay-X.Y.Z-android.apk
#
# Hace falta, en ~/Android (o donde digan ANDROID_HOME, NDK_HOME, JAVA_HOME):
# el SDK con su plataforma y build-tools, el NDK y un JDK 17; y en Rust los
# destinos de Android (`rustup target add aarch64-linux-android
# armv7-linux-androideabi x86_64-linux-android i686-linux-android`). Ver
# docs/CONTRIBUTING.md, «La app del celular».
#
# La firma sale de ~/Android/claves/keystore.properties (o de donde diga
# DANPLAY_KEYSTORE). Tiene que ser SIEMPRE la misma llave: un teléfono solo
# acepta una actualización firmada como la que tiene instalada.
set -euo pipefail
cd "$(dirname "$0")/.."

step () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
die () { printf '\n\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export JAVA_HOME="${JAVA_HOME:-$HOME/Android/jdk-17}"
if [ -z "${NDK_HOME:-}" ]; then
    NDK_HOME=$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)
    export NDK_HOME
fi
export PATH="$JAVA_HOME/bin:$PATH"
[ -d "$ANDROID_HOME/platforms" ] || die "no encuentro el SDK de Android en $ANDROID_HOME"
[ -n "$NDK_HOME" ] && [ -d "$NDK_HOME" ] || die "no encuentro el NDK (NDK_HOME)"
[ -x "$JAVA_HOME/bin/java" ] || die "no encuentro el JDK 17 (JAVA_HOME)"
[ -d desktop/node_modules ] || die "faltan las dependencias de la interfaz: cd desktop && npm ci"

VERSION=$(sed -n 's/^__version__ = "\([^"]*\)"/\1/p' danplay/__init__.py)
OUTPUT="dist/installers/DanPlay-$VERSION-android.apk"
APKS=mobile/gen/android/app/build/outputs/apk/universal

if [ "${1:-}" = "--emulator" ]; then
    step "APK de depuración para el emulador (x86_64)"
    (cd mobile && npm run tauri -- android build --debug --target x86_64 --apk)
    ls -la "$APKS/debug/app-universal-debug.apk"
    exit 0
fi

KEYSTORE="${DANPLAY_KEYSTORE:-$HOME/Android/claves/keystore.properties}"
[ -f "$KEYSTORE" ] || die "no está la llave de firma ($KEYSTORE): sin ella el APK no se puede instalar ni actualizar"

step "APK de DanPlay para Android $VERSION (arm64 y armv7)"
(cd mobile && npm run tauri -- android build --apk --target aarch64 --target armv7)
APK="$APKS/release/app-universal-release.apk"
[ -f "$APK" ] || die "no salió el APK ($APK)"

step "comprobar la firma"
# el de las build-tools más nuevas, salvo que se diga cuál (la CI tiene varias)
APKSIGNER="${APKSIGNER:-$(ls -d "$ANDROID_HOME"/build-tools/*/apksigner | sort -V | tail -1)}"
# Si algo falla, lo que dijo apksigner: antes se lo tragaba el grep y moría
# sin explicar nada.
signature=$("$APKSIGNER" verify --print-certs "$APK" 2>&1) \
  || { printf '%s\n' "$signature" >&2; die "el APK no está bien firmado ($APKSIGNER)"; }
printf '%s\n' "$signature" | grep -E "Signer #1 certificate (DN|SHA-256)" \
  || { printf '%s\n' "$signature" >&2; die "apksigner no dice con qué llave está firmado ($APKSIGNER)"; }

mkdir -p dist/installers
# (también los de antes de 1.27, que se llamaban DanPlay-Pistas-…)
rm -f dist/installers/DanPlay-*-android.apk
cp "$APK" "$OUTPUT"
ls -la "$OUTPUT"
