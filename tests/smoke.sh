#!/bin/sh
# Prueba de humo sobre la aplicacion real: la arranca, comprueba el puente,
# la seguridad y el ciclo de vida, y la cierra.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
APP="${1:-$ROOT/target/release/danplay-app}"
# El socket vive en un directorio propio con permisos 0700. Antes estaba
# suelto en $XDG_RUNTIME_DIR (o, sin el, en /tmp, que es de todos).
BASE="${XDG_RUNTIME_DIR:-$HOME/.cache}"
DIR="$BASE/danplay"
SOCK="$DIR/core.sock"
PY="$ROOT/.venv/bin/python"
passed=0; failed=0
pass () { echo "  ok    $1"; passed=$((passed+1)); }
fail () { echo "  FALLA $1"; failed=$((failed+1)); }

[ -x "$APP" ] || { echo "no encuentro la app en $APP"; exit 2; }
command -v ffmpeg >/dev/null 2>&1 || { echo "hace falta ffmpeg para la biblioteca de prueba"; exit 2; }

# Todo aparte: ajustes, base, indice y una biblioteca propia, en un temporal.
# Antes la prueba usaba los datos de quien la lanzara (y fallaba si su
# biblioteca estaba vacia); ahora no toca nada tuyo y da lo mismo en
# cualquier equipo.
SANDBOX=$(mktemp -d)
trap 'rm -rf "$SANDBOX"' EXIT
export XDG_CONFIG_HOME="$SANDBOX/config" XDG_DATA_HOME="$SANDBOX/data" XDG_CACHE_HOME="$SANDBOX/cache"
export DANPLAY_PROJECT_ENV=0 DANPLAY_LIBRARY="$SANDBOX/Musica"
mkdir -p "$SANDBOX/Musica/Artistas/Prueba"
# una con caratula (para las miniaturas) y otra sin ella. En dos pasos: con la
# imagen como segunda entrada, ffmpeg cierra la salida al acabarse su unico
# fotograma y el audio se quedaba en 0,05 s.
A="$SANDBOX/Musica/Artistas/Prueba"
ffmpeg -loglevel error -f lavfi -i "sine=frequency=440:duration=3" -c:a libmp3lame -b:a 128k \
    "$SANDBOX/tone.mp3" || { echo "ffmpeg no pudo"; exit 2; }
ffmpeg -loglevel error -f lavfi -i "color=c=teal:s=600x600" -frames:v 1 "$SANDBOX/cover.jpg"
ffmpeg -loglevel error -i "$SANDBOX/tone.mp3" -i "$SANDBOX/cover.jpg" -map 0 -map 1 -c copy \
    -disposition:v attached_pic -metadata artist=Prueba -metadata title="Con portada" \
    "$A/Prueba - Con portada.mp3"
ffmpeg -loglevel error -i "$SANDBOX/tone.mp3" -c copy \
    -metadata artist=Prueba -metadata title="Sin portada" "$A/Prueba - Sin portada.mp3"
rm -f "$SOCK"
# se lanza en su propio grupo y se sigue SOLO a este proceso: si hay otras
# instancias abiertas (la del usuario, por ejemplo) no deben confundir la prueba
"$APP" >/tmp/danplay-smoke.log 2>&1 &
PID=$!
i=0; while [ $i -lt 25 ]; do [ -S "$SOCK" ] && break; i=$((i+1)); sleep 1; done
CHILD=$(pgrep -P "$PID" -x danplay-core | head -1)

[ -S "$SOCK" ] && pass "la app crea su socket" || { fail "no arranco"; exit 1; }
[ "$(stat -c %a "$SOCK")" = "600" ] && pass "el socket es privado (0600)" \
  || fail "permisos del socket: $(stat -c %a "$SOCK")"
[ "$(stat -c %a "$DIR")" = "700" ] && pass "su carpeta tambien (0700)" \
  || fail "permisos de la carpeta: $(stat -c %a "$DIR")"
[ -n "$CHILD" ] && pass "el nucleo empaquetado arranca (pid $CHILD)" \
  || fail "no se lanzo el nucleo"
# El puerto exacto, no la subcadena: `grep 8730` tambien casaba con 48730 y
# con cualquier IP que llevara esos digitos.
listening=$(ss -ltnH 2>/dev/null | awk '{print $4}' | sed 's/.*://' | grep -cx 8730)
[ "$listening" = "0" ] && pass "no hay puertos TCP abiertos" \
  || fail "hay un puerto TCP escuchando en 8730"

