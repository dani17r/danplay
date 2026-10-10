"""Una lista de canciones contra un repertorio: comprobarla o meterla, por codigo.

Es lo que hay detras de `find_songs` y `fill_playlist` del asistente. La
persona pega la lista del domingo (o nombra unas canciones) y pide meterla en
una lista, comprobarla o dejarla exacta. Antes lo hacia el modelo a base de
vueltas: buscaba cancion por cancion con todas las palabras obligatorias, se
liaba con los ids, reescribia la lista cuando solo le pedian verificarla y se
quedaba sin vueltas a la cuarta cancion. Aqui:

  1. Lo pedido sale de la lista pegada (`intake`, ya leida por codigo) o, si no
     hay, de lo que mande el modelo.
  2. Cada cancion se busca en la biblioteca (`resolve`): biblioteca primero,
     siempre.
  3. Se decide que hacer con las GUARDAS de la persona, no del modelo
     (`intake.read_intent`): «verifica» nunca cambia nada; dejar una lista
     exacta solo si lo pidio con sus palabras; crear una lista solo si la
     pidio.
  4. Se mete lo encontrado de una vez (`playlists.place`/`set_songs`, bajo el
     cerrojo de las listas) en el orden de la lista pegada.
  5. Lo que falta se propone para descargar en una tarjeta (`plan`), con lo
     que hay que hacer al terminar: meter cada una en su sitio.

El resultado va en dos partes: lo que lee el modelo (corto: contadores, ids,
lo que falta) y `_ui`, la tabla entera y la tarjeta, que el bucle aparta
antes de mandarle nada al modelo y entrega a la interfaz.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from itertools import pairwise
from typing import Any

from .. import names, playlists, youtube
from . import intake, resolve, safe

log = logging.getLogger(__name__)

MAX_ITEMS = intake.MAX_SETLIST_ITEMS
# Lo que va en la lista: la tenemos (una version, otra version, o varias y se
# eligio una). «probable» (una errata, otro orden) se enseña pero no entra.
ADDABLE = frozenset({"found", "other_version", "ambiguous"})
# Lo que el modelo lee de la tabla, como mucho (la tabla entera va a la interfaz)
MODEL_ROWS_CHARS = 1800
# Nombres que se enseñan en una frase antes de «y N mas»
SHOWN_NAMES = 3

# Palabras que acompañan al nombre de una lista sin ser su nombre: «la lista
# de los domingo» es «domingo».
_FILLER = frozenset(
    {
        *("lista", "listas", "repertorio", "repertorios", "playlist", "playlists"),
        *("setlist", "set", "la", "las", "el", "los", "lo", "de", "del", "para"),
        *("mi", "mis", "tu", "tus", "nuestra", "nuestro", "esta", "este"),
    }
)


@dataclass
class Turn:
    """Lo que el bucle sabe del turno y las herramientas necesitan.

    Se pasa explicito (`run_tool(name, args, turn)`), nunca por hilo: las
    peticiones del chat corren en hilos que se reutilizan.

    `setlist`: la ultima lista pegada de la conversacion (o None).
    `fresh`: si esa lista esta en el ULTIMO mensaje de la persona; si es de
        antes, solo se usa cuando el modelo no manda canciones.
    `intent`: lo que la persona pidio con sus palabras (`intake.read_intent`).
    `can_ask`: si aun se puede pedir permiso en este turno (no hay otra
        confirmacion esperando).
    """

    setlist: intake.SetlistParse | None = None
    fresh: bool = False
    intent: intake.Intent = field(default_factory=intake.Intent)
    name_hint: str = ""
    can_ask: bool = True
    chat_id: int | None = None
    cancel: Any = None


# --------------------------------------------------------------- utilidades


def _light(song: dict | None) -> dict | None:
    """Una cancion como la pinta la interfaz: id, artista y titulo."""
    if not song:
        return None
    return {
        "id": int(song.get("id") or 0),
        "artist": safe.datum(song.get("artist") or "", 120),
        "title": safe.datum(song.get("title") or "", 160),
    }


def _song_name(song: dict | None) -> str:
    if not song:
        return ""
    artist, title = song.get("artist") or "", song.get("title") or ""
    return f"{artist} - {title}" if artist else title


def _names(values: list[str], limit: int = SHOWN_NAMES) -> str:
    """«A», «A y B», «A, B y C», «A, B, C y 2 mas».

    Para la persona: con tildes."""
    vals = [v for v in values if v]
    if not vals:
        return ""
    if len(vals) > limit:
        rest = len(vals) - limit
        return ", ".join(vals[:limit]) + f" y {rest} más"
    if len(vals) == 1:
        return vals[0]
    return ", ".join(vals[:-1]) + " y " + vals[-1]


def _core(name: str) -> tuple[str, ...]:
    """Las palabras que nombran una lista, sin relleno y sin plural.

    «la lista de los domingos» y «Domingo» dan lo mismo: («domingo»,)."""
    toks = names.search_tokens(name)
    core = [t for t in toks if t not in _FILLER] or toks
    return tuple(t[:-1] if len(t) > 4 and t.endswith("s") else t for t in core)


def find_playlist(name: str) -> tuple[dict | None, list[dict]]:
    """La lista a la que se refiere `name`, o (None, las que se parecen).

    Primero la que tiene el mismo nombre en la forma de buscar; luego sin
    relleno ni plural («los domingo» = «domingo»); luego con la ñ plegada;
    y por ultimo la UNICA que contiene todas las palabras pedidas («ensayo» →
    «ensayo domingo»). Si hay varias, ninguna: se devuelven para preguntar.
    """
    want_form = names.search_form(name)
    want = _core(name)
    if not want:
        return None, []
    lists = playlists.list_all()
    same = [p for p in lists if names.search_form(p["name"]) == want_form]
    if same:
        return same[0], []
    core = [p for p in lists if _core(p["name"]) == want]
    if len(core) == 1:
        return core[0], []
    folded = tuple(names.fold_enye(t) for t in want)
    enye = [p for p in lists if tuple(names.fold_enye(t) for t in _core(p["name"])) == folded]
    if len(enye) == 1:
        return enye[0], []
    if core or enye:
        return None, core or enye
    inside = [p for p in lists if set(want) <= set(_core(p["name"]))]
    if len(inside) == 1:
        return inside[0], []
    return None, inside


def _playlist_names(lists: list[dict], limit: int = 12) -> str:
    shown = ", ".join(f"«{p['name']}»" for p in lists[:limit])
    if len(lists) > limit:
        shown += f" y {len(lists) - limit} mas"
    return shown or "ninguna"


# ------------------------------------------------------------ lo que se pide


def _item_from_text(raw: str) -> dict:
    """Un item que manda el modelo como texto: un titulo o un enlace."""
    text = str(raw or "").strip()
    if youtube.host_of(text):
        url = intake.clean_youtube_url(text[: intake.MAX_URL])
        kind = "playlist" if "/playlist?" in url else ("video" if url else "")
        return {"title": "", "artist": "", "url": url, "kind": kind, "note": "", "raw": text[:300]}
    return {
        "title": text[: intake.MAX_TITLE],
        "artist": "",
        "url": "",
        "kind": "",
        "note": "",
        "raw": text[:300],
    }


def _item_from_dict(raw: dict) -> dict:
    url = str(raw.get("url") or "").strip()[: intake.MAX_URL]
    clean = intake.clean_youtube_url(url) if url else ""
    out: dict[str, Any] = {
        "title": str(raw.get("title") or "").strip()[: intake.MAX_TITLE],
        "artist": str(raw.get("artist") or "").strip()[: intake.MAX_TITLE],
        "url": clean,
        "kind": "playlist" if "/playlist?" in clean else ("video" if clean else ""),
        "note": str(raw.get("note") or "").strip()[: intake.MAX_NOTE],
        "raw": "",
    }
    if raw.get("id") not in (None, ""):
        out["id"] = raw.get("id")
    return out


def requested(items: Any, turn: Turn) -> tuple[list[dict], dict]:
    """Lo que se pide, numerado, y de donde salio.

    La lista pegada manda cuando esta en el ultimo mensaje de la persona: el
    modelo la copiaba mal (con las @menciones de artista, «Voz» en el titulo,
    enlaces cruzados). Si la lista es de antes, solo se usa si el modelo no
    manda canciones: «verifica la lista» tras pegarla, si; «añade Way Maker»
    diez mensajes despues, no.

    Devuelve (items, origen) con origen = {"source": "pegada"|"modelo",
    "truncated": N, "ignored": [...]}.
    """
    sl = turn.setlist
    model_items = items if isinstance(items, list) else ([items] if items else [])
    if sl is not None and sl.items and (turn.fresh or not model_items):
        out = []
        for i, it in enumerate(sl.items[:MAX_ITEMS]):
            out.append(
                {
                    "n": it.n or i + 1,
                    "title": it.title,
                    "artist": it.artist,
                    "url": it.url,
                    "kind": it.kind,
                    "note": it.note,
                    "people": list(it.people),
                    "raw": it.raw,
                }
            )
        # la numeracion de la persona puede repetirse: n unico
        seen: set[int] = set()
        for it in out:
            if it["n"] in seen:
                it["n"] = max(seen) + 1
            seen.add(it["n"])
        return out, {
            "source": "pegada",
            "truncated": sl.truncated,
            "ignored": [safe.datum(x, 80) for x in sl.extras if not _is_request(x)][:10],
        }
    out = []
    for raw in model_items[:MAX_ITEMS]:
        it = _item_from_dict(raw) if isinstance(raw, dict) else _item_from_text(str(raw))
        if not (it["title"] or it["url"] or it.get("id")):
            continue
        it["n"] = len(out) + 1
        out.append(it)
    return out, {
        "source": "modelo",
        "truncated": max(0, len(model_items) - MAX_ITEMS),
        "ignored": [],
    }


# Las palabras con las que se pide algo: «agrégame esto a la lista…»,
# «verifica…». Esa linea es lo que la persona dice, no una seccion de la lista.
_ASK_STEMS = (
    "agreg",
    "anad",
    "añad",
    "mete",
    "met",
    "pon",
    "verific",
    "comprueb",
    "revis",
    "crea",
    "arma",
    "descarg",
    "baj",
    "dej",
    "orden",
    "quit",
    "pasa",
)


def _is_request(line: str) -> bool:
    """Si una linea que no es cancion es lo que se pide y no una seccion
    («Banda: …», «Ensayo: …»), que si merece decirse que se dejo fuera."""
    words = names.search_tokens(line)
    if not words:
        return False
    if words[0].startswith(_ASK_STEMS):
        return True
    asks = any(w.startswith(_ASK_STEMS) for w in words[:3])
    return asks and (str(line).rstrip().endswith(":") or "lista" in words)


def _resolve(items: list[dict], *, prefer: list[int], overrides: Any, strict: bool) -> list:
    return resolve.resolve_items(
        [
            {
                "n": it["n"],
                "title": it["title"],
                "artist": it["artist"],
                "url": it["url"] if it.get("kind") != "playlist" else "",
                "note": it["note"],
                **({"id": it["id"]} if it.get("id") not in (None, "") else {}),
            }
            for it in items
        ],
        prefer_ids=prefer,
        overrides=_overrides(overrides),
        strict=strict,
    )


def _in_lists(exclude: int | None = None) -> list[int]:
    """Las canciones que ya estan en alguna lista.

    Cuando una cancion esta en varias versiones (en vivo, de otro artista), la
    que ya se uso en un repertorio es la que la persona toca: gana el empate
    (`resolve`, `prefer_ids`). La de la propia lista va delante (la pasa quien
    llama)."""
    out: list[int] = []
    try:
        for p in playlists.list_all():
            if p["id"] != exclude:
                out.extend(s["id"] for s in playlists.songs(p["id"], light=True))
    except Exception:
        log.warning("no se pudieron leer las listas para desempatar", exc_info=True)
    return list(dict.fromkeys(out))


def _overrides(raw: Any) -> list[dict]:
    out = []
    for o in raw or ():
        if not isinstance(o, dict):
            continue
        try:
            out.append({"n": int(o["n"]), "id": int(o["id"])})
        except (KeyError, TypeError, ValueError):
            continue
    return out[:MAX_ITEMS]


def _only(raw: Any) -> set[int]:
    out: set[int] = set()
    for x in raw or ():
        try:
            out.add(int(x))
        except (TypeError, ValueError):
            continue
    return out


# ------------------------------------------------------------------- filas


def _row(item: dict, res) -> dict:
    """Una fila de la tabla: lo pedido junto a lo encontrado."""
    status = res.status
    if status == "found" and res.other_version:
        status = "other_version"
    song = res.song if status != "missing" else None
    alts = []
    for c in res.candidates:
        sid = c.song.get("id")
        if song and sid == song.get("id"):
            continue
        alts.append(_light(c.song))
        if len(alts) >= 3:
            break
    title = item["title"] or (f"enlace {item['url']}" if item["url"] else "")
    note = res.note or ""
    if item.get("kind") == "playlist":
        note = "es una lista de YouTube entera: no la bajo desde una lista pegada"
    return {
        "n": item["n"],
        "title": safe.datum(title, 160),
        "status": status,
        "song": _light(song),
        "alts": alts if status in ("ambiguous", "probable", "other_version") else [],
        "note": safe.datum(note, 160),
        "link": {"url": item["url"]} if item["url"] else None,
        "in_list": False,
        "kind": item.get("kind") or "",
        "_item": item,
    }


def _model_rows(rows: list[dict]) -> str:
    """La tabla para el modelo, en una linea por fila y con tope.

    «n=1 id142 Digno De Adorar | n=2 falta (enlace) | n=3 id230 dudosa [alt: 226 X]»
    """
    parts = []
    for r in rows:
        s = r["song"]
        if r["status"] == "missing":
            piece = f"n={r['n']} falta" + (" (enlace)" if r["link"] else "")
        else:
            label = {
                "found": "",
                "other_version": " otra version (el enlace no)",
                "ambiguous": " varias versiones",
                "probable": " dudosa, NO añadida",
            }[r["status"]]
            piece = f"n={r['n']} id{s['id']} {s['title'][:40]}{label}"
            if r["alts"]:
                piece += (
                    " [alt: "
                    + ", ".join(f"{a['id']} {a['artist'][:20]}" for a in r["alts"][:2])
                    + "]"
                )
            if r["in_list"]:
                piece += " (en la lista)"
        parts.append(piece)
    out = " | ".join(parts)
    if len(out) > MODEL_ROWS_CHARS:
        out = out[: MODEL_ROWS_CHARS - 12].rsplit(" | ", 1)[0] + " | [recortado]"
    return out


def _public(rows: list[dict]) -> list[dict]:
    return [{k: v for k, v in r.items() if not k.startswith("_")} for r in rows]


# ------------------------------------------------------------- find_songs


def find(items: Any, turn: Turn) -> dict:
    """`find_songs`: ¿tenemos estas? Solo mira, no cambia nada."""
    wanted, origin = requested(items, turn)
    if not wanted:
        return {"error": "no hay canciones que buscar: pasa los titulos en items"}
    resolved = _resolve(
        wanted, prefer=_in_lists(), overrides=None, strict=turn.intent.strict_version
    )
    rows = [_row(it, res) for it, res in zip(wanted, resolved, strict=True)]
    found = sum(1 for r in rows if r["status"] in ADDABLE)
    return {
        "total": len(rows),
        "found": found,
        "missing": [r["n"] for r in rows if r["status"] == "missing"],
        "doubtful": [r["n"] for r in rows if r["status"] in ("ambiguous", "probable")],
        "rows": _model_rows(rows),
        "source": origin["source"],
        "_ui": {"found": _public(rows)},
    }


# ----------------------------------------------------------- fill_playlist


def _mode(asked: str | None, intent: intake.Intent) -> tuple[str, str]:
    """(modo, por que se cambio lo pedido o «»).

    Lo deciden las palabras de la persona, no el modelo: «verifica» siempre
    es comprobar, y dejar una lista exacta (quitar lo que sobra) solo si lo
    dijo ella."""
    asked = (asked or "").strip().lower()
    if intent.check:
        return "check", ("" if asked in ("", "check") else "pediste verificar: no cambie nada")
    if asked == "check":
        return "check", ""
    if asked == "set":
        if intent.replace:
            return "set", ""
        return "add", "no dijiste que la dejara exacta: solo añadi, no quite nada"
    if not asked and intent.replace:
        return "set", ""
    return "add", ""


def _target(name: str, turn: Turn, create: bool) -> tuple[dict | None, bool, str]:
    """(lista, si se creo ahora, error)."""
    name = (name or "").strip()[:120]
    if not name:
        if turn.name_hint:
            pl, _ = find_playlist(turn.name_hint)
            if pl:
                return pl, False, ""
        lists = playlists.list_all()
        return (
            None,
            False,
            (
                "no me dijiste en que lista. Las que hay: "
                + _playlist_names(lists)
                + ". Preguntale a cual, o si quiere una nueva."
            ),
        )
    pl, similar = find_playlist(name)
    if pl:
        return pl, False, ""
    if similar:
        return (
            None,
            False,
            (
                f"«{safe.datum(name, 60)}» puede ser varias listas: "
                + _playlist_names(similar)
                + ". Preguntale cual."
            ),
        )
    if create and turn.intent.create:
        made = playlists.create(name)
        pl = playlists.by_id(made["id"])
        return pl, bool(made.get("created")), ""
    lists = playlists.list_all()
    why = (
        " (no la creo: no te pidieron una lista nueva)" if create and not turn.intent.create else ""
    )
    return (
        None,
        False,
        (
            f"no existe la lista «{safe.datum(name, 60)}»{why}. Las que hay: "
            + _playlist_names(lists)
            + ". Si quiere una nueva, que lo diga."
        ),
    )


def _order_differs(rows: list[dict], final: list[int]) -> bool:
    """Si las de la lista pegada que estan en la lista van en otro orden."""
    pos = {sid: i for i, sid in enumerate(final)}
    seq = []
    seen: set[int] = set()
    for r in rows:
        s = r["song"]
        if r["status"] in ADDABLE and s and s["id"] in pos and s["id"] not in seen:
            seen.add(s["id"])
            seq.append(pos[s["id"]])
    return any(b < a for a, b in pairwise(seq))


def _youtube_query(item: dict) -> str:
    """Lo que se busca en YouTube por una cancion sin enlace: en la forma de
    buscar de la casa (sin tildes ni simbolos, con la ñ). Nunca una URL.

    Sin artista se le añade «cancion»: un titulo suelto como «Cena Del Señor»
    daba de primero predicaciones de veinte minutos, y con la palabra salen
    canciones. En los titulos que ya daban la cancion, el primer resultado es
    el mismo (medido con YouTube de verdad)."""
    text = " ".join(x for x in (item.get("artist"), item.get("title")) if x)
    form = names.search_form(text)[:190]
    if form and not item.get("artist"):
        form += " cancion"
    return form


def fill(args: dict, turn: Turn) -> dict:
    """`fill_playlist`: mete (o comprueba) unas canciones en una lista."""
    wanted, origin = requested(args.get("items"), turn)
    only = _only(args.get("only"))
    if only:
        wanted = [it for it in wanted if it["n"] in only]
    if not wanted:
        return {
            "error": "no hay canciones: pega la lista o pasa los titulos en items"
            + (" (ninguna con esos numeros en only)" if only else "")
        }
    mode, mode_note = _mode(args.get("mode"), turn.intent)
    pl, created, bad = _target(
        str(args.get("playlist") or args.get("name") or ""), turn, bool(args.get("create"))
    )
    if pl is None:
        return {"error": bad}

    with playlists.LOCK:
        before = [s["id"] for s in playlists.songs(pl["id"], light=True)]
        resolved = _resolve(
            wanted,
            prefer=before + _in_lists(exclude=pl["id"]),
            overrides=args.get("overrides"),
            strict=turn.intent.strict_version,
        )
        rows = [_row(it, res) for it, res in zip(wanted, resolved, strict=True)]
        ids: list[int] = []
        for r in rows:
            s = r["song"]
            if r["status"] in ADDABLE and s and s["id"] not in ids:
                ids.append(s["id"])
        previous = None
        applied = False
        removed: list[int] = []
        if mode == "add":
            new = [i for i in ids if i not in before]
            if new:
                playlists.place(pl["id"], before + new)
                applied = True
        elif mode == "set":
            if not ids:
                return {
                    "error": "no encontre ninguna de esas canciones en la biblioteca: no "
                    "toque la lista (dejarla vacia no es lo que pidio)"
                }
            if ids != before:
                removed = [i for i in before if i not in ids]
                previous = list(before)
                playlists.set_songs(pl["id"], ids)
                applied = True
        after = playlists.songs(pl["id"], light=True)
    final = [s["id"] for s in after]
    in_final = set(final)
    by_id = {s["id"]: s for s in after}
    for r in rows:
        r["in_list"] = bool(r["song"] and r["song"]["id"] in in_final)
    named = {r["song"]["id"] for r in rows if r["song"]}
    extras = [_light(by_id[i]) for i in final if i not in named]
    removed_songs = [r for r in (_light(s) for s in _songs_by_ids(removed)) if r]
    added = [i for i in ids if i not in before and i in in_final]
    already = [i for i in ids if i in before]

    proposal, ask_note = _propose(rows, pl, mode, args, turn, final)

    ui = {
        "playlist": {"id": pl["id"], "name": pl["name"], "created": created},
        "mode": mode,
        "applied": applied,
        "rows": _public(rows),
        "added": added,
        "already_in": already,
        "removed": removed_songs,
        "extras_in_list": extras if origin["source"] == "pegada" else [],
        "order_differs": _order_differs(rows, final),
        "previous": previous,
        "ignored": origin["ignored"],
        "truncated": origin["truncated"],
        "mode_note": mode_note,
        "ask_note": ask_note,
    }
    res: dict = {
        "playlist": pl["name"],
        "mode": mode,
        "applied": applied,
        "created": created,
        "added": len(added),
        "already_in": len(already),
        "removed": len(removed),
        "missing": [r["n"] for r in rows if r["status"] == "missing"],
        "doubtful": [r["n"] for r in rows if r["status"] in ("ambiguous", "probable")],
        "rows": _model_rows(rows),
        "extras_in_list": len(ui["extras_in_list"]),
        "order_differs": ui["order_differs"],
        "download": (
            "tarjeta de permiso puesta para las que faltan: no la pidas tu"
            if proposal
            else (ASK[ask_note][0] if ask_note else "no falta ninguna")
        ),
        "_ui": {"setlist": ui, "confirm": proposal},
    }
    if mode_note:
        res["note"] = mode_note
    return res


def _songs_by_ids(ids: list[int]) -> list[dict]:
    if not ids:
        return []
    from .. import library

    return [dict(s) for s in (library.by_id(i) for i in ids) if s]


# Por que no hay tarjeta de permiso para lo que falta: para el modelo (sin
# tildes) y para la persona (con ellas).
ASK = {
    "not_requested": ("no se pidio descargar", "no me pediste bajarlas"),
    "no_download": ("dijo que no descargara", "dijiste que no las bajara"),
    "unavailable": ("ahora no se puede descargar", "ahora no se puede descargar"),
    "busy": (
        "ya hay otra cosa esperando su visto bueno en este turno",
        "ya hay otra cosa esperando tu visto bueno",
    ),
    "error": ("no se pudo preparar el permiso", "no pude preparar el permiso"),
}


def _propose(
    rows: list[dict], pl: dict, mode: str, args: dict, turn: Turn, final: list[int]
) -> tuple[dict | None, str]:
    """La tarjeta de permiso para bajar lo que falta, o (None, por que no: una
    clave de `ASK`, o «» si no falta nada).

    Faltan = las que no estan en la biblioteca. Con ellas van, SIN MARCAR, las
    dudosas (por si no es la misma) y la version del enlace cuando se uso la
    de la biblioteca (la persona prefiere la que ya tiene; si quiere la del
    enlace, la marca). Si no falta ninguna, no hay tarjeta."""
    if mode == "check" or not any(r["status"] == "missing" for r in rows):
        return None, ""
    offer = [r for r in rows if r["status"] in ("missing", "probable", "other_version")]
    if args.get("download_missing") is False:
        return None, "not_requested"
    if turn.intent.no_download:
        return None, "no_download"
    if not youtube.available():
        return None, "unavailable"
    if not turn.can_ask:
        return None, "busy"
    from . import plan

    items: list[str] = []
    plan_rows = []
    for r in offer:
        it = r["_item"]
        if it.get("kind") == "playlist":
            continue
        if r["status"] == "other_version" and not it["url"]:
            continue
        item = it["url"] or _youtube_query(it)
        if not item or item in items:
            continue
        if len(items) >= plan.MAX_DOWNLOADS:
            break
        items.append(item)
        note = ""
        if r["status"] == "probable" and r["song"]:
            note = f"¿ya la tienes? Se parece a «{_song_name(r['song'])}»"
        elif r["status"] == "other_version" and r["song"]:
            note = f"otra versión: ya tienes «{_song_name(r['song'])}»"
        # la forma de `plan.register`: casillas sin marcar para lo dudoso
        plan_rows.append(
            {
                "n": r["n"],
                "item": item,
                "label": r["title"],
                "note": note,
                "checked": r["status"] == "missing",
            }
        )
    if not items:
        return None, ""
    already = [
        {"n": r["n"], "title": r["title"], "song": r["song"]}
        for r in rows
        if r["status"] in ADDABLE and r["song"]
    ]
    slots = []
    for r in rows:
        s = r["song"]
        if s and s["id"] in final and r["status"] in ADDABLE:
            slots.append({"n": r["n"], "id": s["id"]})
        else:
            match = next((p for p in plan_rows if p["n"] == r["n"]), None)
            if match:
                slots.append({"n": r["n"], "item": match["item"]})
    after = {"playlist": {"id": pl["id"], "name": pl["name"]}, "slots": slots[: plan.MAX_SLOTS]}
    try:
        confirm = plan.register(
            "download_music",
            {"items": items, "file_it": True, "force": False},
            plan={"rows": plan_rows, "already": already},
            after=after,
            chat_id=turn.chat_id,
        )
    except Exception:
        log.warning("no se pudo preparar la tarjeta de descarga", exc_info=True)
        return None, "error"
    turn.can_ask = False
    # compatibilidad: una interfaz de antes confirma con {tool, args}
    return {**confirm, "args": {"items": items, "file_it": True, "force": False}}, ""


# -------------------------------------------------------------- el informe


def report(ui: dict, confirm: dict | None) -> str:
    """El texto de la respuesta cuando no hace falta otra vuelta del modelo.

    En pasado y con nombres: lo que se hizo, lo que estaba, las dudas, lo que
    falta y si abajo hay una tarjeta (solo si de verdad la hay)."""
    rows = ui["rows"]
    pl = ui["playlist"]["name"]
    lines: list[str] = []
    mode = ui["mode"]
    found = [r for r in rows if r["status"] in ADDABLE]
    missing = [r for r in rows if r["status"] == "missing"]
    probable = [r for r in rows if r["status"] == "probable"]
    by_id = {}
    for r in rows:
        if r["song"]:
            by_id.setdefault(r["song"]["id"], r)

    if ui["truncated"]:
        lines.append(f"Leí las primeras {len(rows)} canciones de la lista.")
    if ui["playlist"].get("created"):
        lines.append(f"Creé la lista «{pl}».")
    if mode == "check":
        there = [r["title"] for r in found if r["in_list"]]
        not_there = [r["title"] for r in found if not r["in_list"]]
        bits = []
        if there:
            bits.append(f"están {_names(there)}")
        if not_there:
            bits.append(f"no están (pero las tienes) {_names(not_there)}")
        if missing:
            bits.append(f"no tienes {_names([r['title'] for r in missing])}")
        extras = [x["title"] for x in ui["extras_in_list"]]
        if extras:
            bits.append(f"sobra{'n' if len(extras) > 1 else ''} {_names(extras)}")
        lines.append(f"Comprobé «{pl}»: " + ("; ".join(bits) or "nada que comparar") + ".")
        if ui["order_differs"]:
            lines.append("El orden de la lista no es el de la que pegaste.")
        lines.append("No cambié nada.")
        return " ".join(lines)

    added = [by_id[i]["title"] for i in ui["added"] if i in by_id]
    already = [by_id[i]["title"] for i in ui["already_in"] if i in by_id]
    if mode == "set" and ui["applied"]:
        lines.append(f"Dejé «{pl}» solo con estas, en el orden de tu lista.")
        if ui["removed"]:
            lines.append(f"Quité {_names([x['title'] for x in ui['removed']])}.")
    elif added:
        lines.append(f"Añadí a «{pl}»: {_names(added)}.")
    if already and not (mode == "set" and ui["applied"]):
        where = "" if added else f" en «{pl}»"
        lines.append(f"Ya estaban{where}: {_names(already)}.")
    if not added and not already and not (mode == "set" and ui["applied"]) and found:
        lines.append(f"No añadí nada a «{pl}».")

    amb = [r for r in rows if r["status"] == "ambiguous"][:SHOWN_NAMES]
    for r in amb:
        extra = len(r["alts"])
        more = f"; tienes {extra} más" if extra else ""
        lines.append(f"{r['title']}: usé «{_song_name(r['song'])}»{more}.")
    other = [r for r in rows if r["status"] == "other_version"][:SHOWN_NAMES]
    for r in other:
        where = "; la del enlace va sin marcar abajo" if confirm else ""
        lines.append(f"{r['title']}: usé «{_song_name(r['song'])}», que ya tenías{where}.")
    lines.extend(
        f"{r['title']}: ¿es «{_song_name(r['song'])}»? No la añadí." for r in probable[:SHOWN_NAMES]
    )

    if missing:
        what = _names([r["title"] for r in missing])
        n = len(missing)
        them = "las" if n > 1 else "la"
        head = f"Falta{'n' if n > 1 else ''} {n}: {what}."
        code = ui.get("ask_note") or ""
        if confirm:
            lines.append(f"{head} Abajo te pido permiso para bajar{them}.")
        elif code == "no_download":
            lines.append(f"{head} No {them} bajo, como dijiste.")
        elif code == "not_requested":
            lines.append(f"{head} Si quieres, te {them} busco en YouTube.")
        elif code in ASK:
            lines.append(
                f"{head} Ahora no puedo pedirte permiso para bajar{them} ({ASK[code][1]}); "
                "dímelo y lo repito."
            )
        else:
            lines.append(head)
    if ui["extras_in_list"] and mode == "add":
        extras = [x["title"] for x in ui["extras_in_list"]]
        n = len(extras)
        lines.append(
            f"En «{pl}» hay {n} de otras veces: {_names(extras)}; no l{'as' if n > 1 else 'a'} quité."
        )
    if ui["order_differs"] and mode == "add":
        lines.append("El orden de la lista no es el de la que pegaste.")
    if ui["ignored"]:
        lines.append(f"No tomé por canción: {_names(ui['ignored'])}.")
    if ui.get("mode_note") and mode == "add" and ui["mode_note"].startswith("no dijiste"):
        lines.append("No quité nada: si quieres dejarla solo con estas, dímelo.")
    return " ".join(lines) if lines else f"No cambié nada en «{pl}»."


def report_found(found_rows: list[dict]) -> str | None:
    """El texto de `find_songs` sin otra vuelta del modelo: solo si estan
    TODAS, sin dudas (si no, que lo cuente el modelo)."""
    if not found_rows or any(r["status"] != "found" for r in found_rows):
        return None
    names_ = [_song_name(r["song"]) or r["title"] for r in found_rows]
    if len(names_) == 1:
        return f"Sí, la tienes: {names_[0]}."
    return f"Sí, las tienes todas: {_names(names_, limit=6)}."
