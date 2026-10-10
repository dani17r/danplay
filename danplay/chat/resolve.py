"""El emparejador con la biblioteca: decide si una cancion pedida «ya la tenemos».

Es la pieza detras de «biblioteca primero»: antes de proponer bajar nada de
YouTube, cada titulo de una lista pegada (o de una peticion suelta) se mira
aqui contra lo que hay. No usa el modelo: es codigo, y se mide (ver mas abajo).

Lo que hace, en orden:

  1. Lee la peticion como la escribe la gente: con numeracion, negritas,
     emojis, @menciones, anotaciones entre parentesis, «Video Oficial»,
     erratas, el artista pegado delante o detras. Todo se compara en la forma
     de buscar de la casa (`names.search_form`, la regla del usuario: sin
     tildes, en minusculas, sin simbolos y CON la ñ) en LOS DOS LADOS.
  2. La biblioteca se lee UNA vez por `library.revision()` (indice de formas
     en memoria, bajo cerrojo, con `invalidate()`). Cada cancion aporta varios
     nombres: el de la etiqueta y el del archivo, con y sin lo que va entre
     parentesis, cada parte de un medley y cada subtitulo. Hasta ~5 000
     canciones se puntua sin prefiltro; por encima, un prefiltro FTS5 (OR de
     palabras, `bm25` por encima, 400 candidatos) y la puntuacion solo sobre
     esos.
  3. Puntua por COBERTURA de las palabras del titulo pedido (las vacias pesan
     poco), con frase contigua, una errata de una letra en palabras de 5 o mas
     y una segunda pasada ñ≈n con descuento (`why="enye~n"`). La pista del
     artista, de los parentesis y de las menciones SOLO SUMA: nunca resta, asi
     que un artista equivocado o una @mencion no esconden la cancion.
  4. Decide: `found` (la tenemos), `ambiguous` (la tenemos en varias versiones:
     se elige una y las demas van de alternativas), `probable` (parece esta,
     pero no del todo: una errata, solo el comienzo, otro orden) o `missing`.
  5. Los enlaces mandan: si el video ya se bajo (tabla `downloads`) y la fila
     sigue siendo ESA cancion (`songs.path` contra `downloads.target`; los ids
     se reutilizan), es esa. Si el enlace no esta en el historial y hay una
     cancion con el mismo titulo, se usa la de la biblioteca y se marca
     `other_version` para ofrecer la del enlace (o es `missing` con `strict`).

Nada de esto toca la red ni descarga: solo lee la base.
"""

import logging
import os
import re
import sqlite3
import threading
import time
import unicodedata
import urllib.parse
from collections import Counter
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from itertools import pairwise
from typing import Any

from .. import config, library, names

log = logging.getLogger(__name__)

__all__ = [
    "INDEX",
    "Candidate",
    "Index",
    "Resolution",
    "invalidate",
    "match_song",
    "rank",
    "resolve_items",
    "title_key",
]

# ------------------------------------------------------------------ ajustes

# Hasta aqui el indice entero en memoria y sin prefiltro; por encima, FTS5.
FULL_LIMIT = 5000
# Candidatos que deja pasar el prefiltro FTS5 (y la tolerancia de una errata
# solo se aplica sobre ellos).
PREFILTER = 400
# Lo que se mira de un titulo pedido: el resto es ruido o un ataque. Un texto
# de 200 KB no puede costar mas que uno de 1 000 caracteres.
MAX_CHARS = 1000
MAX_TOKENS = 40
MAX_GROUPS = 8
# Items de un lote y tiempo total que se le da (segundos).
MAX_ITEMS = 200
BATCH_BUDGET = 10.0

JOIN_MIN = 8  # letras que tiene que tener un titulo para compararlo «pegado»
FORMS_CACHE = 20000  # formas recordadas entre lecturas de la base

T_FOUND = 0.90  # desde aqui «la tenemos»
T_MISSING = 0.60  # por debajo, «no la tenemos»
T_NEAR = 0.40  # por debajo ni se enseña como alternativa
AMB_DELTA = 0.035  # versiones distintas que empatan con la mejor: ambigua (menos que B_VERSION)
TIE = 0.01  # puntuaciones que se tratan como iguales para desempatar
ENYE_DISCOUNT = 0.94  # lo que vale un acierto que solo lo es plegando la ñ
FUZZY_W = 0.9  # peso de una palabra casada con una errata
STOP_W = 0.3  # peso de una palabra vacia
PEN_EXTRA_STOP = 0.9  # lo que resta una palabra vacia de MAS en lo pedido
PEN_EDGE_STOP = 0.5  # una vacia del principio o del final del titulo sin escribir
PEN_MID_STOP = 1.0  # una vacia de en medio del titulo sin escribir

B_ARTIST = 0.07  # el artista pedido esta en la cancion
B_VERSION = 0.04  # la version pedida («en vivo», «play along») esta en la cancion
B_PAREN_ARTIST = 0.042  # lo que va entre parentesis es el artista de la cancion
B_HINT = 0.008  # menciones y notas que aparecen en la cancion (menos que TIE: nunca deciden)

# Palabras que casi no distinguen un titulo de otro (espanol e ingles).
STOP = frozenset(
    """a al ante con de del e el en es la las le les lo los me mi mis no o para por que
    se si sin su sus te tu tus u un una y ya yo the of and to in is my you your i it on
    at for with we our an be""".split()  # noqa: SIM905
)
# Lo que dice «que version es» y no «que cancion es».
VERSION = frozenset(
    """live vivo acoustic acustico acustica instrumental pista karaoke remix remaster
    remastered demo cover version original studio estudio espontaneo reprise tutorial
    drum drums drumless cam play along playalong playback sesion session""".split()  # noqa: SIM905
)
# Roles que se apuntan junto al titulo en una lista de repertorio.
ROLE = frozenset(
    "voz voces coro coros guitarra bateria bajo piano teclado teclas solista tono".split()  # noqa: SIM905
)
# Carpetas que contienen a las de los artistas, no son un artista.
GENERIC_FOLDERS = frozenset("artistas artists musica music biblioteca library".split())  # noqa: SIM905
# Un parentesis que empieza asi es un credito («feat. X», «con X»), no un subtitulo.
CREDIT = frozenset("feat ft featuring con com with junto prod".split())  # noqa: SIM905
# Palabras de lo pedido que ni suman ni restan cuando no estan en la cancion.
FREE_Q = VERSION | ROLE

_WHY = {
    "exact": "titulo exacto",
    "phrase": "frase en titulo",
    "words": "palabras",
    "fuzzy": "difuso",
}

# ------------------------------------------------------------ texto de fuera

