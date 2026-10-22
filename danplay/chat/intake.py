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
tiempo lineal y `parse_setlist` nunca lanza.

Lo hace Rust (biblioteca/src/asistente/listas.rs, regla a regla lo que antes
estaba aqui: como se limpia la entrada, como se trocea cada linea en piezas,
como se agrupan en bloques y que se devuelve en `extras` y en `people`); esto
es la puerta de Python, con los mismos nombres. El corpus de muestras esta en
tests/fixtures/intake_samples.json.
"""

import re
from dataclasses import dataclass, field

import danplay_core as _rust

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

_MAX_EXTRAS = 40  # lineas que no son cancion que se devuelven (las mismas que en Rust)

# Los sustitutos sueltos (un texto con bytes que no son UTF-8) no llegan a
# Rust: pasan a un caracter de uso privado, que el lector quita como cualquier
# invisible (como hace `safe`).
_SURROGATE = re.compile("[\ud800-\udfff]")


def _txt(s) -> str:
    if not isinstance(s, str):
        return ""
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        return _SURROGATE.sub("", s)
    return s


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


def clean_youtube_url(url) -> str:
    """La forma canonica de un enlace de YouTube; "" si no lo es.

    Un video sale siempre como «https://www.youtube.com/watch?v=<id>», venga
    como venga (youtu.be/ID, m. o music., sin «https://», con rastreo): se
    quitan `si`, `feature`, `pp`, `t`, `index`, `start_radio` y `list=` cuando
    hay `v=` (una lista entera no se baja desde un enlace pegado). Una URL
    «/playlist?list=ID» sin video sale como «https://www.youtube.com/playlist?list=ID».
    Mira el DOMINIO de verdad, nunca el texto: «youtube.com.evil.example»,
    «youtube.com@evil» o «evil.example/watch?v=…» no son de YouTube.
    """
    if not isinstance(url, str):
        return ""
    return _rust.intake_clean_youtube_url(_txt(url))


def youtube_video_id(url) -> str:
    """El id de 11 caracteres de un video de YouTube; "" si no es de un video."""
    if not isinstance(url, str):
        return ""
    return _rust.intake_youtube_video_id(_txt(url))


# ------------------------------------------------------------------- la lista


def _as_parse(d: dict) -> SetlistParse:
    return SetlistParse(
        is_setlist=d["is_setlist"],
        name_hint=d["name_hint"],
        items=[SetItem(**it) for it in d["items"]],
        people=d["people"],
        extras=d["extras"],
        truncated=d["truncated"],
    )


def _parse(text) -> SetlistParse:
    """`parse_setlist` sin su red: un fallo se ve (para las pruebas)."""
    return _as_parse(_rust.intake_parse(_txt(text), False))


def parse_setlist(text: str) -> SetlistParse:
    """La lista de canciones que hay en un texto pegado; sin lista, `is_setlist=False`.

    Nunca lanza: lo que se le da lo escribe cualquiera.
    """
    try:
        return _as_parse(_rust.intake_parse(_txt(text), True))
    except Exception:  # noqa: BLE001
        return SetlistParse()


# ---------------------------------------------------------------------- intencion


def read_intent(text: str) -> Intent:
    """Lo que la persona pide hacer, leido de SUS palabras (no de las canciones).

    Con y sin tildes: verifica/comprueba/revisa («están bien»); no bajes / sin
    descargar / solo agrégalas; déjala exactamente / solo estas / reemplaza /
    ordénala / corrígela; crea / créame / ármame una lista; la versión de… / la
    de este enlace. Una lista pegada se deja fuera (un titulo como «Solo Estas»
    no pide nada), y si el mensaje dice a la vez que verifique y que agregue,
    gana agregar: la persona no quiere repetirlo.
    """
    check, no_download, replace, create, strict = _rust.intake_read_intent(_txt(text))
    return Intent(
        check=check,
        no_download=no_download,
        replace=replace,
        create=create,
        strict_version=strict,
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
