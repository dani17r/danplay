"""Lo que no es de ninguna cancion pero se guarda en la base: el historial de
descargas y lo que gasta la IA. El escaneo no lo toca (solo vacia `songs`).

Lo hace Rust (library/src/index/history.rs); esto es la puerta de
Python, con los mismos nombres.
"""

import datetime as _dt

import danplay_core as _rust

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
    with db.connect() as conn:
        _rust.history_log_download(conn, entry)


def download_history(limit=60, offset=0) -> list[dict]:
    with db.connect() as conn:
        return _rust.history_downloads(conn, int(limit), int(offset))


def download_count() -> int:
    with db.connect() as conn:
        return _rust.history_download_count(conn)


def clear_download_history() -> int:
    with db.connect() as conn:
        return _rust.history_clear_downloads(conn)


def log_ai_usage(
    provider: str, model: str, purpose: str, prompt: int, completion: int, cost: float | None
) -> None:
    with db.connect() as conn:
        _rust.history_log_ai_usage(
            conn, provider, model, purpose, int(prompt), int(completion), cost
        )


def ai_usage_summary() -> dict:
    """Lo gastado hoy, este mes y en total: llamadas, tokens y coste (solo
    de las llamadas con precio conocido; las demas se cuentan aparte)."""
    now = _dt.datetime.now()
    day = _dt.datetime(now.year, now.month, now.day).timestamp()
    month = _dt.datetime(now.year, now.month, 1).timestamp()
    with db.connect() as conn:
        return _rust.history_ai_usage_summary(conn, day, month)