# La bandeja: en Linux se habla StatusNotifierItem por D-Bus, y si el icono no
# se registra no hay donde quedarse al cerrar la ventana.
if command -v busctl >/dev/null 2>&1 && [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  if busctl --user --no-pager list 2>/dev/null | grep -q "StatusNotifierWatcher"; then
    i=0; registered=0
    while [ $i -lt 10 ]; do
      if busctl --user --no-pager list 2>/dev/null \
         | grep -q "StatusNotifierItem-$PID"; then registered=1; break; fi
      i=$((i+1)); sleep 1
    done
    [ "$registered" = "1" ] && pass "el icono se registra en la bandeja" \
      || fail "el icono no aparecio en la bandeja"
  else
    echo "  --    sin bandeja en esta sesion; se omite"
  fi
fi

get () { curl -s --unix-socket "$SOCK" "http://localhost$1"; }
post () { curl -s --unix-socket "$SOCK" -X POST -H 'content-type: application/json' -d "$2" "http://localhost$1"; }
get /api/status | grep -q '"stats"' && pass "responde /api/status" || fail "/api/status"

# la biblioteca de prueba: se da de alta y se escanea (una tarea larga: se
# espera a que acabe consultando /api/jobs)
post /api/folders "{\"path\": \"$SANDBOX/Musica\"}" | grep -q '"added"' \
  && pass "da de alta una carpeta" || fail "no dio de alta la carpeta"
post /api/scan '{}' >/dev/null
i=0; while [ $i -lt 30 ]; do
  get /api/jobs/scan | grep -q '"active":false' && break; i=$((i+1)); sleep 1
done
get /api/status | grep -q '"total":2' && pass "escanea en segundo plano (2 canciones)" \
  || fail "el escaneo no indexo las 2 canciones"
get /api/facets | grep -q '"artists"' && pass "responde /api/facets" || fail "/api/facets"
get "/api/search?limit=1" | grep -q '"songs"' && pass "responde /api/search" || fail "/api/search"
get /api/playlists | grep -q '"playlists"' && pass "responde /api/playlists" || fail "/api/playlists"
get /api/folders | grep -q '"folders"' && pass "responde /api/folders" || fail "/api/folders"

CID=$(get "/api/search?q=portada&limit=5" | "$PY" -c "import json,sys;d=[s for s in json.load(sys.stdin)['songs'] if s['title']=='Con portada'];print(d[0]['id'] if d else '')" 2>/dev/null)
if [ -n "$CID" ]; then
  get "/api/song/$CID/path" | grep -q '"path"' \
    && pass "Rust puede obtener la ruta del audio" || fail "/api/song/N/path"
  N=$(get "/api/song/$CID/audio" | wc -c)
  [ "$N" -gt 20000 ] && pass "el audio se sirve ($((N/1024)) KB)" || fail "audio vacio"
  # una miniatura no puede pesar mas que la portada entera
  FULL=$(get "/api/song/$CID/cover" | wc -c)
  THUMB=$(get "/api/song/$CID/cover?size=96" | wc -c)
  if [ "$FULL" -gt 1000 ]; then
    [ "$THUMB" -gt 0 ] && [ "$THUMB" -le "$FULL" ] \
      && pass "la miniatura pesa menos ($((THUMB/1024)) KB de $((FULL/1024)) KB)" \
      || fail "la miniatura no encogio: $THUMB de $FULL"
  else
    echo "  --    esa cancion no tiene portada; se omite"
  fi
else
  fail "sin canciones indexadas para probar"
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
[ -S "$SOCK" ] && pass "una segunda instancia no se lleva el socket" \
  || fail "la segunda instancia dejo a la primera sin socket"
get /api/status | grep -q '"stats"' \
  && pass "y el nucleo sigue contestando despues" \
  || fail "el nucleo dejo de contestar tras la segunda instancia"
[ "$(pgrep -cx danplay-app)" = "1" ] && pass "sigue habiendo una sola instancia" \
  || fail "quedaron $(pgrep -cx danplay-app) instancias"

kill -9 "$PID" 2>/dev/null
if [ -n "$CHILD" ]; then
  i=0; while [ $i -lt 8 ]; do kill -0 "$CHILD" 2>/dev/null || break; i=$((i+1)); sleep 1; done
  kill -0 "$CHILD" 2>/dev/null && fail "el nucleo queda huerfano" \
    || pass "el nucleo muere con la app (${i}s)"
else
  fail "no se pudo seguir al nucleo"
fi
rm -f "$SOCK"

echo
echo "  $passed correctas, $failed fallidas"
[ "$failed" -eq 0 ]
