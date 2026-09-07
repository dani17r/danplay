# -*- mode: python ; coding: utf-8 -*-
import os
from PyInstaller.utils.hooks import collect_submodules, collect_data_files

hidden = (collect_submodules('uvicorn') + collect_submodules('fastapi')
           + collect_submodules('yt_dlp')
           + collect_submodules('mutagen') + collect_submodules('anyio')
           + ['danplay_core', 'danplay.api', 'danplay.cli', 'danplay.library',
              'danplay.names', 'danplay.tags', 'danplay.playlists',
              'danplay.ingest', 'danplay.enrich', 'danplay.convert',
              'danplay.duplicates', 'danplay.fingerprint', 'danplay.ai',
              'danplay.theory', 'danplay.config', 'danplay.youtube',
              'danplay.web', 'acoustid', 'musicbrainzngs', 'rapidfuzz',
              'openai', 'dotenv', 'watchdog.observers', 'sqlite3',
              'uvicorn.logging', 'uvicorn.protocols.http.h11_impl',
              'uvicorn.protocols.websockets.websockets_impl',
              'uvicorn.lifespan.on', 'uvicorn.loops.asyncio'])

a = Analysis(['core_entry.py'],
             pathex=[os.path.abspath(os.path.join(SPECPATH, '..'))],
             binaries=[],
             datas=collect_data_files('certifi'),
             hiddenimports=hidden,
             excludes=['tkinter', 'matplotlib', 'PIL', 'numpy.distutils', 'pytest'],
             noarchive=False)
pyz = PYZ(a.pure)
exe = EXE(pyz, a.scripts, a.binaries, a.datas, [],
          name='danplay-core',
          debug=False, strip=False, upx=False, console=True,
          disable_windowed_traceback=False, argv_emulation=False)
