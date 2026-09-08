# -*- coding: utf-8 -*-
"""Fixtures compartidas.

Audio sintetico: las pruebas no deben depender de la musica de nadie. Con
ffmpeg se generan mp3 de un segundo de silencio, con etiquetas ID3 escritas
por mutagen, y con eso se monta una biblioteca temporal completa.
"""
import os, pathlib, shutil, subprocess, sys
import pytest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

FFMPEG = shutil.which("ffmpeg")


def make_mp3(path, artist="", title="", album="", seconds=1.0, **extra):
    """Crea un mp3 real (silencio) en `path` con las etiquetas dadas."""
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([FFMPEG, "-y", "-loglevel", "error", "-f", "lavfi",
                    "-i", f"anullsrc=r=44100:cl=mono", "-t", str(seconds),
                    "-codec:a", "libmp3lame", "-b:a", "64k", str(path)],
                   check=True, capture_output=True, timeout=60)
    if artist or title or album or extra:
        from mutagen.id3 import ID3, TPE1, TIT2, TALB, ID3NoHeaderError
        try:
            id3 = ID3(str(path))
        except ID3NoHeaderError:
            id3 = ID3()
        if artist:
            id3.add(TPE1(encoding=3, text=artist))
        if title:
            id3.add(TIT2(encoding=3, text=title))
        if album:
            id3.add(TALB(encoding=3, text=album))
        id3.save(str(path))
    return str(path)


@pytest.fixture(scope="session")
def synthetic_ok():
    if not FFMPEG:
        pytest.skip("hace falta ffmpeg para generar audio de prueba")
    return True


@pytest.fixture
def synthetic_library(tmp_path, synthetic_ok):
    """Una biblioteca temporal con dos artistas y tres canciones sinteticas.

    Devuelve (raiz, {nombre: ruta}). No toca la configuracion global: cada
    prueba decide si apunta `config` a ella.
    """
    lib = tmp_path / "Musica"
    songs = {
        "gozo": make_mp3(lib / "Artistas/Barak/Barak - Mi Gozo.mp3",
                         artist="Barak", title="Mi Gozo", album="Gozo"),
        "tierra": make_mp3(lib / "Artistas/Barak/Barak - Sera Llena La Tierra.mp3",
                           artist="Barak", title="Sera Llena La Tierra", album="Gozo"),
        "shekinah": make_mp3(lib / "Artistas/New Wine/New Wine - Shekinah.mp3",
                             artist="New Wine", title="Shekinah", album="Libertad"),
    }
    for sub in ("Entrada", "Revisar", "Secuencias"):
        (lib / sub).mkdir(parents=True, exist_ok=True)
    return lib, songs


@pytest.fixture
def configured_library(synthetic_library, tmp_path, monkeypatch):
    """La biblioteca sintetica ya apuntada en `config`, con base de datos propia."""
    from danplay import config
    lib, songs = synthetic_library
    monkeypatch.setattr(config, "LIBRARY", lib)
    monkeypatch.setattr(config, "INBOX", lib / "Entrada")
    monkeypatch.setattr(config, "ARTISTS_DIR", lib / "Artistas")
    monkeypatch.setattr(config, "REVIEW_DIR", lib / "Revisar")
    monkeypatch.setattr(config, "DATABASE", tmp_path / "danplay.db")
    monkeypatch.setattr(config, "WRITE_TAGS", True)
    monkeypatch.setattr(config, "AI_ENABLED", False)
    return lib, songs