_NOISE = re.compile("|".join(names.NOISE), re.IGNORECASE)
# decoracion al principio: *, _, ~, vinetas, emojis… (un parentesis de apertura no lo es)
_LEAD = re.compile(r"^[^\w(\[{]+")
# numeracion de una lista: «3.-», «3)», «(3)», «03 - », «3:», «3º», y el «3» con marco de teclado
# el «3» con marco de teclado es 3 + U+FE0F (selector de emoji) + U+20E3: con chr(), que a ojo no se ve
_KEYCAP = chr(0xFE0F) + "?" + chr(0x20E3)
_NUM = re.compile(r"^(?:[(\[]?\d{1,3}\s*(?:[)\]]|[.\-–—:]+|[ºª°]|" + _KEYCAP + r")[.)\-–—:]*\s*)+")
# la primera @mencion: desde ahi son personas, no titulo
_PEOPLE = re.compile(r"(?:^|(?<=[\s(\[¡¿,;:|]))@\S")
# sufijo de las copias repetidas de la casa: « - r», « - r2»
_DUP = re.compile(r"\s+-\s+r\d*\s*$", re.IGNORECASE)
# lo que separa artista y titulo en «Artista - Titulo»
_SEG = re.compile(r"\s+[-–—|]\s+")
# las partes de un medley
_MEDLEY = re.compile(r"\s*\+\s*|\s*/\s*")
_VIDEO_ID = re.compile(r"[A-Za-z0-9_-]{11}")
# un enlace pegado dentro del titulo: no es texto de titulo (y no se normaliza)
_URL = re.compile(r"(?:https?://|www\.)\S+|\b(?:youtu\.be|youtube\.com)/\S+", re.IGNORECASE)


def _toks(text: str) -> list[str]:
    return names.search_tokens(text)


def _fold(token: str) -> str:
    return token.replace("ñ", "n")


def _fold_all(tokens: Iterable[str]) -> tuple[str, ...]:
    return tuple(_fold(t) for t in tokens)


def _weight(token: str) -> float:
    return STOP_W if token in STOP else 1.0


def _prep(text: str, *, numbering: bool = True) -> str:
    """El texto sin lo que no es nombre: controles, decoracion, numeracion de
    lista y el ruido de descargas («Video Oficial», «HD», «.mp3»…)."""
    t = names.strip_controls(unicodedata.normalize("NFC", str(text or ""))).replace("_", " ")
    t = _LEAD.sub("", t)
    if numbering:
        t = _LEAD.sub("", _NUM.sub("", t, count=1))
    return _NOISE.sub(" ", t)


def _zones(text: str) -> tuple[str, list[str]]:
    """(texto fuera de parentesis, [lo de cada parentesis/corchete/llave]).

    Una pasada y en tiempo lineal: los anidados («((cover X))») quedan dentro
    del grupo de fuera, y uno sin cerrar llega hasta el final.
    """
    core: list[str] = []
    groups: list[str] = []
    buf: list[str] = []
    depth = 0
    for ch in text:
        if ch in "([{":
            if depth == 0:
                buf = []
                core.append(" ")
            else:
                buf.append(ch)
            depth += 1
        elif ch in ")]}" and depth:
            depth -= 1
            if depth == 0:
                groups.append("".join(buf))
                core.append(" ")
            else:
                buf.append(ch)
        elif depth:
            buf.append(ch)
        else:
            core.append(ch)
    if depth:
        groups.append("".join(buf))
    return "".join(core), groups


def _near(a: str, b: str) -> bool:
    """Una errata: las dos palabras difieren en UNA letra (cambiada, de mas,
    de menos o dos seguidas cambiadas de sitio) y la mas larga tiene 5 o mas.

    La ñ por la n NO cuenta como errata: eso es la segunda pasada, con su
    descuento (Mañana no es manana). Se mira la palabra MAS LARGA: una letra
    borrada de una palabra de 5 deja una de 4 y sigue siendo una errata.
    """
    la, lb = len(a), len(b)
    if a == b or abs(la - lb) > 1 or min(la, lb) < 4 or max(la, lb) < 5 or _fold(a) == _fold(b):
        return False
    if la == lb:
        d = [i for i in range(la) if a[i] != b[i]]
        if len(d) == 1:
            return True
        return len(d) == 2 and d[1] == d[0] + 1 and a[d[0]] == b[d[1]] and a[d[1]] == b[d[0]]
    if la < lb:
        a, b = b, a
    i = 0
    while i < len(b) and a[i] == b[i]:
        i += 1
    return a[i + 1 :] == b[i:]


def _is_version(tokens: Sequence[str]) -> bool:
    """Un parentesis que dice la version o el credito, no otro nombre."""
    if not tokens:
        return True
    if tokens[0] in CREDIT:
        return True
    return any(t in VERSION for t in tokens)


# ------------------------------------------------------------ lo pedido


@dataclass(slots=True)
class _Q:
    """Una peticion ya leida."""

    core: list[str] = field(default_factory=list)  # el titulo, fuera de parentesis
    full: list[str] = field(default_factory=list)  # con lo de los parentesis
    fcore: tuple[str, ...] = ()
    ffull: tuple[str, ...] = ()
    ver: frozenset[str] = frozenset()  # version pedida (parentesis, «en vivo»)
    people: frozenset[str] = frozenset()  # menciones y notas
    artist: list[str] = field(default_factory=list)
    fartist: tuple[str, ...] = ()
    fold_pass: bool = False  # tiene ñ: la segunda pasada puede cambiar algo
    fts: list[str] = field(default_factory=list)  # palabras para el prefiltro FTS5
    loose: list[str] = field(default_factory=list)  # prefijos de 3 letras, para erratas
    empty: bool = True


def _glue_initials(tokens: list[str]) -> list[str]:
    """«D Clario» es «DClario»: una letra suelta se pega a la palabra siguiente
    (las vacias de una letra, como «y», no)."""
    out: list[str] = []
    i = 0
    while i < len(tokens):
        t = tokens[i]
        if len(t) == 1 and t.isalpha() and t not in STOP and i + 1 < len(tokens):
            out.append(t + tokens[i + 1])
            i += 2
        else:
            out.append(t)
            i += 1
    return out


def _cut_people(text: str) -> tuple[str, str]:
    m = _PEOPLE.search(text)
    if not m:
        return text, ""
    return text[: m.start()], text[m.start() :]


def _parse(title: Any, artist: Any = "", hints: Iterable[Any] = ()) -> _Q:
    """Lo pedido, partido en lo que es titulo y lo que es pista."""
    raw = _URL.sub(" ", str(title or "")[:MAX_CHARS])
    head, people = _cut_people(raw)
    head = _DUP.sub("", head)
    prepped = _prep(head)
    core_text, groups = _zones(prepped)
    core = _toks(core_text)[:MAX_TOKENS]
    full = _toks(prepped)[:MAX_TOKENS]
    if not core:
        core = full
    if not core:
        # todo era ruido o estaba entre parentesis: se mira el texto tal cual
        core = full = _toks(head)[:MAX_TOKENS]
    ver: set[str] = set()
    for g in groups[:MAX_GROUPS]:
        ver.update(t for t in _toks(g)[:MAX_TOKENS] if t not in STOP)
    ver.update(t for t in core if t in VERSION)
    extra: list[str] = []
    for h in list(hints)[:MAX_GROUPS]:
        extra.extend(_toks(str(h or "")[:MAX_CHARS])[:MAX_TOKENS])
    extra.extend(_toks(people)[:MAX_TOKENS])
    ver.update(t for t in extra if t in VERSION)
    q = _Q(
        core=core,
        full=full,
        fcore=_fold_all(core),
        ffull=_fold_all(full),
        ver=frozenset(_fold_all(ver)),
        people=frozenset(_fold_all(t for t in extra if t not in STOP)),
        artist=_glue_initials(_toks(str(artist or "")[:200])[:12]),
    )
    q.fartist = _fold_all(q.artist)
    q.fold_pass = q.fcore != tuple(core) or q.ffull != tuple(full)
    q.empty = not core
    # palabras para el prefiltro: las del indice FTS5 parten por el apostrofo
    # («D'Clario» son «d» y «clario»), de 3 o mas letras y sin vacias ni repetidas
    seen: dict[str, None] = {}
    for t in _toks(re.sub(r"['’‘ʼ´`]", " ", _prep(head))[:MAX_CHARS]):
        if len(t) >= 3 and t not in STOP:
            seen.setdefault(t, None)
    q.fts = sorted(seen, key=lambda t: (-len(t), t))[:8]
    q.loose = sorted({t[:3] for t in seen if len(t) >= 5})[:8]
    return q


