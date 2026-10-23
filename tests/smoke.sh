#!/bin/sh
# Prueba de humo sobre la aplicacion de verdad: la compilada
# (target/release/danplay-app), el usr/bin/danplay-app de un .deb extraido
# (`dpkg-deb -x`) o el propio .AppImage. La arranca en una caja de arena,
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

# Los que siguen vivos con este entorno (lo que el bus de la prueba arranco
# por su cuenta, como los portales del escritorio)
strays () {
    for p in /proc/[0-9]*; do
        { tr '\0' '\n' < "$p/environ"; } 2>/dev/null | grep -qx "DANPLAY_SMOKE_SANDBOX=$1" && echo "${p#/proc/}"
    done
}

# Lo primero, la caja de arena, y otro bus de D-Bus que ya nazca dentro de
# ella: lo que el bus arranque por su cuenta (un portal del escritorio) tiene
# que ver la casa y la carpeta de ejecucion de la prueba, no las tuyas.
if [ -z "${DANPLAY_SMOKE_SANDBOX:-}" ]; then
    # la de tu sesion, por si no hay otra (con la ruta entera: la carpeta de
    # ejecucion de la prueba es otra)
    SMOKE_DISPLAY=${DISPLAY:-}
    case "${WAYLAND_DISPLAY:-}" in
        "") SMOKE_WAYLAND="" ;;
        /*) SMOKE_WAYLAND=$WAYLAND_DISPLAY ;;
        *) SMOKE_WAYLAND="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$WAYLAND_DISPLAY" ;;
    esac
    export SMOKE_DISPLAY SMOKE_WAYLAND
    SANDBOX=$(mktemp -d)
    mkdir -p "$SANDBOX/home" "$SANDBOX/run" "$SANDBOX/Musica/Artistas/Prueba"
    chmod 700 "$SANDBOX/run"
    export DANPLAY_SMOKE_SANDBOX="$SANDBOX"
    export HOME="$SANDBOX/home" XDG_RUNTIME_DIR="$SANDBOX/run"
    export XDG_CONFIG_HOME="$SANDBOX/config" XDG_DATA_HOME="$SANDBOX/share"
    export XDG_CACHE_HOME="$SANDBOX/cache" XDG_STATE_HOME="$SANDBOX/state"
    export DANPLAY_DATA_DIR="$SANDBOX/data" DANPLAY_CONFIG_DIR="$SANDBOX/danplay-config"
    export DANPLAY_LIBRARY="$SANDBOX/Musica" DANPLAY_PROJECT_ENV=0
    unset DANPLAY_TOOLS_DIR DANPLAY_ONNXRUNTIME ORT_DYLIB_PATH DANPLAY_RESOURCES DBUS_SESSION_BUS_ADDRESS
    # Tu pantalla, apartada: la prueba usa una suya si puede (abajo). Con
    # DISPLAY puesto, kwin se quedaba colgado hablando con la tuya.
    unset DISPLAY WAYLAND_DISPLAY
    # Y un entorno limpio de lo que dejan otras apps: el VS Code de snap pone
    # sus GIO_MODULE_DIR, GTK_PATH, LOCPATH, LD_LIBRARY_PATH..., y con ellos
    # GTK no arranca; las carpetas del sistema, sin las de tu casa.
    for v in $(env | sed -n 's/^\(SNAP[A-Z_]*\|GIO_[A-Z_]*\|GSETTINGS_[A-Z_]*\|GTK[A-Z0-9_]*\|GDK_[A-Z_]*\|LOCPATH\|LD_LIBRARY_PATH\|LD_PRELOAD\|XDG_[A-Z_]*_VSCODE_SNAP_ORIG\)=.*/\1/p'); do
        unset "$v"
    done
    export XDG_DATA_DIRS=/usr/local/share:/usr/share XDG_CONFIG_DIRS=/etc/xdg
    if command -v dbus-run-session >/dev/null 2>&1; then
        dbus-run-session -- "$0" "$APP"
        code=$?
        left=$(strays "$SANDBOX")
        [ -n "$left" ] && kill $left 2>/dev/null
        rm -rf "$SANDBOX"
        exit "$code"
    fi
    echo "  --    sin dbus-run-session: con el bus de tu sesion (cierra DanPlay antes)"
    OWN_SANDBOX=1
fi
SANDBOX=$DANPLAY_SMOKE_SANDBOX

passed=0; failed=0
pass () { echo "  ok    $1"; passed=$((passed+1)); }
fail () { echo "  FALLA $1"; failed=$((failed+1)); }
# vivo de verdad: un proceso que ya acabo y nadie ha recogido (zombi) no cuenta
alive () { s=$(ps -o stat= -p "$1" 2>/dev/null); [ -n "$s" ] && [ "${s#Z}" = "$s" ]; }
# un proceso y todos los que cuelgan de el
tree () { echo "$1"; for c in $(pgrep -P "$1"); do tree "$c"; done; }

