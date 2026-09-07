#!/bin/sh
# Prueba de humo sobre la aplicacion real: la arranca, comprueba el puente,
# la seguridad y el ciclo de vida, y la cierra.
set -u
RAIZ=$(cd "$(dirname "$0")/.." && pwd)
APP="${1:-$RAIZ/desktop/src-tauri/target/release/danplay-app}"
SOCK="${XDG_RUNTIME_DIR:-/tmp}/danplay.sock"
PY="$RAIZ/.venv/bin/python"
ok=0; mal=0
pasa () { echo "  ok    $1"; ok=$((ok+1)); }
falla () { echo "  FALLA $1"; mal=$((mal+1)); }

[ -x "$APP" ] || { echo "no encuentro la app en $APP"; exit 2; }
rm -f "$SOCK"
# se lanza en su propio grupo y se sigue SOLO a este proceso: si hay otras
# instancias abiertas (la del usuario, por ejemplo) no deben confundir la prueba
"$APP" >/tmp/danplay-humo.log 2>&1 &
PID=$!
i=0; while [ $i -lt 25 ]; do [ -S "$SOCK" ] && break; i=$((i+1)); sleep 1; done
HIJO=$(pgrep -P "$PID" -x danplay-core | head -1)

[ -S "$SOCK" ] && pasa "la app crea su socket" || { falla "no arranco"; exit 1; }
[ "$(stat -c %a "$SOCK")" = "600" ] && pasa "el socket es privado (0600)" \
  || falla "permisos del socket: $(stat -c %a "$SOCK")"
[ -n "$HIJO" ] && pasa "el nucleo empaquetado arranca (pid $HIJO)" \
  || falla "no se lanzo el nucleo"
# El puerto exacto, no la subcadena: `grep 8730` tambien casaba con 48730 y
# con cualquier IP que llevara esos digitos.
escuchando=$(ss -ltnH 2>/dev/null | awk '{print $4}' | sed 's/.*://' | grep -cx 8730)
[ "$escuchando" = "0" ] && pasa "no hay puertos TCP abiertos" \
  || falla "hay un puerto TCP escuchando en 8730"

pide () { curl -s --unix-socket "$SOCK" "http://localhost$1"; }
pide /api/status | grep -q '"stats"' && pasa "responde /api/status" || falla "/api/status"
pide /api/facets | grep -q '"artists"' && pasa "responde /api/facets" || falla "/api/facets"
pide "/api/search?limit=1" | grep -q '"songs"' && pasa "responde /api/search" || falla "/api/search"
pide /api/playlists | grep -q '"playlists"' && pasa "responde /api/playlists" || falla "/api/playlists"
pide /api/folders | grep -q '"folders"' && pasa "responde /api/folders" || falla "/api/folders"

CID=$(pide "/api/search?limit=1" | "$PY" -c "import json,sys;d=json.load(sys.stdin)['songs'];print(d[0]['id'] if d else '')" 2>/dev/null)
if [ -n "$CID" ]; then
  pide "/api/song/$CID/path" | grep -q '"path"' \
    && pasa "Rust puede obtener la ruta del audio" || falla "/api/song/N/path"
  N=$(pide "/api/song/$CID/audio" | wc -c)
  [ "$N" -gt 1000 ] && pasa "el audio se sirve ($((N/1024)) KB)" || falla "audio vacio"
else
  falla "sin canciones indexadas para probar"
fi

kill -9 "$PID" 2>/dev/null
if [ -n "$HIJO" ]; then
  i=0; while [ $i -lt 8 ]; do kill -0 "$HIJO" 2>/dev/null || break; i=$((i+1)); sleep 1; done
  kill -0 "$HIJO" 2>/dev/null && falla "el nucleo queda huerfano" \
    || pasa "el nucleo muere con la app (${i}s)"
else
  falla "no se pudo seguir al nucleo"
fi
rm -f "$SOCK"

echo
echo "  $ok correctas, $mal fallidas"
[ "$mal" -eq 0 ]
