#!/usr/bin/env bash
# Lo pesado, en GitHub y no en tu equipo: las pruebas completas y los paquetes
# de cada sistema se hacen en la integración continua, y aquí solo se espera
# (esperar no gasta nada) y se baja el resultado.
#
#   ./scripts/github.sh pruebas                  las de pruebas.yml, las mismas
#                                                que ./scripts/test.sh; si algo
#                                                falla, enseña qué
#   ./scripts/github.sh paquetes                 los siete de cada versión, a la
#                                                vez, y los baja: .deb y
#                                                .AppImage, el APK, los dos .dmg
#                                                de Mac y el instalador y el
#                                                portátil de Windows
#   ./scripts/github.sh paquetes linux android   solo esos (linux, android,
#                                                macos, windows)
#   ./scripts/github.sh bajar ID...              los de ejecuciones ya lanzadas
#                                                (si cerraste esto a medias o
#                                                se cortó la descarga)
#
# Los paquetes quedan en dist/installers/ con los nombres de las versiones
# publicadas, en lugar de los de la versión anterior. (Para publicar no hace
# falta nada de esto: la etiqueta de versión ya los sube todos.)
#
# Se construye lo que hay en GitHub, no lo de esta carpeta: el commit de esta
# rama tiene que estar subido (git push). Si ese commit ya tiene una ejecución
# en marcha o que salió bien (la del push a main, la de la etiqueta de
# versión), se espera a esa en vez de lanzar otra igual; la de pruebas, además,
# cancelaría a la que ya corre (concurrency en pruebas.yml).
#
# Hace falta `gh` con sesión iniciada.
set -euo pipefail
cd "$(dirname "$0")/.."

