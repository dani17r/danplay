"""El texto de una guia hablada, listo para decirse: los numeros en
palabras («x2» es «dos veces», «1/2» es «medio») y, para las voces de Piper,
los fonemas al estilo de espeak-ng, que es con lo que se entrenaron.

espeak-ng es GPL y no viaja con la app: sus reglas para el castellano estan
copiadas aqui a mano (de su salida, no de su codigo). Coincide palabra por
palabra en un 96-97 % de un corpus normal; lo que falla son excepciones de su
diccionario (`México`, nombres de letras), que en una guia no aparecen.

Lo hace el crate de Rust (biblioteca/src/habla.rs, regla a regla lo que
antes estaba aqui); esto es la puerta de Python, con los mismos nombres.
"""

import danplay_core as _rust

ONES, EN_ONES = (list(t) for t in _rust.habla_tables())


def _txt(s: str) -> str:
    """Un texto que se pueda pasar a Rust: los «surrogates» pasan a «�»."""
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        return s.encode("utf-8", "surrogateescape").decode("utf-8", "replace")
    return s


# ------------------------------------------------------------ los numeros


def number(n: int, lang: str = "es") -> str:
    """Un numero entero en palabras (hasta 999 999; mas, cifra a cifra)."""
    if n > 0x7FFF_FFFF_FFFF_FFFF:
        # no cabe en 64 bits: cifra a cifra, como los de un millon en adelante
        return " ".join((EN_ONES if lang == "en" else ONES)[int(d)] for d in str(n))
    return _rust.habla_number(n, lang)


def normalize(text: str, lang: str = "es") -> str:
    """El texto como se dice: sin cifras ni abreviaturas, con su punto al
    final (sin el, algunas voces cortan la ultima silaba)."""
    return _rust.habla_normalize(_txt(str(text or "")), lang)


# ------------------------------------------------------------ los fonemas


def word_ipa(word: str, seseo=False, yeismo=False, stress: str | None = "primary") -> str:
    """Una palabra en fonemas, con su acento (`ˈ` delante de la vocal)."""
    return _rust.habla_word_ipa(_txt(word), bool(seseo), bool(yeismo), stress)


def phonemes(text: str, dialect: str = "es") -> str:
    """El texto en fonemas como los da espeak-ng: `es` (España) o `es-419`
    (America: seseo y yeismo)."""
    return _rust.habla_phonemes(_txt(str(text or "")), dialect)


def piper_ids(phones: str, id_map: dict) -> list[int]:
    """Los fonemas como los numeros que espera una voz de Piper: ^ _ p _ p _ … $
    (cada caracter descompuesto en NFD; lo que la voz no conoce, fuera)."""
    return _rust.habla_piper_ids(
        _txt(phones), {k: list(v) for k, v in id_map.items() if isinstance(k, str)}
    )
