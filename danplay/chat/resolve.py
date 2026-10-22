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

Lo hace Rust (biblioteca/src/asistente/emparejar.rs, cuenta a cuenta y en el
mismo orden que antes estaba aqui, con los umbrales y los pesos); esto es la
puerta de Python: el indice por revision y su cerrojo, el lote con sus topes
y su presupuesto, y los mismos nombres.
"""

import logging
import re
import threading
import time
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from typing import Any

import danplay_core as _rust

from .. import config, library

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
# (los pesos y el resto de umbrales estan en emparejar.rs)

# Hasta aqui el indice entero en memoria y sin prefiltro; por encima, FTS5.
FULL_LIMIT = 5000
# Candidatos que deja pasar el prefiltro FTS5.
PREFILTER = 400
# Lo que se mira de un titulo pedido: el resto es ruido o un ataque.
MAX_CHARS = 1000
MAX_TOKENS = 40
MAX_GROUPS = 8
# Items de un lote y tiempo total que se le da (segundos).
MAX_ITEMS = 200
BATCH_BUDGET = 10.0

T_FOUND = 0.90  # desde aqui «la tenemos»
T_MISSING = 0.60  # por debajo, «no la tenemos»
T_NEAR = 0.40  # por debajo ni se enseña como alternativa

# Los sustitutos sueltos (un texto con bytes que no son UTF-8) no llegan a
# Rust: pasan a «�».
_SURROGATE = re.compile("[\ud800-\udfff]")


def _s(value: Any) -> str:
    """`str(value or "")`, que se pueda pasar a Rust."""
    s = str(value or "")
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        return _SURROGATE.sub("�", s)
    return s


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


def _parse(title: Any, artist: Any = "", hints: Iterable[Any] = ()) -> _Q:
    """Lo pedido, partido en lo que es titulo y lo que es pista."""
    d = _rust.resolve_parse(_s(title), _s(artist), [_s(h) for h in list(hints)[:MAX_GROUPS]])
    return _Q(
        core=d["core"],
        full=d["full"],
        fcore=tuple(d["fcore"]),
        ffull=tuple(d["ffull"]),
        ver=frozenset(d["ver"]),
        people=frozenset(d["people"]),
        artist=d["artist"],
        fartist=tuple(d["fartist"]),
        fold_pass=d["fold_pass"],
        fts=d["fts"],
        loose=d["loose"],
        empty=d["empty"],
    )


def _near(a: str, b: str) -> bool:
    """Una errata: las dos palabras difieren en UNA letra (cambiada, de mas,
    de menos o dos seguidas cambiadas de sitio) y la mas larga tiene 5 o mas.
    La ñ por la n NO cuenta como errata (Mañana no es manana)."""
    return _rust.resolve_near(_s(a), _s(b))


def _video_id(url: Any) -> str:
    """El id de un video de YouTube, o «» si no es un enlace de un video.

    Mira el DOMINIO (`youtube.host_of` y su lista cerrada), nunca el texto:
    «https://youtube.com.otro.example/watch?v=...» no es YouTube.
    """
    return _rust.resolve_video_id(_s(url))


def title_key(title: Any, artist: Any = "") -> str:
    """Una clave estable de «esta cancion pedida», para recordar la version
    elegida entre una semana y otra.

    Es el titulo en la forma de buscar, sin numeracion, negritas, emojis,
    menciones, parentesis ni ruido: «*3.-Digno De Adorar* ✅ @Fulano (En Su
    Presencia)» y «digno de adorar» dan la misma. Con `artist` (sin
    menciones) se le añade el artista; quien quiera recordar por titulo a
    secas pasa `artist=""`. La ñ se conserva, como en toda comparacion.
    """
    return _rust.resolve_title_key(_s(title), _s(artist))


# ------------------------------------------------------- la biblioteca


class _Snapshot:
    """La biblioteca vista por el emparejador, en el estado de una revision
    (la lee y la guarda Rust: `ResolveIndex`)."""

    def __init__(self, key: tuple[str, int], forms_cache: dict | None = None) -> None:
        self.key = key
        self._rs: Any = None

    @property
    def large(self) -> bool:
        return bool(self._rs is not None and self._rs.large)

    @property
    def count(self) -> int:
        return self._rs.count if self._rs is not None else 0

    def load(self, conn: library.Connection) -> None:
        self._rs = _rust.resolve_load(conn, self.key[0], FULL_LIMIT)


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


def _query(item: Mapping[str, Any], n: int | None) -> dict:
    return {
        "title": str(item.get("title") or "")[:300],
        "artist": str(item.get("artist") or "")[:200],
        "url": str(item.get("url") or "")[:300],
        "note": str(item.get("note") or "")[:200],
        "n": n,
    }


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
    query = _query({"title": title, "artist": artist, "url": url, "note": note}, n)
    d = snap._rs.matching(
        _s(title),
        _s(artist),
        [_s(h) for h in hints],
        _s(url),
        _s(video_title),
        sorted(prefer),
        int(limit),
        bool(strict),
    )
    return Resolution(
        index=index,
        query=query,
        status=d["status"],
        song=d["song"],
        candidates=[
            Candidate(song=c["song"], score=c["score"], why=c["why"]) for c in d["candidates"]
        ],
        via=d["via"],
        other_version=d["other_version"],
        note=d["note"],
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
        query=_query(item, n),
        status="missing",
        song=None,
        candidates=[],
        note=note,
        failed=True,
    )


def _by_id(snap: _Snapshot, sid: Any) -> dict | None:
    """La cancion con ese id (fila ligera), o None."""
    try:
        n = int(sid)
    except (TypeError, ValueError):
        return None
    if not -(2**63) <= n < 2**63:
        return None
    return snap._rs.by_id(n)


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
            query = _query(item, n)
            wanted = chosen.get(n, item.get("id"))
            if wanted not in (None, ""):
                song = _by_id(snap, wanted)
                if song is not None:
                    out.append(
                        Resolution(
                            index=i,
                            query=query,
                            status="found",
                            song=dict(song),
                            candidates=[Candidate(song=dict(song), score=1.0, why="elegida")],
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
