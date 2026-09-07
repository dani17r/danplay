#!/bin/sh
# Lanza todas las pruebas del proyecto.
set -u
cd "$(dirname "$0")/.."
fallo=0
seccion () { echo; echo "=== $1 ==="; }

seccion "nucleo Python"
./.venv/bin/python -m pytest tests/test_core.py -q --no-header || fallo=1

seccion "API"
./.venv/bin/python -m pytest tests/test_api.py -q --no-header || fallo=1

seccion "frontend: URLs de medios"
node tests/test_frontend.mjs || fallo=1

seccion "frontend: componentes y reactividad"
( cd desktop && npx vitest run --reporter=dot ) > /tmp/danplay-vitest.log 2>&1
if [ $? -eq 0 ]; then
  grep -E "Tests +[0-9]" /tmp/danplay-vitest.log | tail -1
else
  fallo=1; tail -20 /tmp/danplay-vitest.log
fi

seccion "Rust: nucleo de audio"
( cd core && cargo test --release ) > /tmp/danplay-rust1.log 2>&1
if [ $? -eq 0 ]; then grep "test result" /tmp/danplay-rust1.log; else fallo=1; tail -15 /tmp/danplay-rust1.log; fi

seccion "Rust: reproductor nativo"
( cd desktop/src-tauri && cargo test --release ) > /tmp/danplay-rust2.log 2>&1
if [ $? -eq 0 ]; then grep "test result" /tmp/danplay-rust2.log; else fallo=1; tail -15 /tmp/danplay-rust2.log; fi

seccion "humo: la app real"
./tests/smoke.sh || fallo=1

echo
[ "$fallo" = "0" ] && echo "TODO CORRECTO" || echo "HAY FALLOS"
exit "$fallo"
