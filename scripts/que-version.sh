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

rojo=$(printf '\033[31m'); verde=$(printf '\033[32m'); ambar=$(printf '\033[33m')
gris=$(printf '\033[90m'); fin=$(printf '\033[0m')

fecha () { [ -e "$1" ] && date -r "$1" +"%d-%b %H:%M" || echo "no existe"; }
epoca () { [ -e "$1" ] && date -r "$1" +%s || echo 0; }

APP=desktop/src-tauri/target/release/danplay-app
WEB=desktop/dist/index.html
NUCLEO=dist/core/danplay-core
# Sin la version en el nombre: al subirla, estas lineas se quedaban apuntando
# a un archivo que ya no existe y el aviso de «paquete viejo» dejaba de salir
# justo cuando mas falta hace.
PAQUETES=$(ls -1 dist/installers/*.AppImage dist/installers/*.deb 2>/dev/null)
APPIMAGE=$(printf '%s\n' "$PAQUETES" | grep '\.AppImage$' | head -1)
DEB=$(printf '%s\n' "$PAQUETES" | grep '\.deb$' | head -1)

echo "Cuando se genero cada capa"
printf "  %-34s %s\n" "interfaz (desktop/dist)"   "$(fecha $WEB)"
printf "  %-34s %s\n" "nucleo (dist/core)"        "$(fecha $NUCLEO)"
printf "  %-34s %s\n" "app de escritorio"         "$(fecha $APP)"
printf "  %-34s %s\n" ".AppImage"                 "$(fecha $APPIMAGE)"
printf "  %-34s %s\n" ".deb"                      "$(fecha $DEB)"

fallo=0
echo
echo "Comprobaciones"

# 1) la interfaz va dentro del binario: si es mas nueva, el binario esta viejo
if [ "$(epoca $WEB)" -gt "$(epoca $APP)" ]; then
    echo "  ${rojo}x${fin} la interfaz es mas nueva que la app."
    echo "    La interfaz va INCRUSTADA en el binario, asi que lo que ves"
    echo "    sigue siendo lo anterior. Ejecuta: ./scripts/build.sh"
    fallo=1
else
    echo "  ${verde}ok${fin} la app lleva la interfaz actual"
fi

# 2) los paquetes solo se rehacen con --package. Se miran TODOS los que haya:
#    si queda alguno de una version anterior, hay que decirlo.
for p in $PAQUETES; do
    [ -e "$p" ] || continue
    if [ "$(epoca "$p")" -lt "$(epoca $APP)" ]; then
        echo "  ${ambar}!${fin}  $(basename "$p") es mas viejo que la app."
        echo "    Si abres ese paquete veras una version anterior."
        echo "    Rehazlo con: ./scripts/build.sh --package"
        fallo=1
    fi
done

# 3) que el css que referencia el index sea el que esta dentro del binario
if [ -e "$APP" ] && [ -e "$WEB" ]; then
    css=$(grep -o 'assets/[^"]*\.css' "$WEB" | head -1)
    if [ -n "$css" ] && ! grep -aq "$css" "$APP"; then
        echo "  ${rojo}x${fin} el binario no contiene $css"
        echo "    Recompilalo: ./scripts/build.sh"
        fallo=1
    elif [ -n "$css" ]; then
        echo "  ${verde}ok${fin} el binario incrusta $css"
    fi
fi

echo
if [ "$fallo" = "0" ]; then
    echo "${verde}Todo al dia.${fin} Abrela con ./danplay-app.sh"
else
    echo "${ambar}Hay algo desfasado${fin} (mira arriba)."
fi
echo "${gris}Y cierra la app antes de reabrirla: una ventana ya abierta${fin}"
echo "${gris}sigue con la interfaz que cargo al arrancar.${fin}"
exit "$fallo"
