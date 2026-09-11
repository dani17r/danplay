#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Regenera la foto del catalogo de modelos que viaja dentro de la app.

    ./.venv/bin/python scripts/actualizar-modelos.py

La app consulta models.dev en vivo cada vez que se abre el apartado de IA,
asi que esta foto solo sirve para el primer arranque sin internet. Aun asi,
se regenera en cada compilacion (scripts/build.sh la llama) para que ningun
paquete salga con una lista de hace meses. Solo lleva los proveedores del
catalogo de DanPlay: la base entera son 4,6 MB y aqui sobra la mitad.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from danplay import model_catalog, providers  # noqa: E402

wanted = {p["models_dev"] for p in providers.CATALOG if p["models_dev"]}
try:
    n = model_catalog.build_snapshot(only=wanted)
except Exception as e:                                       # noqa: BLE001
    print(f"no se pudo descargar models.dev ({e}); se conserva la foto anterior")
    sys.exit(0 if model_catalog.SNAPSHOT.is_file() else 1)
size = model_catalog.SNAPSHOT.stat().st_size // 1024
print(f"foto actualizada: {n} modelos de {len(wanted)} proveedores, {size} KB "
      f"en {model_catalog.SNAPSHOT.relative_to(Path.cwd()) if model_catalog.SNAPSHOT.is_relative_to(Path.cwd()) else model_catalog.SNAPSHOT}")