def title_key(title: Any, artist: Any = "") -> str:
    """Una clave estable de «esta cancion pedida», para recordar la version
    elegida entre una semana y otra.

    Es el titulo en la forma de buscar, sin numeracion, negritas, emojis,
    menciones, parentesis ni ruido: «*3.-Digno De Adorar* ✅ @Fulano (En Su
    Presencia)» y «digno de adorar» dan la misma. Con `artist` (sin
    menciones) se le añade el artista; quien quiera recordar por titulo a
    secas pasa `artist=""`. La ñ se conserva, como en toda comparacion.
    """
    q = _parse(title)
    key = " ".join(q.core)
    a = " ".join(_toks(str(artist or "")[:200])[:12])
    return f"{key}|{a}" if a else key


# ------------------------------------------------------- la biblioteca


@dataclass(slots=True)
class _Name:
    """Un nombre con el que se puede pedir una cancion."""

    toks: tuple[str, ...]
    ftoks: tuple[str, ...]  # lo mismo con la ñ como n
    w: float  # cuanto se fia uno de que sea EL nombre
    kind: str  # core | full | sub | part

    @property
    def has_enye(self) -> bool:
        return self.toks != self.ftoks


@dataclass(slots=True)
class _Forms:
    """Las formas de buscar una cancion. Salen solo de sus campos de texto, asi
    que se pueden recordar entre lecturas de la base."""

    names: tuple[_Name, ...]
    fctx: frozenset[str]  # todas sus palabras (ñ como n): lo que «explica» lo pedido
    art: frozenset[str]  # las del artista, el invitado y la carpeta
    ver: frozenset[str]  # las de lo que va entre parentesis y las de version sueltas
    ver_key: tuple[str, tuple[str, ...]]  # misma clave = misma version (copias « - r»)


@dataclass(slots=True)
class _Entry:
    sid: int
    song: dict
    forms: _Forms
    path: str
    file: str
    match_key: str
    shortness: int
    stars: int
    favorite: bool


_COLS = (
    'id, path, file, folder, artist, title, album, feat, duration, "key", bpm, stars, '
    "favorite, match_key"
)


def _group_names(
    primary: str, extra_groups: Sequence[str]
) -> tuple[list[_Name], set[str], list[str], tuple[str, ...]]:
    """Los nombres de un titulo («Algo (En Vivo) + Otro»): el nucleo, el
    completo, cada parte de un medley y cada subtitulo.

    -> (nombres, palabras de version, palabras de contexto, tokens completos)
    """
    core_text, groups = _zones(primary)
    groups = [*groups, *extra_groups]
    full = tuple(_toks(primary))
    core = tuple(_toks(core_text)) or full
    out: list[_Name] = []
    ver: set[str] = set()
    ctx: list[str] = []
    gtoks = [tuple(_toks(g)) for g in groups]
    if core:
        out.append(_Name(core, _fold_all(core), 1.0, "core"))
    if full and full != core:
        out.append(_Name(full, _fold_all(full), 1.0, "full"))
    parts = [p for p in _MEDLEY.split(core_text) if p.strip()]
    if len(parts) > 1:
        for p in parts[:12]:
            pt = tuple(_toks(p))
            if pt and pt != core:
                out.append(_Name(pt, _fold_all(pt), 0.92 if len(pt) > 1 else 0.7, "part"))
    for g in gtoks:
        if not g:
            continue
        ctx.extend(g)
        ver.update(g)
        if not _is_version(g):
            out.append(_Name(g, _fold_all(g), 0.88 if len(g) > 1 else 0.55, "sub"))
    ver.update(t for t in core if t in VERSION)
    return out, ver, ctx, full


def _build_forms(artist: str, title: str, album: str, file: str, folder: str, feat: str) -> _Forms:
    stem = _DUP.sub("", os.path.splitext(file or "")[0])
    parts = _SEG.split(_prep(stem)) if stem else []
    file_artist = parts[0] if len(parts) > 1 else ""
    file_title = parts[1] if len(parts) > 1 else (parts[0] if parts else "")
    file_extra = parts[2:] if len(parts) > 2 else []

    art_join = {j for j in ("".join(_toks(artist)), "".join(_toks(file_artist))) if len(j) >= 3}

    def artist_like(seg: str) -> bool:
        j = "".join(_toks(seg))
        return bool(j) and any(
            j == a or (min(len(j), len(a)) >= 5 and (j in a or a in j)) for a in art_join
        )

    tag_parts = _SEG.split(_prep(_DUP.sub("", title or ""))) if title else []
    kept = [s for s in tag_parts if not artist_like(s)] or tag_parts[:1]
    tag_title = kept[0] if kept else ""
    tag_extra = kept[1:]

    all_names: dict[tuple[str, ...], _Name] = {}
    ver: set[str] = set()
    ctx: list[str] = []
    full_for_key: tuple[str, ...] = ()
    for primary, extra in ((file_title, file_extra), (tag_title, tag_extra)):
        if not primary.strip():
            continue
        nms, v, c, full = _group_names(primary, extra)
        ver.update(v)
        ctx.extend(c)
        if not full_for_key:
            full_for_key = full
        for nm in nms:
            old = all_names.get(nm.toks)
            if old is None or nm.w > old.w:
                all_names[nm.toks] = nm
    ctx.extend(_toks(artist))
    ctx.extend(_toks(file_artist))
    ctx.extend(_toks(album))
    ctx.extend(_toks(feat))
    # la carpeta: todas sus palabras explican lo pedido, pero solo la primera que no es
    # un contenedor («Artistas/Barak/Disco En Vivo») es el artista; las demas son discos
    parts_f = [_toks(c) for c in str(folder or "").replace("\\", "/").split("/")]
    parts_f = [c for c in parts_f if c and not all(t in GENERIC_FOLDERS for t in c)]
    ctx.extend(t for c in parts_f for t in c if t not in GENERIC_FOLDERS)
    ctx.extend(t for nm in all_names.values() for t in nm.toks)

    art_t = [*_toks(artist), *_toks(file_artist), *_toks(feat), *(parts_f[0] if parts_f else [])]
    art = set(_fold_all(art_t))
    for a, b in pairwise(art_t):
        if len(a) <= 2:  # «D' Clario» queda en «d» y «clario»; «DClario», en una
            art.add(_fold(a + b))
    first_artist = "".join(_toks(file_artist)) or "".join(_toks(artist))
    return _Forms(
        names=tuple(all_names.values()),
        fctx=frozenset(_fold_all(ctx)),
        art=frozenset(art),
        ver=frozenset(_fold_all(ver)),
        ver_key=(_fold(first_artist), _fold_all(full_for_key)),
    )


