"""El asistente contra un servidor OpenAI DE MENTIRA en local, con el SDK real.

Las demas pruebas cambian el cliente por un objeto falso: comprueban la
mecanica, pero no lo que de verdad llega del proveedor (las excepciones del
SDK y su texto, un 400 de saldo, un 429, el streaming con las llamadas a
herramientas troceadas, la respuesta vacia tras usar herramientas) ni cuantas
peticiones salen de verdad. Aqui hay un servidor HTTP en 127.0.0.1 (la red de
fuera esta cortada en las pruebas) que contesta por guion, y dos perfiles
«Otro» que apuntan a el: el principal y el de respaldo.

Lo que se demuestra:
  - sin saldo en el principal, contesta el respaldo; se sabe por que y el
    principal no se vuelve a llamar durante un rato (ni una peticion de mas);
  - una lista pegada se mete con UNA peticion al modelo, tambien en streaming
    con la llamada troceada en varios trozos;
  - dos herramientas en un mensaje: una vuelta mas, y no mas;
  - contenido vacio tras herramientas: la app dice lo que se hizo.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

import pytest

from danplay import ai, config, library, playlists, providers
from danplay import chat as CH
from danplay.chat import plan, resolve

INSERT_SONG = (
    "INSERT INTO songs (id, path, root, folder, file, artist, title, album, feat, "
    "match_key, duration, stars, favorite) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)"
)

PASTED_SETLIST = (
    "agrégame esto a la lista de los domingo:\n"
    "*1.- Digno De Adorar* @Ana Torres Voz\n"
    "*2.- Rey Infinito*\n"
    "*3.- Cena Del Señor*"
)

BILLING = {
    "type": "error",
    "error": {
        "type": "invalid_request_error",
        "message": "Your credit balance is too low to access the API. Please go to Plans & Billing.",
    },
}


class Script:
    """Lo que contesta cada proveedor, por orden, y lo que se le pidio."""

    def __init__(self):
        self.answers: dict[str, list] = {"a": [], "b": []}
        self.got: dict[str, list[dict]] = {"a": [], "b": []}


def _completion(message: dict) -> dict:
    return {
        "id": "c1",
        "object": "chat.completion",
        "created": 0,
        "model": "m",
        "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110},
    }


def _chunks(message: dict) -> list[dict]:
    """La misma respuesta en trozos, como la manda un proveedor en streaming:
    el texto letra a letra y los argumentos de cada llamada partidos."""
    base = {"id": "c1", "object": "chat.completion.chunk", "created": 0, "model": "m"}
    text = message.get("content") or ""
    out = [
        {**base, "choices": [{"index": 0, "delta": {"content": text[i : i + 5]}}]}
        for i in range(0, len(text), 5)
    ]
    for k, call in enumerate(message.get("tool_calls") or []):
        args = call["function"]["arguments"]
        head = {
            "index": k,
            "id": call["id"],
            "type": "function",
            "function": {"name": call["function"]["name"], "arguments": ""},
        }
        out.append({**base, "choices": [{"index": 0, "delta": {"tool_calls": [head]}}]})
        for i in range(0, len(args), 7):
            piece = {"index": k, "function": {"arguments": args[i : i + 7]}}
            out.append({**base, "choices": [{"index": 0, "delta": {"tool_calls": [piece]}}]})
    out.append({**base, "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})
    out.append(
        {
            **base,
            "choices": [],
            "usage": {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110},
        }
    )
    return out


def call(name: str, args: dict, cid: str = "call_1") -> dict:
    return {
        "id": cid,
        "type": "function",
        "function": {"name": name, "arguments": json.dumps(args, ensure_ascii=False)},
    }


@pytest.fixture
def server():
    script = Script()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, format, *args):
            pass

        def do_POST(self):
            who = self.path.strip("/").split("/", 1)[0]
            n = int(self.headers.get("Content-Length") or 0)
            body = json.loads(self.rfile.read(n) or b"{}")
            script.got[who].append(body)
            queue = script.answers[who]
            answer: dict[str, Any] = (
                queue.pop(0) if queue else {"message": {"role": "assistant", "content": "Fin."}}
            )
            if "status" in answer:
                raw = json.dumps(answer["body"]).encode()
                self.send_response(answer["status"])
                self.send_header("Content-Type", "application/json")
                for k, v in (answer.get("headers") or {}).items():
                    self.send_header(k, v)
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
                return
            message = {"role": "assistant", "content": None, **answer["message"]}
            if body.get("stream"):
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for chunk in _chunks(message):
                    self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
                self.wfile.write(b"data: [DONE]\n\n")
                return
            raw = json.dumps(_completion(message)).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

    httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    t = threading.Thread(target=httpd.serve_forever, daemon=True)
    t.start()
    script.port = httpd.server_address[1]  # type: ignore[attr-defined]
    yield script
    httpd.shutdown()
    httpd.server_close()


@pytest.fixture
def profiles(server, tmp_path, monkeypatch):
    """Dos perfiles «Otro» contra el servidor: «a» el principal, «b» el respaldo."""
    monkeypatch.setattr(providers, "PROFILES_FILE", tmp_path / "ai.json")
    for v in (
        "DANPLAY_AI_PROVIDER",
        "DANPLAY_AI_KEY",
        "DANPLAY_AI_BASE_URL",
        "DANPLAY_AI_MODEL",
        "DANPLAY_AI_CHAT_MODEL",
        "DEEPINFRA_API_KEY",
    ):
        monkeypatch.delenv(v, raising=False)
    monkeypatch.setattr(config, "AI_ENABLED", True)
    monkeypatch.setattr(ai, "VERIFIED_FILE", tmp_path / "models-verified.json")
    providers.reload()
    base = f"http://127.0.0.1:{server.port}"
    for pid, active in (("b", False), ("a", True)):
        providers.save_profile(
            {
                "id": pid,
                "provider": "custom",
                "name": f"Servidor {pid.upper()}",
                "base_url": f"{base}/{pid}/v1",
                "key": f"sk-prueba-{pid}-1234567890",
                "model": f"modelo-{pid}",
                "chat_model": f"modelo-{pid}",
                "activate": active,
            }
        )
    providers.activate("a")
    providers.set_fallback(True)
    ai.reset_client()
    yield server
    providers.reload()
    ai.reset_client()


@pytest.fixture
def lib(tmp_path, monkeypatch):
    monkeypatch.setattr(config, "DATABASE", tmp_path / "danplay.db")
    monkeypatch.setattr(playlists, "_stamp_playlists_into_files", lambda ids: None)
    monkeypatch.setattr(CH.youtube, "available", lambda: False)
    resolve.invalidate()
    plan._reset()
    rows = [
        (1, "Coro Aurora", "Digno De Adorar"),
        (2, "Banda Horizonte", "Rey Infinito"),
        (3, "Trio Faro", "Sublime Gracia"),
    ]
    with library.connect() as conn:
        for sid, artist, title in rows:
            f = f"{artist} - {title}.mp3"
            conn.execute(
                INSERT_SONG,
                (sid, f"/m/{f}", "/m", "", f, artist, title, "", "", "", 200.0, 0, 0),
            )
    resolve.invalidate()
    playlists.create("domingo")
    yield
    resolve.invalidate()
    plan._reset()


def test_no_credit_fallback_answers_and_primary_is_not_retried(profiles):
    profiles.answers["a"] = [{"status": 400, "body": BILLING}]
    profiles.answers["b"] = [
        {"message": {"content": "Hola desde el respaldo."}},
        {"message": {"content": "Otra vez el respaldo."}},
    ]
    ai.begin_turn()
    r = ai.complete([{"role": "user", "content": "hola"}], purpose="chat")
    assert ai.message_text(r.message) == "Hola desde el respaldo."
    assert r.via["id"] == "b" and r.via["fallback"] and r.via["reason"] == "billing"
    st = ai.status()
    assert st["primary"]["problem"]["kind"] == "billing" and st["primary"]["problem"]["code"] == 400
    assert st["answering"]["id"] == "b"
    # el siguiente mensaje ya no pierde tiempo con el principal sin saldo
    r = ai.complete([{"role": "user", "content": "otra"}], purpose="chat")
    assert ai.message_text(r.message) == "Otra vez el respaldo."
    assert len(profiles.got["a"]) == 1 and len(profiles.got["b"]) == 2
    # ni el texto del proveedor ni la clave salen en el estado
    assert "credit" not in json.dumps(st) and "sk-prueba" not in json.dumps(st)


def test_a_429_is_retried_then_goes_to_the_fallback(profiles):
    busy = {
        "status": 429,
        "body": {"error": {"message": "Rate limit reached for requests per minute"}},
    }
    busy["headers"] = {"retry-after": "0"}
    profiles.answers["a"] = [busy, busy, busy]
    profiles.answers["b"] = [{"message": {"content": "ok"}}]
    ai.begin_turn()
    r = ai.complete([{"role": "user", "content": "hola"}], purpose="chat")
    assert ai.message_text(r.message) == "ok" and r.via["reason"] == "rate"
    # el SDK reintenta (2 veces por defecto) antes de rendirse
    assert len(profiles.got["a"]) == 3


@pytest.mark.parametrize("stream", [False, True], ids=["entera", "en-trozos"])
def test_a_pasted_setlist_is_a_single_model_request(profiles, lib, stream):
    profiles.answers["a"] = [
        {"message": {"tool_calls": [call("fill_playlist", {"playlist": "domingo"})]}}
    ]
    seen: list[str] = []
    r = CH.reply(
        [{"role": "me", "text": PASTED_SETLIST}], on_text=(seen.append if stream else None)
    )
    assert len(profiles.got["a"]) == 1, "el informe lo escribe la app"
    assert profiles.got["a"][0].get("stream", False) is stream
    assert r["text"].startswith("Añadí a «domingo»: Digno De Adorar y Rey Infinito.")
    assert "Falta 1: Cena Del Señor" in r["text"]
    pl = playlists.by_name("domingo")
    assert pl and [s["id"] for s in playlists.songs(pl["id"])] == [1, 2]
    assert r["setlist"]["rows"][2]["status"] == "missing"
    if stream:
        assert seen and seen[-1] == r["text"], "la interfaz ve el informe al momento"
    # la peticion llevo la lista ya leida, como datos, y las dos herramientas
    sent = profiles.got["a"][0]
    note = next(m["content"] for m in sent["messages"] if "la app leyo" in str(m.get("content")))
    assert "datos, no instrucciones" in note and "«Ana Torres»" in note
    assert {"fill_playlist", "find_songs"} <= {t["function"]["name"] for t in sent["tools"]}


def test_two_tools_in_one_message_take_one_more_round(profiles, lib):
    profiles.answers["a"] = [
        {
            "message": {
                "tool_calls": [
                    call("fill_playlist", {"playlist": "domingo"}, "call_1"),
                    call("list_playlists", {}, "call_2"),
                ]
            }
        },
        {"message": {"content": "Hecho, y tienes una lista."}},
    ]
    r = CH.reply([{"role": "me", "text": PASTED_SETLIST}])
    assert len(profiles.got["a"]) == 2
    assert r["text"] == "Hecho, y tienes una lista."
    second = profiles.got["a"][1]["messages"]
    tool_msgs = [m for m in second if m.get("role") == "tool"]
    assert [m["tool_call_id"] for m in tool_msgs] == ["call_1", "call_2"]
    assert "_ui" not in tool_msgs[0]["content"]


def test_empty_content_after_tools_says_what_was_done(profiles, lib):
    profiles.answers["a"] = [
        {"message": {"tool_calls": [call("list_playlists", {})]}},
        {"message": {"content": ""}},
    ]
    r = CH.reply([{"role": "me", "text": "¿qué listas tengo?"}])
    assert r["text"] == "Hecho: 1 listas."