paso () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
aviso () { printf '\033[33m%s\033[0m\n' "$1"; }
morir () { printf '\n\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

command -v gh >/dev/null || morir "falta gh (la herramienta de GitHub): https://cli.github.com"

# El commit que se va a construir ($SHA, de la rama $RAMA). Tiene que ser el
# mismo aquí y en GitHub: si no, lo que bajes no es lo que hay en esta carpeta.
commit_subido () {
    RAMA=$(git symbolic-ref -q --short HEAD) || morir "no estás en ninguna rama"
    git fetch -q origin "$RAMA" 2>/dev/null \
      || morir "la rama $RAMA no está en GitHub: git push -u origin $RAMA"
    SHA=$(git rev-parse HEAD)
    [ "$SHA" = "$(git rev-parse FETCH_HEAD)" ] \
      || morir "$RAMA aquí y en GitHub no coinciden: sube los cambios primero (git push)"
    [ -z "$(git status --porcelain --untracked-files=no)" ] \
      || aviso "Hay cambios sin commit: esos no viajan. Se usa ${SHA:0:7}, lo último subido."
}

# ejecucion FLUJO: el número de una ejecución de FLUJO para $SHA. La que ya
# esté en marcha o haya salido bien, si la hay; si no, una nueva en $RAMA.
ejecucion () {
    local flujo=$1 id antes
    id=$(gh run list --workflow "$flujo" --commit "$SHA" -L 20 --json databaseId,status,conclusion \
           --jq '[.[] | select(.status != "completed" or .conclusion == "success")][0].databaseId // empty')
    if [ -z "$id" ]; then
        antes=$(gh run list --workflow "$flujo" -L 1 --json databaseId --jq '.[0].databaseId // 0')
        gh workflow run "$flujo" --ref "$RAMA" >/dev/null
        # GitHub tarda unos segundos en dar de alta la ejecución
        for _ in $(seq 1 30); do
            id=$(gh run list --workflow "$flujo" --commit "$SHA" -L 1 --json databaseId \
                   --jq ".[] | select(.databaseId > $antes) | .databaseId")
            [ -n "$id" ] && break
            sleep 2
        done
        [ -n "$id" ] || morir "GitHub no dio de alta $flujo; mírala en Actions"
    fi
    echo "$id"
}

# esperar ID: vuelve cuando la ejecución termina, con cómo acabó
esperar () {
    gh run watch "$1" --interval 30 >/dev/null 2>&1 || true
    gh run view "$1" --json conclusion --jq .conclusion
}

enlace () { gh run view "$1" --json url --jq .url; }

pruebas () {
    local id estado
    commit_subido
    paso "pruebas de ${SHA:0:7} ($RAMA)"
    id=$(ejecucion pruebas.yml)
    printf '  %s\n' "$(enlace "$id")"

    paso "esperar a que terminen (unos 10 minutos)"
    estado=$(esperar "$id")
    gh run view "$id" --json jobs \
      --jq '.jobs[] | ({"success": "  ok   ", "skipped": "  --   "}[.conclusion] // "  MAL  ") + .name'
    if [ "$estado" = success ]; then
        printf '\n\033[32mTODO CORRECTO\033[0m\n'
        return
    fi
    # De cada trabajo que falló, lo que salió justo antes de su primer
    # ##[error]. El final del registro no sirve (es la limpieza de checkout),
    # y por paso no se puede cortar: gh los da todos como «UNKNOWN STEP».
    paso "lo que falló"
    gh run view "$id" --log-failed | awk -F'\t' '
        { sub(/^[0-9T:.-]+Z /, "", $3); n[$1]++; linea[$1, n[$1]] = $3 }
        /##\[error\]/ && !visto[$1]++ { orden[++k] = $1; corte[$1] = n[$1] }
        END {
            for (i = 1; i <= k; i++) {
                t = orden[i]; print "  -- " t
                for (m = corte[t] - 12; m <= corte[t]; m++) if (m > 0) print "     " linea[t, m]
            }
        }'
    morir "HAY FALLOS: el registro entero en $(enlace "$id")"
}

paquetes () {
    local sistemas=("$@") ids=() s id
    # de la que menos tarda a la que más: se baja cada una según termina
    [ ${#sistemas[@]} -gt 0 ] || sistemas=(linux android macos windows)
    for s in "${sistemas[@]}"; do
        case "$s" in
            linux|android|macos|windows) ;;
            *) morir "no conozco «$s»: linux, android, macos o windows" ;;
        esac
    done
    commit_subido

    paso "lanzar ${sistemas[*]} con ${SHA:0:7} ($RAMA)"
    for s in "${sistemas[@]}"; do
        id=$(ejecucion "$s.yml")
        ids+=("$id")
        printf '  %-8s %s\n' "$s" "$(enlace "$id")"
    done
    aviso "Si cierras esto, siguen en GitHub; para bajarlos luego: $0 bajar ${ids[*]}"
    bajar "${ids[@]}"
}

# copiar ORIGEN NOMBRE: a dist/installers/. Se desenlaza antes, como en
# build.sh: un AppImage abierto no deja que lo sobrescriban.
copiar () {
    rm -f "dist/installers/$2"
    cp "$1" "dist/installers/$2"
    printf '  %s  (%s)\n' "$2" "$(du -h "dist/installers/$2" | cut -f1)"
}

# colocar SISTEMA DIR: de lo bajado en DIR, los paquetes a dist/installers/,
# quitando antes los de otra versión
colocar () {
    local dir=$2 deb img exe zip apk dmg par nombre falta=0
    case "$1" in
    linux)
        deb=$(find "$dir" -name 'DanPlay_*.deb' | head -1)
        img=$(find "$dir" -name 'DanPlay_*.AppImage' | head -1)
        [ -n "$deb" ] && [ -n "$img" ] || { aviso "  no salieron el .deb y el .AppImage"; return 1; }
        rm -f dist/installers/*.deb dist/installers/*.AppImage
        copiar "$deb" "$(basename "$deb")"
        copiar "$img" "$(basename "$img")"
        # el artefacto viaja en un .zip, que no guarda el permiso de ejecución
        chmod +x "dist/installers/$(basename "$img")"
        ;;
    android)
        apk=$(find "$dir" -name 'DanPlay-*-android.apk' | head -1)
        [ -n "$apk" ] || { aviso "  no salió el APK"; return 1; }
        rm -f dist/installers/DanPlay-*-android.apk
        copiar "$apk" "$(basename "$apk")"
        ;;
    windows)
        # dos trabajos en la misma ejecución: si uno falló, el otro sirve igual
        exe=$(find "$dir" -name 'DanPlay_*-setup.exe' | head -1)
        if [ -n "$exe" ]; then
            rm -f dist/installers/DanPlay_*-setup.exe
            copiar "$exe" "$(basename "$exe")"
        else
            aviso "  no salió el instalador"; falta=1
        fi
        zip=$(find "$dir" -name 'DanPlay-*-windows-portatil.zip' | head -1)
        if [ -n "$zip" ]; then
            rm -f dist/installers/DanPlay-*-windows-portatil.zip
            copiar "$zip" "$(basename "$zip")"
        else
            aviso "  no salió el portátil"; falta=1
        fi
        return "$falta"
        ;;
    macos)
        for par in aarch64:apple-silicon x64:intel; do
            nombre=${par#*:}
            dmg=$(find "$dir" -name "DanPlay-*-mac-$nombre.dmg" | head -1)
            # las ejecuciones de antes lo dejaban con el nombre de Tauri
            [ -n "$dmg" ] || dmg=$(find "$dir" -name "DanPlay_*_${par%%:*}.dmg" | head -1)
            if [ -z "$dmg" ]; then
                aviso "  no salió el de Mac $nombre"; falta=1; continue
            fi
            rm -f dist/installers/DanPlay-*-mac-"$nombre".dmg
            copiar "$dmg" "DanPlay-$(basename "$dmg" | sed 's/^DanPlay[-_]\([0-9.]*\)[-_].*/\1/')-mac-$nombre.dmg"
        done
        aviso '  Sin firma de Apple: la primera vez, Ajustes del Sistema > Privacidad y seguridad > «Abrir igualmente».'
        return "$falta"
        ;;
    *)
        aviso "  «$1» no deja paquetes"; return 1 ;;
    esac
}

