#!/bin/sh
# Lanza todas las pruebas y comprobaciones del proyecto, las mismas que la CI.
#
#   ./scripts/test.sh          todo, incluidas las e2e (Chrome) y la de humo
#                              (que arranca la app compilada de verdad)
#   ./scripts/test.sh --quick  sin las e2e ni la de humo
#   ./scripts/test.sh api      solo las pruebas de la API de Python, contra
#                              el nucleo de Rust (DANPLAY_API_CORE=rust; el
#                              danplay-server de DANPLAY_SERVER_BIN, o lo
#                              compila: ver tests/api_core.py)
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
        summary=$(grep -E 'passed|Tests +[0-9]|test result|All checks|already formatted|0 errors|files? checked' \
                  "$LOGS/$name.log" | tail -1)
        printf '  ok   %-14s %s\n' "$name" "$summary"
    else
        failed=1
        printf '  MAL  %s\n' "$name"
        tail -25 "$LOGS/$name.log" | sed 's/^/       /'
    fi
}
# las herramientas de Python, las del .venv del proyecto
py () { tool=$1; shift; ".venv/bin/$tool" "$@"; }

if [ "${1:-}" = "api" ]; then
    section "la API de Python contra el nucleo de Rust"
    if [ -z "${DANPLAY_SERVER_BIN:-}" ]; then
        run_check servidor cargo build -p danplay-server
    fi
    # en modo Rust solo se recogen las que hablan con la API; test_api.py
    # va entero en un proceso, asi que con tres sobra
    export DANPLAY_API_CORE=rust
    run_check api-rust py python -m pytest --no-header --no-cov -n 3 --dist loadgroup
    echo
    [ "$failed" = "0" ] && echo "TODO CORRECTO" || echo "HAY FALLOS"
    exit "$failed"
fi

section "nucleo Python"
run_check ruff        py ruff check .
run_check formato     py ruff format --check .
run_check tipos       py basedpyright
# una sola vez: mide la cobertura del total, y por trozos no llegaria al minimo
run_check pytest      py python -m pytest -q --no-header -n auto --dist loadgroup

section "interfaz"
run_check estilo      sh -c 'cd desktop && npm run lint && npm run lint:css && npm run format:check'
run_check tipos-js    sh -c 'cd desktop && npm run typecheck'
run_check vitest      sh -c 'cd desktop && npx vitest run --reporter=dot'
run_check medios      node tests/test_frontend.mjs

section "Rust (workspace: nucleo de audio y app de escritorio)"
# Sin `extension-module` en Cargo.toml, las pruebas del crate PyO3
# enlazarian con libpython; asi se compilan como la extension de maturin.
export PYO3_BUILD_EXTENSION_MODULE=1
run_check rustfmt     cargo fmt --all --check
run_check clippy      cargo clippy --workspace --all-targets --release -- -D warnings
run_check cargo-test  cargo test --workspace --release

if [ "${1:-}" != "--quick" ]; then
    section "e2e: interfaz y nucleo juntos, en Chrome"
    run_check playwright sh -c 'cd desktop && npm run e2e'
    section "humo: la app real"
    ./tests/smoke.sh || failed=1
fi

echo
[ "$failed" = "0" ] && echo "TODO CORRECTO" || echo "HAY FALLOS"
exit "$failed"