def _light(row: Mapping[str, Any]) -> dict:
    """La fila ligera que se devuelve (la misma forma que `tools._song_brief`)."""
    return {
        "id": row["id"],
        "artist": row["artist"] or "",
        "title": row["title"] or "",
        "album": row["album"] or "",
        "duration": round(row["duration"] or 0),
        "key": row["key"] or "",
        "bpm": round(row["bpm"]) if row["bpm"] else 0,
        "stars": row["stars"] or 0,
        "favorite": bool(row["favorite"]),
    }


class _Snapshot:
    """La biblioteca vista por el emparejador, en el estado de una revision.

    Con pocas canciones lleva todas las formas y un indice de palabras; con
    muchas, solo cuenta y fabrica las formas de las que el prefiltro deja pasar.
    """

    def __init__(self, key: tuple[str, int], forms_cache: dict) -> None:
        self.key = key
        self.large = False
        self.count = 0
        self.entries: list[_Entry] = []
        self.by_id: dict[int, _Entry] = {}
        self.post: dict[str, list[int]] = {}
        self.dels: dict[str, list[str]] = {}
        self.bag: dict[tuple[str, ...], list[int]] = {}
        self.joined: dict[str, list[int]] = {}
        self._forms = forms_cache

    # ---- construccion

    def _entry(self, row: Mapping[str, Any]) -> _Entry:
        sig = (
            row["artist"] or "",
            row["title"] or "",
            row["album"] or "",
            row["file"] or "",
            row["folder"] or "",
            row["feat"] or "",
        )
        forms = self._forms.get(sig)
        if forms is None:
            forms = _build_forms(*sig)
            if len(self._forms) > FORMS_CACHE:
                self._forms.clear()
            self._forms[sig] = forms
        song = _light(row)
        return _Entry(
            sid=int(row["id"]),
            song=song,
            forms=forms,
            path=row["path"] or "",
            file=row["file"] or "",
            match_key=row["match_key"] or "",
            shortness=len(song["title"] or row["file"] or ""),
            stars=song["stars"],
            favorite=song["favorite"],
        )

    def load(self, conn: sqlite3.Connection) -> None:
        self.count = conn.execute("SELECT COUNT(*) FROM songs").fetchone()[0] or 0
        if self.count > FULL_LIMIT:
            self.large = True
            return
        rows = conn.execute(f"SELECT {_COLS} FROM songs ORDER BY id").fetchall()  # noqa: S608
        for row in rows:
            e = self._entry(row)
            idx = len(self.entries)
            self.entries.append(e)
            self.by_id[e.sid] = e
            seen: set[str] = set()
            for nm in e.forms.names:
                seen.update(nm.ftoks)
                self.bag.setdefault(tuple(sorted(nm.ftoks)), []).append(idx)
                j = "".join(nm.ftoks)
                if len(j) >= JOIN_MIN:
                    self.joined.setdefault(j, []).append(idx)
            for t in seen:
                self.post.setdefault(t, []).append(idx)
        for t in self.post:
            if len(t) >= 4:
                for i in range(len(t)):
                    self.dels.setdefault(t[:i] + t[i + 1 :], []).append(t)
                self.dels.setdefault(t, []).append(t)

    # ---- consulta

    def fetch(self, ids: Iterable[int]) -> list[_Entry]:
        """Las canciones con esos ids (para el camino con prefiltro)."""
        ids = [int(i) for i in ids]
        have = {i: self.by_id[i] for i in ids if i in self.by_id}
        missing = [i for i in ids if i not in have]
        if missing:
            marks = ",".join("?" * len(missing))
            with library.connect() as conn:
                rows = conn.execute(
                    f"SELECT {_COLS} FROM songs WHERE id IN ({marks})",  # noqa: S608
                    missing,
                ).fetchall()
            for row in rows:
                e = self._entry(row)
                self.by_id[e.sid] = e
                have[e.sid] = e
        return [have[i] for i in ids if i in have]

    def neighbors(self, token: str) -> set[str]:
        """Palabras del indice a una errata de `token` (por borrado de una letra)."""
        out: set[str] = set()
        variants = {token, *(token[:i] + token[i + 1 :] for i in range(len(token)))}
        for v in variants:
            for cand in self.dels.get(v, ()):
                if cand != token and _near(token, cand):
                    out.add(cand)
        return out


