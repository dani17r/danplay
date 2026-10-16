"""El texto de una guia hablada, listo para decirse: los numeros en
palabras («x2» es «dos veces», «1/2» es «medio») y, para las voces de Piper,
los fonemas al estilo de espeak-ng, que es con lo que se entrenaron.

espeak-ng es GPL y no viaja con la app: sus reglas para el castellano estan
copiadas aqui a mano (de su salida, no de su codigo). Coincide palabra por
palabra en un 96-97 % de un corpus normal; lo que falla son excepciones de su
diccionario (`México`, nombres de letras), que en una guia no aparecen.
"""

import re
import unicodedata

# ------------------------------------------------------------ los numeros

ONES = [
    "cero",
    "uno",
    "dos",
    "tres",
    "cuatro",
    "cinco",
    "seis",
    "siete",
    "ocho",
    "nueve",
    "diez",
    "once",
    "doce",
    "trece",
    "catorce",
    "quince",
    "dieciséis",
    "diecisiete",
    "dieciocho",
    "diecinueve",
    "veinte",
    "veintiuno",
    "veintidós",
    "veintitrés",
    "veinticuatro",
    "veinticinco",
    "veintiséis",
    "veintisiete",
    "veintiocho",
    "veintinueve",
]
TENS = {
    3: "treinta",
    4: "cuarenta",
    5: "cincuenta",
    6: "sesenta",
    7: "setenta",
    8: "ochenta",
    9: "noventa",
}
HUNDREDS = {
    1: "ciento",
    2: "doscientos",
    3: "trescientos",
    4: "cuatrocientos",
    5: "quinientos",
    6: "seiscientos",
    7: "setecientos",
    8: "ochocientos",
    9: "novecientos",
}
EN_ONES = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
]
EN_TENS = {
    2: "twenty",
    3: "thirty",
    4: "forty",
    5: "fifty",
    6: "sixty",
    7: "seventy",
    8: "eighty",
    9: "ninety",
}


def number(n: int, lang: str = "es") -> str:
    """Un numero entero en palabras (hasta 999 999; mas, cifra a cifra)."""
    if lang == "en":
        return _en(n)
    if n < 30:
        return ONES[n]
    if n < 100:
        t, u = divmod(n, 10)
        return TENS[t] + ("" if u == 0 else " y " + ONES[u])
    if n < 1000:
        if n == 100:
            return "cien"
        h, r = divmod(n, 100)
        return HUNDREDS[h] + ("" if r == 0 else " " + number(r))
    if n < 1_000_000:
        k, r = divmod(n, 1000)
        head = "mil" if k == 1 else number(k) + " mil"
        return head + ("" if r == 0 else " " + number(r))
    return " ".join(ONES[int(d)] for d in str(n))


def _en(n: int) -> str:
    if n < 20:
        return EN_ONES[n]
    if n < 100:
        t, u = divmod(n, 10)
        return EN_TENS[t] + ("" if u == 0 else "-" + EN_ONES[u])
    if n < 1000:
        h, r = divmod(n, 100)
        return EN_ONES[h] + " hundred" + ("" if r == 0 else " " + _en(r))
    if n < 1_000_000:
        k, r = divmod(n, 1000)
        return _en(k) + " thousand" + ("" if r == 0 else " " + _en(r))
    return " ".join(EN_ONES[int(d)] for d in str(n))


