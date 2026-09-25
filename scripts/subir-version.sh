#!/bin/sh
# Sube el numero de version en TODOS los sitios donde vive, de una vez.
#
#   ./scripts/subir-version.sh 1.2.0
#
# La version se escribe en tres sitios: el nucleo Python (danplay/__init__.py,
# de donde la lee pyproject.toml), el workspace de Cargo (Cargo.toml de la
# raiz, que heredan los dos crates; Tauri y maturin la toman de ahi) y la
# interfaz (desktop/package.json). Ademas la repiten los cerrojos y la
# insignia del README. Hay una prueba que falla si no coinciden
# (test_all_the_pieces_carry_the_same_version), pero mejor no darle la ocasion.
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
sed -i "0,/^version = \"$actual\"/s//version = \"$nueva\"/" Cargo.toml
sed -i "0,/\"version\": \"$actual\"/s//\"version\": \"$nueva\"/" desktop/package.json
sed -i "s|badge/version-$actual-|badge/version-$nueva-|" README.md

# El cerrojo de Cargo lleva la version de los dos crates del workspace: si no
# se toca, el siguiente `cargo build` lo reescribe y el arbol queda sucio.
if [ -f Cargo.lock ]; then
    awk -v a="$actual" -v n="$nueva" '
        /^name = "danplay/ { mine = 1; print; next }
        mine && /^version = / { sub("\"" a "\"", "\"" n "\""); mine = 0 }
        { print }' Cargo.lock > Cargo.lock.tmp && mv Cargo.lock.tmp Cargo.lock
fi
# el de npm, que repite la version dos veces (raiz y paquete "")
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
# y el de uv, que apunta la version del propio paquete y la del crate
if command -v uv >/dev/null 2>&1 && [ -f uv.lock ]; then
    uv lock --quiet || echo "  (uv lock no pudo: ejecutalo tu antes del commit)" >&2
else
    echo "  sin uv: ejecuta «uv lock» antes del commit para poner al dia uv.lock" >&2
fi

echo "version: $actual -> $nueva"
grep -n -e "\"$nueva\"" -e "version-$nueva-" \
    danplay/__init__.py Cargo.toml desktop/package.json README.md | sed 's/^/  /'
echo
echo "Ahora: ./scripts/test.sh --rapido, commit propio («chore: subir a $nueva»),"
echo "y para publicarla: git tag v$nueva && git push --tags (la CI construye los paquetes)."