bajar () {
    [ $# -gt 0 ] || morir "uso: $0 bajar ID..."
    local id sistema estado intento intentos fallo=0
    TMP=$(mktemp -d)
    trap 'rm -rf "$TMP"' EXIT
    mkdir -p dist/installers

    # Una detrás de otra, de la que menos tarda a la que más: mientras se
    # espera a una, las demás siguen corriendo en GitHub.
    for id in "$@"; do
        sistema=$(gh run view "$id" --json workflowName --jq .workflowName) \
          || morir "no encuentro la ejecución $id"
        case "$sistema" in
            linux|android) paso "$sistema: esperar (unos 10 minutos)" ;;
            macos)         paso "macos: esperar (unos 20 minutos)" ;;
            *)             paso "$sistema: esperar (unos 25 minutos)" ;;
        esac
        estado=$(esperar "$id")
        intentos=3
        if [ "$estado" != success ]; then
            # se baja lo que haya: de Windows y de Mac salen dos cosas, y que
            # una falle no estropea la otra
            aviso "  terminó en «$estado»: $(enlace "$id")"
            fallo=1; intentos=1
        fi
        # Son paquetes de 200-300 MB: si la conexión se corta a medias, otra
        # vez (la ejecución ya está hecha, no hace falta repetirla)
        for intento in $(seq 1 "$intentos"); do
            rm -rf "${TMP:?}/$id"
            gh run download "$id" -D "$TMP/$id" 2>/dev/null && break
            if [ "$intento" = "$intentos" ]; then
                [ "$estado" = success ] && aviso "  no pude bajarlos; otra vez con: $0 bajar $id"
                fallo=1; continue 2
            fi
            aviso "  se cortó la descarga; otra vez ($((intento + 1)) de $intentos)"
        done
        colocar "$sistema" "$TMP/$id" || fallo=1
        rm -rf "${TMP:?}/$id"
    done

    [ "$fallo" = 0 ] || morir "falta algo: mira arriba qué ejecución y por qué"
    printf '\n\033[32mLISTO\033[0m  dist/installers/\n'
}

orden=${1:-}
[ $# -gt 0 ] && shift
case "$orden" in
    pruebas)  pruebas ;;
    paquetes) paquetes "$@" ;;
    bajar)    bajar "$@" ;;
    *) sed -n '2,/^set /{/^#/s/^# \{0,1\}//p}' "$0"; exit 2 ;;
esac
