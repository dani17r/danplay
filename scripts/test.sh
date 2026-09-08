#!/bin/sh
# Lanza todas las pruebas del proyecto.
#
#   ./scripts/test.sh          todo
#   ./scripts/test.sh --rapido sin las de humo (que arrancan la app de verdad)
set -u
cd "$(dirname "$0")/.."
LOGS=$(mktemp -d)
trap 'rm -rf "$LOGS"' EXIT
fallo=0
seccion () { echo; echo "=== $1 ==="; }

seccion "nucleo Python"
./.venv/bin/python -m pytest tests/test_core.py -q --no-header || fallo=1

seccion "API"
./.venv/bin/python -m pytest tests/test_api.py -q --no-header || fallo=1

seccion "frontend: URLs de medios"
node tests/test_frontend.mjs || fallo=1

seccion "frontend: estilo y tipos"
( cd desktop && npx eslint src tests scripts ) > "$LOGS/lint.log" 2>&1
if [ $? -eq 0 ]; then echo "  sin errores"; else fallo=1; tail -20 "$LOGS/lint.log"; fi
( cd desktop && npx stylelint "src/**/*.css" ) > "$LOGS/css.log" 2>&1
if [ $? -eq 0 ]; then echo "  css sin errores"; else fallo=1; tail -15 "$LOGS/css.log"; fi

seccion "frontend: componentes y reactividad"
( cd desktop && npx vitest run --reporter=dot ) > "$LOGS/vitest.log" 2>&1
if [ $? -eq 0 ]; then
  grep -E "Tests +[0-9]" "$LOGS/vitest.log" | tail -1
else
  fallo=1; tail -25 "$LOGS/vitest.log"
fi

seccion "Rust: nucleo de audio"
( cd core && cargo test --release ) > "$LOGS/rust1.log" 2>&1
if [ $? -eq 0 ]; then grep "test result" "$LOGS/rust1.log"; else fallo=1; tail -15 "$LOGS/rust1.log"; fi

seccion "Rust: reproductor, cola y bandeja"
( cd desktop/src-tauri && cargo test --release ) > "$LOGS/rust2.log" 2>&1
if [ $? -eq 0 ]; then grep "test result" "$LOGS/rust2.log"; else fallo=1; tail -15 "$LOGS/rust2.log"; fi

if [ "${1:-}" != "--rapido" ]; then
  seccion "humo: la app real"
  ./tests/smoke.sh || fallo=1
fi

echo
[ "$fallo" = "0" ] && echo "TODO CORRECTO" || echo "HAY FALLOS"
exit "$fallo"
