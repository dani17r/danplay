# -*- mode: python ; coding: utf-8 -*-
#
# El nucleo Python, empaquetado para viajar dentro del .deb, el .AppImage o
# el instalador de Windows.
#
# En una carpeta (onedir: `dist/core/danplay-core/`, el ejecutable y su
# `_internal`), no en un solo archivo. El onefile se descomprimia entero en
# un temporal cada vez que arrancaba: 224 MB y casi un segundo, en un /tmp que
# en Linux suele estar en la RAM, y otra vez en cada proceso del pulso y del
# separador. La carpeta viaja como recurso de Tauri (`nucleo/`, ver
# tauri.conf.json), no como `externalBin`, que solo sabe copiar un archivo.
import os
import sys
from PyInstaller.utils.hooks import (
    collect_data_files, collect_dynamic_libs, collect_submodules, copy_metadata,
)

ROOT = os.path.abspath(os.path.join(SPECPATH, '..'))
sys.path.insert(0, ROOT)          # para que collect_submodules encuentre `danplay`

hidden = (collect_submodules('uvicorn') + collect_submodules('fastapi')
          + collect_submodules('yt_dlp') + collect_submodules('yt_dlp_ejs')
          + collect_submodules('anyio')
          # el vigilante de carpetas: watchdog elige su observador (inotify,
          # Windows, macOS) al importarse, y sin esto podia no viajar
          + collect_submodules('watchdog')
          # TODO el nucleo, sin lista a mano: la de antes ya no tenia ni la
          # mitad de los modulos nuevos y solo se notaba dentro del .deb
          + collect_submodules('danplay')
          # el separador de pistas: la red corre en ONNX Runtime
          + collect_submodules('onnxruntime.capi')
          + ['danplay_core', 'acoustid', 'rapidfuzz', 'openai', 'dotenv', 'platformdirs',
             'send2trash', 'watchdog.observers', 'sqlite3', 'compileall',
             'uvicorn.logging', 'uvicorn.protocols.http.h11_impl',
             'uvicorn.lifespan.on', 'uvicorn.loops.asyncio'])

a = Analysis(['core_entry.py'],
             pathex=[ROOT],
             # La biblioteca de ONNX Runtime (libonnxruntime.so.*, .dylib,
             # onnxruntime.dll) en su sitio, onnxruntime/capi/: el pulso la
             # carga desde Rust (`pulse.onnxruntime_library`). El modulo de
             # Python de onnxruntime no depende de ella (en Linux lleva todo
             # dentro), asi que PyInstaller no la recogia y en el paquete el
             # clic se quedaba sin la red.
             binaries=collect_dynamic_libs('onnxruntime'),
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
                        'danplay/data')]
                    # los grafos del separador (sin pesos: se bajan al usarlo)
                    + [(os.path.join(ROOT, 'danplay', 'data', 'separador'),
                        'danplay/data/separador')]
                    # el detector de pulso (Beat This!, con sus pesos)
                    + [(os.path.join(ROOT, 'danplay', 'data', 'pulso'),
                        'danplay/data/pulso')]
                    # el catalogo de voces de las guias habladas (sin las
                    # voces: se bajan al elegirlas)
                    + [(os.path.join(ROOT, 'danplay', 'data', 'voces.json'),
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
             #
             # Y mutagen tampoco: es GPL, y las etiquetas las lee y escribe el
             # crate de Rust (danplay_core, con lofty). Se instala porque lo
             # pide yt-dlp[default], pero yt-dlp solo lo usaria para meter la
             # caratula en un m4a/ogg/opus, y DanPlay baja siempre mp3 (eso lo
             # hace con ffmpeg). Fuera tambien danplay.tags_mutagen, que es
             # para cuando no hay crate.
             excludes=['tkinter', 'matplotlib', 'PIL', 'numpy.distutils', 'pytest',
                       'musicbrainzngs', 'watchfiles', 'yaml', 'hypothesis',
                       'mutagen', 'danplay.tags_mutagen'],
             noarchive=False)
pyz = PYZ(a.pure)
exe = EXE(pyz, a.scripts, [],
          exclude_binaries=True,
          name='danplay-core',
          debug=False, strip=False, upx=False,
          # En Windows una consola negra detras de la aplicacion no pinta nada.
          console=sys.platform != 'win32',
          disable_windowed_traceback=False, argv_emulation=False)
coll = COLLECT(exe, a.binaries, a.datas, strip=False, upx=False, name='danplay-core')
