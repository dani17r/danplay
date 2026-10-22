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

Lo que se decide y se escribe lo hace Rust (library/src/assistant/setlist.rs);
aqui queda lo que toca la base y la red: buscar en la biblioteca, cambiar la
lista bajo su cerrojo, crear una lista y apuntar la tarjeta.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any

import danplay_core as _rust

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
    return _rust.setlist_light(song)


def _song_name(song: dict | None) -> str:
    return _rust.setlist_song_name(song)


def _names(values: list[str], limit: int = SHOWN_NAMES) -> str:
    """«A», «A y B», «A, B y C», «A, B, C y 2 mas» (para la persona: con tildes)."""
    return _rust.setlist_names(values, limit)


def _core(name: str) -> tuple[str, ...]:
    """Las palabras que nombran una lista, sin relleno y sin plural.

    «la lista de los domingos» y «Domingo» dan lo mismo: («domingo»,)."""
    return tuple(_rust.setlist_core(names._txt(name or "")))


def find_playlist(name: str) -> tuple[dict | None, list[dict]]:
    """La lista a la que se refiere `name`, o (None, las que se parecen).

    Primero la que tiene el mismo nombre en la forma de buscar; luego sin
    relleno ni plural («los domingo» = «domingo»); luego con la ñ plegada;
    y por ultimo la UNICA que contiene todas las palabras pedidas («ensayo» →
    «ensayo domingo»). Si hay varias, ninguna: se devuelven para preguntar.
    """
    name = names._txt(name or "")
    if not _rust.setlist_core(name):
        return None, []
    lists = playlists.list_all()
    at, similar = _rust.setlist_find_playlist(name, lists)
    return (lists[at] if at is not None else None), [lists[i] for i in similar]


def _playlist_names(lists: list[dict], limit: int = 12) -> str:
    return _rust.setlist_playlist_names(lists, limit)


# ------------------------------------------------------------ lo que se pide


def _item_from_text(raw: str) -> dict:
    """Un item que manda el modelo como texto: un titulo o un enlace."""
    return _rust.setlist_item_from_text(safe._txt(str(raw or "")))


def _item_from_dict(raw: dict) -> dict:
    return _rust.setlist_item_from_dict(raw)


def requested(items: Any, turn: Turn) -> tuple[list[dict], dict]:
    """Lo que se pide, numerado, y de donde salio.

    La lista pegada manda cuando esta en el ultimo mensaje de la persona; si
    es de antes, solo se usa si el modelo no manda canciones. Devuelve
    (items, origen) con origen = {"source": "pegada"|"modelo",
    "truncated": N, "ignored": [...]}.
    """
    return _rust.setlist_requested(items, turn.setlist, turn.fresh)


def _is_request(line: str) -> bool:
    """Si una linea que no es cancion es lo que se pide y no una seccion
    («Banda: …», «Ensayo: …»), que si merece decirse que se dejo fuera."""
    return _rust.setlist_is_request(names._txt(line))


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
    return _rust.setlist_overrides(raw)


def _only(raw: Any) -> set[int]:
    return _rust.setlist_only(raw)


# ------------------------------------------------------------------- filas


def _row(item: dict, res) -> dict:
    """Una fila de la tabla: lo pedido junto a lo encontrado."""
    return _rust.setlist_row(item, res)


def _model_rows(rows: list[dict]) -> str:
    """La tabla para el modelo, en una linea por fila y con tope.

    «n=1 id142 Digno De Adorar | n=2 falta (enlace) | n=3 id230 dudosa [alt: 226 X]»
    """
    return _rust.setlist_model_rows(rows)


def _public(rows: list[dict]) -> list[dict]:
    return _rust.setlist_public(rows)


# ------------------------------------------------------------- find_songs


def find(items: Any, turn: Turn) -> dict:
    """`find_songs`: ¿tenemos estas? Solo mira, no cambia nada."""
    wanted, origin = requested(items, turn)
    if not wanted:
        return {"error": "no hay canciones que buscar: pasa los titulos en items"}
    resolved = _resolve(
        wanted, prefer=_in_lists(), overrides=None, strict=turn.intent.strict_version
    )
    return _rust.setlist_find_result(wanted, resolved, origin["source"])


# ----------------------------------------------------------- fill_playlist


def _mode(asked: str | None, intent: intake.Intent) -> tuple[str, str]:
    """(modo, por que se cambio lo pedido o «»): lo deciden las palabras de la
    persona, no el modelo."""
    return _rust.setlist_mode(asked, intent)


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
    return _rust.setlist_order_differs(rows, final)


def _youtube_query(item: dict) -> str:
    """Lo que se busca en YouTube por una cancion sin enlace: en la forma de
    buscar de la casa, con «cancion» si no hay artista. Nunca una URL."""
    return _rust.setlist_youtube_query(item)


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
        rows = _rust.setlist_rows(wanted, resolved)
        ids = _rust.setlist_fill_ids(rows)
        previous = None
        applied = False
        removed: list[int] = []
        change = _rust.setlist_fill_change(mode, ids, before)
        if change is not None:
            if change[0] == "empty":
                return {"error": change[1]}
            if change[0] == "place":
                playlists.place(pl["id"], change[1])
            else:
                removed = change[2]
                previous = list(before)
                playlists.set_songs(pl["id"], change[1])
            applied = True
        after = playlists.songs(pl["id"], light=True)
    rows, final, extras, added, already = _rust.setlist_fill_settle(rows, ids, before, after)
    removed_songs = [r for r in (_light(s) for s in _songs_by_ids(removed)) if r]

    proposal, ask_note = _propose(rows, pl, mode, args, turn, final)
    res = _rust.setlist_fill_result(
        {
            "playlist": pl,
            "created": created,
            "mode": mode,
            "mode_note": mode_note,
            "applied": applied,
            "rows": rows,
            "final": final,
            "extras": extras,
            "added": added,
            "already": already,
            "removed_songs": removed_songs,
            "removed": len(removed),
            "previous": previous,
            "origin": origin,
            "has_proposal": bool(proposal),
            "ask_note": ask_note,
        }
    )
    res["_ui"]["confirm"] = proposal
    return res


def _songs_by_ids(ids: list[int]) -> list[dict]:
    if not ids:
        return []
    from .. import library

    return [dict(s) for s in (library.by_id(i) for i in ids) if s]


# Por que no hay tarjeta de permiso para lo que falta: para el modelo (sin
# tildes) y para la persona (con ellas).
ASK = dict(_rust.setlist_ask())


def _propose(
    rows: list[dict], pl: dict, mode: str, args: dict, turn: Turn, final: list[int]
) -> tuple[dict | None, str]:
    """La tarjeta de permiso para bajar lo que falta, o (None, por que no: una
    clave de `ASK`, o «» si no falta nada).

    Que va en ella lo decide Rust; aqui se apunta (`plan.register`)."""
    from . import plan

    got = _rust.setlist_propose(
        rows,
        final,
        mode,
        args.get("download_missing") is False,
        turn.intent.no_download,
        turn.can_ask,
        youtube.available,
        plan.MAX_DOWNLOADS,
        plan.MAX_SLOTS,
    )
    if isinstance(got, str):
        return None, got
    items, plan_rows, already, slots = got
    after = {"playlist": {"id": pl["id"], "name": pl["name"]}, "slots": slots}
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
    return _rust.setlist_report(ui, bool(confirm))


def report_found(found_rows: list[dict]) -> str | None:
    """El texto de `find_songs` sin otra vuelta del modelo: solo si estan
    TODAS, sin dudas (si no, que lo cuente el modelo)."""
    return _rust.setlist_report_found(found_rows)
