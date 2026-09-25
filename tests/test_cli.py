"""La linea de ordenes: cada orden hace lo suyo y, si falta un dato, lo dice
en castellano en vez de soltar una traza de Python."""

import shutil

import pytest
from conftest import make_mp3

from danplay import cli


@pytest.fixture
def lib(configured_library, tmp_path, monkeypatch):
    from danplay import config, library

    monkeypatch.setattr(config, "DATA_DIR", tmp_path / "datos")
    (tmp_path / "datos").mkdir()
    root, songs = configured_library
    library.add_folder(root)
    library.scan()
    return root, songs


def run(capsys, *argv) -> str:
    assert cli.main(list(argv)) == 0
    return capsys.readouterr().out


def test_estado_escanear_y_buscar(lib, capsys):
    out = run(capsys, "status")
    assert "Biblioteca" in out and "3 canciones" in out
    out = run(capsys, "scan")
    assert "3 canciones indexadas" in out
    out = run(capsys, "search", "artista:barak")
    assert "Mi Gozo" in out and "2 resultados" in out


@pytest.mark.parametrize(
    "argv",
    [
        ("folder", "add"),
        ("folder", "remove"),
        ("exclude", "add"),
        ("exclude", "remove"),
        ("playlist", "create"),
        ("playlist", "remove"),
        ("playlist", "show"),
        ("playlist", "export"),
    ],
)
def test_sin_el_dato_que_hace_falta_se_dice_y_ya(lib, capsys, argv):
    with pytest.raises(SystemExit) as e:
        cli.main(list(argv))
    assert "falta" in str(e.value)


def test_carpetas_y_exclusiones(lib, capsys, tmp_path):
    otra = tmp_path / "otra"
    otra.mkdir()
    assert "agregada" in run(capsys, "folder", "add", str(otra), "--label", "Otra")
    assert "Otra" in run(capsys, "folder")
    assert "no existe" in run(capsys, "folder", "add", str(tmp_path / "no-esta"))
    assert "quitada" in run(capsys, "folder", "remove", str(otra))
    assert "excluido" in run(capsys, "exclude", "add", "Secuencias", "--note", "pistas")
    assert "Secuencias" in run(capsys, "exclude")
    assert "quitada" in run(capsys, "exclude", "remove", "Secuencias")


def test_listas_por_id_o_por_nombre(lib, capsys):
    from danplay import library, playlists

    assert "creada" in run(capsys, "playlist", "create", "Domingo")
    assert "ya existia" in run(capsys, "playlist", "create", "Domingo")
    lid = playlists.by_name("Domingo")["id"]
    playlists.add(lid, [s["id"] for s in library.search("", limit=2)])
    out = run(capsys, "playlist", "show", "domingo")
    assert " 1. " in out and " 2. " in out
    assert run(capsys, "playlist", "export", str(lid)).strip().endswith(".m3u8")
    assert "Domingo" in run(capsys, "playlist")
    with pytest.raises(SystemExit) as e:
        cli.main(["playlist", "show", "No existe"])
    assert "No existe" in str(e.value)
    assert "borrada" in run(capsys, "playlist", "remove", "Domingo")
    assert playlists.by_name("Domingo") is None


def test_duplicados_con_una_carpeta_fuera_de_la_biblioteca(lib, capsys, tmp_path):
    """`relative_to(LIBRARY)` reventaba con cualquier carpeta de otro sitio."""
    from danplay import library

    fuera = tmp_path / "disco-externo"
    shutil.copytree(lib[0] / "Artistas", fuera)
    library.add_folder(fuera)
    library.scan()
    out = run(capsys, "duplicates")
    assert "IDENTICOS" in out and str(fuera) in out
    assert "Nada se ha borrado" in out


def test_importar_convertir_y_organizar_sin_tocar_nada(lib, capsys):
    from danplay import config

    root, _ = lib
    make_mp3(config.INBOX / "Barak - Nueva.mp3", artist="Barak", title="Nueva")
    out = run(capsys, "import", "--dry-run", "-v")
    assert "SIM" in out and "SIMULACION" in out
    assert (config.INBOX / "Barak - Nueva.mp3").exists()
    out = run(capsys, "convert")
    assert "Nada que convertir" in out or "simulacion" in out
    (root / "Revueltas").mkdir()
    make_mp3(root / "Revueltas" / "BARAK mi gozo VIDEO OFICIAL.mp3")
    out = run(capsys, "organize", "Revueltas")
    assert "simulacion" in out and (root / "Revueltas" / "BARAK mi gozo VIDEO OFICIAL.mp3").exists()
    assert "No existe" in run(capsys, "organize", "no-hay-tal")


def test_youtube_sin_red_lo_cuenta(lib, capsys, monkeypatch):
    from danplay import youtube

    monkeypatch.setattr(youtube, "available", lambda: True)
    monkeypatch.setattr(
        youtube, "info", lambda q, n: {"ok": False, "reason": "sin conexion", "items": []}
    )
    assert cli.main(["youtube", "--list", "barak"]) == 1
    assert "sin conexion" in capsys.readouterr().out
