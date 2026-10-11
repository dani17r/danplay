#!/usr/bin/env bash
# El paquete de macOS, que se construye en la integración continua de GitHub
# (desde Linux no se puede: hace falta el SDK de Apple y un Mac).
#
#   ./scripts/build-macos.sh            lanza .github/workflows/macos.yml con
#                                       lo que hay en main, espera y lo baja
#   ./scripts/build-macos.sh ID         baja el de una ejecución ya terminada
#
# Los dos .dmg quedan en dist/installers/, junto al .deb, el .AppImage y los
# de Windows, con nombres que dicen para qué Mac es cada uno:
#   DanPlay-X.Y.Z-mac-apple-silicon.dmg   M1 en adelante
#   DanPlay-X.Y.Z-mac-intel.dmg           los de antes
#
# Hace falta `gh` con sesión iniciada, y lo de main subido: la CI construye lo
# que hay en GitHub, no lo de esta carpeta.
set -euo pipefail
cd "$(dirname "$0")/.."

paso () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
aviso () { printf '\033[33m%s\033[0m\n' "$1"; }
morir () { printf '\n\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

command -v gh >/dev/null || morir "falta gh (la herramienta de GitHub)"
VERSION=$(sed -n 's/^__version__ = "\([^"]*\)"/\1/p' danplay/__init__.py)
RUN="${1:-}"

if [ -z "$RUN" ]; then
    # Lo que se construye es main en GitHub: si aquí hay algo sin subir, el
    # paquete no sería lo que hay en esta carpeta.
    git fetch -q origin main
    [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] \
      || morir "main aquí y en GitHub no coinciden: sube los cambios primero (git push)"
    SHA=$(git rev-parse HEAD)

    paso "lanzar macos.yml para $VERSION (${SHA:0:7})"
    antes=$(gh run list --workflow macos.yml -L 1 --json databaseId --jq '.[0].databaseId // 0')
    gh workflow run macos.yml --ref main
    # GitHub tarda unos segundos en dar de alta la ejecución
    for _ in $(seq 1 30); do
        RUN=$(gh run list --workflow macos.yml -L 1 --json databaseId,headSha \
                --jq ".[0] | select(.databaseId != $antes and .headSha == \"$SHA\") | .databaseId")
        [ -n "$RUN" ] && break
        sleep 2
    done
    [ -n "$RUN" ] || morir "GitHub no dio de alta la ejecución; mírala en Actions"
fi

paso "esperar a la ejecución $RUN (unos 20 minutos)"
gh run watch "$RUN" --interval 30 > /dev/null || true
estado=$(gh run view "$RUN" --json conclusion --jq .conclusion)
[ "$estado" = success ] || aviso "  la ejecución terminó en «$estado»: se baja lo que haya"

paso "bajar los paquetes"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# Son dos archivos de ~200 MB: si la conexión se corta a medias, otra vez
# (la ejecución ya está hecha, no hace falta repetirla).
for intento in 1 2 3; do
    rm -rf "${tmp:?}"/*
    gh run download "$RUN" -D "$tmp" && break
    [ "$intento" = 3 ] && morir "no pude bajar los paquetes; vuelve a probar con: $0 $RUN"
    aviso "  se cortó la descarga; otra vez ($((intento + 1)) de 3)"
done

mkdir -p dist/installers
faltan=0
for par in aarch64:apple-silicon x64:intel; do
    arch=${par%%:*}; nombre=${par#*:}
    dmg=$(find "$tmp" -name "DanPlay_*_$arch.dmg" | head -1)
    if [ -z "$dmg" ]; then
        aviso "  no salió el de Mac $nombre"; faltan=1; continue
    fi
    # Cinturón, como en build.sh: un paquete de otra versión no se copia.
    case "$(basename "$dmg")" in
        DanPlay_"$VERSION"_*) ;;
        *) morir "$(basename "$dmg") no es de la versión $VERSION" ;;
    esac
    rm -f dist/installers/DanPlay-*-mac-"$nombre".dmg
    cp "$dmg" "dist/installers/DanPlay-$VERSION-mac-$nombre.dmg"
    printf '  %s  (%s)\n' "DanPlay-$VERSION-mac-$nombre.dmg" \
        "$(du -h "dist/installers/DanPlay-$VERSION-mac-$nombre.dmg" | cut -f1)"
done

[ "$faltan" = 0 ] || morir "falta algún paquete: mira la ejecución $RUN en Actions"
printf '\n\033[32mLISTO\033[0m  dist/installers/\n'
aviso 'Sin firma de Apple: la primera vez, Ajustes del Sistema > Privacidad y seguridad > «Abrir igualmente».'
