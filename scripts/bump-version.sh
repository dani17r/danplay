#!/bin/sh
# Sube el numero de version en TODOS los sitios donde vive, de una vez.
#
#   ./scripts/bump-version.sh 1.2.0
#
# La version se escribe en dos sitios: el workspace de Cargo (Cargo.toml de
# la raiz, que heredan todos los crates; Tauri la toma de ahi) y la interfaz
# (desktop/package.json). Ademas la repiten el cerrojo de Cargo y la
# insignia del README. Hay una prueba que falla si no coinciden
# (test_all_the_pieces_carry_the_same_version), pero mejor no darle la ocasion.
#
# Regla de la casa: CADA cambio que se entrega sube la version (ver
# docs/CONTRIBUTING.md, «La version»). Sin subirla, `apt install` con el mismo
# .deb no hace nada y Windows enseña el mismo numero antes y despues.
#
# De paso saca la foto del catalogo de modelos de IA de hoy
# (data/models-snapshot.json, la que viaja en los paquetes) con la app ya
# compilada. Antes la sacaba cada paquete al construirse: cada sistema podia
# llevar una distinta y el paquete no salia igual dos veces.
set -eu
cd "$(dirname "$0")/.."

new_version="${1:-}"
case "$new_version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "uso: $0 X.Y.Z" >&2; exit 2 ;;
esac

# la del workspace: la primera `version = ` de Cargo.toml
current=$(sed -n '0,/^version = "/s/^version = "\([^"]*\)"/\1/p' Cargo.toml)
[ -n "$current" ] || { echo "no encuentro la version actual en Cargo.toml" >&2; exit 1; }
if [ "$new_version" = "$current" ]; then
    echo "ya esta en $current" >&2; exit 1
fi

# Solo la linea que toca en cada archivo: la version del paquete, nunca la de
# una dependencia que casualmente lleve el mismo numero.
sed -i "0,/^version = \"$current\"/s//version = \"$new_version\"/" Cargo.toml
sed -i "0,/\"version\": \"$current\"/s//\"version\": \"$new_version\"/" desktop/package.json
sed -i "s|badge/version-$current-|badge/version-$new_version-|" README.md

# El cerrojo de Cargo lleva la version de los crates del workspace: si no
# se toca, el siguiente `cargo build` lo reescribe y el arbol queda sucio.
if [ -f Cargo.lock ]; then
    awk -v a="$current" -v n="$new_version" '
        /^name = "danplay/ { mine = 1; print; next }
        mine && /^version = / { sub("\"" a "\"", "\"" n "\""); mine = 0 }
        { print }' Cargo.lock > Cargo.lock.tmp && mv Cargo.lock.tmp Cargo.lock
fi
# (el de Bun, desktop/bun.lock, no apunta la version de la interfaz: nada que
# tocar. El package-lock.json de npm si la llevaba, y se fue con npm.)

echo "version: $current -> $new_version"
grep -n -e "\"$new_version\"" -e "version-$new_version-" \
    Cargo.toml desktop/package.json README.md | sed 's/^/  /'

# La foto del catalogo: `--models-snapshot` lo entienden el nucleo suelto y la
# app (que lo lleva dentro); el que haya, de lo compilado. Sin red, o sin
# nada compilado, se queda la de antes y se dice.
target="${CARGO_TARGET_DIR:-target}"
snapshot=""
for tool in "$target/release/danplay-server" "$target/ci/danplay-server" "$target/release/danplay-app"; do
    [ -x "$tool" ] && { snapshot=$tool; break; }
done
echo
if [ -z "$snapshot" ]; then
    echo "AVISO: sin nada compilado no saco la foto del catalogo de modelos; despues:"
    echo "  target/release/danplay-server --models-snapshot data/models-snapshot.json"
elif "$snapshot" --models-snapshot data/models-snapshot.json; then
    echo "foto del catalogo de modelos al dia (data/models-snapshot.json): va en el mismo commit"
else
    echo "AVISO: no pude sacar la foto del catalogo de modelos (¿sin red?): se queda la de antes"
fi

# Lo que no toca este guion y tiene que llevar la version nueva: la prueba de
# «Acerca de» (desktop/tests/about.test.js) lo caza, pero tarde
if ! grep -q "^## $new_version " CHANGELOG.md; then
    echo
    echo "AVISO: falta la entrada de $new_version en CHANGELOG.md y en desktop/src/data/releases.js"
fi
echo
echo "Ahora: ./scripts/test.sh --quick, commit propio («chore: subir a $new_version»),"
echo "y para publicarla, una etiqueta anotada (la CI construye los paquetes):"
echo "  git tag -a v$new_version -m $new_version && git push --tags"
