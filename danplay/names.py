# -*- coding: utf-8 -*-
"""Limpieza y normalizacion de nombres.

Reglas acordadas con el usuario:
  - sin acentos, pero la ñ se conserva
  - las palabras en MAYUSCULA pasan a Capitalizado
  - formato final: "Artista - Titulo (feat. X).mp3"
  - fuera el ruido de descargas (VIDEO OFICIAL, LETRA, y2mate.com, ...)
  - duplicados: sufijo " - r", " - r2", ...
"""
import logging, os, re, unicodedata

log = logging.getLogger("danplay")

NOISE = [
    r"lyric\s*video\s*oficial", r"official\s*(lyric\s*)?video", r"videoclip\s*oficial",
    r"video\s*oficial", r"video\s*lyric", r"video\s*con\s*letras?", r"video\s*sencillo",
    r"cancion\s*oficial", r"letras?\s*oficiales?", r"con\s*letras?", r"\bletras?\b",
    r"\bvideoclip\b", r"\bvideo\b", r"\boficial\b", r"\bofficial\b", r"\blyrics?\b",
    r"m[uú]sica\s*cristiana", r"la\s*mejor\s*musica\s*cristiana",
    r"\d{2,3}\s*kbps", r"\b\d{3}\s*k\b", r"\bhd\b", r"\bfull\s*hd\b", r"\b4k\b",
    r"y2mate\.com\s*-*", r"ssvid\.net\s*-*", r"savefrom\.net", r"ceenaija\.com",
    r"_+m4a_+\d*k_*", r"\bvevo\b", r"\bm4a\b", r"\bmp3\b", r"\bwav\b",
]
SEPARATORS = re.compile(r"\s*[-–—|·:]\s*|\s{2,}")
INVALID_CHARS   = str.maketrans({c: "" for c in '\\/:*?"<>|'})
CONTROL_CHARS     = re.compile("[​-‏‪-‮⁦-⁩]")
# "feat./ft." vale en cualquier sitio; "con" SOLO dentro de parentesis,
# porque hay titulos legitimos que empiezan por con ("Con Poder", "Con Todo").
FEAT = re.compile(r"\s*[\(\[]?\s*\b(?:feat|ft|featuring)\b\.?\s+"
                  r"([^()\[\]]+?)\s*[\)\]]?\s*(?=$|[-–—|(\[])", re.IGNORECASE)
FEAT_IN_PARENS = re.compile(r"\s*[\(\[]\s*(?:feat|ft|featuring|con|junto\s+a)\b\.?\s+"
                        r"([^()\[\]]+?)\s*[\)\]]", re.IGNORECASE)


def strip_accents(s: str) -> str:
    """Quita tildes y diacriticos pero respeta la ñ."""
    s = s.replace("ñ", "\x00").replace("Ñ", "\x01")
    s = "".join(c for c in unicodedata.normalize("NFD", s)
                if unicodedata.category(c) != "Mn")
    return s.replace("\x00", "ñ").replace("\x01", "Ñ")


def capitalize_words(s: str) -> str:
    """Palabras enteras en MAYUSCULA -> Capitalizadas. Respeta CamelCase."""
    return re.sub(r"[A-Za-zÑñ]{2,}",
                  lambda m: m.group(0).capitalize() if m.group(0).isupper() else m.group(0), s)


def split_glued(s: str) -> str:
    """VERSIONButterflyFull -> VERSION Butterfly Full."""
    s = re.sub(r"(?<=[a-z]{2})(?=[A-Z][a-z])", " ", s)
    return re.sub(r"(?<=[A-Z]{3})(?=[A-Z][a-z]{2})", " ", s)


# Las mismas reglas de NOISE, en UNA sola expresion ya compilada. Antes esto
# daba treinta pasadas sobre cada nombre; lo llaman el escaneo (una vez por
# cancion) y la deteccion de duplicados (otra vez por cancion), asi que se
# nota. El resultado es identico: la alternancia prueba las alternativas de
# izquierda a derecha en el mismo orden en que estaban.
_NOISE_RE = re.compile("|".join(NOISE), re.IGNORECASE)


def strip_noise(s: str) -> str:
    s = _NOISE_RE.sub(" ", s)
    s = re.sub(r"^\s*[a-zA-Z]?\d{3,6}\s+", " ", s)
    s = re.sub(r"^\s*\d{1,3}\s*[.\-]\s*", " ", s)
    s = re.sub(r"[\[\{][^\]\}]*[\]\}]", " ", s)
    s = re.sub(r"@\s*[\w.]+", " ", s)
    s = re.sub(r"\(\s*\)", " ", s)
    return s


def clean(s: str) -> str:
    """Pipeline completo sobre un texto sin extension."""
    s = CONTROL_CHARS.sub("", s)
    s = s.replace("_", " ").replace("⁄", "-").replace("·", " ")
    s = strip_noise(s)
    s = strip_accents(s)
    s = capitalize_words(s)
    s = split_glued(s)
    s = s.translate(INVALID_CHARS)
    s = re.sub(r"\s*\.\s*(mp3|m4a|wav|flac)\s*$", "", s, flags=re.IGNORECASE)
    s = re.sub(r"\s*([,;])\s*", r"\1 ", s)
    # al quitar ruido de dentro de un parentesis quedan huecos: "(4K Remaster)"
    # se convertia en "( Remaster)". Se cierran y los vacios ya cayeron antes.
    s = re.sub(r"\(\s+", "(", s)
    s = re.sub(r"\s+\)", ")", s)
    s = re.sub(r"\s{2,}", " ", s)
    return s.strip(" .-–—,")


