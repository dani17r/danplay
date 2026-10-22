#!/bin/sh
# ¿La app que abres lleva los ultimos cambios?
#
# Existe mas de una forma de arrancar DanPlay (el binario de desarrollo, el
# .AppImage, el .deb) y es facil abrir una vieja sin darse cuenta: la
# interfaz va INCRUSTADA dentro del binario, asi que recompilar solo el
# frontend no cambia nada de lo que ves.
#
# Esto compara las fechas de las cuatro capas y avisa de lo que no cuadre.
set -u
cd "$(dirname "$0")/.."

red=$(printf '\033[31m'); green=$(printf '\033[32m'); amber=$(printf '\033[33m')
gray=$(printf '\033[90m'); reset=$(printf '\033[0m')

mtime () { [ -e "$1" ] && date -r "$1" +"%d-%b %H:%M" || echo "no existe"; }
epoch () { [ -e "$1" ] && date -r "$1" +%s || echo 0; }

APP=target/release/danplay-app
WEB=desktop/dist/index.html
CORE=dist/core/danplay-core/danplay-core
# Sin la version en el nombre: al subirla, estas lineas se quedaban apuntando
# a un archivo que ya no existe y el aviso de «paquete viejo» dejaba de salir
# justo cuando mas falta hace.
PACKAGES=$(ls -1 dist/installers/*.AppImage dist/installers/*.deb 2>/dev/null)
APPIMAGE=$(printf '%s\n' "$PACKAGES" | grep '\.AppImage$' | head -1)
DEB=$(printf '%s\n' "$PACKAGES" | grep '\.deb$' | head -1)

echo "Cuando se genero cada capa"
printf "  %-34s %s\n" "interfaz (desktop/dist)"   "$(mtime $WEB)"
printf "  %-34s %s\n" "nucleo (dist/core)"        "$(mtime $CORE)"
printf "  %-34s %s\n" "app de escritorio"         "$(mtime $APP)"
printf "  %-34s %s\n" ".AppImage"                 "$(mtime $APPIMAGE)"
printf "  %-34s %s\n" ".deb"                      "$(mtime $DEB)"

stale=0
echo
echo "Comprobaciones"

# 1) la interfaz va dentro del binario: si es mas nueva, el binario esta viejo
if [ "$(epoch $WEB)" -gt "$(epoch $APP)" ]; then
    echo "  ${red}x${reset} la interfaz es mas nueva que la app."
    echo "    La interfaz va INCRUSTADA en el binario, asi que lo que ves"
    echo "    sigue siendo lo anterior. Ejecuta: ./scripts/build.sh"
    stale=1
else
    echo "  ${green}ok${reset} la app lleva la interfaz actual"
fi

# 2) los paquetes solo se rehacen con --package. Se miran TODOS los que haya:
#    si queda alguno de una version anterior, hay que decirlo.
for p in $PACKAGES; do
    [ -e "$p" ] || continue
    if [ "$(epoch "$p")" -lt "$(epoch $APP)" ]; then
        echo "  ${amber}!${reset}  $(basename "$p") es mas viejo que la app."
        echo "    Si abres ese paquete veras una version anterior."
        echo "    Rehazlo con: ./scripts/build.sh --package"
        stale=1
    fi
done

# 3) que el css que referencia el index sea el que esta dentro del binario
if [ -e "$APP" ] && [ -e "$WEB" ]; then
    css=$(grep -o 'assets/[^"]*\.css' "$WEB" | head -1)
    if [ -n "$css" ] && ! grep -aq "$css" "$APP"; then
        echo "  ${red}x${reset} el binario no contiene $css"
        echo "    Recompilalo: ./scripts/build.sh"
        stale=1
    elif [ -n "$css" ]; then
        echo "  ${green}ok${reset} el binario incrusta $css"
    fi
fi

echo
if [ "$stale" = "0" ]; then
    echo "${green}Todo al dia.${reset} Abrela con ./danplay-app.sh"
else
    echo "${amber}Hay algo desfasado${reset} (mira arriba)."
fi
echo "${gray}Y cierra la app antes de reabrirla: una ventana ya abierta${reset}"
echo "${gray}sigue con la interfaz que cargo al arrancar.${reset}"
exit "$stale"
