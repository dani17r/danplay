"""Lo que no debe salir de su sitio: los datos del usuario, las claves de IA
y los permisos de los archivos que las guardan."""

import json
import os
import pathlib
import socket
import stat
import subprocess
import sys
import threading

import conftest
import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent


# ------------------------------------------------------------ las pruebas


def test_the_tests_do_not_touch_the_real_data():
    """Todo lo que cuelga del usuario cae en el temporal de la sesion: la
    base, los ajustes con las claves, los perfiles de IA y las caches."""
    from danplay import config, model_catalog, providers, thumbnails, waveform

    sandbox = conftest.SANDBOX.resolve()
    # lo que calcula config al importarse (algunas pruebas cambian luego
    # DATABASE o LIBRARY a su propio temporal, pero estas no)
    for p in (
        config.DATA_DIR,
        config.CONFIG_DIR,
        config.ENV_FILE,
        config.DATA_DIR / "danplay.db",
        providers.PROFILES_FILE,
        model_catalog.CACHE,
        waveform._folder(),
        thumbnails.folder(),
        os.environ["DANPLAY_LIBRARY"],
        pathlib.Path.home(),
    ):
        assert sandbox in pathlib.Path(p).resolve().parents, p
    assert not config.USE_PROJECT_ENV, "el .env del proyecto lleva claves de verdad"


def test_the_tests_do_not_reach_the_internet():
    with pytest.raises(OSError):
        socket.create_connection(("example.com", 80), timeout=2)
    with pytest.raises(OSError):
        socket.create_connection(("93.184.215.14", 80), timeout=2)
    import urllib.request

    with pytest.raises(OSError):
        urllib.request.urlopen("https://models.dev/api.json", timeout=2)


def test_localhost_still_works():
    server = socket.socket()
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    try:
        socket.create_connection(server.getsockname(), timeout=2).close()
    finally:
        server.close()


# ------------------------------------------------------ carpetas y archivos


@pytest.mark.skipif(os.name != "posix", reason="permisos POSIX")
def test_the_data_folders_are_user_only():
    from danplay import config

    for folder in (config.DATA_DIR, config.CONFIG_DIR):
        assert stat.S_IMODE(folder.stat().st_mode) == 0o700, folder


@pytest.mark.skipif(os.name != "posix", reason="permisos POSIX")
def test_an_existing_folder_is_locked_down(tmp_path):
    from danplay import config

    folder = tmp_path / "abierta"
    folder.mkdir(mode=0o755)
    os.chmod(folder, 0o755)  # noqa: S103  (abierta a proposito)
    config.private_dir(folder)
    assert stat.S_IMODE(folder.stat().st_mode) == 0o700


@pytest.mark.skipif(os.name != "posix", reason="permisos POSIX")
def test_the_db_is_created_and_stays_user_only(tmp_path, monkeypatch):
    """Lleva el historial del chat: con la umask de siempre era legible por
    cualquiera del equipo."""
    from danplay import config, library

    db = tmp_path / "nueva.db"
    monkeypatch.setattr(config, "DATABASE", db)
    library.connect().close()
    assert stat.S_IMODE(db.stat().st_mode) == 0o600
    # una que ya existia con permisos abiertos, tambien
    old = tmp_path / "vieja.db"
    old.touch()
    os.chmod(old, 0o644)
    monkeypatch.setattr(config, "DATABASE", old)
    library.connect().close()
    assert stat.S_IMODE(old.stat().st_mode) == 0o600


@pytest.mark.skipif(os.name != "posix", reason="permisos POSIX")
def test_secrets_are_written_atomically_and_privately(tmp_path):
    from danplay import config

    target = tmp_path / "secreto.json"
    target.write_text("viejo")
    os.chmod(target, 0o644)
    config.write_private(target, "nuevo")
    assert target.read_text() == "nuevo"
    assert stat.S_IMODE(target.stat().st_mode) == 0o600
    assert [p.name for p in tmp_path.iterdir()] == ["secreto.json"], "sin temporales"


def test_a_failure_midway_leaves_the_previous_file(tmp_path, monkeypatch):
    """Si algo se corta al escribir, queda el archivo entero de antes."""
    from danplay import config

    target = tmp_path / "ai.json"
    target.write_text('{"entero": true}')

    def failing_replace(*a, **k):
        raise OSError("disco lleno")

    monkeypatch.setattr(os, "replace", failing_replace)
    with pytest.raises(OSError):
        config.write_private(target, '{"cort')
    assert json.loads(target.read_text()) == {"entero": True}
    assert [p.name for p in tmp_path.iterdir()] == ["ai.json"], "el temporal no se queda"


# ----------------------------------------------------------- danplay.env