def normalize(text: str, lang: str = "es") -> str:
    """El texto como se dice: sin cifras ni abreviaturas, con su punto al
    final (sin el, algunas voces cortan la ultima silaba)."""
    text = unicodedata.normalize("NFC", str(text or "")).strip()
    text = re.sub(r"\s+", " ", text)
    if lang == "en":
        text = re.sub(
            r"\bx\s*(\d+)\b|\b(\d+)\s*x\b", lambda m: _en(int(m[1] or m[2])) + " times", text
        )
        text = re.sub(r"\b1/2\b|½", "half", text)
        text = re.sub(r"\b1/4\b|¼", "a quarter", text)
    else:
        text = re.sub(
            r"\bx\s*(\d+)\b|\b(\d+)\s*x\b",
            lambda m: "una vez" if int(m[1] or m[2]) == 1 else number(int(m[1] or m[2])) + " veces",
            text,
        )
        text = re.sub(r"\b1/2\b|½", "medio", text)
        text = re.sub(r"\b1/4\b|¼", "un cuarto", text)
        text = re.sub(r"\b3/4\b|¾", "tres cuartos", text)
        text = re.sub(r"(\d+)\s*%", lambda m: m[1] + " por ciento", text)
        text = re.sub(r"\bbpm\b", "pulsos por minuto", text, flags=re.IGNORECASE)
    text = re.sub(r"\d+", lambda m: " " + number(int(m.group()), lang) + " ", text)
    text = re.sub(r"\s+", " ", text).strip(" ")
    if text and not re.search(r"[.!?;:,…]$", text):
        text += "."
    return text


# ------------------------------------------------------------ los fonemas

# Sin acento en la frase (como en espeak): articulos, preposiciones...
UNSTRESSED = {
    "el",
    "la",
    "lo",
    "los",
    "las",
    "le",
    "les",
    "de",
    "del",
    "al",
    "a",
    "en",
    "con",
    "por",
    "que",
    "y",
    "e",
    "o",
    "u",
    "mi",
    "tu",
    "su",
    "mis",
    "tus",
    "sus",
    "se",
    "me",
    "te",
    "nos",
    "os",
    "si",
    "sin",
    "tan",
}
# Con acento secundario solamente.
SECONDARY_ONLY = {
    "pero",
    "como",
    "para",
    "cuando",
    "hacia",
    "sobre",
    "entre",
    "desde",
    "donde",
    "mientras",
    "porque",
    "aunque",
}
VOWELS = "aeiouáéíóúü"
ACCENTED = {"á": "a", "é": "e", "í": "i", "ó": "o", "ú": "u"}
# Los grupos que empiezan silaba: «pr», «bl»...
ONSETS = {
    ("p", "ɾ"),
    ("b", "ɾ"),
    ("t", "ɾ"),
    ("d", "ɾ"),
    ("k", "ɾ"),
    ("ɡ", "ɾ"),
    ("f", "ɾ"),
    ("p", "l"),
    ("b", "l"),
    ("k", "l"),
    ("ɡ", "l"),
    ("f", "l"),
}
PAUSES = ",.?!:;"


def _units(w: str, seseo: bool, yeismo: bool) -> list[list]:
    """Una palabra (en minusculas) en sonidos: [fonema, 'V'|'C', acentuada]."""
    out: list[list] = []
    i, n = 0, len(w)
    while i < n:
        c = w[i]
        nx = w[i + 1] if i + 1 < n else ""
        nx2 = w[i + 2] if i + 2 < n else ""
        step = 1
        if c in VOWELS:
            base = ACCENTED.get(c, "u" if c == "ü" else c)
            out.append([base, "V", (c in ACCENTED) if c != "ü" else "syl"])
        elif c == "c":
            if nx == "h":
                out.append(["tʃ", "C", False])
                step = 2
            elif nx and nx in "eiéí":
                out.append(["s" if seseo else "θ", "C", False])
            else:
                out.append(["k", "C", False])
        elif c == "q":
            out.append(["k", "C", False])
            step = 2 if nx == "u" else 1
        elif c == "g":
            if nx and nx in "eiéí":
                out.append(["x", "C", False])
            elif nx == "u" and nx2 and nx2 in "eiéí":
                out.append(["ɡ", "C", False])
                step = 2
            else:
                out.append(["ɡ", "C", False])
        elif c == "h":
            pass
        elif c == "j":
            out.append(["x", "C", False])
        elif c == "l":
            if nx == "l":
                out.append(["ʝ" if yeismo else "ʎ", "C", False])
                step = 2
            else:
                out.append(["l", "C", False])
        elif c == "r":
            if nx == "r":
                out.append(["r", "C", False])
                step = 2
            else:
                trill = i == 0 or (bool(out) and out[-1][0] in ("n", "l", "s"))
                out.append(["r" if trill else "ɾ", "C", False])
        elif c == "ñ":
            out.append(["ɲ", "C", False])
        elif c in "bv":
            out.append(["b", "C", False])
        elif c == "z":
            out.append(["s" if seseo else "θ", "C", False])
        elif c == "x":
            if i == 0:
                out.append(["s", "C", False])
            else:
                out.append(["k", "C", False])
                out.append(["s", "C", False])
        elif c == "y":
            # consonante delante de vocal; vocal al final o delante de consonante
            if nx and nx in VOWELS:
                out.append(["ʝ", "C", False])
            else:
                out.append(["i", "V", False])
        elif c in "wk" or c in "dfmnpst":
            out.append([c, "C", False])
        i += step
    return out


