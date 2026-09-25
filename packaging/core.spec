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
from PyInstaller.utils.hooks import collect_data_files, collect_submodules, copy_metadata

ROOT = os.path.abspath(os.path.join(SPECPATH, '..'))
sys.path.insert(0, ROOT)          # para que collect_submodules encuentre `danplay`

hidden = (collect_submodules('uvicorn') + collect_submodules('fastapi')
          + collect_submodules('yt_dlp') + collect_submodules('yt_dlp_ejs')
          + collect_submodules('mutagen') + collect_submodules('anyio')
          # el vigilante de carpetas: watchdog elige su observador (inotify,
          # Windows, macOS) al importarse, y sin esto podia no viajar
          + collect_submodules('watchdog')
          # TODO el nucleo, sin lista a mano: la de antes ya no tenia ni la
          # mitad de los modulos nuevos y solo se notaba dentro del .deb
          + collect_submodules('danplay')
          + ['danplay_core', 'acoustid', 'rapidfuzz', 'openai', 'dotenv', 'platformdirs',
             'send2trash', 'watchdog.observers', 'sqlite3', 'compileall',
             'uvicorn.logging', 'uvicorn.protocols.http.h11_impl',
             'uvicorn.lifespan.on', 'uvicorn.loops.asyncio'])

a = Analysis(['core_entry.py'],
             pathex=[ROOT],
             binaries=[],
             # La foto del catalogo de modelos (models.dev) viaja dentro: sin
             # ella, el primer arranque sin internet no sabria ni un modelo.
             # Los .js de yt-dlp (sus retos de YouTube, ademas de los de
             # yt-dlp-ejs que ya recoge su propio gancho) y los metadatos de
             # yt-dlp: con ellos se sabe que version viaja con la app y si la
             # descargada al actualizar es mas nueva (danplay/ytdlp.py).
             datas=(collect_data_files('certifi')
                    + collect_data_files('yt_dlp', includes=['**/*.js'])
                    + copy_metadata('yt-dlp') + copy_metadata('yt-dlp-ejs')
                    # la del dia si scripts/build.sh la saco (DANPLAY_SNAPSHOT);
                    # si no, la del repositorio
                    + [(os.environ.get('DANPLAY_SNAPSHOT')
                        if os.path.isfile(os.environ.get('DANPLAY_SNAPSHOT', ''))
                        else os.path.join(ROOT, 'danplay', 'data', 'models-snapshot.json'),
                        'danplay/data')]),
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
                       'musicbrainzngs', 'watchfiles', 'yaml', 'hypothesis'],
             noarchive=False)
pyz = PYZ(a.pure)
exe = EXE(pyz, a.scripts, a.binaries, a.datas, [],
          name='danplay-core',
          debug=False, strip=False, upx=False,
          # En Windows una consola negra detras de la aplicacion no pinta nada.
          console=sys.platform != 'win32',
          disable_windowed_traceback=False, argv_emulation=False)