# Nombres que Windows no deja usar como archivo ni carpeta, con o sin
# extension y sin distinguir mayusculas: «Con» (hay un titulo «Con Poder») o
# «Aux» darian una carpeta imposible de crear en un disco NTFS. En Linux se
# aplica igual para que la biblioteca se pueda copiar a un Windows tal cual.
WINDOWS_RESERVED = {"CON", "PRN", "AUX", "NUL",
                    *(f"COM{i}" for i in range(1, 10)),
                    *(f"LPT{i}" for i in range(1, 10))}


def sanitize(s: str) -> str:
    """Nombre valido de archivo o carpeta en cualquier sistema.

    Quita caracteres prohibidos y de control, los puntos y espacios finales
    (Windows los recorta en silencio y el nombre deja de coincidir) y añade
    «_» a los nombres reservados.
    """
    s = CONTROL_CHARS.sub("", s).translate(INVALID_CHARS)
    s = re.sub(r"\s{2,}", " ", s).strip(" .")
    stem, ext = os.path.splitext(s)
    if stem.upper() in WINDOWS_RESERVED:
        s = stem + "_" + ext
    return s


def extract_feat(text: str) -> tuple[str, str]:
    """Saca el 'feat. X' del texto. Devuelve (texto_sin_feat, invitados)."""
    guests = []
    def _cap(m):
        guests.append(m.group(1).strip(" .,-"))
        return " "
    base = FEAT_IN_PARENS.sub(_cap, text)
    base = FEAT.sub(_cap, base)
    base = re.sub(r"\s{2,}", " ", base).strip(" -–—,")
    return base, ", ".join(i for i in guests if i)


def final_name(artist: str, title: str, feat: str = "",
                 extra: str = "", ext: str = ".mp3") -> str:
    """Construye 'Artista - Titulo (feat. X) (extra).ext'."""
    artist, title = sanitize(artist), sanitize(title)
    n = f"{artist} - {title}" if artist else title
    if feat:
        n += f" (feat. {sanitize(feat)})"
    if extra:
        extra = sanitize(extra)
        if extra and f"({extra.lower()})" not in n.lower():
            n += f" ({extra})"
    return re.sub(r"\s{2,}", " ", n).strip() + ext


def match_key(s: str) -> str:
    """Clave normalizada para comparar canciones (deteccion de duplicados)."""
    s = strip_accents(os.path.splitext(s)[0]).lower()
    s = strip_noise(s)
    s = s.replace("'", "").replace("\u2019", "")   # D'Clario y DClario son lo mismo
    s = re.sub(r"[^\w\s]", " ", s)
    stopwords = {"en","vivo","live","de","el","la","los","las","del","al","un","una","a",
              "tu","mi","es","se","que","no","lo","y","feat","ft","con","the","r","r2"}
    return " ".join(sorted({p for p in s.split() if p not in stopwords and len(p) > 2}))


def free_name(folder: str, name: str) -> str:
    """Si ya existe, aplica el sufijo ' - r', ' - r2', ... (regla de duplicados)."""
    base, ext = os.path.splitext(name)
    candidate, i = name, 0
    while os.path.exists(os.path.join(folder, candidate)):
        i += 1
        candidate = f"{base} - r{'' if i == 1 else i}{ext}"
    return candidate


# ---------------------------------------------------------------- vocabulario

def _flat(s: str) -> str:
    return re.sub(r"[^\w\s]", "", strip_accents(s).lower()).strip()


def vocabulary(artists_dir) -> dict:
    """{clave_plana: NombreRealDeCarpeta} a partir de las carpetas existentes."""
    vocab = {}
    if not os.path.isdir(artists_dir):
        return vocab
    for d in sorted(os.listdir(artists_dir)):
        if os.path.isdir(os.path.join(artists_dir, d)):
            vocab[_flat(d)] = d
    return vocab


def detect_artist(name: str, vocab: dict) -> dict:
    """Intenta resolver artista/titulo usando los artistas ya conocidos.

    Devuelve {'artista','titulo','feat','confianza'}. La confianza dice si el
    resultado es fiable (>=0.8) o si conviene preguntarle a la IA.
    """
    base = clean(os.path.splitext(name)[0])
    base, feat = extract_feat(base)
    flat = _flat(base)

    hits = []
    for k, real_name in vocab.items():
        if not k or len(k) < 3:
            continue
        pos = flat.find(k)
        if pos < 0:
            continue
        if pos == 0:                       score = 0.92   # el nombre empieza por el artista
        elif pos + len(k) >= len(flat):    score = 0.86   # termina por el artista
        else:                              score = 0.60   # aparece en medio
        hits.append((score, len(k), real_name, k))

    if not hits:
        return {"artist": "", "title": base, "feat": feat, "confidence": 0.0}

    hits.sort(key=lambda h: (h[0], h[1]), reverse=True)
    score, _, artist, k = hits[0]

    # varios artistas distintos en el nombre -> ambiguo, que decida la IA
    distinct = {h[2] for h in hits}
    if len(distinct) > 1:
        score = min(score, 0.55)

    pattern = re.compile(r"\s*\b" + r"[\s.'\-]*".join(map(re.escape, k.split())) + r"\b\s*",
                        re.IGNORECASE)
    title = pattern.sub(" ", strip_accents(base))
    title = re.sub(r"^\s*(feat\.?|ft\.?|con|y|&|x)\s+", "", title, flags=re.IGNORECASE)
    title = re.sub(r"\s{2,}", " ", title).strip(" -–—,")

    if not title:
        title, score = base, min(score, 0.5)
    return {"artist": artist, "title": title, "feat": feat, "confidence": round(score, 2)}
