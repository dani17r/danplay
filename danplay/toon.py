# -*- coding: utf-8 -*-
"""TOON (Token-Oriented Object Notation): lo que devuelven las herramientas,
en el formato que menos tokens gasta.

Los resultados de las herramientas del asistente son casi siempre listas de
objetos iguales (canciones, repertorios, resultados de busqueda). En JSON
cada fila repite todas las claves y va llena de llaves, comillas y corchetes;
al modelo se le cobra cada uno. TOON escribe las claves una vez, como
cabecera, y una fila por elemento:

    total: 3
    songs[3]{id,artist,title,key}:
      12,Barak,Mi Gozo,Bb
      40,New Wine,Shekinah,
      51,Barak,"Sera Llena, La Tierra",G

Medido sobre los resultados reales de DanPlay ahorra entre un tercio y la
mitad de los tokens de cada resultado (ver tests/test_ai.py). Es lo que se
MANDA al modelo; lo que el modelo devuelve sigue siendo JSON, que es lo que
ha visto un millon de veces y lo que garantizan los modos JSON de los
proveedores.

Sigue la especificacion 1.x (github.com/toon-format/spec): comillas solo
cuando hace falta, numeros y booleanos tal cual, `null` para None, listas de
primitivos en una linea, tablas para listas de objetos iguales y planos, y
la forma de lista con «- » para lo demas. Solo codifica: no hace falta leerlo.
"""
import math
import re

_KEY_OK = re.compile(r"^[A-Za-z_][A-Za-z0-9_.]*$")
_NUMERIC = re.compile(r"^[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?$")
_RESERVED = ("true", "false", "null")
_INDENT = "  "


def encode(value, delimiter: str = ",") -> str:
    """El valor entero en TOON. `delimiter`: «,» (normal), «\\t» o «|»."""
    if delimiter not in (",", "\t", "|"):
        raise ValueError("delimitador no admitido")
    if isinstance(value, dict):
        lines = _object_lines(value, 0, delimiter)
        return "\n".join(lines)
    if isinstance(value, (list, tuple)):
        return "\n".join(_array_lines("", list(value), 0, delimiter))
    return _primitive(value, delimiter)


# ---------------------------------------------------------------- escalares

def _primitive(v, delimiter: str) -> str:
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        if math.isnan(v) or math.isinf(v):
            return "null"
        if v == int(v) and abs(v) < 1e15:
            return str(int(v))
        return repr(v)
    return _string(str(v), delimiter)


def _string(s: str, delimiter: str) -> str:
    if _needs_quotes(s, delimiter):
        return '"' + _escape(s) + '"'
    return s


def _needs_quotes(s: str, delimiter: str) -> bool:
    if s == "" or s != s.strip():
        return True
    if s in _RESERVED or _NUMERIC.match(s):
        return True
    if s[0] in "-#":
        return True
    for ch in s:
        if ch in ':"\\[]{}' or ch == delimiter or ord(ch) < 32 or ch == "\x7f":
            return True
    return False


def _escape(s: str) -> str:
    out = []
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 32 or ch == "\x7f":
            out.append(f"\\u{ord(ch):04x}")
        else:
            out.append(ch)
    return "".join(out)


def _key(k) -> str:
    k = str(k)
    return k if _KEY_OK.match(k) else '"' + _escape(k) + '"'


def _is_primitive(v) -> bool:
    return v is None or isinstance(v, (bool, int, float, str))


def _delim_mark(delimiter: str) -> str:
    """Lo que va dentro de los corchetes ademas del numero: nada con coma."""
    return "" if delimiter == "," else delimiter


# ----------------------------------------------------------------- objetos

def _object_lines(obj: dict, depth: int, delimiter: str) -> list[str]:
    pad = _INDENT * depth
    lines: list[str] = []
    for k, v in obj.items():
        key = _key(k)
        if isinstance(v, dict):
            lines.append(f"{pad}{key}:")
            lines.extend(_object_lines(v, depth + 1, delimiter))
        elif isinstance(v, (list, tuple)):
            lines.extend(_array_lines(key, list(v), depth, delimiter))
        else:
            lines.append(f"{pad}{key}: {_primitive(v, delimiter)}")
    return lines


# ----------------------------------------------------------------- listas

def _tabular_fields(items: list) -> list | None:
    """Las columnas si TODOS son objetos planos con las mismas claves."""
    if not items or not all(isinstance(x, dict) and x for x in items):
        return None
    fields = list(items[0].keys())
    want = set(fields)
    for x in items:
        if set(x.keys()) != want or not all(_is_primitive(v) for v in x.values()):
            return None
    return fields


def _array_lines(key: str, items: list, depth: int, delimiter: str) -> list[str]:
    pad = _INDENT * depth
    head = f"{key}" if key else ""
    n = len(items)
    mark = _delim_mark(delimiter)
    if n == 0:
        return [f"{pad}{head}: []" if key else f"{pad}[]"]
    if all(_is_primitive(x) for x in items):
        cells = delimiter.join(_primitive(x, delimiter) for x in items)
        return [f"{pad}{head}[{n}{mark}]: {cells}"]
    fields = _tabular_fields(items)
    if fields:
        header = delimiter.join(_key(f) for f in fields)
        lines = [f"{pad}{head}[{n}{mark}]{{{header}}}:"]
        for x in items:
            lines.append(_INDENT * (depth + 1)
                         + delimiter.join(_primitive(x[f], delimiter) for f in fields))
        return lines
    # forma de lista: «- » y cada elemento debajo
    lines = [f"{pad}{head}[{n}{mark}]:"]
    for x in items:
        lines.extend(_list_item(x, depth + 1, delimiter))
    return lines


def _list_item(x, depth: int, delimiter: str) -> list[str]:
    pad = _INDENT * depth
    if _is_primitive(x):
        return [f"{pad}- {_primitive(x, delimiter)}"]
    if isinstance(x, (list, tuple)):
        inner = _array_lines("", list(x), depth + 1, delimiter)
        return [pad + "- " + inner[0].lstrip()] + inner[1:]
    if not x:
        return [f"{pad}-"]
    # el primer campo va en la linea del guion; los demas alineados debajo
    inner = _object_lines(x, depth + 1, delimiter)
    return [pad + "- " + inner[0].lstrip()] + inner[1:]