@pytest.mark.parametrize(
    "value",
    [
        "/home/ana/Musica #2",
        "  espacios  al final",
        "comilla ' simple",
        'comilla " doble',
        "barra \\ invertida",
        "dólar $HOME y ${NO}",
        "Canción, ñandú",
        "=igual=",
        "",
    ],
)
def test_settings_read_back_as_they_were_saved(tmp_path, monkeypatch, value):
    """`DANPLAY_LIBRARY=/home/ana/Musica #2` se releia como `/home/ana/Musica`:
    sin comillas, lo de detras de « #» es un comentario para python-dotenv."""
    from dotenv import dotenv_values

    from danplay import config

    env_file = tmp_path / "danplay.env"
    env_file.write_text("OTRA=1\nDANPLAY_LIBRARY=vieja\n")
    monkeypatch.setattr(config, "ENV_FILE", env_file)
    config.save_env({"DANPLAY_LIBRARY": value})
    got = dotenv_values(env_file, interpolate=False)
    assert got["DANPLAY_LIBRARY"] == value.strip()
    assert got["OTRA"] == "1", "las demas variables no se tocan"
    assert env_file.read_text().count("DANPLAY_LIBRARY=") == 1


def test_a_value_with_a_newline_does_not_invent_variables(tmp_path, monkeypatch):
    from dotenv import dotenv_values

    from danplay import config

    env_file = tmp_path / "danplay.env"
    monkeypatch.setattr(config, "ENV_FILE", env_file)
    config.save_env({"ACOUSTID_API_KEY": "abc\nDANPLAY_AI=0"})
    assert set(dotenv_values(env_file)) == {"ACOUSTID_API_KEY"}


# ------------------------------------------------ migracion del nombre viejo


def _import_config(env: dict) -> str:
    """Importa `danplay.config` en un proceso aparte con ese entorno."""
    code = (
        f"import sys; sys.path.insert(0, {str(ROOT)!r}); from danplay import config; "
        "print(config.ENV_FILE.read_text() if config.ENV_FILE.exists() else '')"
    )
    clean = {k: v for k, v in os.environ.items() if not k.startswith(("XDG_", "DANPLAY_"))}
    r = subprocess.run(
        [sys.executable, "-c", code],
        env={**clean, **env},
        capture_output=True,
        text=True,
        timeout=120,
        check=True,
    )
    return r.stdout


@pytest.mark.skipif(
    not sys.platform.startswith("linux"),
    reason="XDG_* solo cuenta en Linux (Windows y macOS preguntan al sistema)",
)
def test_the_migration_from_melodia_respects_xdg(tmp_path):
    """Con las rutas fijas de antes (~/.config/melodia), un entorno aislado se
    tragaba las claves del usuario de verdad."""
    home = tmp_path / "home"
    real = home / ".config/melodia"
    real.mkdir(parents=True)
    (real / "melodia.env").write_text("DEEPINFRA_API_KEY=de-verdad\n")
    isolated = tmp_path / "aislado"
    env = {
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(isolated / "config"),
        "XDG_DATA_HOME": str(isolated / "data"),
        "DANPLAY_PROJECT_ENV": "0",
        "DANPLAY_LIBRARY": str(tmp_path / "Musica"),
    }
    assert "de-verdad" not in _import_config(env), "el entorno aislado vio la clave de verdad"
    # y con la melodia vieja DENTRO del entorno, se migra como siempre
    old = isolated / "config/melodia"
    old.mkdir(parents=True)
    (old / "melodia.env").write_text("ACOUSTID_API_KEY=la-mia\n")
    assert "la-mia" in _import_config(env)


# ----------------------------------------------------- perfiles de IA


@pytest.fixture
def temp_profiles(tmp_path, monkeypatch):
    from danplay import ai, providers

    monkeypatch.setattr(providers, "PROFILES_FILE", tmp_path / "ai.json")
    providers.reload()
    ai.reset_client()
    yield providers
    providers.reload()
    ai.reset_client()


@pytest.mark.skipif(os.name != "posix", reason="permisos POSIX")
def test_profiles_are_saved_for_the_user_only(temp_profiles):
    temp_profiles.save_profile({"provider": "openai", "key": "sk-una-clave-larga"})
    assert stat.S_IMODE(temp_profiles.PROFILES_FILE.stat().st_mode) == 0o600


def test_the_saved_key_does_not_travel_to_another_url(temp_profiles):
    """«Probar» con la URL cambiada le mandaba la clave de verdad a ese host."""
    temp_profiles.save_profile({"provider": "openai", "key": "sk-de-verdad-1234567890"})
    same = temp_profiles.with_saved_key({"provider": "openai"})
    assert same["key"] == "sk-de-verdad-1234567890", "a la misma URL si va"
    same = temp_profiles.with_saved_key(
        {"provider": "openai", "base_url": "https://api.openai.com/v1/"}
    )
    assert same["key"] == "sk-de-verdad-1234567890", "una barra de mas no es otro sitio"
    other = temp_profiles.with_saved_key(
        {"provider": "openai", "base_url": "https://otro.example/v1"}
    )
    assert not other.get("key"), "la clave no puede salir hacia otro servidor"
    other = temp_profiles.with_saved_key({"id": "openai", "provider": "openrouter"})
    assert not other.get("key")
    # y con el borrador trayendo su propia clave, esa manda
    assert (
        temp_profiles.with_saved_key({"provider": "openai", "key": "sk-nueva"})["key"] == "sk-nueva"
    )


