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
echo
echo "Ahora: ./scripts/test.sh --quick, commit propio («chore: subir a $new_version»),"
echo "y para publicarla: git tag v$new_version && git push --tags (la CI construye los paquetes)."