def _syllables(units: list[list]) -> list[dict]:
    """Las silabas: cada una con sus consonantes de delante, su nucleo (una
    vocal o un diptongo) y las de detras (indices de `units`)."""
    nuclei = []
    i = 0
    while i < len(units):
        if units[i][1] != "V":
            i += 1
            continue
        j = i + 1
        while j < len(units) and units[j][1] == "V":
            a, b = units[j - 1], units[j]
            a_weak = a[0] in "iu" and not a[2]
            b_weak = b[0] in "iu" and not b[2]
            if (a_weak or b_weak) and a[0] != b[0]:
                j += 1
            else:
                break
        nuclei.append((i, j))
        i = j
    if not nuclei:
        return []
    sylls = []
    for k, (s, e) in enumerate(nuclei):
        end = nuclei[k + 1][0] if k + 1 < len(nuclei) else len(units)
        sylls.append({"pre": [], "nuc": list(range(s, e)), "post": list(range(e, end))})
    # las consonantes de en medio: todas las que puedan, a la silaba de detras
    for k in range(len(sylls) - 1):
        cons = sylls[k]["post"]
        if len(cons) <= 1:
            onset = cons
        else:
            last2 = (units[cons[-2]][0], units[cons[-1]][0])
            onset = cons[-2:] if last2 in ONSETS else cons[-1:]
        sylls[k]["post"] = [c for c in cons if c not in onset]
        sylls[k + 1]["pre"] = onset
    sylls[0]["pre"] = list(range(nuclei[0][0]))
    return sylls


def _stressed(units, sylls, word: str) -> int:
    for si, s in enumerate(sylls):
        for ui in s["nuc"]:
            if units[ui][2] is True:
                return si
    if len(sylls) == 1:
        return 0
    if word[-1] in "aeiouns" and not word.endswith("y"):
        return len(sylls) - 2
    return len(sylls) - 1


def word_ipa(word: str, seseo=False, yeismo=False, stress: str | None = "primary") -> str:
    """Una palabra en fonemas, con su acento (`ˈ` delante de la vocal)."""
    units = _units(word, seseo, yeismo)
    sylls = _syllables(units)
    if not sylls:
        return "".join(u[0] for u in units)
    si = _stressed(units, sylls, word)
    marks: dict[int, str] = {}
    if stress == "primary":
        marks[si] = "ˈ"
        for k in range(0, si - 1, 2):
            marks[k] = "ˌ"
    elif stress == "secondary":
        marks[si] = "ˌ"
    out = []
    for k, s in enumerate(sylls):
        out.extend(units[ui][0] for ui in s["pre"])
        nuc = [units[ui] for ui in s["nuc"]]
        # la vocal que suena: la acentuada o la fuerte; si no, la ultima debil
        peak = None
        for idx, v in enumerate(nuc):
            if v[2] is True:
                peak = idx
        if peak is None:
            peak = next((idx for idx, v in enumerate(nuc) if v[0] in "aeo"), None)
        if peak is None:
            peak = 0 if word.endswith("uy") and k == len(sylls) - 1 else len(nuc) - 1
        for idx, v in enumerate(nuc):
            ph = v[0]
            if idx < peak:
                ph = {"i": "j", "u": "w"}.get(ph, ph)
            elif idx > peak:
                weak = {"i": "ɪ", "u": "ʊ"} if nuc[peak][0] in "aeo" else {"i": "j", "u": "w"}
                ph = weak.get(ph, ph)
            if idx == peak and k in marks:
                out.append(marks[k])
            # la e acentuada delante de n y consonante, abierta (como espeak)
            if (
                idx == peak
                and ph == "e"
                and k == si
                and stress == "primary"
                and s["post"]
                and units[s["post"][0]][0] == "n"
                and k + 1 < len(sylls)
            ):
                ph = "ɛ"
            out.append(ph)
            if (
                idx == peak
                and ph == "i"
                and k == si
                and not s["post"]
                and k + 1 < len(sylls)
                and not sylls[k + 1]["pre"]
            ):
                out.append("ʲ")
        out.extend(units[ui][0] for ui in s["post"])
    return "".join(out)


