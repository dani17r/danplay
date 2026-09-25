#!/usr/bin/env python3
"""Regenera requirements*.txt desde el cerrojo de uv (uv.lock).

    uv lock                                   # si cambiaste pyproject.toml
    ./.venv/bin/python scripts/exportar-requisitos.py

Las dependencias se declaran en pyproject.toml y se fijan en uv.lock; estos
archivos son para quien instala con pip (la CI de Windows, quien no use uv)
y llevan TODAS las versiones fijadas, tambien las que arrastran las
dependencias, que antes quedaban a lo que tocara ese dia:

  requirements.txt       lo que DanPlay necesita para funcionar (con hashes)
  requirements-test.txt  solo lo de las pruebas, sin el crate de Rust ni el
                         empaquetado (con hashes): la CI de Windows prueba el
                         nucleo como viaja en el instalador, sin Rust
  requirements-dev.txt   todo, con el crate de Rust editable (-e ./core); sin
                         hashes, porque pip no los admite con un editable

Con `--check` no escribe nada: falla si alguno no esta al dia.
"""

import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASE = ["export", "--format", "requirements-txt", "--frozen", "--no-emit-project", "--no-header"]
FILES = {
    "requirements.txt": (["--no-dev"], "Lo que DanPlay necesita para funcionar."),
    "requirements-test.txt": (
        ["--only-group", "test"],
        (
            "Solo lo de las pruebas (sin el crate de Rust ni el empaquetado):\n"
            "# pip install -r requirements.txt -r requirements-test.txt"
        ),
    ),
    "requirements-dev.txt": (
        ["--all-groups", "--no-hashes"],
        (
            "Para trabajar en DanPlay: todo, con el crate de Rust editable.\n"
            "# Sin hashes: pip no los admite junto a un editable (-e ./core)."
        ),
    ),
}


def uv() -> str:
    found = shutil.which("uv") or str(pathlib.Path.home() / ".local/bin/uv")
    if not pathlib.Path(found).exists():
        sys.exit("no encuentro uv: https://docs.astral.sh/uv/")
    return found


def render(name: str, args: list[str], what: str) -> str:
    out = subprocess.run(
        [uv(), *BASE, *args], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout
    command = " ".join(["uv", *BASE, *args, "-o", name])
    return (
        f"# {what}\n#\n"
        "# GENERADO desde uv.lock: no lo edites a mano. Para regenerarlo:\n"
        "#   ./.venv/bin/python scripts/exportar-requisitos.py\n"
        f"# (que ejecuta: {command})\n" + out
    )


def main() -> int:
    check = "--check" in sys.argv[1:]
    stale = []
    for name, (args, what) in FILES.items():
        text = render(name, args, what)
        path = ROOT / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                stale.append(name)
            continue
        path.write_text(text, encoding="utf-8")
        print(f"{name}: {sum(1 for line in text.splitlines() if '==' in line)} paquetes")
    if stale:
        print("no estan al dia con uv.lock: " + ", ".join(stale), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
