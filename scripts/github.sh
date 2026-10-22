#!/usr/bin/env bash
# Lo pesado, en GitHub y no en tu equipo: las pruebas completas y los paquetes
# de cada sistema se hacen en la integración continua, y aquí solo se espera
# (esperar no gasta nada) y se baja el resultado.
#
#   ./scripts/github.sh tests                    las de tests.yml, las mismas
#                                                que ./scripts/test.sh; si algo
#                                                falla, enseña qué
#   ./scripts/github.sh packages                 los siete de cada versión, a la
#                                                vez, y los baja: .deb y
#                                                .AppImage, el APK, los dos .dmg
#                                                de Mac y el instalador y el
#                                                portátil de Windows
#   ./scripts/github.sh packages linux android   solo esos (linux, android,
#                                                macos, windows)
#   ./scripts/github.sh download ID...           los de ejecuciones ya lanzadas
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
# cancelaría a la que ya corre (concurrency en tests.yml).
#
# Hace falta `gh` con sesión iniciada.
set -euo pipefail
cd "$(dirname "$0")/.."

step () { printf '\n\033[36m== %s\033[0m\n' "$1"; }
warn () { printf '\033[33m%s\033[0m\n' "$1"; }
die () { printf '\n\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

command -v gh >/dev/null || die "falta gh (la herramienta de GitHub): https://cli.github.com"

# El commit que se va a construir ($SHA, de la rama $BRANCH). Tiene que ser el
# mismo aquí y en GitHub: si no, lo que bajes no es lo que hay en esta carpeta.
pushed_commit () {
    BRANCH=$(git symbolic-ref -q --short HEAD) || die "no estás en ninguna rama"
    git fetch -q origin "$BRANCH" 2>/dev/null \
      || die "la rama $BRANCH no está en GitHub: git push -u origin $BRANCH"
    SHA=$(git rev-parse HEAD)
    [ "$SHA" = "$(git rev-parse FETCH_HEAD)" ] \
      || die "$BRANCH aquí y en GitHub no coinciden: sube los cambios primero (git push)"
    [ -z "$(git status --porcelain --untracked-files=no)" ] \
      || warn "Hay cambios sin commit: esos no viajan. Se usa ${SHA:0:7}, lo último subido."
}

# workflow_run WORKFLOW: el número de una ejecución de WORKFLOW para $SHA. La
# que ya esté en marcha o haya salido bien, si la hay; si no, una nueva en
# $BRANCH.
workflow_run () {
    local workflow=$1 id before
    id=$(gh run list --workflow "$workflow" --commit "$SHA" -L 20 --json databaseId,status,conclusion \
           --jq '[.[] | select(.status != "completed" or .conclusion == "success")][0].databaseId // empty')
    if [ -z "$id" ]; then
        before=$(gh run list --workflow "$workflow" -L 1 --json databaseId --jq '.[0].databaseId // 0')
        gh workflow run "$workflow" --ref "$BRANCH" >/dev/null
        # GitHub tarda unos segundos en dar de alta la ejecución
        for _ in $(seq 1 30); do
            id=$(gh run list --workflow "$workflow" --commit "$SHA" -L 1 --json databaseId \
                   --jq ".[] | select(.databaseId > $before) | .databaseId")
            [ -n "$id" ] && break
            sleep 2
        done
        [ -n "$id" ] || die "GitHub no dio de alta $workflow; mírala en Actions"
    fi
    echo "$id"
}

# wait_run ID: vuelve cuando la ejecución termina, con cómo acabó
wait_run () {
    gh run watch "$1" --interval 30 >/dev/null 2>&1 || true
    gh run view "$1" --json conclusion --jq .conclusion
}

run_url () { gh run view "$1" --json url --jq .url; }

tests () {
    local id conclusion
    pushed_commit
    step "pruebas de ${SHA:0:7} ($BRANCH)"
    id=$(workflow_run tests.yml)
    printf '  %s\n' "$(run_url "$id")"

    step "esperar a que terminen (unos 10 minutos)"
    conclusion=$(wait_run "$id")
    gh run view "$id" --json jobs \
      --jq '.jobs[] | ({"success": "  ok   ", "skipped": "  --   "}[.conclusion] // "  MAL  ") + .name'
    if [ "$conclusion" = success ]; then
        printf '\n\033[32mTODO CORRECTO\033[0m\n'
        return
    fi
    # De cada trabajo que falló, lo que salió justo antes de su primer
    # ##[error]. El final del registro no sirve (es la limpieza de checkout),
    # y por paso no se puede cortar: gh los da todos como «UNKNOWN STEP».
    step "lo que falló"
    gh run view "$id" --log-failed | awk -F'\t' '
        { sub(/^[0-9T:.-]+Z /, "", $3); n[$1]++; line[$1, n[$1]] = $3 }
        /##\[error\]/ && !seen[$1]++ { order[++k] = $1; cut[$1] = n[$1] }
        END {
            for (i = 1; i <= k; i++) {
                t = order[i]; print "  -- " t
                for (m = cut[t] - 12; m <= cut[t]; m++) if (m > 0) print "     " line[t, m]
            }
        }'
    die "HAY FALLOS: el registro entero en $(run_url "$id")"
}

packages () {
    local systems=("$@") ids=() s id
    # de la que menos tarda a la que más: se baja cada una según termina
    [ ${#systems[@]} -gt 0 ] || systems=(linux android macos windows)
    for s in "${systems[@]}"; do
        case "$s" in
            linux|android|macos|windows) ;;
            *) die "no conozco «$s»: linux, android, macos o windows" ;;
        esac
    done
    pushed_commit

    step "lanzar ${systems[*]} con ${SHA:0:7} ($BRANCH)"
    for s in "${systems[@]}"; do
        id=$(workflow_run "$s.yml")
        ids+=("$id")
        printf '  %-8s %s\n' "$s" "$(run_url "$id")"
    done
    warn "Si cierras esto, siguen en GitHub; para bajarlos luego: $0 download ${ids[*]}"
    download "${ids[@]}"
}

# copy_out SOURCE NAME: a dist/installers/. Se desenlaza antes, como en
# build.sh: un AppImage abierto no deja que lo sobrescriban.
copy_out () {
    rm -f "dist/installers/$2"
    cp "$1" "dist/installers/$2"
    printf '  %s  (%s)\n' "$2" "$(du -h "dist/installers/$2" | cut -f1)"
}

# place SYSTEM DIR: de lo bajado en DIR, los paquetes a dist/installers/,
# quitando antes los de otra versión
place () {
    local dir=$2 deb img exe zip apk dmg pair name missing=0
    case "$1" in
    linux)
        deb=$(find "$dir" -name 'DanPlay_*.deb' | head -1)
        img=$(find "$dir" -name 'DanPlay_*.AppImage' | head -1)
        [ -n "$deb" ] && [ -n "$img" ] || { warn "  no salieron el .deb y el .AppImage"; return 1; }
        rm -f dist/installers/*.deb dist/installers/*.AppImage
        copy_out "$deb" "$(basename "$deb")"
        copy_out "$img" "$(basename "$img")"
        # el artefacto viaja en un .zip, que no guarda el permiso de ejecución
        chmod +x "dist/installers/$(basename "$img")"
        ;;
    android)
        apk=$(find "$dir" -name 'DanPlay-*-android.apk' | head -1)
        [ -n "$apk" ] || { warn "  no salió el APK"; return 1; }
        rm -f dist/installers/DanPlay-*-android.apk
        copy_out "$apk" "$(basename "$apk")"
        ;;
    windows)
        # dos trabajos en la misma ejecución: si uno falló, el otro sirve igual
        exe=$(find "$dir" -name 'DanPlay_*-setup.exe' | head -1)
        if [ -n "$exe" ]; then
            rm -f dist/installers/DanPlay_*-setup.exe
            copy_out "$exe" "$(basename "$exe")"
        else
            warn "  no salió el instalador"; missing=1
        fi
        zip=$(find "$dir" -name 'DanPlay-*-windows-portatil.zip' | head -1)
        if [ -n "$zip" ]; then
            rm -f dist/installers/DanPlay-*-windows-portatil.zip
            copy_out "$zip" "$(basename "$zip")"
        else
            warn "  no salió el portátil"; missing=1
        fi
        return "$missing"
        ;;
    macos)
        for pair in aarch64:apple-silicon x64:intel; do
            name=${pair#*:}
            dmg=$(find "$dir" -name "DanPlay-*-mac-$name.dmg" | head -1)
            # las ejecuciones de antes lo dejaban con el nombre de Tauri
            [ -n "$dmg" ] || dmg=$(find "$dir" -name "DanPlay_*_${pair%%:*}.dmg" | head -1)
            if [ -z "$dmg" ]; then
                warn "  no salió el de Mac $name"; missing=1; continue
            fi
            rm -f dist/installers/DanPlay-*-mac-"$name".dmg
            copy_out "$dmg" "DanPlay-$(basename "$dmg" | sed 's/^DanPlay[-_]\([0-9.]*\)[-_].*/\1/')-mac-$name.dmg"
        done
        warn '  Sin firma de Apple: la primera vez, Ajustes del Sistema > Privacidad y seguridad > «Abrir igualmente».'
        return "$missing"
        ;;
    *)
        warn "  «$1» no deja paquetes"; return 1 ;;
    esac
}

