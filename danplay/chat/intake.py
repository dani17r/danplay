"""El lector de listas pegadas, por codigo y sin modelo.

Cada semana llega por WhatsApp la lista de canciones del domingo, con
`*negritas*`, emojis, `@menciones` de quien canta o toca, «Voz», «(En Su
Presencia)» y enlaces de YouTube, y la persona se la pega al asistente. Un
modelo la leia mal: tomaba las menciones por artistas, metia «Voz» en la
busqueda y emparejaba los enlaces con la cancion equivocada. Aqui se lee con
reglas, siempre igual, y el modelo recibe la lista ya hecha.

    parse_setlist(texto)        la lista que hay en un texto (o que no hay)
    read_intent(texto)          lo que la persona pide hacer con ella
    last_setlist(mensajes)      la ultima lista pegada de una conversacion
    clean_youtube_url(url)      la forma canonica de un enlace de video
    youtube_video_id(url)       su id de 11 caracteres

Es puro: no toca la red ni la base de datos. Lo que lee viene de fuera (el
chat de un grupo, un titulo cualquiera), asi que se escribio pensando en que
nadie le va a dar un texto amable: la entrada se acota a 200 KB, se recorre en
tiempo lineal (ninguna expresion regular con cuantificadores anidados sobre
texto de fuera: cada busqueda o tiene un tope de caracteres o no puede cruzar
el delimitador que busca) y `parse_setlist` nunca lanza.

Como se lee (el corpus de muestras esta en tests/fixtures/intake_samples.json):

* La entrada se limpia: invisibles fuera, saltos y espacios raros normalizados.
* Cada linea se trocea en piezas: texto, negrita, enlaces, @menciones,
  parentesis, numeracion, vinetas, emojis. Con las piezas se reconstruyen las
  lineas que el campo de una sola linea aplastaba (numeracion que sigue la
  secuencia 1, 2, 3…, vinetas, negritas, etiquetas «Banda:»).
* Las lineas se agrupan en bloques: una cancion (o un medley de varias) con
  sus enlaces, menciones y notas de debajo.
* Todo lo que no es una cancion se devuelve en `extras` y las menciones en
  `people`: nunca se pierde una linea sin dejar rastro.
* Prefiere no detectar una lista a detectarla mal.
"""

import functools
import itertools
import re
import unicodedata
import urllib.parse
from dataclasses import dataclass, field
from typing import Any

from .. import names
from . import safe

__all__ = [
    "MAX_NOTE",
    "MAX_SETLIST_ITEMS",
    "MAX_TITLE",
    "MAX_URL",
    "PLAYLIST",
    "WATCH",
    "Intent",
    "SetItem",
    "SetlistParse",
    "clean_youtube_url",
    "last_setlist",
    "parse_setlist",
    "read_intent",
    "youtube_video_id",
]

MAX_SETLIST_ITEMS = 60
MAX_TITLE = 200
MAX_URL = 300
MAX_NOTE = 200

MAX_RAW = 400  # lo que se guarda de la linea de donde sale cada cancion
MAX_TEXT = 200_000  # lo que se lee de un texto: mas no cabe en un mensaje
_MAX_LINES = 8_000
_MAX_TOKENS = 60_000
_LOOK = 2_000  # de una linea larga solo se mira el principio para saber que es
_MAX_LINE_TOKENS = 20_000  # piezas de una linea
_MAX_RECS = 3_000  # registros que se leen: una lista de 60 canciones no pasa de unos cientos
_MAX_SEGS = 16  # titulos de un medley
_MAX_URLS = 400  # enlaces que se miran de un texto
_MAX_URL_WORD = 2_000  # una «palabra» mas larga que esto no es un enlace
_MAX_MENTIONS = 1_000
_MAX_EXTRAS = 40
_MAX_PEOPLE = 60
_MAX_ITEM_PEOPLE = 12
_MAX_PERSON = 80
_PAREN_WINDOW = 160  # lo mas lejos que se busca el cierre de un parentesis
_BRACKET_WINDOW = 200
_HAND_WINDOW = 120  # lo mas largo que puede ser lo escrito a mano tras una mencion
_NAME_WORDS = 8  # palabras de una mencion

# --------------------------------------------------------------------- datos


@dataclass
class SetItem:
    """Una cancion de la lista pegada, tal como se leyo.

    `n` es la posicion de lectura (1, 2, 3…), no el numero escrito. `title` es
    el titulo legible (tildes, mayusculas y puntuacion propia) y `query` su
    forma de busqueda (`names.search_form`): lo unico que se manda a la
    biblioteca y a YouTube. `artist` solo se rellena si el texto lo dice con
    claridad (titulo en negrita y, justo detras, texto plano); si no, el
    artista se queda dentro de `title`. `people` son las @menciones (quien lo
    toca, nunca el artista) y `note` lo entre parentesis, lo escrito a mano
    tras una mencion, el tono y roles como «Voz». `url` es la forma canonica
    del enlace (`clean_youtube_url`) y `kind` dice que enlace es: "" ninguno,
    "video" o "playlist". `raw` es la linea de donde sale.
    """

    n: int | None = None
    title: str = ""
    query: str = ""
    artist: str = ""
    url: str = ""
    kind: str = ""
    people: list[str] = field(default_factory=list)
    note: str = ""
    raw: str = ""


@dataclass
class SetlistParse:
    """Lo que hay en un texto pegado.

    `is_setlist` solo con dos canciones o mas (marcas de lista, enlaces o un
    titulo por linea), o con un unico enlace de video en un mensaje que pide
    algo. `name_hint` es el nombre que da la cabecera («Domingo»), nunca el de
    la orden de la persona. `people` son todas las menciones distintas, por
    orden de aparicion (integrantes, no artistas). `extras` son las lineas que
    no se tomaron por cancion (la orden, «Banda:», lo tachado…) y `truncated`
    cuantas canciones sobran por encima de `MAX_SETLIST_ITEMS`.
    """

    is_setlist: bool = False
    name_hint: str = ""
    items: list[SetItem] = field(default_factory=list)
    people: list[str] = field(default_factory=list)
    extras: list[str] = field(default_factory=list)
    truncated: int = 0


@dataclass
class Intent:
    """Lo que la persona pide hacer, leido de SUS palabras.

    Lo decide el codigo y no el modelo (revisor F4/S3/T5): `check` no cambia
    nada; `no_download` prohibe proponer descargas; `replace` es el unico
    disparador de dejar una lista EXACTAMENTE como esta; `create` el de crear
    una lista; `strict_version` pide la version del enlace y no otra.
    """

    check: bool = False
    no_download: bool = False
    replace: bool = False
    create: bool = False
    strict_version: bool = False


# ------------------------------------------------------------ enlaces de YouTube

WATCH = "https://www.youtube.com/watch?v="
PLAYLIST = "https://www.youtube.com/playlist?list="

_VIDEO_ID = re.compile(r"[A-Za-z0-9_-]{11}")
_LIST_ID = re.compile(r"[A-Za-z0-9_-]{2,80}")
_SHORT_HOSTS = frozenset({"youtu.be", "www.youtu.be"})
_ID_IN_PATH = frozenset({"shorts", "live", "embed", "v"})
_MAX_URL_PARSE = 2_000


@functools.cache
def _yt():
    """El modulo `youtube`, en el momento de usarlo.

    Importarlo arrastra la biblioteca, las etiquetas ID3, la IA y la
    configuracion (medio segundo): este lector es puro y no lo necesita para
    leer una lista, solo para saber que dominios son de YouTube.
    """
    from .. import youtube

    return youtube


