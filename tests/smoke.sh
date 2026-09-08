#!/bin/sh
# Prueba de humo sobre la aplicacion real: la arranca, comprueba el puente,
# la seguridad y el ciclo de vida, y la cierra.
set -u
RAIZ=$(cd "$(dirname "$0")/.." && pwd)
APP="${1:-$RAIZ/desktop/src-tauri/target/release/danplay-app}"
# El socket vive en un directorio propio con permisos 0700. Antes estaba
# suelto en $XDG_RUNTIME_DIR (o, sin el, en /tmp, que es de todos).
BASE="${XDG_RUNTIME_DIR:-$HOME/.cache}"
DIR="$BASE/danplay"
SOCK="$DIR/core.sock"
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
[ "$(stat -c %a "$DIR")" = "700" ] && pasa "su carpeta tambien (0700)" \
  || falla "permisos de la carpeta: $(stat -c %a "$DIR")"
[ -n "$HIJO" ] && pasa "el nucleo empaquetado arranca (pid $HIJO)" \
  || falla "no se lanzo el nucleo"
# El puerto exacto, no la subcadena: `grep 8730` tambien casaba con 48730 y
# con cualquier IP que llevara esos digitos.
escuchando=$(ss -ltnH 2>/dev/null | awk '{print $4}' | sed 's/.*://' | grep -cx 8730)
[ "$escuchando" = "0" ] && pasa "no hay puertos TCP abiertos" \
  || falla "hay un puerto TCP escuchando en 8730"

# La bandeja: en Linux se habla StatusNotifierItem por D-Bus, y si el icono no
# se registra no hay donde quedarse al cerrar la ventana.
if command -v busctl >/dev/null 2>&1 && [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  if busctl --user --no-pager list 2>/dev/null | grep -q "StatusNotifierWatcher"; then
    i=0; registrado=0
    while [ $i -lt 10 ]; do
      if busctl --user --no-pager list 2>/dev/null \
         | grep -q "StatusNotifierItem-$PID"; then registrado=1; break; fi
      i=$((i+1)); sleep 1
    done
    [ "$registrado" = "1" ] && pasa "el icono se registra en la bandeja" \
      || falla "el icono no aparecio en la bandeja"
  else
    echo "  --    sin bandeja en esta sesion; se omite"
  fi
fi

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
  # una miniatura no puede pesar mas que la portada entera
  GRANDE=$(pide "/api/song/$CID/cover" | wc -c)
  CHICA=$(pide "/api/song/$CID/cover?size=96" | wc -c)
  if [ "$GRANDE" -gt 1000 ]; then
    [ "$CHICA" -gt 0 ] && [ "$CHICA" -le "$GRANDE" ] \
      && pasa "la miniatura pesa menos ($((CHICA/1024)) KB de $((GRANDE/1024)) KB)" \
      || falla "la miniatura no encogio: $CHICA de $GRANDE"
  else
    echo "  --    esa cancion no tiene portada; se omite"
  fi
else
  falla "sin canciones indexadas para probar"
fi

# Una SEGUNDA instancia: es lo que pasa al abrir una cancion con DanPlay ya
# abierto. Debe pasarle los argumentos a la primera y desaparecer sin tocar
# nada de ella.
#
# Esto se rompio de verdad: el segundo proceso levantaba su propio nucleo
# antes de que el plugin de instancia unica pudiera cortarlo, y al irse se
# llevaba el socket. La primera seguia viva y sonando, pero sin nucleo: ni
# busqueda, ni caratulas, ni lista, hasta reiniciar. Y no se notaba hasta que
# pinchabas algo.
"$APP" >/dev/null 2>&1
i=0; while [ $i -lt 5 ]; do [ "$(pgrep -cx danplay-app)" = "1" ] && break; i=$((i+1)); sleep 1; done
[ -S "$SOCK" ] && pasa "una segunda instancia no se lleva el socket" \
  || falla "la segunda instancia dejo a la primera sin socket"
pide /api/status | grep -q '"stats"' \
  && pasa "y el nucleo sigue contestando despues" \
  || falla "el nucleo dejo de contestar tras la segunda instancia"
[ "$(pgrep -cx danplay-app)" = "1" ] && pasa "sigue habiendo una sola instancia" \
  || falla "quedaron $(pgrep -cx danplay-app) instancias"

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
