"""Lo que el modelo ve de cada turno ademas del mensaje: la ventana de
historial, las notas de lo que hizo de verdad en los anteriores y el estado
real de la app (lo que hay, lo que la persona tiene delante)."""

from .. import library, playlists, toon, youtube
from .narration import claims_action, has_markers

# Cuanto historial se le da al modelo. Por mensajes Y por tamaño: los de la
# app (Descargando…, el informe, el aviso oculto) son cortos pero cuentan, y
# con 24 la peticion original quedaba fuera en cuanto habia una descarga por
# medio. Se corta siempre en un mensaje del usuario, para no empezar por una
# respuesta suelta.
HISTORY_MESSAGES = 48
HISTORY_CHARS = 16_000


def _window(messages: list[dict]) -> list[dict]:
    """Los ultimos mensajes que caben, empezando por uno del usuario."""
    kept: list[dict] = []
    size = 0
    for m in reversed(messages):
        size += len(str(m.get("text") or "")) + 40
        if kept and (len(kept) >= HISTORY_MESSAGES or size > HISTORY_CHARS):
            break
        kept.append(m)
    kept.reverse()
    while kept and kept[0].get("role") == "ai" and len(kept) > 1:
        kept.pop(0)
    return kept


def _brief(name: str, res: dict, limit=10) -> str:
    """Lo que devolvio una herramienta, en una linea con ids y nombres.

    Es la memoria entre turnos: sin esto, al turno siguiente el modelo solo
    sabia «3 resultados» y no PODIA entender «la segunda» o «esa», ni tenia
    los ids de lo que el mismo acababa de enseñar.
    """
    if not isinstance(res, dict) or res.get("error") or res.get("needs_confirmation"):
        return ""

    def song(c):
        who = f"{c.get('artist')} - " if c.get("artist") else ""
        return f"id {c.get('id')} «{who}{c.get('title')}»"

    if name in (
        "search_songs",
        "playlist_songs",
        "create_playlist",
        "add_to_playlist",
        "set_playlist_songs",
        "remove_from_playlist",
    ):
        songs = res.get("songs") or []
        head = (
            f"«{res.get('name')}» (id {res.get('playlist_id')}): " if res.get("playlist_id") else ""
        )
        more = f" y {len(songs) - limit} mas" if len(songs) > limit else ""
        if songs:
            return head + ", ".join(song(c) for c in songs[:limit]) + more
        return head + "vacia" if head else ""
    if name == "list_playlists":
        return ", ".join(
            f"«{l['name']}» (id {l['id']}, {l['items']} temas)"
            for l in (res.get("playlists") or [])[:limit]
        )
    if name == "search_youtube":
        return "; ".join(f"«{t.get('title')}» {t.get('url')}" for t in (res.get("items") or [])[:5])
    if name in ("play_song",):
        return res.get("playing") or ""
    if name == "edit_song" and res.get("song"):
        return song(res["song"])
    if name in ("set_stars", "set_favorite", "delete_song"):
        return str(res.get("song") or res.get("name") or "")
    return ""


def _tool_note(text: str, tools, app=False) -> str:
    """La nota de sistema que acompaña a un mensaje anterior del asistente.

    El modelo lee sus propias frases de turnos pasados como hechos: si dijo
    «ya la cree» sin llamar a nada, al turno siguiente da la lista por hecha.
    Con herramientas se anota cuales y QUE devolvieron (ids y nombres: la
    memoria entre turnos); sin ellas, y si el texto afirma haber hecho algo,
    se le señala que no ocurrio. Vacia si no hay nada que decir.
    """
    if tools:
        parts = []
        for t in tools:
            if not t.get("name"):
                continue
            piece = f"{t['name']} ({t['summary']})" if t.get("summary") else str(t["name"])
            if t.get("detail"):
                piece += f": {str(t['detail'])[:600]}"
            parts.append(piece)
        return "Nota de la app: en el mensaje anterior usaste " + "; ".join(parts) + "."
    if app:
        return "Nota de la app: el mensaje anterior lo escribio la app, no tu."
    if has_markers(text) or claims_action(text):
        return (
            "Nota de la app: el mensaje anterior NO uso ninguna herramienta; lo "
            "que dice haber hecho o estar haciendo NO ocurrio."
        )
    return ""


# Cuantas canciones de la vista se le enseñan al modelo: «pon la segunda»
# necesita ver la lista, pero una biblioteca entera no cabe ni hace falta.
CONTEXT_ROWS = 20


