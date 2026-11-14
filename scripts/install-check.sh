#!/bin/sh
# Lo que se comprueba DENTRO de un sistema recien hecho (lo lanza
# `scripts/test-install.sh` en un contenedor, como root): instalar el .deb
# como lo haria cualquiera y mirar que no le falte nada para sonar.
#
#   /tmp/danplay.deb   el paquete (montado por quien lo lanza)
#
# Sale con 0 si todo esta, y con 1 diciendo cada cosa que falta. Nace de un
# fallo de verdad: tras reinstalar el sistema, DanPlay «sonaba» sin error y
# no se oia nada, porque el paquete no pedia el puente de ALSA al servidor de
# sonido y nadie lo decia.
set -u
export DEBIAN_FRONTEND=noninteractive
FAILED=0
ok () { printf '  ok     %s\n' "$1"; }
bad () { printf '  FALTA  %s\n' "$1"; FAILED=1; }

echo "== instalar"
apt-get update -qq >/dev/null 2>&1 || { echo "sin red para apt-get update"; exit 1; }
# con apt, que es quien resuelve lo que pide el paquete (dpkg -i no lo hace)
if ! apt-get install -y -qq /tmp/danplay.deb >/tmp/install.log 2>&1; then
    tail -20 /tmp/install.log
    echo "  FALTA  apt no pudo instalar el paquete"
    exit 1
fi
ok "apt install ./DanPlay.deb"

APP=/usr/bin/danplay-app
TOOLS=/usr/lib/DanPlay/tools
DATA=/usr/lib/DanPlay/data

echo "== bibliotecas"
for binary in "$APP" "$TOOLS/ffmpeg" "$TOOLS/fpcalc" "$TOOLS/qjs" "$DATA"/libonnxruntime.so*; do
    [ -e "$binary" ] || { bad "$binary no esta en el paquete"; continue; }
    # un programa enlazado entero (estatico) no tiene nada que buscar
    missing=$(ldd "$binary" 2>/dev/null | grep 'not found' || true)
    if [ -n "$missing" ]; then
        bad "$(basename "$binary") no encuentra: $(printf '%s' "$missing" | awk '{print $1}' | tr '\n' ' ')"
    else
        ok "$(basename "$binary") tiene todas sus bibliotecas"
    fi
done

echo "== el puente de sonido (ALSA -> PipeWire o PulseAudio)"
if dpkg -s pipewire-alsa >/dev/null 2>&1; then
    ok "pipewire-alsa instalado"
elif dpkg -s pulseaudio >/dev/null 2>&1; then
    ok "pulseaudio instalado"
else
    bad "ni pipewire-alsa ni pulseaudio: con PipeWire no sonaria nada"
fi
# y que ALSA lo conozca por su nombre, que es como lo abre el motor
if grep -rqsE '^pcm\.(!?default|pipewire|pulse)' /usr/share/alsa/alsa.conf.d /etc/alsa/conf.d 2>/dev/null; then
    ok "ALSA tiene configurado el PCM del servidor de sonido"
else
    bad "ALSA no tiene el PCM pipewire ni pulse"
fi

echo "== los programas que viajan dentro"
"$TOOLS/ffmpeg" -hide_banner -version >/dev/null 2>&1 && ok "ffmpeg contesta" || bad "ffmpeg no arranca"
"$TOOLS/fpcalc" -version >/dev/null 2>&1 && ok "fpcalc contesta" || bad "fpcalc no arranca"
echo 'console.log(1+1)' >/tmp/dos.js
[ "$("$TOOLS/qjs" /tmp/dos.js 2>/dev/null)" = "2" ] && ok "qjs contesta" || bad "qjs no arranca"
"$TOOLS/yt-dlp" --version >/dev/null 2>&1 && ok "yt-dlp contesta" || bad "yt-dlp no arranca"

echo "== abrir un .danpista desde el explorador"
# por dentro es un ZIP: sin su tipo, el escritorio lo abre con el gestor de
# archivos comprimidos. Se mira con un ZIP de verdad (su firma, y vacio).
if apt-get install -y -qq xdg-utils >/dev/null 2>&1; then
    printf 'PK\005\006\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0' >/tmp/cancion.danpista
    kind=$(xdg-mime query filetype /tmp/cancion.danpista 2>/dev/null)
    if [ "$kind" = "application/x-danpista" ]; then
        ok "un .danpista es $kind"
    else
        bad "un .danpista se toma por «$kind», no por application/x-danpista"
    fi
    opener=$(grep -ls 'application/x-danpista' /usr/share/applications/*.desktop 2>/dev/null | head -1)
    [ -n "$opener" ] && ok "lo abre $(basename "$opener")" || bad "ningun .desktop dice abrir application/x-danpista"
else
    echo "  (sin xdg-utils: no se mira)"
fi

echo "== la app"
# el detector de pulso es la propia app con --pulse: sin argumentos dice
# como se usa y sale con 2. Con eso ya cargo todas sus bibliotecas.
"$APP" --pulse >/dev/null 2>&1
[ $? -eq 2 ] && ok "danplay-app arranca (--pulse)" || bad "danplay-app no arranca"

# y entera, con una pantalla de mentira: que el nucleo conteste
if apt-get install -y -qq xvfb xauth dbus-x11 >/dev/null 2>&1; then
    useradd -m prueba 2>/dev/null
    su prueba -c "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 \
        dbus-run-session -- xvfb-run -a $APP >/tmp/app.log 2>&1 &"
    LOG=/home/prueba/.local/share/local.danplay.app/logs/danplay.log
    answered=0
    for _ in $(seq 1 60); do
        if grep -qs 'el nucleo contesta' "$LOG"; then
            answered=1
            break
        fi
        sleep 1
    done
    pkill -f "$APP" 2>/dev/null
    if [ "$answered" -eq 1 ]; then
        ok "la app se abre y el nucleo contesta"
        # lo que dice del sonido al arrancar, para verlo
        grep -sE 'sonido|puente|salida' "$LOG" | sed 's/^/         /' | head -5
    else
        bad "la app no llego a contestar en un minuto"
        tail -15 "$LOG" 2>/dev/null | sed 's/^/         /'
        tail -15 /tmp/app.log 2>/dev/null | sed 's/^/         /'
    fi
else
    echo "  (sin xvfb: no se prueba a abrir la ventana)"
fi

exit "$FAILED"
