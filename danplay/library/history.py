"""Lo que no es de ninguna cancion pero se guarda en la base: el historial de
descargas y lo que gasta la IA. El escaneo no lo toca (solo vacia `songs`)."""

import time
from typing import Any

from . import db

# ---------------------------------------------------------- historial
DOWNLOAD_FIELDS = (
    "at",
    "source",
    "query",
    "title",
    "channel",
    "url",
    "ok",
    "already",
    "reason",
    "song_id",
    "artist",
    "song",
    "target",
    "quality",
    "kbps",
)


def log_download(entry: dict) -> None:
    """Apunta una descarga. Da igual si vino del boton o del asistente."""
    row = {k: entry.get(k) for k in DOWNLOAD_FIELDS}
    row["at"] = row["at"] or time.time()
    row["ok"] = 1 if row["ok"] else 0
    row["already"] = 1 if row["already"] else 0
    for k in (
        "source",
        "query",
        "title",
        "channel",
        "url",
        "reason",
        "artist",
        "song",
        "target",
        "quality",
        "kbps",
    ):
        row[k] = str(row[k] or "")
    with db.connect() as conn:
        conn.execute(
            f"INSERT INTO downloads ({','.join(DOWNLOAD_FIELDS)}) "  # noqa: S608
            f"VALUES ({','.join('?' * len(DOWNLOAD_FIELDS))})",
            [row[k] for k in DOWNLOAD_FIELDS],
        )


def download_history(limit=60, offset=0) -> list[dict]:
    with db.connect() as conn:
        rows = conn.execute(
            "SELECT * FROM downloads ORDER BY at DESC LIMIT ? OFFSET ?", (int(limit), int(offset))
        ).fetchall()
    return [dict(r) for r in rows]


def download_count() -> int:
    with db.connect() as conn:
        n = conn.execute("SELECT COUNT(*) n FROM downloads").fetchone()["n"]
    return n or 0


def clear_download_history() -> int:
    with db.connect() as conn:
        n = conn.execute("SELECT COUNT(*) n FROM downloads").fetchone()["n"] or 0
        conn.execute("DELETE FROM downloads")
    return n


def log_ai_usage(
    provider: str, model: str, purpose: str, prompt: int, completion: int, cost: float | None
) -> None:
    with db.connect() as conn:
        conn.execute(
            "INSERT INTO ai_usage (at, provider, model, purpose, prompt, completion, cost) "
            "VALUES (?,?,?,?,?,?,?)",
            (time.time(), provider, model, purpose, int(prompt), int(completion), cost),
        )


def ai_usage_summary() -> dict:
    """Lo gastado hoy, este mes y en total: llamadas, tokens y coste (solo
    de las llamadas con precio conocido; las demas se cuentan aparte)."""
    import datetime as _dt

    now = _dt.datetime.now()
    day = _dt.datetime(now.year, now.month, now.day).timestamp()
    month = _dt.datetime(now.year, now.month, 1).timestamp()
    with db.connect() as conn:

        def part(since):
            f = conn.execute(
                "SELECT COUNT(*) calls, COALESCE(SUM(prompt),0) prompt, COALESCE(SUM(completion),0) completion, "
                "COALESCE(SUM(cost),0) cost, SUM(cost IS NULL) unpriced FROM ai_usage WHERE at>=?",
                (since,),
            ).fetchone()
            return {
                "calls": f["calls"] or 0,
                "prompt": f["prompt"] or 0,
                "completion": f["completion"] or 0,
                "cost": round(f["cost"] or 0, 6),
                "unpriced": f["unpriced"] or 0,
            }

        out: dict[str, Any] = {"today": part(day), "month": part(month), "total": part(0)}
        out["by_provider"] = [
            dict(r)
            for r in conn.execute(
                "SELECT provider, COUNT(*) calls, COALESCE(SUM(prompt),0)+COALESCE(SUM(completion),0) tokens, "
                "COALESCE(SUM(cost),0) cost FROM ai_usage WHERE at>=? GROUP BY provider ORDER BY cost DESC, calls DESC",
                (month,),
            ).fetchall()
        ]
        # el registro no crece para siempre: mas de un año no cuenta ya nada
        conn.execute("DELETE FROM ai_usage WHERE at < ?", (time.time() - 400 * 86400,))
        conn.commit()
    return out