class Index:
    """El indice del emparejador: se construye una vez por `library.revision()`
    (y por base de datos), bajo cerrojo, y se comparte entre hilos."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._snap: _Snapshot | None = None
        self._forms: dict = {}

    @staticmethod
    def _key() -> tuple[str, int]:
        return (str(config.DATABASE), library.revision())

    def snapshot(self) -> _Snapshot:
        snap = self._snap
        if snap is not None and snap.key == self._key():
            return snap
        with self._lock:
            # la revision se lee ANTES que los datos: si alguien escribe mientras
            # se lee, la siguiente llamada ve otra revision y vuelve a construir
            key = self._key()
            snap = self._snap
            if snap is not None and snap.key == key:
                return snap
            snap = _Snapshot(key, self._forms)
            with library.connect() as conn:
                snap.load(conn)
            self._snap = snap
            return snap

    def invalidate(self) -> None:
        """Olvida lo construido (la proxima consulta vuelve a leer la base)."""
        with self._lock:
            self._snap = None


INDEX = Index()


def invalidate() -> None:
    """Fuerza que el emparejador vuelva a leer la biblioteca."""
    INDEX.invalidate()


# --------------------------------------------------------- la puntuacion


def _eval(
    q: Sequence[str], n: Sequence[str], fctx: frozenset[str], *, whole_only: bool = False
) -> tuple[float, str]:
    """Cuanto se parece lo pedido `q` a un nombre `n`: -> (puntuacion, tipo).

    Ademas de palabra a palabra (`_eval_tokens`), se mira con las letras
    pegadas: «DeseoEterno», o «Eres Santo» por «El Es Santo», son el mismo
    titulo con los espacios en otro sitio (o con una letra distinta).
    """
    s, kind = _eval_tokens(q, n, fctx, whole_only=whole_only)
    if s < 0.84 and (len(q) > 1 or len(n) > 1):
        jq, jn = "".join(q), "".join(n)
        if len(jq) >= JOIN_MIN and abs(len(jq) - len(jn)) <= 1:
            if jq == jn:
                return 0.93, "words"
            if _near(jq, jn):
                return 0.84, "fuzzy"
    return s, kind


def _explains(token: str, fctx: frozenset[str]) -> bool:
    """Si el resto de la cancion (artista, album, carpeta…) lleva esa palabra, o
    una a una errata de ella («marcos» por «marco»)."""
    ft = _fold(token)
    return ft in fctx or (len(ft) >= 4 and any(_near(ft, c) for c in fctx))


def _trailing_free(tokens: Sequence[str], free: frozenset[str]) -> int:
    """Desde que posicion todo lo que queda son palabras de version (o vacias):
    el «en vivo» del final de «Santo Por Siempre En Vivo». Un «vivo» al
    principio («Vivo Para Adorarte») es del titulo y no se perdona."""
    k = len(tokens)
    while k > 0 and (tokens[k - 1] in free or tokens[k - 1] in STOP):
        k -= 1
    return k


def _eval_tokens(
    q: Sequence[str], n: Sequence[str], fctx: frozenset[str], *, whole_only: bool = False
) -> tuple[float, str]:
    """La comparacion palabra a palabra de `_eval`.

    Se emparejan las palabras (iguales, o con una errata) y se mira que queda
    sin emparejar a cada lado:

      - solo palabras vacias distintas  → casi exacto (el orden importa poco);
      - al nombre entero le sobra algo a lo pedido → «contiene»: vale lo que
        cubre, restando lo que sobra;
      - a lo pedido entero le sobra algo al nombre → una parte (el comienzo
        vale mas que el medio; una sola palabra no vale);
      - sobra en los dos lados → otra cancion.
    """
    lq, ln = len(q), len(n)
    if not lq or not ln:
        return 0.0, ""
    if lq == ln and all(a == b for a, b in zip(q, n, strict=True)):
        return 1.0, "exact"
    used = [False] * ln
    qpos = [-1] * lq
    fuzzy: set[int] = set()
    for i, t in enumerate(q):
        for j in range(ln):
            if not used[j] and n[j] == t:
                used[j] = True
                qpos[i] = j
                break
    for i, t in enumerate(q):
        if qpos[i] < 0 and len(t) >= 4:
            for j in range(ln):
                if not used[j] and _near(t, n[j]):
                    used[j] = True
                    qpos[i] = j
                    fuzzy.add(i)
                    break
    qidx = [i for i in range(lq) if qpos[i] >= 0]
    if not qidx:
        return 0.0, ""
    pos = [qpos[i] for i in qidx]
    first, last = min(pos), max(pos)
    ordered = all(pos[k] < pos[k + 1] for k in range(len(pos) - 1))

    # que pasa con cada palabra pedida: casada, libre (version al final), explicada
    # por el resto de la cancion (artista, album…) o de mas
    q_free_from = _trailing_free(q, FREE_Q)
    kind_of = ["match" if qpos[i] >= 0 else "extra" for i in range(lq)]
    for i, t in enumerate(q):
        if qpos[i] >= 0:
            continue
        if t in FREE_Q and i >= q_free_from:
            kind_of[i] = "free"
        elif _weight(t) == 1.0 and t not in n and _explains(t, fctx):
            # una palabra repetida del propio titulo no se explica
            kind_of[i] = "explained"
    for i, t in enumerate(q):
        # una vacia entre dos palabras explicadas es del mismo nombre («Cuan Grande
        # Es Dios», el artista, pegado al titulo)
        if (
            kind_of[i] == "extra"
            and t in STOP
            and 0 < i < lq - 1
            and kind_of[i - 1] == kind_of[i + 1] == "explained"
        ):
            kind_of[i] = "explained"
    m = 0.0  # peso casado
    m_content = 0  # palabras con contenido casadas
    uq = 0.0  # peso de lo pedido que no esta (ni lo explica el resto de la cancion)
    uq_c = uq_s = 0
    for i, t in enumerate(q):
        w = _weight(t)
        if kind_of[i] == "match":
            m += w * (FUZZY_W if i in fuzzy else 1.0)
            m_content += w == 1.0
        elif kind_of[i] == "extra":
            uq += w
            if w == 1.0:
                uq_c += 1
            else:
                uq_s += 1
    explained = "explained" in kind_of
    n_free_from = _trailing_free(n, VERSION)
    un = 0.0  # peso del nombre que no se ha pedido
    un_c = 0
    stop_edge = stop_mid = 0.0  # vacias sin pedir: en los extremos o por dentro
    for j in range(ln):
        if used[j] or (n[j] in VERSION and j >= n_free_from):
            continue
        w = _weight(n[j])
        un += w
        if w == 1.0:
            un_c += 1
        elif first < j < last:
            stop_mid += w
        else:
            stop_edge += w
    nf = len(fuzzy)

    if uq_c == 0 and un_c == 0:
        # solo difieren palabras vacias (o nada): es ese nombre. Una vacia de MAS
        # en lo pedido pesa mas que una que se dejo sin escribir, y saltarse una
        # de en medio mas que no escribir la del principio o la del final
        s = 0.985 if ordered else (0.86 if m >= 1.29 else 0.60)
        wq = STOP_W * uq_s
        s -= PEN_EXTRA_STOP * wq / (m + wq)
        s -= PEN_EDGE_STOP * stop_edge / (m + stop_edge)
        s -= PEN_MID_STOP * stop_mid / (m + stop_mid)
        s -= 0.10 * min(nf, 2)
        if ordered and not m_content and (uq_s or stop_edge or stop_mid):
            s = min(s, 0.55)  # solo palabras vacias casadas y alguna sin casar: no basta
        if m_content == 1 and stop_edge + stop_mid >= 2 * STOP_W - 1e-9:
            s = min(s, 0.55)  # una sola palabra con contenido y dos vacias del titulo sin pedir
        if explained:
            s -= 0.01
        return max(s, 0.0), "fuzzy" if nf else ("exact" if ordered else "words")
    if whole_only:
        return 0.0, ""
    if un_c == 0:
        # el nombre entero esta dentro de lo pedido, que trae algo de mas
        s = m / (m + 0.8 * uq)
        if not (ordered and qidx[-1] - qidx[0] == len(qidx) - 1):
            s *= 0.8
        if m < 2.0:  # un nombre de una sola palabra de contenido dice poco
            s *= 0.85
        s *= 0.9**nf
        if explained:
            s -= 0.01
        kind = "fuzzy" if nf else ("phrase" if ordered else "words")
        return min(s, 0.96), kind
    if uq == 0:
        # todo lo pedido esta, pero al nombre le sobra: una parte
        ccov = m / (m + un)
        contiguous = ordered and last - first == len(pos) - 1
        lead_ok = first == 0 or all(n[j] in STOP for j in range(first))
        if contiguous and lead_ok:
            if m >= 1.29:
                s = 0.78 + 0.06 * min(1.0, (m - 1.3) / 2.0) + 0.12 * ccov
            elif lq >= 2:
                s = 0.62  # dos palabras vacias (o casi): el comienzo de un titulo, y poco mas
            else:
                s = min(0.58, 0.45 + 0.15 * ccov)
            if first:  # no empieza por lo pedido: se salta una vacia del principio
                s -= 0.05
            kind = "phrase"
        elif contiguous:
            s = min(0.58, 0.45 + 0.15 * ccov)
            kind = "phrase"
        else:
            s = min(0.55, 0.40 + 0.15 * ccov)
            kind = "words"
        s *= 0.85**nf
        return s, "fuzzy" if nf else kind
    # a cada lado le sobra algo que el otro no tiene: es otra cancion
    return min(0.5, 0.5 * m / (m + uq + un)), "words"


def _art_cov(tokens: Sequence[str], art: frozenset[str]) -> float:
    """Que parte (por peso) de las palabras del artista pedido esta en la cancion."""
    total = got = 0.0
    for t in tokens:
        w = _weight(t)
        total += w
        ft = _fold(t)
        if ft in art:
            got += w
        elif len(ft) >= 4 and any(_near(ft, a) for a in art):
            got += 0.8 * w
    return got / total if total else 0.0


def _score(q: _Q, e: _Entry) -> tuple[float, str] | None:
    """La puntuacion de una cancion para lo pedido: (puntos, why) o None."""
    forms = e.forms
    best, why = 0.0, ""
    for nm in forms.names:
        whole = nm.kind in ("sub", "part")
        qq = q.full if nm.kind == "full" else q.core
        s, k = _eval(qq, nm.toks, forms.fctx, whole_only=whole)
        s *= nm.w
        if s > best:
            best, why = s, _WHY.get(k, "")
        if q.fold_pass or nm.has_enye:
            fq = q.ffull if nm.kind == "full" else q.fcore
            s2, _k = _eval(fq, nm.ftoks, forms.fctx, whole_only=whole)
            s2 *= nm.w * ENYE_DISCOUNT
            if s2 > best:
                best, why = s2, "enye~n"
    if q.artist and best < 0.97 and _art_cov(q.core, forms.art) >= 0.99:
        # etiquetas cruzadas: lo pedido como titulo es el artista de la cancion
        # y el artista pedido es su titulo. Casan las DOS cosas: es esa
        for nm in forms.names:
            if nm.kind in ("core", "full"):
                s3, _k = _eval(q.artist, nm.toks, forms.fctx, whole_only=True)
                if s3 >= 0.95 and best < 0.97 + B_ARTIST:
                    best, why = 0.97 + B_ARTIST, "titulo exacto"
    if best < T_NEAR:
        return None
    if q.artist:
        best += B_ARTIST * _art_cov(q.artist, forms.art)
    if q.ver:
        got = sum(1 for t in q.ver if t in forms.ver)
        best += B_VERSION * got / len(q.ver)
        # «Hay Libertad (Ana Torres)»: lo que va entre parentesis tambien puede ser el artista
        aparte = sorted(t for t in q.ver if t not in VERSION)
        if aparte:
            best += B_PAREN_ARTIST * _art_cov(aparte, forms.art)
    if q.people:
        got = sum(1 for t in q.people if t in forms.fctx)
        best += B_HINT * got / len(q.people)
    return best, why


# ----------------------------------------------------- enlaces e historial


def _video_id(url: Any) -> str:
    """El id de un video de YouTube, o «» si no es un enlace de un video.

    Mira el DOMINIO (`youtube.host_of` y su lista cerrada), nunca el texto:
    «https://youtube.com.otro.example/watch?v=...» no es YouTube.
    """
    from .. import youtube

    u = str(url or "").strip()[:500]
    if not u:
        return ""
    if "://" not in u and u.lower().startswith(
        ("youtu.be/", "www.youtu.be/", "youtube.com/", "www.youtube.com/", "m.youtube.com/")
    ):
        u = "https://" + u
    host = youtube.host_of(u)
    if host not in youtube.ALLOWED_HOSTS:
        return ""
    try:
        parts = urllib.parse.urlsplit(u)
        segs = [s for s in parts.path.split("/") if s]
        if host.endswith("youtu.be"):
            vid = segs[0] if segs else ""
        elif segs and segs[0] == "watch":
            vid = urllib.parse.parse_qs(parts.query).get("v", [""])[0]
        elif len(segs) > 1 and segs[0] in ("shorts", "embed", "live", "v"):
            vid = segs[1]
        else:
            vid = ""
    except ValueError:
        return ""
    return vid if _VIDEO_ID.fullmatch(vid) else ""


def _base(path: str) -> str:
    return str(path or "").replace("\\", "/").rsplit("/", 1)[-1].strip().lower()


def _same_file(song_path: str, target: str) -> bool:
    """El archivo de la cancion es el que dejo aquella descarga."""
    if not song_path or not target:
        return False
    return song_path == target or _base(song_path) == _base(target)


def _from_history(vid: str, snap: _Snapshot) -> _Entry | None:
    """La cancion que una descarga de ese video dejo en la biblioteca, si sigue
    siendo ELLA.

    Los ids de cancion se reutilizan al borrar la ultima fila, y las etiquetas
    se editan: lo unico que no cambia es el archivo. Una fila del historial
    solo vale si `songs.path` es el `downloads.target` (o el mismo nombre de
    archivo). Se recorren de la mas nueva a la mas vieja y vale la primera que
    valide; si ninguna, se sigue por el titulo.
    """
    try:
        with library.connect() as conn:
            rows = conn.execute(
                "SELECT url, query, ok, already, song_id, target, title FROM downloads "
                "WHERE (instr(url, ?) > 0 OR instr(query, ?) > 0) "
                "ORDER BY at DESC, id DESC LIMIT 50",
                (vid, vid),
            ).fetchall()
    except sqlite3.Error:
        log.debug("no se pudo leer el historial de descargas", exc_info=True)
        return None
    for row in rows:
        if not (row["ok"] or row["already"]) or not row["song_id"]:
            continue
        if vid not in (_video_id(row["url"]), _video_id(row["query"])):
            continue
        found = snap.fetch([row["song_id"]])
        if not found:
            continue
        e = found[0]
        if _same_file(e.path, row["target"] or ""):
            return e
        # «ya la tenias»: no hay archivo nuevo; se comprueba por la clave de nombre
        if (
            row["already"]
            and not row["target"]
            and row["title"]
            and e.match_key
            and e.match_key == names.match_key(names.clean(row["title"]))
        ):
            return e
    return None


# ------------------------------------------------------- el resultado


@dataclass(slots=True)
class Candidate:
    """Una cancion que podria ser la pedida. `why`: titulo exacto, frase en
    titulo, palabras, difuso, enye~n, enlace o elegida (la dijo la persona)."""

    song: dict
    score: float
    why: str


@dataclass(slots=True)
class Resolution:
    """Que se hizo con un item: la cancion elegida (si la hay) y las demas
    posibilidades. `status`:

      found      la tenemos (una sola version, o copias de la misma)
      ambiguous  la tenemos en varias versiones: `song` es la elegida y las
                 otras van en `candidates`
      probable   parece esta, pero no del todo (una errata, solo el comienzo,
                 otro orden): se usa con aviso
      missing    no la tenemos. Con `failed` NO quiere decir eso: no se pudo mirar
    """

    index: int
    query: dict
    status: str
    song: dict | None
    candidates: list[Candidate]
    via: str = ""  # enlace | titulo | difuso | «»
    other_version: bool = False
    note: str = ""
    failed: bool = False


def _tiebreak(e: _Entry, prefer: frozenset[int]) -> tuple:
    return (e.sid not in prefer, not e.favorite, -e.stars, e.shortness, e.sid)


def _rank(scored: list[tuple[float, str, _Entry]], prefer: frozenset[int]) -> list:
    """Ordena por puntuacion; las que empatan (a menos de TIE) se desempatan por
    la eleccion recordada, favorita, estrellas y titulo mas corto."""
    scored.sort(key=lambda r: (-r[0], r[2].sid))
    out: list = []
    i = 0
    while i < len(scored):
        j = i + 1
        while j < len(scored) and scored[i][0] - scored[j][0] <= TIE:
            j += 1
        group = sorted(scored[i:j], key=lambda r: _tiebreak(r[2], prefer))
        out.extend(group)
        i = j
    return out


def _candidates(snap: _Snapshot, q: _Q) -> list[_Entry]:
    """Las canciones que merece la pena puntuar para esta peticion."""
    if q.empty:
        return []
    if snap.large:
        return _prefilter(snap, q)
    toks = list(dict.fromkeys(q.fcore))
    content = [t for t in toks if t not in STOP]
    ids: set[int] = set(snap.bag.get(tuple(sorted(q.fcore)), ()))
    ids.update(snap.joined.get("".join(q.fcore), ()))
    if content:
        hits: Counter = Counter()
        for t in content:
            found = set(snap.post.get(t, ()))
            if len(t) >= 4:
                for v in snap.neighbors(t):
                    found.update(snap.post.get(v, ()))
            for i in found:
                hits[i] += 1
        if len(hits) > PREFILTER:
            ids.update(i for i, _ in hits.most_common(PREFILTER))
        else:
            ids.update(hits)
    wanted = [t for t in dict.fromkeys(q.fartist) if t not in STOP and len(t) >= 4]
    if wanted:
        # el artista pedido como titulo de la cancion: etiquetas de artista y titulo cruzadas
        lists = [snap.post.get(t, ()) for t in wanted]
        if all(lists):
            common = set(lists[0]).intersection(*lists[1:])
            if len(common) <= PREFILTER:
                ids.update(common)
    stops = [t for t in toks if t in STOP]
    if stops and (len(stops) >= 2 or not content):
        # las palabras vacias que lleva («Es El» + el artista pegado): los nombres que
        # las llevan todas
        lists = [snap.post.get(t, ()) for t in stops]
        if all(lists):
            common = set(lists[0]).intersection(*lists[1:])
            if len(common) <= PREFILTER:
                ids.update(common)
    return [snap.entries[i] for i in sorted(ids)]


def _prefilter(snap: _Snapshot, q: _Q, *, loose: bool = False) -> list[_Entry]:
    """Con muchas canciones: FTS5 por OR de las palabras y `bm25` por encima.

    Con `loose`, por los tres primeros caracteres de cada palabra larga: es lo
    que deja pasar una errata (que el indice no conoce) a la puntuacion.
    """
    words = q.loose if loose else q.fts
    if words:
        match = " OR ".join('"' + t.replace('"', '""') + '"*' for t in words)
    elif loose:
        return []
    else:
        # sin palabras utiles: el titulo con TODAS las palabras vacias que trae
        toks = list(dict.fromkeys(q.fcore))
        if not toks:
            return []
        match = "title : (" + " AND ".join('"' + t.replace('"', '""') + '"' for t in toks[:6]) + ")"
    try:
        with library.connect() as conn:
            rows = conn.execute(
                "SELECT rowid FROM search_index WHERE search_index MATCH ? "
                "ORDER BY bm25(search_index, 0, 10, 1, 1, 0, 0) LIMIT ?",
                (match, PREFILTER),
            ).fetchall()
    except sqlite3.Error:
        log.debug("prefiltro FTS5 sin resultado", exc_info=True)
        return []
    return snap.fetch(r[0] for r in rows)


def _light_candidates(
    ranked: list[tuple[float, str, _Entry]], limit: int, floor: float = T_NEAR
) -> list[Candidate]:
    return [
        Candidate(song=dict(e.song), score=round(min(s, 1.0), 3), why=w)
        for s, w, e in ranked[: max(1, limit)]
        if s >= floor
    ]


_NOTES = {
    "titulo exacto": "",
    "frase en titulo": "solo coincide una parte del título",
    "palabras": "las mismas palabras, en otro orden o con otras de más",
    "difuso": "se parece, pero hay una letra distinta",
    "enye~n": "coincide si no se cuenta la ñ",
}


def _decide(
    ranked: list[tuple[float, str, _Entry]], limit: int
) -> tuple[str, dict | None, list[Candidate], str, str]:
    """-> (estado, cancion, candidatas, via, nota)."""
    if not ranked or ranked[0][0] < T_MISSING:
        # lo mas parecido, por si sirve de «¿quisiste decir…?»: solo si se parece de verdad
        return "missing", None, _light_candidates(ranked, min(limit, 3), floor=0.5), "", ""
    top_score, top_why, top = ranked[0]
    cands = _light_candidates(ranked, limit, floor=T_MISSING)
    if top_score >= T_FOUND:
        others = [
            r
            for r in ranked[1:]
            if top_score - r[0] <= AMB_DELTA and r[2].forms.ver_key != top.forms.ver_key
        ]
        if others:
            n = len(others)
            return (
                "ambiguous",
                dict(top.song),
                cands,
                "titulo",
                f"hay {n} {'versión' if n == 1 else 'versiones'} más",
            )
        return "found", dict(top.song), cands, "titulo", _NOTES.get(top_why, "")
    return "probable", dict(top.song), cands, "difuso", _NOTES.get(top_why, "")


def _by_id(snap: _Snapshot, sid: Any) -> _Entry | None:
    try:
        n = int(sid)
    except (TypeError, ValueError):
        return None
    found = snap.fetch([n])
    return found[0] if found else None


def _run(snap: _Snapshot, q: _Q, prefer: frozenset[int], limit: int) -> tuple:
    """Puntua las candidatas de una peticion -> (estado, cancion, candidatas, via, nota)."""
    scored: list[tuple[float, str, _Entry]] = []
    seen_ids: set[int] = set()
    for e in _candidates(snap, q):
        seen_ids.add(e.sid)
        r = _score(q, e)
        if r is not None:
            scored.append((r[0], r[1], e))
    if snap.large and q.loose and max((r[0] for r in scored), default=0.0) < T_MISSING:
        # nada convence: puede ser una errata, que el prefiltro exacto no ve
        for e in _prefilter(snap, q, loose=True):
            if e.sid not in seen_ids:
                r = _score(q, e)
                if r is not None:
                    scored.append((r[0], r[1], e))
    return _decide(_rank(scored, prefer), limit)


def _video_query(video_title: str, artist: Any, hints: Iterable[Any]) -> _Q:
    """Lo pedido, leido del titulo de un video de YouTube («Artista - Titulo»).
    El artista del video es una pista, como cualquier otro: solo suma."""
    v = names.from_video(video_title, "")
    return _parse(v.get("title") or video_title, artist or v.get("artist") or "", hints)


def _match(
    snap: _Snapshot,
    title: Any,
    artist: Any,
    *,
    hints: Iterable[Any],
    url: Any,
    video_title: Any,
    prefer: frozenset[int],
    limit: int,
    strict: bool,
    index: int = 0,
    n: int | None = None,
    note: Any = "",
) -> Resolution:
    query = {
        "title": str(title or "")[:300],
        "artist": str(artist or "")[:200],
        "url": str(url or "")[:300],
        "note": str(note or "")[:200],
        "n": n,
    }
    url_s = str(url or "")[:500].strip()
    if not url_s:
        found_url = _URL.search(str(title or "")[:MAX_CHARS])
        url_s = found_url.group(0)[:500] if found_url else ""
    vid = _video_id(url_s) if url_s else ""

    # 1. el enlace manda: el video ya se bajo y la cancion sigue siendo ELLA
    if vid:
        linked = _from_history(vid, snap)
        if linked is not None:
            return Resolution(
                index=index,
                query=query,
                status="found",
                song=dict(linked.song),
                candidates=[Candidate(song=dict(linked.song), score=1.0, why="enlace")],
                via="enlace",
                note="el enlace ya está en la biblioteca",
            )

    # 2. por el titulo; si no hay (o no la encuentra), por el titulo del video
    hints = list(hints)
    vt = str(video_title or "")[:MAX_CHARS].strip()
    q = _parse(title, artist, hints)
    result = _run(snap, q, prefer, limit) if not q.empty else None
    if (result is None or result[0] == "missing") and vt:
        qv = _video_query(vt, artist, hints)
        if not qv.empty:
            again = _run(snap, qv, prefer, limit)
            if again[0] != "missing" or result is None:
                result = again
    if result is None:
        return Resolution(
            index=index,
            query=query,
            status="missing",
            song=None,
            candidates=[],
            note="sin título" if not url_s else "enlace sin título",
        )
    status, song, cands, via, note = result

    other = bool(vid) and status != "missing"
    if other:
        note = "el enlace no está en el historial; se usó la versión de la biblioteca"
        if strict:
            return Resolution(
                index=index,
                query=query,
                status="missing",
                song=None,
                candidates=cands,
                via="",
                other_version=True,
                note="hay otra versión en la biblioteca, pero pediste la del enlace",
            )
    return Resolution(
        index=index,
        query=query,
        status=status,
        song=song,
        candidates=cands,
        via=via,
        other_version=other,
        note=note,
    )


# ------------------------------------------------------------ la API


def _prefer(prefer_ids: Iterable[Any]) -> frozenset[int]:
    out: set[int] = set()
    for i in prefer_ids or ():
        try:
            out.add(int(i))
        except (TypeError, ValueError):
            continue
    return frozenset(out)


def match_song(
    title: Any,
    artist: Any = "",
    *,
    hints: Iterable[Any] = (),
    url: Any = "",
    video_title: Any = "",
    prefer_ids: Iterable[Any] = (),
    limit: int = 5,
    strict: bool = False,
) -> Resolution:
    """¿Tenemos esta cancion? -> `Resolution` (ver su estado y `candidates`).

    `title` puede venir tal cual lo pega la persona (numeracion, negritas,
    emojis, @menciones, parentesis, ruido de video): se limpia aqui. `artist`
    y `hints` (notas, personas) solo suman. `url`/`video_title`: el enlace de
    la cancion y el titulo de ese video, si se conoce. `prefer_ids`: canciones
    que ganan los empates (la version que se eligio otras veces; ver
    `title_key`). Con `strict`, un enlace que no esta en el historial no se
    sustituye por otra version: es `missing` con `other_version=True`.
    """
    return _match(
        INDEX.snapshot(),
        title,
        artist,
        hints=hints,
        url=url,
        video_title=video_title,
        prefer=_prefer(prefer_ids),
        limit=limit,
        strict=strict,
    )


def _as_item(raw: Any) -> dict:
    if isinstance(raw, str):
        return {"title": raw}
    if isinstance(raw, Mapping):
        return dict(raw)
    return {
        k: getattr(raw, k) for k in ("title", "artist", "url", "note", "n", "id") if hasattr(raw, k)
    }


def _failed(index: int, item: dict, n: int | None, note: str) -> Resolution:
    return Resolution(
        index=index,
        query={
            "title": str(item.get("title") or "")[:300],
            "artist": str(item.get("artist") or "")[:200],
            "url": str(item.get("url") or "")[:300],
            "note": str(item.get("note") or "")[:200],
            "n": n,
        },
        status="missing",
        song=None,
        candidates=[],
        note=note,
        failed=True,
    )


def resolve_items(
    items: Iterable[Any],
    *,
    prefer_ids: Iterable[Any] = (),
    video_titles: Mapping[Any, Any] | None = None,
    overrides: Iterable[Mapping[str, Any]] | None = None,
    strict: bool = False,
    budget: float = BATCH_BUDGET,
) -> list[Resolution]:
    """Empareja una lista de items con la biblioteca, uno a uno.

    Cada item es un texto o un dict {title, artist?, url?, note?, n?, id?}.
    Devuelve una `Resolution` por item, en el mismo orden. Cada item va en su
    propio `try`: un titulo raro no tumba el lote (queda `missing` con
    `failed=True`: no se pudo mirar, que no es lo mismo que «no esta»). Con
    `budget` (segundos) agotado, los que faltan quedan igual, `failed`.

    - `overrides`: [{n, id}] — la persona eligio esa cancion para el item `n`.
    - `video_titles`: {url o n: titulo del video} para los items sin titulo.
    - `prefer_ids`, `strict`: ver `match_song`.
    """
    prefer = _prefer(prefer_ids)
    chosen: dict[int, Any] = {}
    for o in overrides or ():
        try:
            chosen[int(o["n"])] = o["id"]
        except (KeyError, TypeError, ValueError):
            continue
    start = time.monotonic()
    out: list[Resolution] = []
    snap: _Snapshot | None = None
    for i, raw in enumerate(items):
        item: dict = {}
        n: int | None = None
        try:
            item = _as_item(raw)
            try:
                n = int(item["n"]) if item.get("n") not in (None, "") else i + 1
            except (TypeError, ValueError):
                n = i + 1
            if i >= MAX_ITEMS:
                out.append(_failed(i, item, n, "demasiados items; no se miraron todos"))
                continue
            if time.monotonic() - start > budget:
                out.append(_failed(i, item, n, "se acabó el tiempo; no se miró"))
                continue
            if snap is None:
                snap = INDEX.snapshot()
            query = {
                "title": str(item.get("title") or "")[:300],
                "artist": str(item.get("artist") or "")[:200],
                "url": str(item.get("url") or "")[:300],
                "note": str(item.get("note") or "")[:200],
                "n": n,
            }
            wanted = chosen.get(n, item.get("id"))
            if wanted not in (None, ""):
                e = _by_id(snap, wanted)
                if e is not None:
                    out.append(
                        Resolution(
                            index=i,
                            query=query,
                            status="found",
                            song=dict(e.song),
                            candidates=[Candidate(song=dict(e.song), score=1.0, why="elegida")],
                            note="la eligió la persona",
                        )
                    )
                    continue
            vt = ""
            if video_titles:
                vt = video_titles.get(item.get("url")) or video_titles.get(n) or ""
            res = _match(
                snap,
                item.get("title"),
                item.get("artist"),
                hints=[item.get("note")] if item.get("note") else (),
                url=item.get("url"),
                video_title=vt,
                prefer=prefer,
                limit=5,
                strict=strict,
                index=i,
                n=n,
                note=item.get("note"),
            )
            if wanted not in (None, "") and not res.note:
                res.note = "ese id no existe; se buscó por el título"
            out.append(res)
        except Exception:
            log.warning("no se pudo emparejar el item %d", i, exc_info=True)
            out.append(_failed(i, item, n, "no se pudo mirar este item"))
    return out


def rank(title: Any, artist: Any = "") -> list[dict]:
    """Adaptador para el medidor (`baseline/measure_search.py --impl`):
    [{id, score, status}] ordenado, el primero con el estado del emparejador;
    vacio si dice que no esta."""
    res = match_song(title, artist, limit=10)
    if res.status == "missing":
        return []
    return [
        {"id": c.song["id"], "score": c.score, "status": res.status if i == 0 else "alt"}
        for i, c in enumerate(res.candidates)
    ]
