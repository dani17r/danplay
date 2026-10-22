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

Lo hace el crate de Rust (library/src/toon.rs); esto es la puerta de
Python, con los mismos nombres.
"""

import danplay_core as _rust


def encode(value, delimiter: str = ",") -> str:
    """El valor entero en TOON. `delimiter`: «,» (normal), «\\t» o «|»."""
    return _rust.toon_encode(value, delimiter)


def _string(s: str, delimiter: str) -> str:
    """Un texto, entre comillas si hace falta."""
    try:
        return _rust.toon_string(s, delimiter)
    except UnicodeEncodeError:
        return _rust.toon_string(
            s.encode("utf-8", "surrogateescape").decode("utf-8", "replace"), delimiter
        )
