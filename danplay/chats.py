"""Las conversaciones con el asistente, guardadas en la base.

Antes el historial vivia en el `localStorage` del WebView: una sola
conversacion, sesenta mensajes como mucho, sin busqueda y atada a esa
instalacion. Aqui hay varias conversaciones, cada mensaje con lo que la
interfaz necesita para pintarlo igual (herramientas usadas, si lo escribio
la app, si fue narracion…), y se puede buscar en todas.

Como las descargas, es cosa del programa y no del mp3: el escaneo no la
toca (solo vacia `songs`).

Lo hace Rust (library/src/chats.rs); esto es la puerta de
Python, con los mismos nombres. `export_markdown` sigue aqui: escribe las
fechas en la hora local.
"""

import danplay_core as _rust

from . import library

SCHEMA = """
CREATE TABLE IF NOT EXISTS chats (
    id      INTEGER PRIMARY KEY,
    title   TEXT DEFAULT '',
    created REAL,
    updated REAL
);
CREATE TABLE IF NOT EXISTS chat_messages (
    id      INTEGER PRIMARY KEY,
    chat_id INTEGER NOT NULL,
    at      REAL,
    role    TEXT DEFAULT '',
    text    TEXT DEFAULT '',
    payload TEXT DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS i_chat_messages ON chat_messages(chat_id, id);
"""
library.register_schema(SCHEMA)

TITLE_LENGTH = 60
# lo que se guarda de cada mensaje ademas del texto (lo que pinta la interfaz):
# tambien la tarjeta de permiso (`confirm`), la tabla de una lista (`setlist`)
# y, en los mensajes de la app que cambian una tarjeta, a cual se refieren
# (`ref`) y que filas de su tabla quedaron con que cancion (`filled`). Solo se
# añade: un mensaje guardado antes se lee igual. (La lista de verdad esta en
# library/src/chats.rs.)
_KEEP = (
    "tools",
    "app",
    "event",
    "hidden",
    "narrated",
    "error",
    "usage",
    "via",
    "canceled",
    "confirm",
    "setlist",
    "ref",
    "filled",
)


def title_from(text: str) -> str:
    """El titulo de una conversacion sale de su primer mensaje: de su primera
    linea con texto y sin marcas. Con la lista del domingo pegada, el titulo
    era «agrégame esto…: *Domingo*✅ *1.- Mi…», con todo el formato."""
    return _rust.chats_title_from(str(text or ""))


def _connect():
    return library.connect()


def list_all(limit=200) -> list[dict]:
    with _connect() as conn:
        return _rust.chats_list_all(conn, int(limit))


def create(title: str = "") -> dict:
    with _connect() as conn:
        return _rust.chats_create(conn, title)


def get(chat_id: int) -> dict | None:
    with _connect() as conn:
        return _rust.chats_get(conn, int(chat_id))


def append(chat_id: int, messages: list[dict]) -> int:
    """Añade mensajes al final. El titulo sale del primero del usuario si la
    conversacion aun no tiene."""
    with _connect() as conn:
        return _rust.chats_append(conn, int(chat_id), list(messages))


def rename(chat_id: int, title: str) -> bool:
    with _connect() as conn:
        return _rust.chats_rename(conn, int(chat_id), title)


def delete(chat_id: int) -> bool:
    with _connect() as conn:
        return _rust.chats_delete(conn, int(chat_id))


def search(query: str, limit=40) -> list[dict]:
    """Mensajes que contienen ese texto, con su conversacion. Sin distinguir
    mayusculas; es un LIKE, que para unos miles de mensajes sobra."""
    q = str(query or "")
    if not q.split():
        return []
    with _connect() as conn:
        return _rust.chats_search(conn, q, int(limit))


def export_markdown(chat_id: int) -> str | None:
    """La conversacion como texto, para llevarsela a otro sitio."""
    import datetime

    c = get(chat_id)
    if not c:
        return None
    lines = [f"# {c['title'] or 'Conversacion'}", ""]
    for m in c["messages"]:
        if m.get("hidden"):
            continue
        when = datetime.datetime.fromtimestamp(float(m.get("at") or 0)).strftime("%d/%m/%Y %H:%M")
        who = "Tu" if m["role"] == "me" else ("DanPlay" if m.get("app") else "Asistente")
        lines.append(f"**{who}** · {when}")
        lines.append("")
        lines.append(m["text"])
        tools = m.get("tools") or []
        if tools:
            lines.append("")
            lines.append(
                "_" + "; ".join(f"{t.get('name')}: {t.get('summary', '')}" for t in tools) + "_"
            )
        lines.append("")
    return "\n".join(lines).strip() + "\n"
