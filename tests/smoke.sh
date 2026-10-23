#!/bin/sh
# Prueba de humo sobre la aplicacion de verdad: la compilada
# (target/release/danplay-app) o la de un paquete (el usr/bin/danplay-app de
# un .deb o de un AppImage extraido). La arranca en una caja de arena,
# comprueba que lleva el nucleo dentro y que contesta, que encuentra sus datos
# y sus programas, que saca el pulso de una cancion, la instancia unica y el
# ciclo de vida, y la cierra.
#
#   tests/smoke.sh [APP]
#
# Todo aparte: la casa, los ajustes, los datos, la biblioteca y la carpeta de
# ejecucion, en un temporal; y su propio bus de D-Bus, para que un DanPlay
# abierto (el tuyo) no se entere: con el de la sesion, la instancia unica le
# pasaria el arranque a ese y la prueba no probaria nada. Sin pantalla
# propia (Xvfb o kwin_wayland --virtual, si estan) se usa la que haya y la
# ventana sale un momento.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
APP="${1:-${CARGO_TARGET_DIR:-$ROOT/target}/release/danplay-app}"
case "$APP" in /*) ;; *) APP="$(pwd)/$APP" ;; esac

[ -x "$APP" ] || { echo "no encuentro la app en $APP"; exit 2; }

# Lo primero, otro bus de D-Bus para todo lo de aqui dentro
if [ -z "${DANPLAY_SMOKE_BUS:-}" ] && command -v dbus-run-session >/dev/null 2>&1; then
    DANPLAY_SMOKE_BUS=1 exec dbus-run-session -- "$0" "$APP"
fi

passed=0; failed=0
pass () { echo "  ok    $1"; passed=$((passed+1)); }
fail () { echo "  FALLA $1"; failed=$((failed+1)); }
# vivo de verdad: un proceso que ya acabo y nadie ha recogido (zombi) no cuenta
alive () { s=$(ps -o stat= -p "$1" 2>/dev/null); [ -n "$s" ] && [ "${s#Z}" = "$s" ]; }

SANDBOX=$(mktemp -d)
cleanup () {
    [ -n "${PID:-}" ] && kill -9 "$PID" 2>/dev/null
    [ -n "${DISPLAY_PID:-}" ] && kill "$DISPLAY_PID" 2>/dev/null
    rm -rf "$SANDBOX"
}
trap cleanup EXIT
mkdir -p "$SANDBOX/home" "$SANDBOX/run" "$SANDBOX/Musica/Artistas/Prueba"
chmod 700 "$SANDBOX/run"
export HOME="$SANDBOX/home" XDG_RUNTIME_DIR="$SANDBOX/run"
export XDG_CONFIG_HOME="$SANDBOX/config" XDG_DATA_HOME="$SANDBOX/share"
export XDG_CACHE_HOME="$SANDBOX/cache" XDG_STATE_HOME="$SANDBOX/state"
export DANPLAY_DATA_DIR="$SANDBOX/data" DANPLAY_CONFIG_DIR="$SANDBOX/danplay-config"
export DANPLAY_LIBRARY="$SANDBOX/Musica" DANPLAY_PROJECT_ENV=0
unset DANPLAY_TOOLS_DIR DANPLAY_ONNXRUNTIME ORT_DYLIB_PATH DANPLAY_RESOURCES

# Lo que viaja con la app, donde lo deja cada paquete (o la compilacion)
BIN=$(dirname "$APP")
DATA=""; TOOLS=""
for d in "$BIN/data" "$BIN/../lib/DanPlay/data"; do [ -d "$d" ] && { DATA=$(cd "$d" && pwd); break; }; done
for d in "$BIN/tools" "$BIN/../lib/DanPlay/tools"; do [ -d "$d" ] && { TOOLS=$(cd "$d" && pwd); break; }; done
FFMPEG="$TOOLS/ffmpeg"
[ -x "$FFMPEG" ] || FFMPEG=$(command -v ffmpeg || true)
[ -n "$FFMPEG" ] || { echo "hace falta ffmpeg para la cancion de prueba"; exit 2; }

[ -n "$DATA" ] && pass "lleva sus datos ($DATA)" || fail "no encuentro data/ junto a la app"
ls "$DATA"/libonnxruntime.* >/dev/null 2>&1 && pass "y ONNX Runtime entre ellos" \
  || fail "no hay ONNX Runtime en $DATA"
if [ -n "$TOOLS" ]; then
    for tool in ffmpeg fpcalc qjs yt-dlp; do
        [ -x "$TOOLS/$tool" ] && pass "lleva $tool" || fail "no lleva $tool en $TOOLS"
    done
else
    echo "  --    sin tools/ junto a la app (la compilada sin --package): se usan los del sistema"
fi

# Una cancion con pulso de verdad: un clic cada medio segundo (120 por minuto)
SONG="$SANDBOX/Musica/Artistas/Prueba/Prueba - Clic.mp3"
"$FFMPEG" -loglevel error -f lavfi -i "aevalsrc='0.8*sin(2*PI*1500*t)*lt(mod(t\,0.5)\,0.03)':s=44100:d=12" \
    -c:a libmp3lame -b:a 128k -metadata artist=Prueba -metadata title=Clic "$SONG" \
  || { echo "ffmpeg no pudo"; exit 2; }

# El detector de pulso es la propia app con un argumento, como lo lanza el
# nucleo: con el grafo y ONNX Runtime que viajan con ella.
OUT=$("$APP" --pulse "$DATA/pulse/beat_this.onnx" "$FFMPEG" "$SONG" "$SANDBOX/crudo.json" 2>"$SANDBOX/pulso.err")
if printf '%s\n' "$OUT" | tail -1 | grep -q '"ok": true' && [ -s "$SANDBOX/crudo.json" ]; then
    pass "saca el pulso de una cancion (--pulse, con su ONNX Runtime)"
else
    fail "el pulso: $(printf '%s\n' "$OUT" | tail -1) $(head -c 300 "$SANDBOX/pulso.err")"
fi

# Una pantalla solo para la prueba, si la hay
if command -v Xvfb >/dev/null 2>&1; then
    Xvfb :87 -nolisten tcp >"$SANDBOX/display.log" 2>&1 &
    DISPLAY_PID=$!; export DISPLAY=:87; unset WAYLAND_DISPLAY
    sleep 2
elif command -v kwin_wayland >/dev/null 2>&1; then
    kwin_wayland --virtual --socket danplay-smoke --no-lockscreen --no-global-shortcuts \
        >"$SANDBOX/display.log" 2>&1 &
    DISPLAY_PID=$!; export WAYLAND_DISPLAY=danplay-smoke; unset DISPLAY
    i=0; while [ $i -lt 20 ] && [ ! -S "$XDG_RUNTIME_DIR/danplay-smoke" ]; do i=$((i+1)); sleep 0.5; done
else
    echo "  --    sin Xvfb ni kwin_wayland: la ventana saldra en tu pantalla"
fi

"$APP" >"$SANDBOX/app.log" 2>&1 &
PID=$!
LOG=""
i=0; while [ $i -lt 40 ]; do
    LOG=$(find "$SANDBOX" -name 'danplay.log' -path '*logs*' 2>/dev/null | head -1)
    [ -n "$LOG" ] && grep -q 'el nucleo contesta' "$LOG" && break
    alive "$PID" || break
    i=$((i+1)); sleep 0.5
done

alive "$PID" && pass "la app arranca (pid $PID)" \
  || { fail "la app no arranco: $(tail -5 "$SANDBOX/app.log")"; exit 1; }
[ -n "$LOG" ] && grep -q 'el nucleo contesta' "$LOG" \
  && pass "el nucleo contesta dentro de la app" || fail "el nucleo no contesto (registro: ${LOG:-ninguno})"
i=0; while [ $i -lt 20 ] && ! grep -q 'ONNX Runtime:' "$LOG" 2>/dev/null; do i=$((i+1)); sleep 0.5; done
found=$(grep 'ONNX Runtime:' "$LOG" 2>/dev/null | tail -1)
echo "        $(printf '%s' "$found" | sed 's/.*datos de la app/datos de la app/')"
printf '%s' "$found" | grep -q 'ONNX Runtime: [^n;]' && pass "el nucleo encuentra ONNX Runtime" \
  || fail "el nucleo no encuentra ONNX Runtime"
printf '%s' "$found" | grep -q 'ffmpeg: [^n;]' && pass "y ffmpeg" || fail "el nucleo no encuentra ffmpeg"
[ -f "$DANPLAY_DATA_DIR/danplay.db" ] && pass "el indice se crea en su carpeta de datos" \
  || fail "no hay $DANPLAY_DATA_DIR/danplay.db"

# Sin nucleo aparte: ni proceso, ni socket, ni puerto
children=$(pgrep -P "$PID" -a 2>/dev/null)
printf '%s\n' "$children" | grep -qiE 'danplay-core|python' \
  && fail "lanza un nucleo aparte: $children" || pass "no lanza ningun nucleo aparte"
[ -z "$(find "$XDG_RUNTIME_DIR" "$HOME" -name '*.sock' -path '*danplay*' 2>/dev/null)" ] \
  && pass "no crea ningun socket" || fail "hay un socket de danplay"
ss -ltnpH 2>/dev/null | grep -q "pid=$PID," && fail "escucha en un puerto TCP" \
  || pass "no abre ningun puerto TCP"

# Una SEGUNDA instancia: es lo que pasa al abrir una cancion con DanPlay ya
# abierto. Debe pasarle los argumentos a la primera e irse sin tocar nada de
# ella (con el nucleo de Python se llevaba su socket; ahora serian dos
# nucleos sobre la misma base).
timeout 10 "$APP" >/dev/null 2>&1
[ $? -ne 124 ] && pass "una segunda instancia se va y deja a la primera" \
  || fail "la segunda instancia no se fue"
alive "$PID" && pass "y la primera sigue viva" || fail "la primera se cerro"

# Al morir la app no queda nada suyo: ni sus procesos de WebKit, ni el pulso
# o el separador si estaban en marcha
KIDS=$(pgrep -P "$PID")
kill -9 "$PID" 2>/dev/null
left=""
i=0; while [ $i -lt 16 ]; do
    left=""
    for kid in $KIDS; do alive "$kid" && left="$left $kid"; done
    [ -z "$left" ] && break
    i=$((i+1)); sleep 0.5
done
[ -z "$left" ] && pass "al cerrarla no queda ningun proceso suyo" \
  || fail "quedan procesos de la app:$left"
PID=""

echo
echo "  $passed correctas, $failed fallidas"
[ "$failed" -eq 0 ]
