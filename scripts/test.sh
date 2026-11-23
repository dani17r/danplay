#!/bin/sh
# Lanza las pruebas y comprobaciones del proyecto, las de la CI salvo tres
# que piden red o mucho tiempo y se quedan alli (tests.yml): la cobertura,
# `vp build` y los avisos de seguridad de las dependencias (`cargo audit`,
# `bun audit`). Los guiones de shell, con shellcheck si lo tienes.
#
#   ./scripts/test.sh          todo, incluidas las e2e (Chrome) y la de humo
#                              (que arranca la app compilada de verdad)
#   ./scripts/test.sh --quick  sin las e2e ni la de humo
#
# Aparte va la instalacion limpia del .deb en un sistema recien hecho
# (`./scripts/test-install.sh`): pide Docker, red y el paquete ya construido,
# y tarda unos minutos, asi que se lanza antes de entregar y no aqui.
set -u
cd "$(dirname "$0")/.." || exit 1
LOGS=$(mktemp -d)
trap 'rm -rf "$LOGS"' EXIT
failed=0
section () { echo; echo "=== $1 ==="; }

# run_check NAME COMMAND...: la ejecuta con la salida a un registro; si va bien,
# enseña la ultima linea que diga algo (el resumen), y si falla, el final.
run_check () {
    name=$1; shift
    if "$@" > "$LOGS/$name.log" 2>&1; then
        summary=$(grep -E 'passed|Tests +[0-9]|test result|All checks|already formatted|correct format|0 errors|files? checked' \
                  "$LOGS/$name.log" | tail -1)
        printf '  ok   %-14s %s\n' "$name" "$summary"
    else
        failed=1
        printf '  MAL  %s\n' "$name"
        tail -25 "$LOGS/$name.log" | sed 's/^/       /'
    fi
}
# las de la interfaz, con Vite+: el `vp` instalado (https://viteplus.dev) o,
# si no esta, el que trae desktop/node_modules (`vp install` en desktop/)
VP=$(command -v vp || echo "$PWD/desktop/node_modules/.bin/vp")

section "interfaz"
run_check estilo      sh -c "cd desktop && $VP run lint && $VP run lint:css && $VP run format:check"
run_check tipos-js    sh -c "cd desktop && $VP run typecheck"
run_check vitest      sh -c "cd desktop && $VP test run --reporter=dot"
run_check iconos      sh -c "cd desktop && node scripts/icons.mjs --check"
run_check medios      node tests/test_frontend.mjs
if command -v shellcheck >/dev/null 2>&1; then
    run_check guiones shellcheck -S warning scripts/*.sh tests/smoke.sh packaging/deb/post* danplay-app.sh
fi

# Con el perfil `ci` (el de release sin LTO ni strip), el mismo que la CI:
# clippy y las pruebas comparten lo compilado, y enlazar cada binario de
# prueba sin LTO es mucho mas rapido. Queda en target/ci/.
section "Rust (workspace: el nucleo, el analisis y las apps)"
run_check rustfmt     cargo fmt --all --check
run_check clippy      cargo clippy --workspace --all-targets --profile ci -- -D warnings
run_check cargo-test  cargo test --workspace --profile ci

if [ "${1:-}" != "--quick" ]; then
    section "e2e: interfaz y nucleo juntos, en Chrome"
    # el nucleo de Rust que arrancan (e2e/server.mjs busca el mas nuevo de
    # target/: el de ci, el de release o el de depuracion)
    run_check servidor   cargo build --profile ci -p danplay-server
    run_check playwright sh -c "cd desktop && $VP run e2e"
    section "humo: la app real"
    ./tests/smoke.sh || failed=1
fi

echo
[ "$failed" = "0" ] && echo "TODO CORRECTO" || echo "HAY FALLOS"
exit "$failed"
