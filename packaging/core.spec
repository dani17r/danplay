# -*- mode: python ; coding: utf-8 -*-
#
# El nucleo Python, empaquetado para viajar dentro del .deb, el .AppImage o
# el instalador de Windows.
#
# Sigue siendo **onefile**, y no por gusto: el empaquetador de Tauri copia UN
# archivo como sidecar (`externalBin`), asi que el modo onedir —un ejecutable
# con su carpeta `_internal` al lado— no llegaria entero al .deb ni al
# instalador. El precio es que descomprime en un temporal al arrancar; se
# mide en menos de un segundo y solo la primera vez de cada sesion.
import os
import sys
from PyInstaller.utils.hooks import collect_submodules, collect_data_files

hidden = (collect_submodules('uvicorn') + collect_submodules('fastapi')
          + collect_submodules('yt_dlp')
          + collect_submodules('mutagen') + collect_submodules('anyio')
          + ['danplay_core', 'danplay.api', 'danplay.cli', 'danplay.library',
             'danplay.names', 'danplay.tags', 'danplay.playlists',
             'danplay.ingest', 'danplay.enrich', 'danplay.convert',
             'danplay.duplicates', 'danplay.fingerprint', 'danplay.ai',
             'danplay.theory', 'danplay.config', 'danplay.youtube',
             'danplay.web', 'danplay.chat', 'acoustid', 'rapidfuzz',
             'openai', 'dotenv', 'platformdirs', 'send2trash',
             'watchdog.observers', 'sqlite3',
             'uvicorn.logging', 'uvicorn.protocols.http.h11_impl',
             'uvicorn.lifespan.on', 'uvicorn.loops.asyncio'])

a = Analysis(['core_entry.py'],
             pathex=[os.path.abspath(os.path.join(SPECPATH, '..'))],
             binaries=[],
             datas=collect_data_files('certifi'),
             hiddenimports=hidden,
             # `musicbrainzngs` no lo importa nadie y `watchfiles` es para
             # `--reload`, que aqui no se usa.
             #
             # OJO con `websockets`: parece que sobra (la API no tiene
             # ninguno) pero uvicorn lo importa al arrancar y yt-dlp lo pide
             # por su cuenta. Excluirlo dejaba el nucleo empaquetado sin
             # arrancar, con un ModuleNotFoundError que solo se veia dentro
             # del .deb y no ejecutando el codigo a mano.
             excludes=['tkinter', 'matplotlib', 'PIL', 'numpy.distutils', 'pytest',
                       'musicbrainzngs', 'watchfiles', 'yaml'],
             noarchive=False)
pyz = PYZ(a.pure)
exe = EXE(pyz, a.scripts, a.binaries, a.datas, [],
          name='danplay-core',
          debug=False, strip=False, upx=False,
          # En Windows una consola negra detras de la aplicacion no pinta nada.
          console=sys.platform != 'win32',
          disable_windowed_traceback=False, argv_emulation=False)