def test_headers_and_extra_go_out_masked_and_come_back_whole(temp_profiles):
    temp_profiles.save_profile(
        {
            "provider": "custom",
            "id": "custom-proxy",
            "name": "Proxy",
            "base_url": "https://proxy.example/v1",
            "headers": {"Authorization": "Bearer secreto-de-proxy-123"},
            "extra": {"api_key": "otro-secreto-456789", "top_k": 5},
        }
    )
    shown = temp_profiles.profiles()["custom-proxy"]
    blob = json.dumps(shown)
    assert "secreto-de-proxy-123" not in blob and "otro-secreto-456789" not in blob
    assert shown["extra"]["top_k"] == 5, "lo que no es texto se enseña"
    # la interfaz devuelve lo enmascarado tal cual: se conserva lo guardado
    temp_profiles.save_profile(
        {
            "provider": "custom",
            "id": "custom-proxy",
            "headers": shown["headers"],
            "extra": shown["extra"],
        }
    )
    saved = json.loads(temp_profiles.PROFILES_FILE.read_text())["profiles"]["custom-proxy"]
    assert saved["headers"]["Authorization"] == "Bearer secreto-de-proxy-123"
    assert saved["extra"]["api_key"] == "otro-secreto-456789"
    # y al probar sin guardar, igual (misma URL)
    draft = temp_profiles.with_saved_key(
        {
            "provider": "custom",
            "id": "custom-proxy",
            "base_url": "https://proxy.example/v1",
            "headers": shown["headers"],
        }
    )
    assert draft["headers"]["Authorization"] == "Bearer secreto-de-proxy-123"
    # cambiar una cabecera de verdad la cambia
    temp_profiles.save_profile(
        {"provider": "custom", "id": "custom-proxy", "headers": {"Authorization": "Bearer otra"}}
    )
    saved = json.loads(temp_profiles.PROFILES_FILE.read_text())["profiles"]["custom-proxy"]
    assert saved["headers"]["Authorization"] == "Bearer otra"


def test_a_broken_ai_json_is_set_aside_not_overwritten(temp_profiles):
    temp_profiles.PROFILES_FILE.write_text('{"profiles": {"openai": {"key": "sk-que-no-se-pierd')
    temp_profiles.reload()
    temp_profiles.save_profile({"provider": "ollama"})
    aside = temp_profiles.PROFILES_FILE.with_name("ai.json.roto")
    assert aside.is_file() and "sk-que-no-se-pierd" in aside.read_text()


def test_saving_profiles_at_once_loses_none(temp_profiles):
    """Dos guardados a la vez leian el mismo estado y el segundo pisaba al
    primero; y leer mientras se guardaba podia romper con «dictionary
    changed size during iteration»."""
    errors = []

    def save(i):
        try:
            temp_profiles.save_profile(
                {
                    "provider": "custom",
                    "name": f"Servidor {i}",
                    "base_url": f"https://s{i}.example/v1",
                },
                activate=False,
            )
            temp_profiles.profiles()
            temp_profiles.fallbacks("")
        except Exception as e:  # noqa: BLE001
            errors.append(e)

    threads = [threading.Thread(target=save, args=(i,)) for i in range(20)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert not errors
    assert len([p for p in temp_profiles.profiles() if p.startswith("custom-")]) == 20


def test_folders_set_by_hand_migrate_nothing(tmp_path):
    """DANPLAY_DATA_DIR y DANPLAY_CONFIG_DIR mandan sobre platformdirs (las
    pruebas en Windows y macOS dependen de ello), y un entorno puesto a mano
    no se trae los datos del nombre viejo del programa."""
    home = tmp_path / "home"
    (home / ".config/melodia").mkdir(parents=True)
    (home / ".config/melodia/melodia.env").write_text("DEEPINFRA_API_KEY=de-verdad\n")
    code = (
        f"import sys; sys.path.insert(0, {str(ROOT)!r}); from danplay import config; "
        "print(config.DATA_DIR); print(config.CONFIG_DIR); print(config.ENV_FILE.exists())"
    )
    clean = {k: v for k, v in os.environ.items() if not k.startswith(("XDG_", "DANPLAY_"))}
    env = {
        **clean,
        "HOME": str(home),
        "USERPROFILE": str(home),
        "DANPLAY_DATA_DIR": str(tmp_path / "datos"),
        "DANPLAY_CONFIG_DIR": str(tmp_path / "ajustes"),
        "DANPLAY_PROJECT_ENV": "0",
        "DANPLAY_LIBRARY": str(tmp_path / "Musica"),
    }
    r = subprocess.run(
        [sys.executable, "-c", code],
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
        check=True,
    )
    data, conf, exists = r.stdout.split()
    assert (data, conf) == (str(tmp_path / "datos"), str(tmp_path / "ajustes"))
    assert exists == "False", "no se migro la clave del nombre viejo"
    assert (tmp_path / "datos").is_dir() and (tmp_path / "ajustes").is_dir()
