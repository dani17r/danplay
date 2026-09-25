#!/usr/bin/env python3
"""Regenera la foto del catalogo de modelos que viaja dentro de la app.

    ./.venv/bin/python scripts/actualizar-modelos.py            la del repositorio
    ./.venv/bin/python scripts/actualizar-modelos.py OTRA.json  en otro sitio

La app consulta models.dev en vivo cada vez que se abre el apartado de IA,
asi que esta foto solo sirve para el primer arranque sin internet. Aun asi,
cada compilacion (scripts/build.sh) saca una al dia para que ningun paquete
salga con una lista de hace meses: la escribe en su carpeta temporal y se la
pasa a PyInstaller (DANPLAY_SNAPSHOT), sin tocar la del repositorio, que
antes se reescribia en cada compilacion y dejaba el arbol sucio. Solo lleva
los proveedores del catalogo de DanPlay: la base entera son 4,6 MB y aqui
sobra la mitad.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from danplay import model_catalog, providers

destino = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else model_catalog.SNAPSHOT
wanted = {p["models_dev"] for p in providers.CATALOG if p["models_dev"]}
try:
    n = model_catalog.build_snapshot(path=destino, only=wanted)
except Exception as e:  # noqa: BLE001
    print(f"no se pudo descargar models.dev ({e}); se conserva la foto anterior")
    sys.exit(0 if model_catalog.SNAPSHOT.is_file() else 1)
size = destino.stat().st_size // 1024
donde = destino.relative_to(Path.cwd()) if destino.is_relative_to(Path.cwd()) else destino
print(f"foto actualizada: {n} modelos de {len(wanted)} proveedores, {size} KB en {donde}")
