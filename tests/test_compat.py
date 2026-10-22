"""Las reglas de Python que el nucleo en Rust imita (biblioteca/src/py.rs),
comparadas con las de verdad: `posixpath`, `ntpath`, `fnmatch`, `round`,
`repr`, `float` e `int`. Si Rust se aparta de Python en una sola, aqui sale
el caso exacto."""

import fnmatch
import json
import math
import ntpath
import os
import posixpath
import unicodedata

import danplay_core as R
import pytest
from hypothesis import assume, example, given, settings
from hypothesis import strategies as st

# Rutas hechas de lo que las rompe: barras de los dos tipos, unidades, «.»,
# «..», «~» y algo que no es ASCII.
PIECES = st.sampled_from(["a", "B", "é", ".", "..", "", "~", ":", "C:", "x y", "?", "UNC"])
SEPS = st.sampled_from(["/", "\\", "//", "\\\\"])
PATH = st.lists(st.tuples(PIECES, SEPS), max_size=7).map(lambda xs: "".join(a + b for a, b in xs))
PATH2 = st.tuples(st.sampled_from(["", "/", "\\", "C:", "C:\\", "c:/", "\\\\srv\\sh\\"]), PATH).map(
    "".join
)


def _compat(name, *args):
    return R._compat(name, list(args))


@settings(max_examples=600)
@given(PATH2)
@example("//a//b/../c/.")
@example("\\\\?\\UNC\\srv\\share\\x")
def test_las_rutas_posix_como_posixpath(p):
    assert _compat("posix_normpath", p) == posixpath.normpath(p)
    assert _compat("posix_split", p) == posixpath.split(p)


@settings(max_examples=600)
@given(PATH2, PATH2)
def test_juntar_y_lo_comun_como_posixpath(a, b):
    assert _compat("posix_join", a, b) == posixpath.join(a, b)
    try:
        expected = posixpath.commonpath([a, b])
    except ValueError:
        expected = None
    assert _compat("posix_commonpath", [a, b]) == expected
    if a.startswith("/") and b.startswith("/"):
        na, nb = posixpath.normpath(a), posixpath.normpath(b)
        assert _compat("posix_relpath_abs", na, nb) == posixpath.relpath(na, nb)


@settings(max_examples=600)
@given(PATH2)
@example("\\\\?\\UNC\\srv\\share\\x")
@example("é:x")
def test_las_rutas_de_windows_como_ntpath(p):
    assert _compat("nt_splitroot", p) == ntpath.splitroot(p)
    assert _compat("nt_normpath", p) == ntpath.normpath(p)
    assert _compat("nt_normcase", p) == ntpath.normcase(p)
    assert _compat("nt_split", p) == ntpath.split(p)
    assert _compat("nt_isabs", p) == ntpath.isabs(p)


@settings(max_examples=600)
@given(PATH2, PATH2)
def test_juntar_y_lo_comun_como_ntpath(a, b):
    assert _compat("nt_join", a, b) == ntpath.join(a, b)
    try:
        expected = ntpath.commonpath([a, b])
    except ValueError:
        expected = None
    assert _compat("nt_commonpath", [a, b]) == expected


@pytest.mark.skipif(
    os.name == "nt",
    reason="en Windows realpath se resuelve con el sistema (canonicalize) y ~otro no se expande",
)
def test_las_rutas_del_sistema(tmp_path, monkeypatch):
    """realpath, abspath, relpath y expanduser de verdad, con enlaces."""
    (tmp_path / "real" / "sub").mkdir(parents=True)
    (tmp_path / "enlace").symlink_to(tmp_path / "real")
    (tmp_path / "vuelta").symlink_to(tmp_path / "vuelta")
    monkeypatch.chdir(tmp_path)
    for p in [
        str(tmp_path / "enlace" / "sub" / ".." / "x"),
        str(tmp_path / "enlace" / "no-existe" / "y"),
        str(tmp_path / "vuelta" / "z"),
        "enlace/sub",
        "../" + tmp_path.name + "/enlace",
        "/",
        "",
        ".",
    ]:
        assert _compat("realpath", p) == os.path.realpath(p), p
        assert _compat("abspath", p) == os.path.abspath(p), p
        if p:
            assert _compat("relpath", p, str(tmp_path)) == os.path.relpath(p, tmp_path), p
    for p in ["~", "~/Musica", "~otro/x", "a/~", "~/"]:
        assert _compat("expanduser", p) == os.path.expanduser(p), p


