#!/bin/sh
# Sube el numero de version en TODOS los sitios donde vive, de una vez.
#
#   ./scripts/subir-version.sh 1.2.0
#
# La version esta escrita en ocho archivos (Python, dos crates de Rust, el
# empaquetador, la interfaz, sus cerrojos y la insignia del README). Subirla a
# mano en unos y no en otros es lo que pasaba; hay una prueba que falla si no
# coinciden (test_all_the_pieces_carry_the_same_version), pero mejor no darle
# la ocasion.
#
# Regla de la casa: CADA cambio que se entrega sube la version (ver
# docs/CONTRIBUIR.md, «La version»). Sin subirla, `apt install` con el mismo
# .deb no hace nada y Windows enseña el mismo numero antes y despues.
set -eu
cd "$(dirname "$0")/.."

nueva="${1:-}"
case "$nueva" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "uso: $0 X.Y.Z" >&2; exit 2 ;;
esac

actual=$(sed -n 's/^__version__ = "\([^"]*\)"/\1/p' danplay/__init__.py)
[ -n "$actual" ] || { echo "no encuentro la version actual en danplay/__init__.py" >&2; exit 1; }
if [ "$nueva" = "$actual" ]; then
    echo "ya esta en $actual" >&2; exit 1
fi

# Solo la linea que toca en cada archivo: la version del paquete, nunca la de
# una dependencia que casualmente lleve el mismo numero.
sed -i "s/^__version__ = \"$actual\"/__version__ = \"$nueva\"/" danplay/__init__.py
sed -i "0,/^version = \"$actual\"/s//version = \"$nueva\"/" pyproject.toml
sed -i "0,/^version = \"$actual\"/s//version = \"$nueva\"/" core/pyproject.toml
sed -i "0,/^version = \"$actual\"/s//version = \"$nueva\"/" core/Cargo.toml
sed -i "0,/^version = \"$actual\"/s//version = \"$nueva\"/" desktop/src-tauri/Cargo.toml
sed -i "0,/\"version\": \"$actual\"/s//\"version\": \"$nueva\"/" desktop/src-tauri/tauri.conf.json
sed -i "0,/\"version\": \"$actual\"/s//\"version\": \"$nueva\"/" desktop/package.json
sed -i "s|badge/version-$actual-|badge/version-$nueva-|" README.md

# Los cerrojos de Cargo llevan la version del propio crate: si no se tocan,
# el siguiente `cargo build` los reescribe y el arbol queda sucio.
for lock in core/Cargo.lock desktop/src-tauri/Cargo.lock; do
    [ -f "$lock" ] || continue
    awk -v a="$actual" -v n="$nueva" '
        /^name = "danplay/ { mine = 1; print; next }
        mine && /^version = / { sub("\"" a "\"", "\"" n "\""); mine = 0 }
        { print }' "$lock" > "$lock.tmp" && mv "$lock.tmp" "$lock"
done
# y el de npm, que repite la version dos veces (raiz y paquete "")
if [ -f desktop/package-lock.json ]; then
    python3 - "$nueva" <<'EOF'
import json, sys, pathlib
p = pathlib.Path("desktop/package-lock.json")
d = json.loads(p.read_text(encoding="utf-8"))
d["version"] = sys.argv[1]
if "" in d.get("packages", {}):
    d["packages"][""]["version"] = sys.argv[1]
p.write_text(json.dumps(d, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
EOF
fi

echo "version: $actual -> $nueva"
grep -rn --include='*.toml' --include='*.json' --include='*.py' --include='README.md' \
    -e "\"$nueva\"" -e "version-$nueva-" \
    danplay/__init__.py pyproject.toml core/pyproject.toml core/Cargo.toml \
    desktop/src-tauri/Cargo.toml desktop/src-tauri/tauri.conf.json desktop/package.json README.md \
    | sed 's/^/  /'
echo
echo "Ahora: ./scripts/test.sh --rapido, commit propio («chore: subir a $nueva») y reconstruir los paquetes."
