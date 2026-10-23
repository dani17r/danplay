#!/bin/sh
# Lanza todas las pruebas y comprobaciones del proyecto, las mismas que la CI.
#
#   ./scripts/test.sh          todo, incluidas las e2e (Chrome) y la de humo
#                              (que arranca la app compilada de verdad)
#   ./scripts/test.sh --quick  sin las e2e ni la de humo
set -u
cd "$(dirname "$0")/.."
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
run_check medios      node tests/test_frontend.mjs

section "Rust (workspace: el nucleo, el analisis y las apps)"
run_check rustfmt     cargo fmt --all --check
run_check clippy      cargo clippy --workspace --all-targets --release -- -D warnings
run_check cargo-test  cargo test --workspace --release

if [ "${1:-}" != "--quick" ]; then
    section "e2e: interfaz y nucleo juntos, en Chrome"
    # el nucleo de Rust que arrancan (e2e/server.mjs busca el de target/)
    run_check servidor   cargo build -p danplay-server
    run_check playwright sh -c "cd desktop && $VP run e2e"
    section "humo: la app real"
    ./tests/smoke.sh || failed=1
fi

echo
[ "$failed" = "0" ] && echo "TODO CORRECTO" || echo "HAY FALLOS"
exit "$failed"
