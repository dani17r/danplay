#!/bin/sh
# Lanza todas las pruebas y comprobaciones del proyecto, las mismas que la CI.
#
#   ./scripts/test.sh          todo, incluidas las e2e (Chrome) y la de humo
#                              (que arranca la app compilada de verdad)
#   ./scripts/test.sh --rapido sin las e2e ni la de humo
set -u
cd "$(dirname "$0")/.."
LOGS=$(mktemp -d)
trap 'rm -rf "$LOGS"' EXIT
fallo=0
seccion () { echo; echo "=== $1 ==="; }

# corre NOMBRE ORDEN...: la ejecuta con la salida a un registro; si va bien,
# enseña la ultima linea que diga algo (el resumen), y si falla, el final.
corre () {
    nombre=$1; shift
    if "$@" > "$LOGS/$nombre.log" 2>&1; then
        resumen=$(grep -E 'passed|Tests +[0-9]|test result|All checks|already formatted|0 errors|files? checked' \
                  "$LOGS/$nombre.log" | tail -1)
        printf '  ok   %-14s %s\n' "$nombre" "$resumen"
    else
        fallo=1
        printf '  MAL  %s\n' "$nombre"
        tail -25 "$LOGS/$nombre.log" | sed 's/^/       /'
    fi
}
# las herramientas de Python, las del .venv del proyecto
py () { herramienta=$1; shift; ".venv/bin/$herramienta" "$@"; }

seccion "nucleo Python"
corre ruff        py ruff check .
corre formato     py ruff format --check .
corre tipos       py basedpyright
# una sola vez: mide la cobertura del total, y por trozos no llegaria al minimo
corre pytest      py python -m pytest -q --no-header

seccion "interfaz"
corre estilo      sh -c 'cd desktop && npm run lint && npm run lint:css && npm run format:check'
corre tipos-js    sh -c 'cd desktop && npm run typecheck'
corre vitest      sh -c 'cd desktop && npx vitest run --reporter=dot'
corre medios      node tests/test_frontend.mjs

seccion "Rust (workspace: nucleo de audio y app de escritorio)"
# Sin `extension-module` en Cargo.toml, las pruebas del crate PyO3
# enlazarian con libpython; asi se compilan como la extension de maturin.
export PYO3_BUILD_EXTENSION_MODULE=1
corre rustfmt     cargo fmt --all --check
corre clippy      cargo clippy --workspace --all-targets --release -- -D warnings
corre cargo-test  cargo test --workspace --release

if [ "${1:-}" != "--rapido" ]; then
    seccion "e2e: interfaz y nucleo juntos, en Chrome"
    corre playwright sh -c 'cd desktop && npm run e2e'
    seccion "humo: la app real"
    ./tests/smoke.sh || fallo=1
fi

echo
[ "$fallo" = "0" ] && echo "TODO CORRECTO" || echo "HAY FALLOS"
exit "$fallo"
