"""La configuracion que leera el nucleo en Rust (biblioteca/src/config.rs)
contra la de Python (danplay/config.py): las mismas carpetas y los mismos
valores, y el archivo de ajustes leido igual que lo lee python-dotenv."""

import io
import os
import subprocess
import sys
import textwrap

import danplay_core as R
from dotenv.main import DotEnv
from hypothesis import example, given, settings
from hypothesis import strategies as st

from danplay import config

KEYS = [
    "DATA_DIR",
    "CONFIG_DIR",
    "ENV_FILE",
    "USE_PROJECT_ENV",
    "LIBRARY",
    "INBOX",
    "ARTISTS_DIR",
    "REVIEW_DIR",
    "LIBRARY_CHOSEN",
    "DATABASE",
    "TOOLS_DIR",
    "AI_ENABLED",
    "ACOUSTID_API_KEY",
    "CONVERT_TO_MP3",
    "MP3_QUALITY",
    "KEEP_ORIGINAL",
    "NEVER_CONVERT",
    "WRITE_TAGS",
    "STEMS_FORMAT",
    "DUPLICATE_THRESHOLD",
]


def _python_config(env: dict) -> dict:
    """config.py importado de cero en otro proceso, con ese entorno."""
    code = textwrap.dedent(
        """
        import json, sys
        sys.path.insert(0, sys.argv[1])
        from danplay import config as c
        out = {k: getattr(c, k) for k in json.loads(sys.argv[2])}
        out["NEVER_CONVERT"] = sorted(out["NEVER_CONVERT"])
        out["ffmpeg"] = c.find_tool("ffmpeg")
        out["default_library"] = c._default_library()
        print(json.dumps({k: str(v) if hasattr(v, "__fspath__") else v for k, v in out.items()}))
        """
    )
    import json

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    r = subprocess.run(
        [sys.executable, "-c", code, root, json.dumps(KEYS)],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(r.stdout)


def _rust_config(env: dict) -> dict:
    code = textwrap.dedent(
        """
        import json, sys
        import danplay_core as R
        out = R._compat("config", [False, sys.argv[1]])
        print(json.dumps({k: str(v) if hasattr(v, "__fspath__") else v for k, v in out.items()}))
        """
    )
    import json

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    r = subprocess.run(
        [sys.executable, "-c", code, root], env=env, capture_output=True, text=True, check=True
    )
    return json.loads(r.stdout)


def test_la_configuracion_de_rust_es_la_de_python(tmp_path):
    """En el entorno de las pruebas, y con ajustes en el archivo (con
    comillas, comentarios, nombres viejos y ${VARIABLES})."""
    base = dict(os.environ)
    assert _rust_config(base) == _python_config(base)

    cfg = tmp_path / "cfg"
    data = tmp_path / "datos"
    cfg.mkdir()
    (cfg / "danplay.env").write_text(
        "DANPLAY_BIBLIOTECA='/tmp/Mi Musica #2'\n"
        "export DANPLAY_CONVERT=1  # comentario\n"
        'DANPLAY_QUALITY="medium"\n'
        "DANPLAY_NO_CONVERTIR=Pistas,Stems,,Pistas\n"
        "DANPLAY_TOOLS_DIR=${HOME}/herramientas\n"
        "DANPLAY_UMBRAL_DUP=0.9\n"
        "DANPLAY_TAGS=no\n"
        "ACOUSTID_API_KEY=${NO_EXISTE:-clave}\n"
        "esto no vale\n",
        encoding="utf-8",
    )
    # sin DANPLAY_LIBRARY en el entorno (las pruebas la ponen): la del archivo
    env = {k: v for k, v in base.items() if k != "DANPLAY_LIBRARY"}
    env.update(DANPLAY_CONFIG_DIR=str(cfg), DANPLAY_DATA_DIR=str(data))
    rust, python = _rust_config(env), _python_config(env)
    assert rust == python
    assert python["LIBRARY"] == "/tmp/Mi Musica #2" and python["CONVERT_TO_MP3"] is True

    # sin carpetas puestas a mano: las de cada sistema (XDG en Linux)
    env = {k: v for k, v in base.items() if k not in ("DANPLAY_DATA_DIR", "DANPLAY_CONFIG_DIR")}
    env.update(XDG_DATA_HOME=str(tmp_path / "xdg-datos"), XDG_CONFIG_HOME=str(tmp_path / "xdg-cfg"))
    assert _rust_config(env) == _python_config(env)


LINE = st.sampled_from(
    [
        "A=1",
        "export B = dos ",
        "C='tres # no es comentario'",
        'D="cua\\ntro \\"x\\" \\\\ \\t"',
        "E=cinco # comentario",
        "F=#seis",
        "G= # vacio",
        "'H'=siete",
        "I",
        "=sin clave",
        "J='sin cerrar",
        'K="multi\nlinea"',
        "L=${A}-${NADA:-def}-${:-x}-${M:N}",
        "# comentario entero",
        "   ",
        "M=\t ocho\t ",
        "N='a\\'b'",
        "O=ñandú",
        "P=${HOME}",
        "Q=${I}",
    ]
)


@settings(max_examples=800)
@given(st.lists(LINE, max_size=8), st.sampled_from(["\n", "\r\n", "\r"]))
@example(["A=1", "L=${A}-${NADA:-def}-${:-x}-${M:N}"], "\n")
def test_el_archivo_de_ajustes_se_lee_como_python_dotenv(lines, newline):
    text = newline.join(lines)
    env = {"HOME": "/home/x", "A": "del entorno"}
    old = os.environ.copy()
    try:
        os.environ.clear()
        os.environ.update(env)
        expected = DotEnv(None, stream=io.StringIO(text), interpolate=True, override=False).dict()
    finally:
        os.environ.clear()
        os.environ.update(old)
    assert R._compat("dotenv_values", [text, env]) == dict(expected)


@given(st.text(max_size=20))
@example("/home/ana/Musica #2")
@example(" lados ")
def test_lo_que_se_guarda_se_escribe_como_python_y_se_relee_igual(value):
    written = config.dotenv_value(value)
    assert R._compat("dotenv_value", [value]) == written
    if "\n" not in value and "\r" not in value:
        got = R._compat("dotenv_values", [f"X={written}\n", {}])
        expected = DotEnv(None, stream=io.StringIO(f"X={written}\n"), interpolate=False).dict()
        assert got == dict(expected) or "${" in value