GLOB = st.text(st.sampled_from("ab-z*?[]!^\\.&|~/"), max_size=8)


@settings(max_examples=1500)
@given(st.text(st.sampled_from("abz-.[]!^\\/&|~\n"), max_size=6), GLOB)
@example("-", "[a-]")
@example("b", "[z-a]")
@example("^", "[^]")
def test_fnmatch_como_fnmatchcase(name, pat):
    assert _compat("fnmatchcase", name, pat) == fnmatch.fnmatchcase(name, pat), (name, pat)


FLOATS = st.floats(allow_nan=False, allow_infinity=False)


@settings(max_examples=2000)
@given(FLOATS, st.integers(0, 6))
@example(2.675, 2)
@example(0.125, 2)
@example(1e300, 3)
def test_round_como_python(x, n):
    assert _compat("round", x, n) == round(x, n)


@settings(max_examples=2000)
@given(st.floats(allow_nan=False))
@example(1e16)
@example(1e-5)
@example(123456789012345678.0)
def test_repr_de_un_float_como_python(x):
    assert _compat("repr_float", x) == repr(x)


NUMERIC = st.text(st.sampled_from("0123456789_.eE+- \tinfaINFAnx"), max_size=10)


@settings(max_examples=2000)
@given(NUMERIC)
@example("1_000.5")
@example(" -inf ")
@example("1__0")
def test_float_e_int_de_un_texto_como_python(s):
    try:
        f = float(s)
    except ValueError:
        f = None
    got = _compat("parse_float", s)
    assert got == f or (f is not None and math.isnan(f) and math.isnan(got)), s
    try:
        i = int(s)
    except ValueError:
        i = None
    assert _compat("int_of", s) == i, s


def _assigned(s: str) -> bool:
    """Sin caracteres que este Python aun no conoce: Rust trae un Unicode mas
    nuevo, y lo que aqui es «sin asignar» (Cn) alli ya es una letra."""
    return all(unicodedata.category(c) != "Cn" for c in s)


JSONISH = st.recursive(
    st.none() | st.booleans() | st.integers(-(2**63), 2**63 - 1) | FLOATS | st.text(max_size=8),
    lambda inner: (
        st.lists(inner, max_size=3) | st.dictionaries(st.text(max_size=4), inner, max_size=3)
    ),
    max_leaves=8,
)


@settings(max_examples=1000)
@given(JSONISH)
def test_repr_como_python(v):
    assume(_assigned(json.dumps(v, ensure_ascii=False)))
    assert _compat("repr", v) == repr(v)


@settings(max_examples=1000)
@given(st.text(max_size=6))
@example("_x1")
@example("1x")
@example("ñandú")
def test_isidentifier_como_python(s):
    assume(_assigned(s))
    assert _compat("isidentifier", s) == s.isidentifier()


@given(st.characters())
def test_isprintable_como_python(c):
    assume(_assigned(c))
    assert _compat("isprintable", c) == c.isprintable()


@settings(max_examples=1000)
@given(JSONISH, st.sampled_from([None, 1, 2]), st.booleans())
def test_json_dumps_como_python(v, indent, sort_keys):
    assume(_assigned(json.dumps(v, ensure_ascii=False)))
    expected = json.dumps(v, ensure_ascii=False, indent=indent, sort_keys=sort_keys)
    assert _compat("dumps", v, indent, sort_keys) == expected