def _next_sound(word: str) -> str:
    return word.lstrip("ˈˌ")[:1]


def _across(tokens: list[str]) -> list[str]:
    """Lo que pasa entre palabras: b d g suaves salvo tras pausa o nasal, la
    n que se vuelve m o ŋ, la d final."""
    res = []
    prev_last: str | None = None  # None: despues de una pausa
    for ti, t in enumerate(tokens):
        if t in PAUSES:
            res.append(t)
            prev_last = None
            continue
        nxt = None
        if ti + 1 < len(tokens) and tokens[ti + 1] not in PAUSES:
            nxt = tokens[ti + 1]
        chars = list(t)
        out: list[str] = []
        last = prev_last
        for ci, ch in enumerate(chars):
            if ch in "bdɡ":
                out.append(
                    ch if last is None or last in "mnɲŋ" else {"b": "β", "d": "ð", "ɡ": "ɣ"}[ch]
                )
            elif ch == "n":
                nx = next((c2 for c2 in chars[ci + 1 :] if c2 not in "ˈˌ"), None)
                if nx is None and nxt:
                    nx = _next_sound(nxt)
                out.append("m" if nx in ("b", "p", "β") else "ŋ" if nx in ("ɡ", "x", "ɣ") else "n")
            else:
                out.append(ch)
            if ch not in "ˈˌ":
                last = out[-1]
        word = "".join(out)
        vowel_next = bool(nxt) and _next_sound(nxt or "") in "aeiouɛ"
        if word.endswith("ð") and not vowel_next:
            word = word[:-1] + "d"
        elif word.endswith("d") and vowel_next:
            word = word[:-1] + "ð"
        res.append(word)
        prev_last = word[-1] if word else prev_last
    return res


def phonemes(text: str, dialect: str = "es") -> str:
    """El texto en fonemas como los da espeak-ng: `es` (España) o `es-419`
    (America: seseo y yeismo)."""
    latin = dialect != "es"
    text = unicodedata.normalize("NFC", normalize(text).lower())
    text = text.replace("¿", " ").replace("¡", " ")
    tokens = []
    for t in re.findall(r"[a-záéíóúüñ]+|[,.?!:;]", text):
        if t in PAUSES:
            tokens.append(t)
        elif t in UNSTRESSED:
            tokens.append(word_ipa(t, latin, latin, stress=None))
        elif t in SECONDARY_ONLY:
            tokens.append(word_ipa(t, latin, latin, stress="secondary"))
        else:
            tokens.append(word_ipa(t, latin, latin))
    s = ""
    for t in _across(tokens):
        s = s.rstrip() + t + " " if t in PAUSES else s + t + " "
    return s.strip()


def piper_ids(phones: str, id_map: dict) -> list[int]:
    """Los fonemas como los numeros que espera una voz de Piper: ^ _ p _ p _ … $
    (cada caracter descompuesto en NFD; lo que la voz no conoce, fuera)."""
    ids = list(id_map["^"]) + list(id_map["_"])
    for ch in unicodedata.normalize("NFD", phones):
        if ch in id_map:
            ids += list(id_map[ch]) + list(id_map["_"])
    return [*ids, *id_map["$"]]