cleanup () {
    [ -n "${PID:-}" ] && kill -9 $(tree "$PID") 2>/dev/null
    [ -n "${DISPLAY_PID:-}" ] && kill "$DISPLAY_PID" 2>/dev/null
    [ -n "${MOUNT_PID:-}" ] && kill "$MOUNT_PID" 2>/dev/null && sleep 1
    [ -n "${OWN_SANDBOX:-}" ] && rm -rf "$SANDBOX"
}
trap cleanup EXIT

# Lo que viaja con la app, donde lo deja cada paquete (o la compilacion). Del
# AppImage se mira dentro, montado (sin extraer nada).
BIN=$(dirname "$APP")
case "$APP" in
    *.AppImage)
        "$APP" --appimage-mount >"$SANDBOX/mount" 2>&1 &
        MOUNT_PID=$!
        i=0; while [ $i -lt 20 ] && [ ! -s "$SANDBOX/mount" ]; do i=$((i+1)); sleep 0.5; done
        BIN="$(head -1 "$SANDBOX/mount")/usr/bin"
        ;;
esac
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
# nucleo: con el grafo y ONNX Runtime que viajan con ella (sin decirle donde
# esta ONNX Runtime: lo tiene que encontrar sola).
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
    # kwin tarda unos segundos en abrir su socket (carga su configuracion)
    i=0; while [ $i -lt 60 ] && [ ! -S "$XDG_RUNTIME_DIR/danplay-smoke" ]; do i=$((i+1)); sleep 0.5; done
    [ -S "$XDG_RUNTIME_DIR/danplay-smoke" ] || echo "  --    kwin_wayland no abrio su pantalla: $(tail -3 "$SANDBOX/display.log")"
else
    echo "  --    sin Xvfb ni kwin_wayland: la ventana saldra en tu pantalla"
    [ -n "$SMOKE_DISPLAY" ] && export DISPLAY="$SMOKE_DISPLAY"
    [ -n "$SMOKE_WAYLAND" ] && export WAYLAND_DISPLAY="$SMOKE_WAYLAND"
fi

"$APP" >"$SANDBOX/app.log" 2>&1 &
PID=$!
# la app de verdad (en el AppImage, el ejecutable que lanza su arranque)
APP_PID=""
LOG=""
i=0; while [ $i -lt 40 ]; do
    if [ -z "$APP_PID" ]; then
        for p in $(tree "$PID"); do
            [ "$(cat "/proc/$p/comm" 2>/dev/null)" = danplay-app ] && { APP_PID=$p; break; }
        done
    fi
    LOG=$(find "$SANDBOX" -name 'danplay.log' -path '*logs*' 2>/dev/null | head -1)
    [ -n "$LOG" ] && grep -q 'el nucleo contesta' "$LOG" && break
    alive "$PID" || break
    i=$((i+1)); sleep 0.5
done
[ -n "$APP_PID" ] || APP_PID=$PID

alive "$APP_PID" && pass "la app arranca (pid $APP_PID)" \
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
children=$(for p in $(tree "$APP_PID"); do cat "/proc/$p/comm" 2>/dev/null; done)
printf '%s\n' "$children" | grep -qiE 'danplay-core|python' \
  && fail "lanza un nucleo aparte: $children" || pass "no lanza ningun nucleo aparte"
[ -z "$(find "$XDG_RUNTIME_DIR" "$HOME" -name '*.sock' -path '*danplay*' 2>/dev/null)" ] \
  && pass "no crea ningun socket" || fail "hay un socket de danplay"
ss -ltnpH 2>/dev/null | grep -q "pid=$APP_PID," && fail "escucha en un puerto TCP" \
  || pass "no abre ningun puerto TCP"

# Una SEGUNDA instancia: es lo que pasa al abrir una cancion con DanPlay ya
# abierto. Debe pasarle los argumentos a la primera e irse sin tocar nada de
# ella (con el nucleo de Python se llevaba su socket; ahora serian dos
# nucleos sobre la misma base).
timeout 15 "$APP" >/dev/null 2>&1
[ $? -ne 124 ] && pass "una segunda instancia se va y deja a la primera" \
  || fail "la segunda instancia no se fue"
alive "$APP_PID" && pass "y la primera sigue viva" || fail "la primera se cerro"

# Al morir la app no queda nada suyo: ni sus procesos de WebKit, ni el pulso
# o el separador si estaban en marcha
KIDS=$(tree "$PID")
kill -9 "$APP_PID" 2>/dev/null
left=""
i=0; while [ $i -lt 16 ]; do
    left=""
    for kid in $KIDS; do alive "$kid" && left="$left $kid"; done
    [ -z "$left" ] && break
    i=$((i+1)); sleep 0.5
done
[ -z "$left" ] && pass "al cerrarla no queda ningun proceso suyo" \
  || fail "quedan procesos de la app:$(for p in $left; do printf ' %s' "$(cat "/proc/$p/comm" 2>/dev/null)"; done)"
PID=""

echo
echo "  $passed correctas, $failed fallidas"
[ "$failed" -eq 0 ]
