# -*- coding: utf-8 -*-
"""Las conversaciones con el asistente, guardadas en la base.

Antes el historial vivia en el `localStorage` del WebView: una sola
conversacion, sesenta mensajes como mucho, sin busqueda y atada a esa
instalacion. Aqui hay varias conversaciones, cada mensaje con lo que la
interfaz necesita para pintarlo igual (herramientas usadas, si lo escribio
la app, si fue narracion…), y se puede buscar en todas.

Como las descargas, es cosa del programa y no del mp3: el escaneo no la
toca (solo vacia `songs`).
"""
import json
import time
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
# lo que se guarda de cada mensaje ademas del texto (lo que pinta la interfaz)
_KEEP = ("tools", "app", "event", "hidden", "narrated", "error", "usage", "via", "canceled")


def title_from(text: str) -> str:
    """El titulo de una conversacion sale de su primer mensaje."""
    t = " ".join(str(text or "").split())
    return (t[:TITLE_LENGTH - 1] + "…") if len(t) > TITLE_LENGTH else t or "Conversacion"


def _connect():
    return library.connect()


def list_all(limit=200) -> list[dict]:
    conn = _connect()
    rows = conn.execute(
        "SELECT c.id, c.title, c.created, c.updated, "
        "(SELECT COUNT(*) FROM chat_messages m WHERE m.chat_id=c.id) n "
        "FROM chats c ORDER BY c.updated DESC LIMIT ?", (int(limit),)).fetchall()
    conn.close()
    return [dict(r) for r in rows]


def create(title: str = "") -> dict:
    now = time.time()
    conn = _connect()
    cur = conn.execute("INSERT INTO chats (title, created, updated) VALUES (?,?,?)",
                       (title.strip(), now, now))
    conn.commit()
    cid = cur.lastrowid
    conn.close()
    return {"id": cid, "title": title.strip(), "created": now, "updated": now, "n": 0}


def get(chat_id: int) -> dict | None:
    conn = _connect()
    row = conn.execute("SELECT * FROM chats WHERE id=?", (int(chat_id),)).fetchone()
    if not row:
        conn.close()
        return None
    rows = conn.execute("SELECT id, at, role, text, payload FROM chat_messages "
                        "WHERE chat_id=? ORDER BY id", (int(chat_id),)).fetchall()
    conn.close()
    messages = []
    for r in rows:
        try:
            extra = json.loads(r["payload"] or "{}")
        except Exception:                                    # noqa: BLE001
            extra = {}
        m = {"id": r["id"], "at": r["at"], "role": r["role"], "text": r["text"]}
        m.update({k: v for k, v in extra.items() if k in _KEEP})
        messages.append(m)
    out = dict(row)
    out["messages"] = messages
    return out


def append(chat_id: int, messages: list[dict]) -> int:
    """Añade mensajes al final. El titulo sale del primero del usuario si la
    conversacion aun no tiene."""
    conn = _connect()
    row = conn.execute("SELECT title FROM chats WHERE id=?", (int(chat_id),)).fetchone()
    if not row:
        conn.close()
        raise ValueError("no existe esa conversacion")
    now = time.time()
    title = row["title"]
    n = 0
    for m in messages:
        if not isinstance(m, dict):
            continue
        role = str(m.get("role") or "")
        text = str(m.get("text") or "")
        extra = {k: m[k] for k in _KEEP if k in m and m[k] not in (None, False, "", [])}
        conn.execute("INSERT INTO chat_messages (chat_id, at, role, text, payload) VALUES (?,?,?,?,?)",
                     (int(chat_id), float(m.get("at") or now), role, text,
                      json.dumps(extra, ensure_ascii=False)))
        n += 1
        if not title and role == "me" and not m.get("hidden") and text.strip():
            title = title_from(text)
    conn.execute("UPDATE chats SET updated=?, title=? WHERE id=?", (now, title, int(chat_id)))
    conn.commit(); conn.close()
    return n


def rename(chat_id: int, title: str) -> bool:
    conn = _connect()
    cur = conn.execute("UPDATE chats SET title=? WHERE id=?", (title.strip(), int(chat_id)))
    conn.commit(); conn.close()
    return bool(cur.rowcount)


def delete(chat_id: int) -> bool:
    conn = _connect()
    conn.execute("DELETE FROM chat_messages WHERE chat_id=?", (int(chat_id),))
    cur = conn.execute("DELETE FROM chats WHERE id=?", (int(chat_id),))
    conn.commit(); conn.close()
    return bool(cur.rowcount)


def search(query: str, limit=40) -> list[dict]:
    """Mensajes que contienen ese texto, con su conversacion. Sin distinguir
    mayusculas; es un LIKE, que para unos miles de mensajes sobra."""
    q = " ".join(str(query or "").split())
    if not q:
        return []
    conn = _connect()
    rows = conn.execute(
        "SELECT m.id, m.chat_id, c.title, m.role, m.text, m.at FROM chat_messages m "
        "JOIN chats c ON c.id=m.chat_id WHERE m.text LIKE ? ESCAPE '\\' AND m.role IN ('me','ai') "
        "ORDER BY m.id DESC LIMIT ?",
        ("%" + q.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%", int(limit))).fetchall()
    conn.close()
    out = []
    for r in rows:
        text = r["text"]
        i = text.lower().find(q.lower())
        start = max(0, i - 60)
        snippet = ("…" if start else "") + text[start:i + len(q) + 80] + ("…" if i + len(q) + 80 < len(text) else "")
        out.append({"id": r["id"], "chat_id": r["chat_id"], "title": r["title"],
                    "role": r["role"], "snippet": snippet, "at": r["at"]})
    return out