download () {
    [ $# -gt 0 ] || die "uso: $0 download ID..."
    local id system conclusion attempt attempts failed=0
    TMP=$(mktemp -d)
    trap 'rm -rf "$TMP"' EXIT
    mkdir -p dist/installers

    # Una detrás de otra, de la que menos tarda a la que más: mientras se
    # espera a una, las demás siguen corriendo en GitHub.
    for id in "$@"; do
        system=$(gh run view "$id" --json workflowName --jq .workflowName) \
          || die "no encuentro la ejecución $id"
        case "$system" in
            linux|android) step "$system: esperar (unos 10 minutos)" ;;
            macos)         step "macos: esperar (unos 20 minutos)" ;;
            *)             step "$system: esperar (unos 25 minutos)" ;;
        esac
        conclusion=$(wait_run "$id")
        attempts=3
        if [ "$conclusion" != success ]; then
            # se baja lo que haya: de Windows y de Mac salen dos cosas, y que
            # una falle no estropea la otra
            warn "  terminó en «$conclusion»: $(run_url "$id")"
            failed=1; attempts=1
        fi
        # Son paquetes de 200-300 MB: si la conexión se corta a medias, otra
        # vez (la ejecución ya está hecha, no hace falta repetirla)
        for attempt in $(seq 1 "$attempts"); do
            rm -rf "${TMP:?}/$id"
            gh run download "$id" -D "$TMP/$id" 2>/dev/null && break
            if [ "$attempt" = "$attempts" ]; then
                [ "$conclusion" = success ] && warn "  no pude bajarlos; otra vez con: $0 download $id"
                failed=1; continue 2
            fi
            warn "  se cortó la descarga; otra vez ($((attempt + 1)) de $attempts)"
        done
        place "$system" "$TMP/$id" || failed=1
        rm -rf "${TMP:?}/$id"
    done

    [ "$failed" = 0 ] || die "falta algo: mira arriba qué ejecución y por qué"
    printf '\n\033[32mLISTO\033[0m  dist/installers/\n'
}

subcommand=${1:-}
[ $# -gt 0 ] && shift
case "$subcommand" in
    tests)    tests ;;
    packages) packages "$@" ;;
    download) download "$@" ;;
    *) sed -n '2,/^set /{/^#/s/^# \{0,1\}//p}' "$0"; exit 2 ;;
esac