def _query_params(query: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for pair in query.split("&", 64)[:64]:
        key, _, value = pair.partition("=")
        out.setdefault(key, value)
    return out


def _youtube_parts(url) -> tuple[str, str] | None:
    """(id de video, id de lista) de un enlace de YouTube; None si no lo es.

    El id de lista solo se da cuando NO hay video (`/playlist?list=…`): un
    `list=` junto a un `v=` es el mix desde el que se compartio el enlace y se
    descarta. Mira el DOMINIO de verdad con `youtube.host_of` y la lista cerrada
    `youtube.ALLOWED_HOSTS`, nunca el texto: «youtube.com.evil.example»,
    «youtube.com@evil» o «evil.example/watch?v=…» no son de YouTube.
    """
    text = url.strip() if isinstance(url, str) else ""
    if not text or len(text) > _MAX_URL_PARSE:
        return None
    if any(c.isspace() or c < " " or c in "\x7f\\" for c in text):
        return None
    if text.startswith("//"):
        text = "https:" + text
    elif "://" not in text:
        text = "https://" + text
    yt = _yt()
    host = yt.host_of(text)
    if host not in yt.ALLOWED_HOSTS:
        return None
    try:
        parts = urllib.parse.urlsplit(text)
    except ValueError:
        return None
    # ni «usuario@» ni puertos raros: un enlace de YouTube de verdad no los lleva
    if parts.netloc.lower() not in (host, f"{host}:443", f"{host}:80"):
        return None
    segments = [s for s in parts.path.split("/") if s]
    query = _query_params(parts.query)
    video = ""
    if host in _SHORT_HOSTS:
        video = segments[0] if segments else ""
    elif segments and segments[0] == "watch":
        video = query.get("v", "")
    elif len(segments) >= 2 and segments[0] in _ID_IN_PATH:
        video = segments[1]
    if not _VIDEO_ID.fullmatch(video):
        video = ""
    listing = ""
    if not video and segments and segments[0] in ("playlist", "watch"):
        listing = query.get("list", "")
        if not _LIST_ID.fullmatch(listing):
            listing = ""
    return video, listing


def clean_youtube_url(url) -> str:
    """La forma canonica de un enlace de YouTube; "" si no lo es.

    Un video sale siempre como «https://www.youtube.com/watch?v=<id>», venga
    como venga (youtu.be/ID, m. o music., sin «https://», con rastreo): se
    quitan `si`, `feature`, `pp`, `t`, `index`, `start_radio` y `list=` cuando
    hay `v=` (una lista entera no se baja desde un enlace pegado). Una URL
    «/playlist?list=ID» sin video sale como «https://www.youtube.com/playlist?list=ID».
    """
    parts = _youtube_parts(url)
    if parts is None:
        return ""
    video, listing = parts
    if video:
        return WATCH + video
    if listing:
        return PLAYLIST + listing
    return ""


def youtube_video_id(url) -> str:
    """El id de 11 caracteres de un video de YouTube; "" si no es de un video."""
    parts = _youtube_parts(url)
    return parts[0] if parts else ""


def _url_info(raw: str) -> tuple[str, str]:
    """(forma canonica, categoria) de un enlace: «video», «playlist» u «other»."""
    canon = clean_youtube_url(raw)
    if canon.startswith(WATCH):
        return canon, "video"
    if canon.startswith(PLAYLIST):
        return canon, "playlist"
    return "", "other"


# ----------------------------------------------------------- limpieza del texto

_LINEBREAKS = re.compile("[\x0b\x0c\x85\U00002028\U00002029]")
_SPACES = re.compile(
    "[\t\x1c-\x1f\U000000a0\U00001680\U00002000-\U0000200a\U0000202f\U0000205f\U00003000]"
)
# «[8:14 p. m., 9/10/2025] Nombre:» (WhatsApp Web) y «9/10/25 20:14 - Nombre:» (exportado):
# metadatos del chat, no parte de la lista
_WA_PREFIX = re.compile(r"\[\d{1,2}:\d{2}[^\]\n]{0,30}\]\s?[^:\n*]{1,40}:\s*")
_WA_EXPORT = re.compile(
    r"\d{1,2}/\d{1,2}/\d{2,4},? \d{1,2}:\d{2}(?::\d{2})?(?: ?[ap]\.? ?m\.?)? - [^:\n*]{1,40}: "
)
_MD_HASH = re.compile(r"#{1,6} +")
# «1 Digno De Adorar»: numeracion sin punto ni parentesis; solo vale si las lineas siguen 1, 2, 3…
_NUM_START = re.compile(r"\W{0,3}\d{1,2}(?:[.)]|\.?[-–—])")
_SOFT_NUM = re.compile(r"(\d{1,2}) +(?=[^\W\d_])")


def _prepare(text) -> str:
    s = text if isinstance(text, str) else ""
    s = safe.strip_invisible(s[:MAX_TEXT])
    s = s.replace("\r\n", "\n").replace("\r", "\n")
    s = _LINEBREAKS.sub("\n", s)
    s = _SPACES.sub(" ", s)
    return unicodedata.normalize("NFC", s)


def _soft_numbering(lines: list[str]) -> list[str]:
    """Pone punto a la numeracion «1 titulo / 2 titulo» cuando es de verdad una secuencia.

    «3 canciones nuevas» o «2 de ellos» no son numeracion: hacen falta al menos dos
    lineas que empiecen por 1, 2, 3… seguidos, y ninguna otra numeracion en el texto.
    """
    found: list[tuple[int, int, int]] = []  # (linea, numero, donde empieza el titulo)
    for i, line in enumerate(lines):
        m = _SOFT_NUM.match(line)
        if m:
            found.append((i, int(m.group(1)), m.end()))
        elif _NUM_START.match(line):
            return lines  # ya hay numeracion de la de siempre
    if len(found) < 2 or found[0][1] not in (1, 2, 3):
        return lines
    if any(b[1] != a[1] + 1 for a, b in itertools.pairwise(found)):
        return lines
    out = list(lines)
    for i, num, end in found:
        out[i] = f"{num}. {lines[i][end:]}"
    return out


def _fold(s: str) -> str:
    """Minusculas y sin tildes (la ñ tambien): para comparar palabras sueltas."""
    d = unicodedata.normalize("NFD", s.lower())
    return "".join(c for c in d if unicodedata.category(c) != "Mn")


# ------------------------------------------------------------------ vocabulario
# Todo sin tildes y en minusculas: se compara con `_fold(palabra)`.


def _vocab(text: str) -> frozenset[str]:
    """Un conjunto de palabras escritas separadas por espacios."""
    return frozenset(text.split())


_GENERIC = _vocab("setlist repertorio lista canciones cantos alabanzas programa orden reproduccion")
_DAYS = _vocab("lunes martes miercoles jueves viernes sabado domingo")
_CONNECTORS = _vocab("de del la el los las para")
_PARTICLES = _vocab("de del la las los da di van von")
_CONJUNCTIONS = _vocab("y e o u con and")
# el texto de un enlace de markdown que no es el titulo («[Ver video](url)»)
_LINK_WORDS = _vocab(
    "ver video videoclip link enlace aqui aca click clic youtube escuchar escucha oir "
    "letra audio cancion version mira en el la de"
)
# charla: una linea que empieza asi no es el titulo de una cancion
_DISCOURSE = _vocab(
    "oye hola buenas buenos que pero porque mira bueno ok okay gracias disculpa perdon "
    "listo entonces ademas tambien"
)
# lo que se contesta en un chat: sueltas no son un titulo; al empezar una linea escrita en
# minusculas tampoco (con mayuscula, «Este Es El Dia» si lo es)
_CHAT_ONE = _vocab(
    "si no ok okay vale dale listo perfecto claro gracias bien ya eso hola buenas bueno "
    "genial excelente exacto correcto cierto aja"
)
_CHAT_START = _vocab(
    "eso esa ese esto ya vale dale cuantas cuantos cual cuales quien quienes donde cuando "
    "creo tengo puedes podrias quiero necesito voy vamos si y"
)
_POINTING = _vocab("esto estas estos esas esos siguiente siguientes abajo")
_PLEASANTRY = _vocab(
    "hola buenas buenos dias tardes noches gracias por favor saludos bendiciones dios te "
    "les bendiga que amen ok listo ya muchas"
)
# verbos con los que se opera una lista (imperativo, infinitivo y subjuntivo): una
# linea que los lleva pegados a un pronombre («agregame», «bajalas») es una orden
_ORDER_VERBS = _vocab(
    "agrega agregar agregue agregues anade anadir anada anadas mete meter meta metas "
    "pon poner ponga pongas incluye incluir incluya suma sumar baja bajar baje bajes "
    "descarga descargar descargue descargues busca buscar busque busques arma armar arme "
    "armes crea crear cree crees haz hacer haga hagas sube subir usa usar use uses "
    "completa completar complete completes verifica verificar verifique comprueba "
    "comprobar compruebe revisa revisar revise chequea chequear checa borra borrar "
    "elimina eliminar quita quitar ordena ordenar reordena reordenar corrige corregir "
    "reemplaza reemplazar sustituye sustituir deja dejar guarda guardar copia"
)
_ASK_VERBS = _ORDER_VERBS | _vocab("quiero necesito quisiera puedes podrias podria")
# la orden en negativo de los que faltan arriba («no la borres», «no toques nada»)
_NEG_ORDERS = _vocab("borres borre elimines elimine quites quite toques toque cambies cambie")
# las palabras que dicen que la orden habla de la lista y no de otra cosa
_LISTISH = _vocab(
    "lista listas repertorio repertorios setlist playlist canciones cancion biblioteca "
    "esto esta este ese esa estas estos esas esos enlace link video url"
)
# tareas de la vida real que NO son canciones («Afinar la bateria», «Comprar baquetas»)
_TASK_VERBS = _vocab(
    "afinar comprar llevar revisar preparar traer limpiar ordenar armar cargar probar avisar "
    "confirmar llamar escribir enviar mandar imprimir cambiar reservar pagar recoger guardar "
    "instalar actualizar configurar subir bajar descargar borrar eliminar crear agregar "
    "anadir poner quitar cerrar marcar renombrar mover copiar mostrar hacer buscar verificar "
    "comprobar revisa quita ordena avisa borra elimina crea agrega anade pon baja descarga "
    "busca verifica comprueba marca cambia renombra guarda manda envia mueve copia muestra "
    "haz arma afina compra prepara confirma llama escribe imprime reserva paga recoge "
    "instala actualiza configura ver"
)
_CLITIC = re.compile(r"(?:me|te|se|nos|las?|los?|les?)$")


def _verb_base(word: str, pool: frozenset[str] | None = None) -> str:
    """La palabra (ya sin tildes) sin los pronombres pegados: «agregalas» -> «agrega».

    Se quitan de uno en uno desde el final (como mucho tres), porque el verbo
    tambien puede acabar como un pronombre: «metela» es «mete» + «la», no «me».
    Con `pool` se queda con la primera forma que este en el (la menos recortada);
    sin el, con la mas recortada que conserve tres letras.
    """
    if not 4 < len(word) <= 24:  # un verbo con pronombres no es mas largo
        return word
    forms = [word]
    w = word
    for _ in range(3):
        m = _CLITIC.search(w)
        if m is None or m.start() < 3:
            break
        w = w[: m.start()]
        forms.append(w)
    if pool is not None:
        return next((f for f in forms if f in pool), word)
    return forms[-1]


def _is_ask(word: str) -> bool:
    return _verb_base(word, _ASK_VERBS) in _ASK_VERBS


def _is_task(word: str) -> bool:
    return _verb_base(word, _TASK_VERBS) in _TASK_VERBS


# ------------------------------------------------------------------ piezas

TXT, BO, BC, SO, SC = "txt", "bo", "bc", "so", "sc"  # texto, negrita, tachado
URL, MEN, PAR, NUM, BUL = "url", "men", "par", "num", "bul"
EMO, SEP, GAP, NL = "emo", "sep", "gap", "nl"


class _T:
    """Una pieza de una linea: lo que el lector ve despues de trocearla.

    Un texto que se junta con otro (`merge`) guarda los trozos en una lista y
    los une solo cuando alguien lo lee: juntar mil trozos no copia mil veces.
    """

    __slots__ = ("_raw", "_s", "k", "ls", "v")

    def __init__(self, k: str, s: str = "", raw: str = "", v: Any = None, ls: bool = False):
        self.k = k  # el tipo (las constantes de abajo)
        self._s: str | None = s if (s or k != TXT) else None  # el contenido limpio
        self._raw: str | list[str] = raw  # el texto tal como se escribio
        self.v = v  # carga segun el tipo
        self.ls = ls  # va tras un espacio

    @property
    def raw(self) -> str:
        if not isinstance(self._raw, str):
            self._raw = "".join(self._raw)
        return self._raw

    @property
    def s(self) -> str:
        if self._s is None:
            self._s = " ".join(self.raw.split())
        return self._s

    def merge(self, other: "_T") -> None:
        if isinstance(self._raw, str):
            self._raw = [self._raw]
        self._raw.append(other.raw)
        self._s = None


# marcas que se dejan en la linea mientras se trocea (no hay otras iguales en el
# texto: `safe.strip_invisible` ya tiro los controles)
_BO, _BC, _SO, _SC, _UL, _UR = "\x01", "\x02", "\x03", "\x04", "\x07", "\x08"
_MARKS = {_BO: BO, _BC: BC, _SO: SO, _SC: SC}

_EMOJI_CLASS = (
    "[\U00002190-\U000021ff\U0000231a-\U000023ff\U000024c2\U000025a0-\U000027bf"
    "\U00002900-\U0000297f\U00002b00-\U00002bff\U00003030\U0000303d\U00003297\U00003299"
    "\U0000fe0f\U000020e3\U0001f000-\U0001faff]"
)
_EMOJI_RE = re.compile(_EMOJI_CLASS + "+")
# «1.» «1)» «1.-» «1 -» «1 –» «1.—» «1-»: el punto no cuenta si le sigue otra cifra (10.000)
_NUM = (
    r"(?<![^\W_])(?<![.,/:\-])"
    r"(?:(?P<key>\d)[\U0000fe0f]?\U000020e3"
    r"|(?P<nv>\d{1,2})(?:(?:\.(?!\d)|\))[ ]*[-–—]?|[ ]*[-–—](?!\d))[ ]*)"
)
_TOKEN = re.compile(
    r"(?P<gap> {2,})"
    r"|(?P<fmt>[\x01-\x04])"
    r"|(?P<url>\x07\d+\x08)"
    r"|(?P<at>@)"
    r"|(?P<lp>\()"
    r"|(?P<lb>\[)"
    r"|(?P<sep>(?<= )[&+/](?= ))"
    r"|(?P<bul>[•‣])"
    r"|(?P<star>\*+)"
    r"|(?P<ten>\U0001f51f)"
    r"|(?P<emo>" + _EMOJI_CLASS + r"+)"
    r"|(?P<num>" + _NUM + r")"
)
_CONTENT_GROUPS = frozenset({"url", "at", "lp", "lb", "sep", "bul", "num", "ten"})
_LEAD_BULLET = re.compile(r"[-–—] +")
_WORD = re.compile(r"\S+")
_NAME_WORD = re.compile(r"[^\W_][\w'’\-]*")
_HAND_STOP = re.compile(r"  |[\x01-\x08@(\[•‣]|" + _EMOJI_CLASS + r"| (?:\d{1,2}(?:[.)]|\.?[-–—]))")
# negrita, tachado y cursiva: el interior no puede llevar el delimitador, asi que cada
# intento acaba en el siguiente y la busqueda es lineal
_BOLD = re.compile(r"(?<![^\W_])(?<!\*)\*{1,3}(?=[^\s*])([^*\n]{1,300}?)(?<=[^\s*])\*{1,3}")
_STRIKE = re.compile(r"(?<![^\W_])(?<!~)~(?=[^\s~])([^~\n]{1,300}?)(?<=[^\s~])~")
_ITALIC = re.compile(r"(?<![^\W_])(?<!_)_(?=[^\s_])([^_\n]{1,300}?)(?<=[^\s_])_(?![^\W_])")
_MD_LINK = re.compile(r"\[([^\[\]\n]{0,200})\]\(\s*(\x07\d+\x08)\s*\)")
_PLACEHOLDER = re.compile(r"\x07(\d+)\x08")
# «Etiqueta: valor»: 1-3 palabras, la primera con mayuscula, y dos puntos
_SECTION = re.compile(r"[A-ZÁÉÍÓÚÑÜ][\w'’.\-]*(?: [\w'’.\-]+){0,2}:(?: |$)")
_LEAD_MEDLEY = re.compile(r"(?i)(medley|contiene)\b *:? *")
_URL_TAIL = ".,;:!?)]}>'\"»”’*~…"
_URL_PREFIXES = (
    "www.",
    "youtu.be/",
    "youtube.com/",
    "m.youtube.com/",
    "music.youtube.com/",
    "youtube-nocookie.com/",
)


def _url_start(w: str) -> int:
    """Donde empieza un enlace dentro de una palabra (-1 si no hay)."""
    i = w.find("://")
    if i >= 0:
        j = i
        while j > 0 and (w[j - 1].isalnum() or w[j - 1] in "+.-"):
            j -= 1
        return j if j < i else -1
    k = 0
    while k < len(w) and not w[k].isalnum():
        k += 1
    rest = w[k:].lower()
    if rest.startswith(_URL_PREFIXES):
        return k
    slash = rest.find("/")
    if slash > 3:
        host = rest[:slash]
        tld = host.rpartition(".")[2]
        if (
            "." in host
            and tld.isalpha()
            and len(tld) >= 2
            and all(c.isalnum() or c in ".-" for c in host)
            and host[0] != "."
        ):
            return k
    return -1


class _Ctx:
    """Lo que se acumula al trocear un texto: la tabla de enlaces y los topes."""

    __slots__ = ("mentions", "urls")

    def __init__(self) -> None:
        self.urls: list[tuple[str, str, str]] = []  # (como se escribio, canonica, categoria)
        self.mentions = 0


def _protect_urls(line: str, ctx: _Ctx) -> str:
    """Cambia cada enlace por una marca (control-7 numero control-8).

    Asi lo que lleva `_` o `~` (los ids de YouTube) no se confunde con una cursiva
    ni con un tachado, y la puntuacion pegada (`url.`, `(url)`, `url,`) queda
    fuera del enlace.
    """
    out: list[str] = []
    last = 0
    for m in _WORD.finditer(line):
        w = m.group()
        if "/" not in w and not w[:8].lower().startswith("www."):
            continue
        start = _url_start(w)
        if start < 0:
            continue
        out.append(line[last : m.start()])
        last = m.end()
        if len(w) > _MAX_URL_WORD or len(ctx.urls) >= _MAX_URLS:
            out.append(" ")
            continue
        end = len(w)
        while end > start and w[end - 1] in _URL_TAIL:
            end -= 1
        if end > start and w[end - 1] == "_" and start > 0 and w[start - 1] == "_":
            end -= 1  # _https://…_ en cursiva
        raw = w[start:end]
        canon, cat = _url_info(raw)
        ctx.urls.append((raw, canon, cat))
        out.append(f"{w[:start]}{_UL}{len(ctx.urls) - 1}{_UR}{w[end:]}")
    out.append(line[last:])
    return "".join(out)


def _md_link(m: re.Match) -> str:
    label = m.group(1).strip()
    words = _fold(label).replace("-", " ").split()
    if not words or all(w.strip(".,:;!?¡¿") in _LINK_WORDS for w in words):
        return m.group(2)
    return f"{label} {m.group(2)}"


def _mark(line: str, ctx: _Ctx) -> str:
    """La linea con enlaces, negrita y tachado cambiados por marcas de control."""
    s = _protect_urls(line, ctx)
    if "[" in s:
        s = _MD_LINK.sub(_md_link, s)
    if "*" in s:
        s = _BOLD.sub(lambda m: f"{_BO}{m.group(1)}{_BC}", s)
    if "~" in s:
        s = _STRIKE.sub(lambda m: f"{_SO}{m.group(1)}{_SC}", s)
    if "_" in s:
        s = _ITALIC.sub(lambda m: m.group(1), s)
    return s


def _is_cap(w: str) -> bool:
    return w[0].isupper() or w[0].isdigit()


def _mention(s: str, at: int) -> tuple[str, str, int] | None:
    """(nombre, nota escrita a mano, fin) de la @mencion que empieza en s[at].

    El nombre es la racha de palabras con mayuscula o cifra (las siglas, como
    «JCM», cuentan); las particulas en minuscula (de, del, la, los…) siguen
    el nombre si les sigue una palabra con mayuscula. Acaba en cualquier otra
    cosa: una coma, un parentesis, un enlace, un emoji, otra @, el fin de
    linea o DOS espacios. Lo que sigue en minuscula, hasta el siguiente limite,
    es texto escrito a mano («voz principal»): la nota.
    """
    n = len(s)
    m = _NAME_WORD.match(s, at + 1)
    if m is None:
        return None
    words = [m.group()]
    pos = m.end()
    while len(words) < _NAME_WORDS and pos + 1 < n and s[pos] == " " and s[pos + 1] != " ":
        m = _NAME_WORD.match(s, pos + 1)
        if m is None:
            break
        w = m.group()
        if w.isdigit() and s[m.end() : m.end() + 1] in (".", ")", "-", "–", "—"):
            break  # «@Ana 2) Titulo»: el 2 es la numeracion de la siguiente
        if _is_cap(w):
            words.append(w)
            pos = m.end()
            continue
        if w.lower() not in _PARTICLES:
            break
        chain, p, joined = [w], m.end(), False
        while len(chain) < 4 and p + 1 < n and s[p] == " " and s[p + 1] != " ":
            m2 = _NAME_WORD.match(s, p + 1)
            if m2 is None:
                break
            w2 = m2.group()
            if _is_cap(w2):
                words.extend([*chain, w2])
                pos, joined = m2.end(), True
                break
            if w2.lower() not in _PARTICLES:
                break
            chain.append(w2)
            p = m2.end()
        if not joined:
            break
    name = " ".join(words)[:_MAX_PERSON]
    hand, end = "", pos
    if pos + 1 < n and s[pos] == " " and s[pos + 1] != " ":
        j = pos + 1
        stop = _HAND_STOP.search(s, j, j + _HAND_WINDOW)
        e = stop.start() if stop else min(n, j + _HAND_WINDOW)
        text = s[j:e].strip()
        if text and text[0].islower():
            hand, end = text, e
    if hand:
        words_h = hand.split()
        while words_h and _fold(words_h[-1]) in _CONJUNCTIONS:
            words_h.pop()
        hand = " ".join(words_h)
        if all(_fold(w) in _CONJUNCTIONS for w in hand.split()):
            hand = ""
    return name, hand[:MAX_NOTE], end


def _plain(inner: str) -> str:
    """El texto de un parentesis sin marcas de control ni enlaces."""
    inner = _PLACEHOLDER.sub(" ", inner)
    inner = re.sub("[\x01-\x08]", "", inner)
    return " ".join(inner.split())


def _add(toks: list[_T], t: _T) -> None:
    """Anade una pieza; dos textos seguidos se juntan."""
    if t.k == TXT and toks and toks[-1].k == TXT:
        toks[-1].merge(t)
        return
    toks.append(t)


def _scan_line(s: str, toks: list[_T], ctx: _Ctx) -> None:
    """Trocea una linea (ya con sus marcas de control) en piezas."""
    n = len(s)
    pos = 0
    first = len(toks)
    content = False  # ya se leyo algo que no es viñeta, emoji ni espacio
    m = _LEAD_BULLET.match(s)
    if m:
        toks.append(_T(BUL, raw=m.group().strip()))
        content = True
        pos = m.end()
    pending: list[str] = []  # texto que espera a la siguiente pieza
    pending_text = False
    tstart = pos

    def flush(upto: int) -> None:
        nonlocal content, pending_text
        raw = "".join(pending) + s[tstart:upto]
        pending.clear()
        pending_text = False
        if raw.strip():
            content = True
            ls = raw[0] == " " or (tstart > 0 and s[tstart - 1] == " ")
            _add(toks, _T(TXT, raw=raw, ls=ls))

    while pos < n and len(toks) - first < _MAX_LINE_TOKENS:
        m = _TOKEN.search(s, pos)
        if m is None:
            break
        a, b = m.start(), m.end()
        g = m.lastgroup
        ls = a > 0 and s[a - 1] == " "
        pos = b
        junk = False  # lo que no sirve (un `(` sin cierre): fuera, sin partir el texto
        if g == "gap":
            flush(a)
            toks.append(_T(GAP, raw=m.group()))
        elif g == "fmt":
            flush(a)
            toks.append(_T(_MARKS[m.group()], ls=ls))
        elif g == "url":
            flush(a)
            raw, canon, cat = ctx.urls[int(m.group()[1:-1])]
            toks.append(_T(URL, s=canon, raw=raw, v=(canon, cat), ls=ls))
        elif g == "at":
            glued = a > 0 and s[a - 1].isalnum()  # un correo, no una mencion
            found = None if glued or ctx.mentions >= _MAX_MENTIONS else _mention(s, a)
            if found is None:
                junk = True
            else:
                name, hand, end = found
                ctx.mentions += 1
                flush(a)
                toks.append(_T(MEN, s=name, raw=s[a:end].strip(), v=hand, ls=ls))
                pos = b = end
        elif g in ("lp", "lb"):
            closer, window = (")", _PAREN_WINDOW) if g == "lp" else ("]", _BRACKET_WINDOW)
            close = s.find(closer, a + 1, a + 1 + window)
            inner = s[a + 1 : close] if close > 0 else ""
            # un parentesis con otro dentro, con una @mencion o sin cierre solo estorba
            if close < 0 or "(" in inner or "[" in inner or "@" in inner:
                junk = True
            else:
                flush(a)
                pos = b = close + 1
                for idx in _PLACEHOLDER.findall(inner):
                    raw, canon, cat = ctx.urls[int(idx)]
                    toks.append(_T(URL, s=canon, raw=raw, v=(canon, cat), ls=ls))
                text = _plain(inner)
                # isdecimal y no isdigit: «(²)» es una cifra para isdigit, pero int() no la lee
                if g == "lp" and text.isdecimal() and len(text) <= 2:
                    toks.append(_T(NUM, raw=s[a:b], v=int(text), ls=ls))
                elif text:
                    toks.append(_T(PAR, s=text, raw=f"({text})", v=s[a], ls=ls))
        elif g == "sep":
            flush(a)
            toks.append(_T(SEP, raw=m.group(), ls=True))
        elif g == "bul":
            flush(a)
            toks.append(_T(BUL, raw=m.group(), ls=ls))
        elif g == "star":
            lone = b - a == 1 and (b >= n or s[b] == " ")
            bullet = lone and (a == 0 or s[a - 1] in (" ", _BC))
            j = b
            while j < n and s[j] == " " and j - b < 4:
                j += 1
            if bullet and (
                not (content or pending_text or s[tstart:a].strip()) or s[j : j + 1] == _BO
            ):
                flush(a)
                toks.append(_T(BUL, raw="*", ls=ls))
            else:
                junk = True
        elif g == "emo":
            flush(a)
            toks.append(_T(EMO, raw=m.group(), ls=ls))
        elif g == "ten":
            flush(a)
            toks.append(_T(NUM, raw=m.group(), v=10, ls=ls))
        else:  # num
            flush(a)
            toks.append(_T(NUM, raw=m.group(), v=int(m.group("nv") or m.group("key")), ls=ls))
        if junk:
            piece = s[tstart:a]
            pending.append(piece)
            pending_text = pending_text or bool(piece.strip())
        elif g in _CONTENT_GROUPS:
            content = True
        tstart = b
    flush(n)


# ------------------------------------------------------------- de piezas a registros


class _Rec:
    """Un registro: lo que cabe en una linea logica (una cancion, una etiqueta…)."""

    __slots__ = ("depth", "last", "line_start", "marker", "nc", "toks")

    def __init__(self, line_start: bool) -> None:
        self.toks: list[_T] = []
        self.line_start = line_start  # empieza una linea del texto
        self.marker: tuple[str, int] | None = None  # ("num", 3) o ("bul", 0)
        self.nc = 0  # piezas con contenido
        self.depth = 0  # negritas abiertas
        self.last = ""  # tipo de la ultima pieza con contenido


def _push(recs: list[_Rec], cur: _Rec) -> None:
    if any(t.k != GAP for t in cur.toks):  # una marca sola («1.-», «•») no dice nada
        recs.append(cur)


def _split(recs: list[_Rec], cur: _Rec) -> _Rec:
    """Corta aqui: lo ya leido es un registro y lo que sigue, otro.

    Las negritas y tachados recien abiertos pasan al nuevo (`*1.- Titulo*`:
    el corte va ANTES del asterisco).
    """
    moved: list[_T] = []
    while cur.toks and cur.toks[-1].k in (BO, SO):
        moved.append(cur.toks.pop())
    _push(recs, cur)
    new = _Rec(False)
    for t in reversed(moved):
        new.toks.append(t)
        new.depth += t.k == BO
    return new


def _content_piece(t: _T) -> bool:
    if t.k == TXT:
        return any(c.isalnum() for c in t.s)
    return t.k in (URL, MEN, PAR, SEP)


def _cut(toks: list[_T]) -> list[_Rec]:
    """Reparte las piezas en registros.

    Corta en cada salto de linea y, donde el campo de una sola linea los habia
    aplastado, en lo que sirve de marca: vinetas «•», numeracion que sigue la
    secuencia (1, 2, 3…), una negrita que abre tras otra cosa y, desde la
    primera etiqueta «Banda:», en cada tramo de 2+ espacios.
    """
    recs: list[_Rec] = []
    cur = _Rec(True)
    seq = 0  # ultimo numero valido de esta linea
    section = False
    for t in toks:
        k = t.k
        if k == NL:
            _push(recs, cur)
            cur = _Rec(True)
            seq = 0
            section = False
            continue
        if k == NUM:
            v = t.v
            true_start = cur.line_start and cur.nc == 0 and cur.marker is None
            if true_start or (v == seq + 1 if seq else 1 <= v <= 3):
                seq = v
                if cur.nc > 0 or cur.marker is not None:
                    cur = _split(recs, cur)
                cur.marker = ("num", v)
                section = False
                continue
            t = _T(TXT, raw=t.raw, ls=t.ls)  # no era numeracion: texto
            k = TXT
        elif k == BUL:
            if cur.nc > 0 or cur.marker is not None:
                cur = _split(recs, cur)
            cur.marker = ("bul", 0)
            section = False
            continue
        elif k == BO:
            if cur.nc > 0 and cur.last != SEP and cur.depth == 0:
                cur = _split(recs, cur)
            cur.depth += 1
        elif k == BC:
            cur.depth = max(0, cur.depth - 1)
        elif k == GAP and section and cur.nc > 0:
            cur = _split(recs, cur)
            continue
        if k == TXT and cur.nc == 0:
            if _SECTION.match(t.s):
                head = t.s.split(":", 1)[0].strip().lower()
                section = section or (head not in ("medley", "contiene") and cur.marker is None)
            elif cur.depth > 0:
                section = False  # una negrita que no es etiqueta: vuelven las canciones
        if _content_piece(t):
            cur.nc += 1
            cur.last = t.k
        _add(cur.toks, t)
    _push(recs, cur)
    return recs


# ----------------------------------------------------------- de registros a canciones


class _Seg:
    """Un titulo de un registro (un medley tiene varios, separados por & + /)."""

    __slots__ = (
        "art",
        "artist",
        "bold",
        "closed",
        "notes",
        "nparts",
        "nstruck",
        "people",
        "tail",
        "title",
        "urls",
    )

    def __init__(self) -> None:
        self.title: list[str] = []
        self.artist: list[str] = []
        self.urls: list[tuple[str, str]] = []  # (canonica, categoria)
        self.notes: list[str] = []
        self.people: list[str] = []
        self.tail = False  # ya paso lo que no es titulo: enlace, mencion, parentesis
        self.closed = False  # el titulo iba en negrita y ya cerro
        self.art = False  # justo tras la negrita: ahi va el artista
        self.bold = False
        self.nparts = 0
        self.nstruck = 0


class _Read:
    """Lo que dice un registro, ya sin marca ni etiqueta."""

    __slots__ = ("bold_led", "order", "segs")

    def __init__(self, segs: list[_Seg], order: list[str], bold_led: bool) -> None:
        self.segs = segs
        self.order = order  # las menciones, en orden de aparicion
        self.bold_led = bold_led  # lo primero es una negrita

    @property
    def has_title(self) -> bool:
        return any(sg.title for sg in self.segs)

    @property
    def urls(self) -> list[tuple[str, str]]:
        return [u for sg in self.segs for u in sg.urls]

    @property
    def has_notes(self) -> bool:
        return any(sg.notes for sg in self.segs)


_EDGE = " .,;:-–—·|'’‘()[]{}<>•/\\"


def _note_ok(s: str) -> bool:
    return any(c.isalnum() for c in s) and _fold(s).strip(" .,;:!?¡¿-") not in _CONJUNCTIONS


def _read(toks: list[_T]) -> _Read:
    segs = [_Seg()]
    seg = segs[0]
    bold = strike = 0
    order: list[str] = []
    bold_led = False
    seen = False
    for t in toks:
        k = t.k
        if k in (GAP, EMO):
            seg.art = False
            continue
        if not seen and k != SO:
            seen = True
            bold_led = k == BO
        if k == TXT:
            if seg.tail or (seg.closed and not seg.art):
                if _note_ok(t.s):
                    seg.notes.append(t.s.strip(_EDGE))
            elif seg.closed:
                seg.artist.append(t.s)
            else:
                seg.title.append(t.s)
                seg.nparts += 1
                seg.nstruck += 1 if strike else 0
                seg.bold = seg.bold or bold > 0
        elif k == BO:
            bold += 1
        elif k == BC:
            bold = max(0, bold - 1)
            if seg.bold and bold == 0 and not seg.closed and not seg.tail and seg.title:
                seg.closed = seg.art = True
        elif k == SO:
            strike += 1
        elif k == SC:
            strike = max(0, strike - 1)
        elif k == URL:
            seg.urls.append(t.v)
            seg.tail, seg.art = True, False
        elif k == MEN:
            seg.people.append(t.s)
            order.append(t.s)
            if t.v:
                seg.notes.append(t.v)
            seg.tail, seg.art = True, False
        elif k == PAR:
            if t.v == "[" and not seg.title and not seg.tail and not seg.closed:
                seg.title.append(t.s)  # «[Titulo]»: los corchetes envuelven el titulo
                seg.nparts += 1
            else:
                if _note_ok(t.s):
                    seg.notes.append(t.s)
                if seg.title or seg.urls:  # «(En vivo) Rey Infinito»: el titulo viene despues
                    seg.tail, seg.art = True, False
        elif k == SEP and (seg.title or seg.urls) and len(segs) < _MAX_SEGS:
            seg = _Seg()
            segs.append(seg)
    return _Read(segs, order, bold_led)


def _lead_label(toks: list[_T], *, section: bool) -> tuple[str, list[_T]]:
    """La etiqueta con la que empieza un registro: «medley», «contiene» o «section».

    «Medley:» / «Contiene:» (en negrita o sueltas) se quitan de las piezas; una
    etiqueta de seccion («Banda:», «Ensayo: Sabado 7pm») solo se marca.
    """
    n = len(toks)
    i = 0
    while i < n and toks[i].k in (BO, SO, EMO, GAP):
        i += 1
    if i >= n or toks[i].k != TXT:
        return "", toks
    t = toks[i]
    m = _LEAD_MEDLEY.match(t.s)
    if m:
        rest = t.s[m.end() :]
        if rest and ":" not in m.group():
            return "", toks  # «Medley de alabanzas»: un titulo, no la etiqueta
        new = list(toks)
        if rest:
            new[i] = _T(TXT, s=rest, raw=rest, ls=t.ls)
        else:
            wrapped = i > 0 and new[i - 1].k == BO and i + 1 < n and new[i + 1].k == BC
            del new[i]
            if wrapped:  # *Medley*: se va tambien la negrita
                del new[i]
                del new[i - 1]
        return m.group(1).lower(), new
    if section and _SECTION.match(t.s):
        return "section", toks
    return "", toks


def _render(toks: list[_T], *, videos: bool = False, limit: int = _LOOK) -> str:
    """Las piezas de vuelta a texto (sin viñeta, numeracion ni asteriscos).

    Los emojis se dejan y el tachado conserva sus «~». Un enlace de video no
    sale (ya es una cancion) salvo que se pida: dentro de una seccion se queda.
    Se para al pasar de `limit` caracteres: nadie lee mas de unas lineas.
    """
    out: list[str] = []
    space = False
    size = 0
    for t in toks:
        if size > limit:
            break
        k = t.k
        if k in (BO, BC, NUM, BUL, NL):
            space = space or t.ls
            continue
        if k == GAP:
            space = True
            continue
        if k == URL and not videos and t.v[1] != "other":
            continue
        piece = "~" if k in (SO, SC) else (t.s if k == TXT else t.raw)
        if not piece:
            continue
        if out and (t.ls or space):
            out.append(" ")
        out.append(piece)
        size += len(piece)
        space = False
    text = " ".join("".join(out).split())
    return re.sub(r" +([,.;:!?])", r"\1", text)


_JUNK = re.compile(r"[*~`#_]+")
_QUOTE_CHARS = re.compile('["«»“”„‟]+')
_TRAIL_TONE = re.compile(r"\s[-–—]\s([A-G][#b]?m?)$")
_TONE = re.compile(r"[A-G][#b]?m?")


def _clean_title(s: str, limit: int = MAX_TITLE) -> str:
    """Un titulo legible: sin emojis, asteriscos, comillas ni bordes de puntuacion.

    Conserva las tildes, la ñ, las mayusculas y la puntuacion propia («¿Quién
    Es Él?», «10.000 Razones», «D’Clario»).
    """
    s = _EMOJI_RE.sub(" ", s)
    s = _JUNK.sub(" ", s)
    s = _QUOTE_CHARS.sub(" ", s)
    s = " ".join(s.split()).strip(_EDGE)
    s = unicodedata.normalize("NFC", s)
    return s[:limit].rstrip(_EDGE)


def _tidy(s: str, limit: int = MAX_NOTE) -> str:
    s = " ".join(s.split())
    return s if len(s) <= limit else s[: limit - 1].rstrip() + "…"


def _words(text: str) -> list[str]:
    """Las primeras palabras de un texto, sin tildes ni signos."""
    return [w for w in (_fold(x).strip(".,;:!?¡¿()\"'«»") for x in text.split(None, 60)[:60]) if w]


def _title_like(text: str) -> bool:
    """Si una linea SIN marcas parece el titulo de una cancion y no una frase."""
    words = text.split(None, 9)
    n = len(words)
    if n == 0 or n > 8 or text[-1] in ",.;:":
        return False
    first = _fold(words[0].lstrip("¿¡")).strip(".,;:!?")
    if first in _DISCOURSE or (n == 1 and first in _CHAT_ONE):
        return False
    if words[0][0].islower() and first in _CHAT_START:
        return False
    if "?" in text and not (text.startswith("¿") and n <= 4):
        return False
    return any(c.isalpha() for c in text)


def _sentence_like(text: str) -> bool:
    """Si una linea CON marca es una frase y no un titulo (una pregunta, un parrafo)."""
    n = len(text.split(None, 12))
    return n >= 12 or (n >= 4 and text.startswith("¿") and text.rstrip().endswith("?"))


def _title_cased(text: str) -> bool:
    """«Pon Tu Mano», «De Gloria en Gloria»: cada palabra con mayuscula, salvo las cortas.

    Un titulo se escribe asi; una orden o una tarea («Afinar la bateria», «quita las
    repetidas») casi nunca.
    """
    words = text.split(None, 12)
    return (
        len(words) >= 2
        and words[0][0].isupper()
        and all(w[0].isupper() or w[0].isdigit() or len(w) <= 3 for w in words[1:12])
    )


def _introduces(text: str) -> bool:
    """Si una orden presenta lo que viene detras («agregame esto:», «estas canciones»)."""
    if text.rstrip().endswith(":"):
        return True
    return any(w in _POINTING for w in _words(text))


def _is_pleasantry(text: str) -> bool:
    words = _words(text)
    return bool(words) and all(w in _PLEASANTRY for w in words)


def _is_order(text: str, has_url: bool = False) -> bool:
    """Si una linea es una orden o un saludo: algo que se dice, no que se canta.

    Un verbo de operar listas (agrega, baja, descarga…) la delata si va pegado a
    un pronombre («agregame», «bajalas»), si la linea habla de la lista o si
    lleva un enlace: «Pon tu mano» o «Deja que tu gloria caiga» son titulos.
    """
    words = _words(text)
    if not words:
        return False
    if _is_pleasantry(text):
        return True
    if text.rstrip().endswith(":") and len(words) >= 3:
        return True
    listish = any(w in _LISTISH for w in words) or (has_url and not _title_cased(text))
    cased = _title_cased(text)
    for i, w in enumerate(words):
        base = _verb_base(w, _ORDER_VERBS)
        if base in _ORDER_VERBS and (w != base or listish):
            return True
        # «si ya la tenemos no la bajes»: una orden en negativo (un titulo iria con mayusculas)
        if (
            not cased
            and (base in _ORDER_VERBS or _verb_base(w, _NEG_ORDERS) in _NEG_ORDERS)
            and any(x in ("no", "nunca") for x in words[max(0, i - 3) : i])
        ):
            return True
    return False


_DATE = re.compile(
    r"\b\d{1,2}[/.-]\d{1,2}(?:[/.-]\d{2,4})?\b|\b\d{1,2} de [a-záéíóúñ]+(?: de \d{4})?\b",
    re.IGNORECASE,
)


def _header_name(text: str) -> str | None:
    """El nombre de lista que da una cabecera; None si la linea no es una cabecera.

    Cabecera = ≤ 7 palabras que EMPIEZAN por una palabra generica («Setlist»,
    «Repertorio»…), un dia de la semana o «ensayo», sin «:» seguido de valor
    (eso es una seccion). Se quitan las fechas y las palabras genericas
    iniciales con sus conectores: «Repertorio del domingo:» da «domingo» y
    «Setlist» da «».
    """
    t = " ".join(_EMOJI_RE.sub(" ", text).replace("#", " ").replace("*", " ").split())
    if not t or len(t.split()) > 7 or re.search(r":\s*\S", t):
        return None
    words = t.rstrip(":").split()
    if not words:  # «:» o «::» solos
        return None
    first = _fold(words[0])
    if first not in _GENERIC and first not in _DAYS and first != "ensayo":
        return None
    words = _DATE.sub(" ", " ".join(words)).split()
    dropped = False
    while words:
        w = _fold(words[0])
        if w in _GENERIC or (dropped and w in _CONNECTORS):
            dropped = True
            words.pop(0)
        else:
            break
    return " ".join(words).strip(_EDGE)[:MAX_TITLE]


class _Prep:
    """Un registro ya leido: marca, etiqueta y contenido."""

    __slots__ = ("label", "marker", "rd", "toks")

    def __init__(self, rec: _Rec) -> None:
        toks = rec.toks
        marker = rec.marker[0] if rec.marker else ""
        if not marker and rec.line_start and toks and toks[0].k == EMO:
            j = 0
            while j < len(toks) and toks[j].k == EMO:
                j += 1
            nxt = toks[j] if j < len(toks) else None
            if nxt is not None and (nxt.k in (TXT, BO, SO) or (nxt.k == PAR and nxt.v == "[")):
                marker, toks = "emo", toks[j:]  # un emoji de viñeta
        self.marker = marker
        self.label, self.toks = _lead_label(toks, section=marker in ("", "emo"))
        self.rd = _read(self.toks)

    def text(self) -> str:
        return _render(self.toks)

    def titles(self) -> str:
        """Los textos del registro juntos (el principio: no hace falta mas)."""
        out: list[str] = []
        size = 0
        for t in self.toks:
            if t.k == TXT:
                out.append(t.s)
                size += len(out[-1])
                if size > _LOOK:
                    break
        return " ".join(out)


class _Draft:
    """Una cancion a medio leer, antes de repartir los enlaces del bloque."""

    __slots__ = ("artist", "notes", "people", "prep", "title", "url")

    def __init__(self, title: str, prep: "_Prep | None", artist: str = "") -> None:
        self.title = title
        self.artist = artist
        self.prep = prep
        self.url: tuple[str, str] | None = None
        self.notes: list[str] = []
        self.people: list[str] = []


class _Block:
    """Una cancion (o un medley) con lo que cuelga de ella.

    Los enlaces, las menciones y las notas del bloque son de TODAS sus
    canciones; los enlaces se reparten en orden entre las que no traen el suyo.
    """

    __slots__ = ("items", "notes", "open", "people", "queue", "word")

    def __init__(self) -> None:
        self.items: list[_Draft] = []
        self.queue: list[tuple[str, str]] = []
        self.notes: list[str] = []
        self.people: list[str] = []
        self.open = False  # un medley abierto admite mas titulos en negrita
        self.word = False  # la palabra «Medley» esta escrita


class _Asm:
    """Junta los registros en bloques, extras y personas."""

    def __init__(self, *, unmarked: bool) -> None:
        self.unmarked = unmarked
        self.blocks: list[_Block] = []
        self.block: _Block | None = None
        self.section = False
        self.extras: list[str] = []
        self.people: dict[str, str] = {}
        self.chatty = 0
        self.n_items = 0  # titulos leidos hasta ahora
        self.evidence = False  # un enlace o una orden que presenta la lista: no esta sola

    def add_extra(self, text: str) -> None:
        text = _tidy(text)
        if text and len(self.extras) < _MAX_EXTRAS:
            self.extras.append(text)

    def add_person(self, name: str) -> None:
        key = names.search_form(name)
        if key and key not in self.people and len(self.people) < _MAX_PEOPLE:
            self.people[key] = name

    def add_url_block(self, url: tuple[str, str]) -> None:
        self.block = _Block()
        self.block.queue.append(url)
        self.blocks.append(self.block)

    # -- un registro
    def feed(self, p: _Prep, kind: str) -> None:
        for name in p.rd.order:
            self.add_person(name)
        if kind == "blank":
            return
        if p.rd.urls or (kind == "phrase" and _introduces(p.titles())):
            self.evidence = True
        if kind == "section":
            self.block = None
            self.section = True
            self.add_extra(_render(p.toks, videos=True))
        elif kind in ("phrase", "chatty"):
            self.chatty += kind == "chatty"
            self.add_extra(_render(p.toks, videos=self.section))
            if not self.section:
                for u in p.rd.urls:
                    if u[1] != "other":
                        self.add_url_block(u)
        elif kind == "cont":
            self.cont(p)
        else:
            self.section = False
            self.item(p)

    def cont(self, p: _Prep) -> None:
        """Enlaces, menciones y notas sueltos: cuelgan de la cancion de arriba."""
        sg = p.rd.segs[0]
        if self.section:
            self.add_extra(_render(p.toks, videos=True))
            return
        videos = [u for u in sg.urls if u[1] != "other"]
        others = [t for t in p.toks if t.k == URL and t.v[1] == "other"]
        if others:
            self.add_extra(_render(others, videos=True))
        if self.block is None:
            if not videos:
                if not others:
                    self.add_extra(_render(p.toks))
                return
            self.block = _Block()
            self.blocks.append(self.block)
        self.block.queue.extend(videos)
        self.block.people.extend(sg.people)
        self.block.notes.extend(sg.notes)

    def item(self, p: _Prep) -> None:
        rd = p.rd
        segs = [sg for sg in rd.segs if sg.title or sg.urls]
        join = (
            self.block is not None
            and self.block.open
            and not p.marker
            and (rd.bold_led or p.label == "contiene")
        )
        if join:
            b = self.block
        else:
            b = self.block = _Block()
            self.blocks.append(b)
        if b is None:
            return
        if p.label or len(segs) > 1:
            b.open = True
        if p.label == "medley":
            b.word = True
        last = len(segs) - 1
        found = False  # salio una cancion o un enlace de YouTube
        for i, sg in enumerate(segs):
            title = " ".join(sg.title)
            # un enlace que no es de YouTube (Spotify, un canal…) no es de ninguna cancion: ni
            # se le asigna ni ocupa el turno de otro en el reparto en orden de un medley
            urls = [u for u in sg.urls if u[1] != "other"]
            found = found or bool(urls)
            if sg.nparts and sg.nstruck == sg.nparts:  # tachado: ya no va
                self.add_extra("~" + _clean_title(title) + "~")
                found = True
            elif title.strip():
                d = _Draft(title, p, " ".join(sg.artist))
                b.items.append(d)
                self.n_items += 1
                found = True
                if i < last:
                    d.notes, d.people = sg.notes, sg.people
                    if urls:
                        d.url = urls[0]
                        b.queue.extend(urls[1:])
                else:
                    b.queue.extend(urls)
            else:
                b.queue.extend(urls)
        if segs:
            b.notes.extend(segs[last].notes)
            b.people.extend(segs[last].people)
        if not found:  # solo enlaces de fuera: no es una cancion, pero no se pierde
            others = [t for t in p.toks if t.k == URL]
            if others:
                self.add_extra(_render(others, videos=True))

    # -- el resultado
    def build(self) -> tuple[list[SetItem], int]:
        """Las canciones (hasta `MAX_SETLIST_ITEMS`) y cuantas sobran."""
        out: list[SetItem] = []
        total = 0
        for b in self.blocks:
            drafts = list(b.items)
            queue = list(b.queue)
            if len(queue) == 1 and len(drafts) >= 2 and not any(d.url for d in drafts):
                for d in drafts:
                    d.url = queue[0]  # un solo enlace para un medley: el del medley entero
                queue = []
            else:
                for d in drafts:
                    if d.url is None and queue:
                        d.url = queue.pop(0)
            drafts.extend(_url_only(u) for u in queue)
            for d in drafts:
                total += 1
                if len(out) < MAX_SETLIST_ITEMS:
                    item = _make_item(d, b, len(out) + 1)
                    if item is None:
                        total -= 1
                    else:
                        out.append(item)
        return out, max(0, total - len(out))


def _url_only(u: tuple[str, str]) -> _Draft:
    d = _Draft("", None)
    d.url = u
    return d


def _make_item(d: _Draft, b: _Block, n: int) -> SetItem | None:
    title = _clean_title(d.title)
    if d.url is not None and d.url[1] not in ("video", "playlist"):
        d.url = None  # solo un enlace de YouTube es de una cancion
    if not title and d.url is None:
        return None
    artist = _clean_title(d.artist)
    notes = list(d.notes)
    tone = _TRAIL_TONE.search(title)
    if tone:  # «Digno De Adorar - G»: el tono, no un artista
        title = title[: tone.start()].rstrip(_EDGE)
        notes.insert(0, tone.group(1))
    if _TONE.fullmatch(artist):
        notes.insert(0, artist)
        artist = ""
    notes = list(dict.fromkeys(["Medley"] * b.word + notes + b.notes))
    people = list(dict.fromkeys(d.people + b.people))[:_MAX_ITEM_PEOPLE]
    url, cat = d.url or ("", "")
    raw = _tidy(_render(d.prep.toks, videos=True), MAX_RAW) if d.prep is not None else ""
    return SetItem(
        n=n,
        title=title,
        query=names.search_form(title),
        artist=artist,
        url=url[:MAX_URL],
        kind=cat if cat in ("video", "playlist") else "",
        people=people,
        note=_tidy(", ".join(notes)),
        raw=raw,
    )


def _kind(p: _Prep, unmarked: bool) -> str:
    """Que es un registro: seccion, medley, cancion, continuacion, frase o charla."""
    rd = p.rd
    if p.label == "section":
        return "section"
    if p.label:
        return "medley"
    has_title = rd.has_title
    if p.marker or (rd.bold_led and has_title):
        if has_title and _sentence_like(p.titles()):
            return "phrase"
        if has_title or rd.urls:
            return "item"
        return "cont" if (rd.order or rd.has_notes) else "blank"
    if not has_title:
        return "cont" if (rd.urls or rd.order or rd.has_notes) else "blank"
    if not unmarked:
        return "phrase"
    text = p.titles()
    if _is_order(text, bool(rd.urls)):
        return "phrase"
    if _title_like(text):
        return "item"
    # «y esta https://…»: la charla que trae un enlace lo deja como cancion sin titulo
    return "phrase" if rd.urls else "chatty"


def _find_header(preps: list[_Prep]) -> tuple[int, str]:
    """La cabecera de la lista: antes de la primera cancion, si la hay."""
    for i, p in enumerate(preps):
        if p.marker in ("", "emo") and not p.rd.urls and not p.rd.order:
            name = _header_name(p.text())
            if name is not None:
                return i, name
        if p.marker in ("num", "bul") or p.label in ("medley", "contiene") or p.rd.bold_led:
            break
        if p.label != "section" and p.rd.has_title and not _is_order(p.titles()):
            break
    return -1, ""


def _starts_list(p: _Prep) -> bool:
    """Si un registro abre OTRA lista («Miercoles:», «Setlist domingo»): solo un dia o una
    palabra generica, con a lo sumo otra palabra. «Domingo de Gloria» es un titulo."""
    if p.marker or p.rd.urls or p.rd.order:
        return False
    text = p.text()
    if _header_name(text) is None:
        return False
    words = _words(_DATE.sub(" ", _EMOJI_RE.sub(" ", text)))
    return 0 < len(words) <= 2 and (words[0] in _DAYS or words[0] in _GENERIC)


def _unmarked_list_ok(items: list[SetItem], count: int, chatty: int, evidenced: bool) -> bool:
    """Si unas lineas cortas SIN marcas son una lista y no charla.

    Con cabecera, enlaces o una orden que la presenta bastan dos; sin nada de eso
    hacen falta mas: tres si casi todas van con mayuscula de titulo y cuatro si no.
    La charla mezclada la tumba, salvo una linea suelta entre muchas con evidencia.
    """
    if chatty and (not evidenced or chatty * 4 > count + chatty):
        return False
    cased = bool(items) and 4 * sum(_title_cased(it.title) for it in items) >= 3 * len(items)
    return count >= (2 if evidenced else 3 if cased else 4)


def _asks(extras: list[str]) -> bool:
    """Si las frases del mensaje piden algo (agrega, pon, descarga…)."""
    return any(_is_ask(w) for line in extras for w in _words(" ".join(line.split()[:60])))


def _parse(text) -> SetlistParse:
    prepared = _prepare(text)
    if not prepared.strip():
        return SetlistParse()
    ctx = _Ctx()
    lines: list[str] = []
    for line in prepared.split("\n"):
        line = line.strip(" ")
        m = _WA_PREFIX.match(line) or _WA_EXPORT.match(line)
        if m:
            line = line[m.end() :].strip(" ")
        m = _MD_HASH.match(line)
        if m:
            line = line[m.end() :]
        if line:
            lines.append(line)
            if len(lines) >= _MAX_LINES:
                break
    toks: list[_T] = []
    for line in _soft_numbering(lines):
        if len(toks) > _MAX_TOKENS:
            break
        if toks:
            toks.append(_T(NL))
        _scan_line(_mark(line, ctx), toks, ctx)
    preps = [_Prep(r) for r in _cut(toks)[:_MAX_RECS]]
    header, hint = _find_header(preps)
    marked = any(
        i != header
        and p.label in ("", "medley", "contiene")
        and (p.marker or (p.rd.bold_led and p.label == "" and p.rd.has_title))
        and (p.rd.has_title or p.rd.urls)
        for i, p in enumerate(preps)
    )
    asm = _Asm(unmarked=not marked)
    for i, p in enumerate(preps):
        if i == header:
            for name in p.rd.order:
                asm.add_person(name)
        elif asm.n_items and _starts_list(p):
            return SetlistParse()  # dos listas en un mensaje: no se sabe a cual va cada cosa
        else:
            asm.feed(p, _kind(p, asm.unmarked))
    items, truncated = asm.build()
    count = len(items) + truncated
    # una cancion con su enlace en un mensaje que la pide (diseño §2); sin marcas solo si
    # trae titulo propio: un enlace metido en una frase («la de este enlace …») lo lee el modelo
    single = count == 1 and bool(items[0].url) and _asks(asm.extras)
    if asm.unmarked:
        single = single and bool(items[0].title) and not asm.chatty
        if not single and not _unmarked_list_ok(
            items, count, asm.chatty, header >= 0 or asm.evidence
        ):
            return SetlistParse()
    tasks = sum(
        1
        for it in items
        if it.title and _is_task(_fold(it.title.split()[0])) and not _title_cased(it.title)
    )
    if items and tasks * 2 >= len(items):
        return SetlistParse()  # tareas e instrucciones numeradas, no canciones
    if count < 2 and not single:
        return SetlistParse()
    return SetlistParse(
        is_setlist=True,
        name_hint=hint,
        items=items,
        people=list(asm.people.values()),
        extras=asm.extras,
        truncated=truncated,
    )


def parse_setlist(text: str) -> SetlistParse:
    """La lista de canciones que hay en un texto pegado; sin lista, `is_setlist=False`.

    Nunca lanza: lo que se le da lo escribe cualquiera.
    """
    try:
        return _parse(text)
    except Exception:  # noqa: BLE001
        return SetlistParse()


# ---------------------------------------------------------------------- intencion
# Se lee de las palabras de la persona, sin tildes y por frases. Cada disparador mira
# unas pocas palabras alrededor; un «no», «nunca» o «sin» justo antes lo apaga.

_NEGATORS = _vocab("no nunca ni tampoco sin jamas")
_CHECK_VERBS = _vocab(
    "verifica verificar verifique comprueba comprobar compruebe revisa revisar revise "
    "chequea chequear checa checar"
)
_ADD_VERBS = _vocab(
    "agrega agregar agregue anade anadir anada mete meter meta pon poner ponga incluye "
    "incluir incluya suma sumar completa completar complete arma armar crea crear sube "
    "subir"
)
# la orden negativa va en subjuntivo («no bajes», «no las descargues»); «descargala»
# tras un «no» es otra cosa: «la que no se consiga, descargala»
_NO_DOWNLOAD_VERBS = _vocab("bajes baje bajen descargues descargue descarguen")
_DOWNLOAD_INFINITIVES = _vocab("bajar descargar")
_MAKE_VERBS = _vocab(
    "crea crear cree arma armar arme haz hacer haga monta montar prepara preparar genera generar"
)
_LIST_NOUNS = _vocab("lista listas repertorio setlist playlist listado")
_THESE = _vocab("estas estos esas esos")
_ONLY = _vocab("solo solamente unicamente")
_HAVE = _vocab("tenemos tengo hay esten estan tengamos")
_LIKE = _vocab("como igual segun asi mismo orden")


def _clauses(text: str) -> list[list[str]]:
    """Las frases del texto como listas de palabras sin tildes ni signos."""
    out: list[list[str]] = []
    for part in re.split(r"[.!?;\n¿¡]+", _fold(text[:20_000])):
        words = re.findall(r"[a-z0-9]+", part)
        if words:
            out.append(words)
    return out


def _is(word: str, pool: frozenset[str]) -> bool:
    return _verb_base(word, pool) in pool


def _neg(words: list[str], i: int) -> bool:
    return any(w in _NEGATORS for w in words[max(0, i - 3) : i])


def _near(words: list[str], i: int, pool: frozenset[str], span: int = 6) -> bool:
    return any(w in pool for w in words[i + 1 : i + 1 + span])


def _check(words: list[str]) -> bool:
    if any(_is(w, _CHECK_VERBS) and not _neg(words, i) for i, w in enumerate(words)):
        return True
    text = " ".join(words)
    return bool(
        "estan bien" in text
        or re.search(r"\besta bien (?:la|el|mi|esta|este) (?:lista|repertorio|setlist)\b", text)
        or re.search(r"\b(?:mira|fijate|ve) si (?:ya )?(?:estan|esta|tengo|tenemos)\b", text)
    )


def _no_download(words: list[str]) -> bool:
    text = " ".join(words)
    for i, w in enumerate(words):
        before = words[max(0, i - 3) : i]
        if w in _NO_DOWNLOAD_VERBS and any(x in ("no", "nunca", "ni") for x in before):
            # «si ya la tenemos no la bajes»: solo vale si la tienen; lo que falta se baja
            if not ("si" in words[:i] or "cuando" in words[:i]):
                return True
        elif w in _DOWNLOAD_INFINITIVES and "sin" in before:
            return True
        if w in _ONLY and i + 1 < len(words):
            nxt = words[i + 1]
            if _is(nxt, _ADD_VERBS):
                return True
            if "que" in words[i + 1 : i + 5] and _near(words, i, _HAVE, 5):
                return True
    return bool(
        re.search(
            r"\bno (?:hace falta|hay que|es necesario|necesito|quiero) (?:que )?(?:baj|descarg)",
            text,
        )
        or re.search(r"\bnada de (?:descargas|descargar|bajar)\b", text)
        or re.search(r"\bcon lo que (?:ya )?(?:tenemos|tengo|hay)\b", text)
    )


def _replace(words: list[str]) -> bool:
    text = " ".join(words)
    n = len(words)
    for i, w in enumerate(words):
        if _neg(words, i):
            continue
        base = _verb_base(w)
        if base in ("reemplaza", "reemplazar", "reemplace", "sustituye", "sustituir"):
            return True
        if base in ("ordena", "ordenar", "reordena", "reordenar") and (
            w != base or base.startswith("re") or _near(words, i, _LIKE, 5)
        ):
            return True
        if w.startswith("corrigel") or (
            w in ("corrige", "corregir") and _near(words, i, _LIST_NOUNS, 3)
        ):
            return True
        if base in ("deja", "dejar", "deje"):
            near = words[i + 1 : i + 8]
            if any(x in ("exactamente", "igual", "identica", "identico") for x in near):
                return True
            if ("tal" in near and "cual" in near) or (
                "como" in near and any(x in ("esta", "estas", "esto", "mia", "mi") for x in near)
            ):
                return True
            if "con" in near and any(x in _ONLY or x in _THESE for x in near):
                return True
        if w in _ONLY and i + 1 < n:
            nxt = words[i + 1]
            if nxt in _THESE or (nxt == "con" and i + 2 < n and words[i + 2] in _THESE):
                return True
        if base in ("quita", "quitar", "elimina", "eliminar", "borra", "borrar"):
            tail = words[i + 1 : i + 6]
            if "demas" in tail or "sobran" in tail or "sobra" in tail:
                return True
    return bool(
        re.search(r"\b(?:esta|esa) es la (?:lista|buena|correcta|que va)\b", text)
        or re.search(r"\basi (?:debe|deberia|tiene que) (?:de )?quedar\b", text)
        or re.search(
            r"\b(?:debe|tiene que) quedar asi\b|\bque quede (?:asi|igual|exactamente)\b", text
        )
    )


def _create(words: list[str]) -> bool:
    for i, w in enumerate(words):
        if _neg(words, i):
            continue
        if _is(w, _MAKE_VERBS) and _near(words, i, _LIST_NOUNS, 5):
            return True
        if w in ("nueva", "nuevo") and any(x in _LIST_NOUNS for x in words[max(0, i - 1) : i + 3]):
            return True
        if w in _LIST_NOUNS and i + 1 < len(words) and words[i + 1] in ("nueva", "nuevo"):
            return True
        if w in ("llamala", "nombrala") or (w == "llame" and "se" in words[max(0, i - 2) : i]):
            return True
        if w in ("ponle", "ponles") and "nombre" in words[max(0, i - 6) : i + 7]:
            return True
    text = " ".join(words)
    return bool(re.search(r"\bponle de nombre\b|\bcon el nombre\b", text))


def _strict(words: list[str]) -> bool:
    text = " ".join(words)
    return bool(
        re.search(r"\b(?:la|esa|esta|esas) version (?:de|del|que|original|exacta)\b", text)
        or re.search(r"\b(?:esa|esta) (?:misma )?version\b", text)
        or re.search(
            r"\bla (?:de|del) (?:este |ese |esta |esa |el |mi )?(?:enlace|link|url|video|youtube)\b",
            text,
        )
        or re.search(
            r"\bla que (?:esta|sale|viene) en (?:el|este|ese) (?:enlace|link|video)\b", text
        )
    )


def read_intent(text: str) -> Intent:
    """Lo que la persona pide hacer, leido de SUS palabras (no de las canciones).

    Con y sin tildes: verifica/comprueba/revisa («están bien»); no bajes / sin
    descargar / solo agrégalas; déjala exactamente / solo estas / reemplaza /
    ordénala / corrígela; crea / créame / ármame una lista; la versión de… / la
    de este enlace. Una lista pegada se deja fuera (un titulo como «Solo Estas»
    no pide nada), y si el mensaje dice a la vez que verifique y que agregue,
    gana agregar: la persona no quiere repetirlo.
    """
    raw = text if isinstance(text, str) else ""
    parsed = parse_setlist(raw)
    clauses = _clauses("\n".join(parsed.extras) if parsed.is_setlist else raw)
    adds = any(
        _is(w, _ADD_VERBS) and not _neg(words, i) for words in clauses for i, w in enumerate(words)
    )
    return Intent(
        check=any(_check(c) for c in clauses) and not adds,
        no_download=any(_no_download(c) for c in clauses),
        replace=any(_replace(c) for c in clauses),
        create=any(_create(c) for c in clauses),
        strict_version=any(_strict(c) for c in clauses),
    )


# ------------------------------------------------------------------- conversacion

_PERSON_ROLES = frozenset({"me", "user"})
_MAX_SCAN = 60


def last_setlist(messages) -> SetlistParse | None:
    """La ultima lista pegada de una conversacion, o None.

    Mira los mensajes de la persona, del mas nuevo al mas viejo, sin contar los
    ocultos, los avisos (`event`) ni los de la app: una lista que la persona
    pego hace tres mensajes sigue siendo «la lista» cuando dice «completa lo que
    puedas» (revisor F3). Se queda con la primera que lee como lista.
    """
    if not isinstance(messages, (list, tuple)):
        return None
    seen = 0
    for m in reversed(messages):
        if not isinstance(m, dict):
            continue
        if m.get("role") not in _PERSON_ROLES or m.get("hidden") or m.get("event") or m.get("app"):
            continue
        seen += 1
        if seen > _MAX_SCAN:
            break
        text = m.get("text")
        if isinstance(text, str) and text.strip():
            parsed = parse_setlist(text)
            if parsed.is_setlist:
                return parsed
    return None