def _screen_note(context: dict | None) -> list[str]:
    """Lo que la persona tiene delante, en dos o tres lineas.

    «Esta», «la segunda», «las seleccionadas», «la que suena»: sin esto el
    modelo no tenia forma de saber a que se referian y preguntaba o
    inventaba. Los ids que van aqui son tan validos como los de una busqueda.
    """
    if not isinstance(context, dict):
        return []
    lines = []
    view = context.get("view") or {}
    songs = [c for c in (context.get("songs") or []) if isinstance(c, dict) and c.get("id")]
    if view.get("name") or songs:
        total = context.get("total") or len(songs)
        head = f"- Esta viendo «{view.get('name') or 'la biblioteca'}»"
        if view.get("kind") == "playlist":
            head += " (repertorio)"
        head += f": {total} canciones."
        if songs:
            rows = [
                {
                    "id": int(c["id"]),
                    "artist": str(c.get("artist") or "")[:60],
                    "title": str(c.get("title") or "")[:80],
                }
                for c in songs[:CONTEXT_ROWS]
            ]
            head += (
                f" Las primeras {len(rows)} en pantalla, en su orden (sirven para «esta», "
                f"«la segunda»…; para contar, buscar u ordenar usa search_songs):\n"
                + toon.encode({"songs": rows})
            )
        lines.append(head)
    selected = [c for c in (context.get("selected") or []) if isinstance(c, dict) and c.get("id")]
    if selected:
        shown = "; ".join(
            f"id {int(c['id'])} «{c.get('artist', '')} - {c.get('title', '')}»"
            for c in selected[:CONTEXT_ROWS]
        )
        more = f" y {len(selected) - CONTEXT_ROWS} mas" if len(selected) > CONTEXT_ROWS else ""
        lines.append(f"- Tiene seleccionadas {len(selected)} canciones: {shown}{more}.")
    playing = context.get("playing")
    if isinstance(playing, dict) and playing.get("id"):
        state = "en pausa" if playing.get("paused") else "sonando"
        lines.append(
            f"- Ahora mismo {state}: id {int(playing['id'])} "
            f"«{playing.get('artist', '')} - {playing.get('title', '')}»."
        )
    return lines


def _context_note(messages: list[dict], context: dict | None = None) -> str:
    """El estado real de la app, para el turno que empieza.

    Lo que hay de verdad manda sobre lo que el modelo dijo antes. Es corto:
    repertorios que existen, si hay una descarga en marcha, y si su ultimo
    mensaje fue solo texto.
    """
    lines = ["ESTADO REAL DE LA APP AHORA (manda sobre lo que hayas dicho antes):"]
    lines.extend(_screen_note(context))
    import datetime as _dt

    lines.append(f"- Hoy es {_dt.date.today().strftime('%d/%m/%Y')}.")
    try:
        # Cuanto hay y de quien: sin esto, el modelo no sabe si «lo de Barak»
        # son tres canciones o cuarenta, y se inventa el tamaño de las cosas.
        stats = library.stats_of()
        top = ", ".join(f"{a['value']} ({a['n']})" for a in library.top_artists(12))
        lines.append(
            f"- Biblioteca: {stats['total']} canciones"
            f"{', ' + str(stats['without_artist']) + ' sin artista' if stats.get('without_artist') else ''}."
            + (f" Artistas con mas temas: {top}." if top else "")
        )
    except Exception:  # noqa: BLE001
        pass
    try:
        lists = playlists.list_all()
        if lists:
            shown = ", ".join(f"«{l['name']}» ({l['n']} temas)" for l in lists[:30])
            if len(lists) > 30:
                shown += f" y {len(lists) - 30} mas"
            lines.append(f"- Repertorios que existen: {shown}.")
        else:
            lines.append("- Repertorios que existen: ninguno.")
    except Exception:  # noqa: BLE001
        pass
    try:
        e = youtube.STATE
        if e["active"]:
            lines.append(
                f"- Descarga en marcha: si ({e['phase']}, {e['index']}/{e['total']}"
                f"{', ' + e['name'] if e['name'] else ''})."
            )
        else:
            lines.append("- Descarga en marcha: no.")
    except Exception:  # noqa: BLE001
        pass
    try:
        recent_downloads = [h for h in library.download_history(6) if h.get("ok")][:3]
        if recent_downloads:
            import time as _t

            parts = []
            for h in recent_downloads:
                minutes = max(0, int((_t.time() - float(h.get("at") or 0)) // 60))
                song = library.by_id(int(h["song_id"])) if h.get("song_id") else None
                where = (
                    f"id {h['song_id']} «{h.get('artist')} - {h.get('song')}»"
                    if song
                    else "ya no esta en la biblioteca (se borro)"
                )
                parts.append(
                    f"hace {minutes} min pediste «{(h.get('title') or h.get('query') or '')[:60]}» → {where}"
                )
            lines.append(
                "- Ultimas descargas hechas: " + "; ".join(parts) + ". El nombre "
                "con el que entra lo decide la identificacion, no YouTube: es la "
                "misma cancion. No la vuelvas a bajar; usa ese id."
            )
    except Exception:  # noqa: BLE001
        pass
    last_ai = next(
        (m for m in reversed(messages) if m.get("role") == "ai" and not m.get("app")), None
    )
    if last_ai is not None and not last_ai.get("tools"):
        lines.append(
            "- Tu ultimo mensaje no uso ninguna herramienta: si prometiste o "
            "dijiste haber hecho algo ahi, NO esta hecho."
        )
    return "\n".join(lines)
