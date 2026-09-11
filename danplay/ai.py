# -*- coding: utf-8 -*-
"""El cerebro: cualquier proveedor compatible con la API de OpenAI.

Aqui vive lo que habla con el modelo. Que proveedor, con que clave y que
modelos lo decide `providers` (el perfil activo); que modelos existen y que
saben hacer lo cuenta `model_catalog`. Este modulo solo pide y tolera.

Tolerar es la parte importante: no hay dos servicios que acepten los mismos
parametros. Anthropic ignora `response_format`, muchos servidores locales
rechazan `tool_choice="required"`, los razonadores de OpenAI no admiten
`temperature` y quieren `max_completion_tokens`. En vez de fallar en seco,
`complete()` quita lo que el servidor rechaza, reintenta y se acuerda para la
proxima vez. Lo que se sabe de antemano por el catalogo ni se manda.
"""
import json
import logging
import re
import threading
import time
from . import config, providers, model_catalog

log = logging.getLogger("danplay.ai")

_client = None
_client_for: tuple | None = None       # con que perfil se construyo
_client_lock = threading.Lock()

# Lo que un servidor ya rechazo, por (proveedor, modelo): no se repite.
_unsupported: dict[tuple[str, str], set[str]] = {}


class ToolsUnsupported(Exception):
    """El modelo no sabe usar herramientas: el asistente no puede con el."""


# Sin perfil no hay con quien hablar; `_get_client` lo dice. Este vacio solo
# sirve para que `complete` monte la peticion igual (las pruebas sustituyen
# el cliente por uno falso y no tienen ningun proveedor configurado).
_NO_PROFILE = {"id": "", "provider": "", "name": "", "base_url": "", "key": "",
               "key_required": False, "headers": {}, "model": "", "chat_model": "",
               "extra": {}, "timeout": providers.DEFAULT_TIMEOUT, "quirks": {},
               "models_dev": None, "local": False}


INSTRUCTIONS = """Eres un catalogador de una biblioteca musical (mucha musica cristiana
de adoracion en español, y algo de pop/rock secular).

A partir de un nombre de archivo sucio (normalmente descargado de video), deduce:
  artist     : el interprete PRINCIPAL. Si el archivo dice "X ft Y", el principal es X.
               Si es un ministerio/proyecto de otro artista, usa el nombre del proyecto.
  title      : el titulo limpio de la cancion, sin ruido de video.
  feat       : artistas invitados, separados por coma. "" si no hay.
  extra      : matiz relevante entre parentesis (En Vivo, Acustico, Cover, Instrumental,
               Play Along, Tutorial, Pista, Medley...). "" si no aplica.
  category   : una de -> song | track | tutorial | sequence | unknown
  confidence : 0.0 a 1.0, que tan seguro estas.

REGLAS DE ESCRITURA (obligatorias):
  - SIN tildes ni acentos. UNICA excepcion: la ñ se conserva.
  - Nada en MAYUSCULA SOSTENIDA: escribe Capitalizado.
  - Nunca inventes un artista. Si no lo sabes, artist="" y confidence baja.

Responde SOLO con un objeto JSON con esas claves EXACTAS, en ingles y sin texto
alrededor. Los valores van en el idioma original de la cancion."""


# ------------------------------------------------------------------ perfil

def profile() -> dict | None:
    """El perfil activo resuelto, o None."""
    return providers.active()


def available() -> bool:
    if not config.AI_ENABLED:
        return False
    ok, _ = providers.usable(profile())
    return ok


def unavailable_reason() -> str:
    """Por que no se puede usar la IA ahora mismo, en castellano. "" si se puede."""
    if not config.AI_ENABLED:
        return "la IA esta apagada en Ajustes"
    ok, why = providers.usable(profile())
    return "" if ok else why


def fast_model() -> str:
    p = profile()
    return p["model"] if p else ""


def chat_model() -> str:
    p = profile()
    return p["chat_model"] if p else ""


def provider_name() -> str:
    p = profile()
    return p["name"] if p else ""


def reset_client() -> None:
    """Olvida el cliente: se vuelve a crear con el perfil actual."""
    global _client, _client_for
    with _client_lock:
        _client, _client_for = None, None
    _unsupported.clear()


def _build_client(p: dict):
    from openai import OpenAI
    # Los servidores locales no piden clave, pero el SDK exige alguna: un
    # texto cualquiera vale (Ollama documenta justo eso).
    # Reintentar un fallo de conexion tiene sentido con la nube; con un
    # servidor local apagado solo añade segundos antes de decir que no esta.
    return OpenAI(api_key=p["key"] or "no-key", base_url=p["base_url"],
                  default_headers=p["headers"] or None,
                  timeout=p["timeout"], max_retries=0 if p.get("local") else 2)


def _get_client():
    """El cliente para el perfil activo; se recrea si el perfil cambio."""
    global _client, _client_for
    p = profile()
    if not p:
        raise RuntimeError("no hay proveedor de IA configurado")
    signature = (p["id"], p["base_url"], p["key"], json.dumps(p["headers"], sort_keys=True),
                 p["timeout"])
    with _client_lock:
        if _client is None or _client_for != signature:
            _client = _build_client(p)
            _client_for = signature
        return _client


# ---------------------------------------------------------------- peticion

# Que parametro rechaza un servidor se sabe por el texto de su error. No hay
# un formato comun: aqui van las pistas que dejan OpenAI, Google, Mistral,
# Ollama, llama.cpp, vLLM, OpenRouter y compañia.
_HINTS = (
    ("response_format", re.compile(r"response_format|json_object|json_schema|json mode|"
                                   r"structured output", re.I)),
    ("tool_choice", re.compile(r"tool_choice|tool choice", re.I)),
    ("temperature", re.compile(r"temperature", re.I)),
    ("max_tokens", re.compile(r"max_tokens.*(?:not supported|unsupported|use .?max_completion_tokens)|"
                              r"max_completion_tokens", re.I)),
    ("parallel_tool_calls", re.compile(r"parallel_tool_calls", re.I)),
)
_TOOLS_HINT = re.compile(r"(?:does not support|doesn't support|not support(?:ed)?|unsupported|"
                         r"no soporta|cannot use|not available).{0,60}(?:tools|function|tool use)|"
                         r"(?:tools|functions?).{0,60}(?:not support|unsupported|not available)",
                         re.I | re.S)


def _strip(kwargs: dict, what: str) -> None:
    if what == "tool_choice":
        if kwargs.get("tool_choice") not in (None, "auto"):
            kwargs["tool_choice"] = "auto"
    elif what == "max_tokens":
        if "max_tokens" in kwargs:
            kwargs["max_completion_tokens"] = kwargs.pop("max_tokens")
    else:
        kwargs.pop(what, None)


def _droppable(kwargs: dict, what: str) -> bool:
    """Si quitar `what` cambia algo en la peticion (si no, reintentar es un bucle)."""
    if what == "tool_choice":
        return kwargs.get("tool_choice") not in (None, "auto")
    if what == "max_tokens":
        return "max_tokens" in kwargs
    return what in kwargs


def _known_limits(p: dict, model: str) -> set[str]:
    """Lo que se sabe sin preguntar: por el catalogo y por el proveedor."""
    out = set(_unsupported.get((p["id"], model), set()))
    if p["quirks"].get("json_mode") is False:
        out.add("response_format")
    info = model_catalog.lookup(p.get("models_dev"), model)
    if info:
        if info.get("temperature") is False:
            out.add("temperature")
        if info.get("json") is False:
            out.add("response_format")
    return out


def complete(messages: list[dict], *, model: str = "", tools=None, tool_choice=None,
             response_format=None, temperature=None, max_tokens=None, timeout=None,
             purpose: str = "fast"):
    """Una peticion al modelo, con las tolerancias descritas arriba.

    Devuelve la respuesta del SDK (`r.choices[0].message`). Levanta
    `ToolsUnsupported` si se pidieron herramientas y el modelo no las admite;
    cualquier otro fallo sale como la excepcion del SDK.
    """
    p = profile() or _NO_PROFILE
    model = model or (p["chat_model"] if purpose == "chat" else p["model"])
    kwargs: dict = {"model": model, "messages": messages}
    if tools:
        info = model_catalog.lookup(p.get("models_dev"), model)
        if info and not info.get("tools") and (p["id"], model) not in _unsupported:
            # el catalogo dice que no; se intenta igual una vez (puede estar
            # desactualizado) y si el servidor lo confirma, ya no se insiste
            log.info("%s: el catalogo dice que %s no usa herramientas", p["id"], model)
        kwargs["tools"] = tools
        if tool_choice:
            kwargs["tool_choice"] = tool_choice
    if response_format:
        kwargs["response_format"] = response_format
    if temperature is not None:
        kwargs["temperature"] = temperature
    if max_tokens:
        kwargs["max_tokens"] = max_tokens
    if timeout:
        kwargs["timeout"] = timeout
    if p["extra"]:
        kwargs["extra_body"] = dict(p["extra"])
    for what in _known_limits(p, model):
        _strip(kwargs, what)

    client = _get_client()
    key = (p["id"], model)
    if tools and "tools" in _unsupported.get(key, ()):
        raise ToolsUnsupported(model)
    for attempt in range(6):
        try:
            return client.chat.completions.create(**kwargs)
        except Exception as e:                               # noqa: BLE001
            status = getattr(e, "status_code", None)
            text = str(e)
            if status not in (400, 404, 422) and not re.match(r"\s*(?:400|404|422)\b", text):
                raise
            # Primero lo que se puede quitar (tool_choice=required, JSON…):
            # un «tool_choice not supported» habla del parametro, no de que
            # el modelo no sepa usar herramientas.
            dropped = next((w for w, rx in _HINTS if rx.search(text) and _droppable(kwargs, w)),
                           None)
            if dropped and attempt < 5:
                log.info("%s rechaza %s con %s; se reintenta sin el", p["id"], dropped, model)
                _unsupported.setdefault(key, set()).add(dropped)
                _strip(kwargs, dropped)
                continue
            if tools and _TOOLS_HINT.search(text):
                _unsupported.setdefault(key, set()).add("tools")
                raise ToolsUnsupported(model) from e
            raise
    raise RuntimeError("sin respuesta")            # no se llega: el bucle sale antes


def message_text(msg) -> str:
    """El texto de una respuesta, sin el razonamiento que algunos dejan escapar."""
    text = getattr(msg, "content", None) or ""
    if isinstance(text, list):                     # partes (texto + otras)
        text = "".join(getattr(x, "text", "") or (x.get("text", "") if isinstance(x, dict) else "")
                       for x in text)
    return strip_thoughts(text)


def strip_thoughts(t: str) -> str:
    """Algunos modelos dejan escapar su razonamiento entre <think>."""
    t = re.sub(r"<think>.*?</think>", "", t or "", flags=re.DOTALL | re.IGNORECASE)
    t = re.sub(r"</?think>", "", t, flags=re.IGNORECASE)
    return t.strip()


# ------------------------------------------------------------------ prueba

def describe_error(e: Exception, p: dict | None = None) -> str:
    """Un fallo del proveedor, en una frase que el usuario entienda."""
    name = (p or {}).get("name") or "el proveedor"
    text = str(e)
    low = text.lower()
    status = getattr(e, "status_code", None)
    if status in (401, 403) or any(x in low for x in ("401", "403", "unauthorized", "invalid api key",
                                                        "invalid_api_key", "authentication",
                                                        "incorrect api key", "permission")):
        return f"{name} rechaza la clave"
    if status == 402 or any(x in low for x in ("402", "insufficient", "credit", "balance", "billing",
                                               "quota")):
        return "la clave es valida pero no tiene credito"
    if status == 429 or "429" in low or "rate limit" in low:
        return f"{name} pide esperar (limite de peticiones)"
    if status == 404 or any(x in low for x in ("404", "not found", "does not exist", "no such model",
                                               "not_found", "unknown model", "model not")):
        return "ese modelo no existe en " + name + " (o la URL no es la de la API)"
    if any(x in low for x in ("connection", "timeout", "timed out", "resolve", "network",
                              "unreachable", "refused", "nodename")):
        if (p or {}).get("local"):
            return f"no hay nada escuchando en {p['base_url']}: ¿esta arrancado {name}?"
        return f"no hay conexion con {name}"
    if "ssl" in low or "certificate" in low:
        return "fallo de certificado al conectar (¿URL con https correcta?)"
    return text[:180]


def _ping(client, model: str, p: dict) -> tuple[bool, str, int]:
    """La llamada mas barata posible: un token. Vale para clave, URL y modelo."""
    t0 = time.monotonic()
    try:
        kwargs = {"model": model, "messages": [{"role": "user", "content": "ok"}],
                  "max_tokens": 1}
        if "temperature" not in _known_limits(p, model):
            kwargs["temperature"] = 0
        try:
            client.chat.completions.create(**kwargs)
        except Exception as e:                               # noqa: BLE001
            low = str(e).lower()
            if "max_completion_tokens" in low or "temperature" in low:
                kwargs.pop("temperature", None)
                kwargs["max_completion_tokens"] = kwargs.pop("max_tokens", 1)
                client.chat.completions.create(**kwargs)
            else:
                raise
        return True, "", int((time.monotonic() - t0) * 1000)
    except Exception as e:                                   # noqa: BLE001
        return False, describe_error(e, p), int((time.monotonic() - t0) * 1000)


_PROBE_TOOL = [{"type": "function", "function": {
    "name": "saluda", "description": "Saluda al usuario por su nombre.",
    "parameters": {"type": "object", "properties": {"nombre": {"type": "string"}},
                   "required": ["nombre"]}}}]


def _probe_tools(client, model: str, p: dict) -> tuple[bool, str]:
    """¿Sabe llamar a una herramienta? Es lo que necesita el asistente."""
    kwargs = {"model": model, "tools": _PROBE_TOOL, "tool_choice": "auto", "max_tokens": 60,
              "messages": [{"role": "user", "content": "Saluda a Dani usando la herramienta."}]}
    for what in _known_limits(p, model):
        _strip(kwargs, what)
    for _ in range(3):
        try:
            r = client.chat.completions.create(**kwargs)
            msg = r.choices[0].message
            if getattr(msg, "tool_calls", None):
                return True, ""
            return False, ("responde sin llamar a la herramienta: el asistente puede fallar "
                           "con este modelo")
        except Exception as e:                               # noqa: BLE001
            text = str(e)
            dropped = next((w for w, rx in _HINTS if rx.search(text) and _droppable(kwargs, w)),
                           None)
            if dropped:
                _strip(kwargs, dropped)
                continue
            if _TOOLS_HINT.search(text) or re.search(r"\btools?\b", text, re.I):
                return False, "este modelo no admite herramientas: el asistente no puede usarlo"
            return False, describe_error(e, p)
    return False, "no se pudo comprobar si admite herramientas"


def check(draft: dict | None = None) -> dict:
    """Comprueba que un perfil funciona de verdad, no solo que este relleno.

    Con `draft` prueba lo que hay en el formulario (sin guardarlo); sin el,
    el perfil activo. Distingue clave rechazada, sin credito, sin conexion y
    modelo inexistente, y ademas prueba si el modelo de conversacion sabe
    usar herramientas, que es lo que necesita el asistente.
    """
    if draft is None:
        if not config.AI_ENABLED:
            return {"ok": False, "reason": "la IA esta apagada en Ajustes"}
        p = profile()
    else:
        d = providers.with_saved_key(draft)
        p = providers.resolve(str(d.get("id") or d.get("provider") or ""), d)
    ok, why = providers.usable(p)
    if not ok:
        return {"ok": False, "reason": why}
    try:
        client = _build_client(p)
    except Exception as e:                                   # noqa: BLE001
        return {"ok": False, "reason": describe_error(e, p)}
    ok, reason, ms = _ping(client, p["model"], p)
    out = {"ok": ok, "reason": reason, "provider": p["name"], "model": p["model"],
           "chat_model": p["chat_model"], "latency_ms": ms}
    if not ok:
        return out
    if p["chat_model"] != p["model"]:
        ok2, reason2, _ = _ping(client, p["chat_model"], p)
        if not ok2:
            out.update(ok=False, reason=f"modelo de conversacion: {reason2}")
            return out
    tools_ok, tools_reason = _probe_tools(client, p["chat_model"], p)
    out.update(tools_ok=tools_ok, tools_reason=tools_reason)
    return out


def list_models(draft: dict | None = None) -> dict:
    """Los modelos que ofrece el proveedor con ESA clave, cruzados con el
    catalogo (precio, herramientas, obsoleto). `draft` como en `check`."""
    if draft is None:
        p = profile()
    else:
        d = providers.with_saved_key(draft)
        p = providers.resolve(str(d.get("id") or d.get("provider") or ""), d)
    if not p:
        return {"ok": False, "reason": "falta la URL del proveedor", "models": []}
    if p["key_required"] and not p["key"]:
        # sin clave no se molesta al proveedor (ni se le cuenta como fallo):
        # la interfaz enseña el catalogo hasta que la escriban
        return {"ok": False, "reason": "", "models": []}
    try:
        client = _build_client(p)
        page = client.models.list()
        raw = list(page.data) if hasattr(page, "data") else list(page)
    except Exception as e:                                   # noqa: BLE001
        return {"ok": False, "reason": describe_error(e, p), "models": []}
    models = []
    for m in raw:
        mid = getattr(m, "id", None) or (m.get("id") if isinstance(m, dict) else None)
        if not mid:
            continue
        extra = {}
        for getter in ("to_dict", "model_dump"):
            fn = getattr(m, getter, None)
            if callable(fn):
                try:
                    extra = fn() or {}
                    break
                except Exception:                            # noqa: BLE001
                    pass
        models.append(_merge(mid, extra, p))
    models.sort(key=lambda x: (x["released"] or "", x["id"]), reverse=True)
    return {"ok": True, "models": models, "source": "provider", "provider": p["name"]}


def _merge(mid: str, extra: dict, p: dict) -> dict:
    """La ficha de un modelo: lo que dice el proveedor mas lo que sabe el
    catalogo. OpenRouter cuenta precio y parametros en su propio /models."""
    info = model_catalog.lookup(p.get("models_dev"), mid) or {}
    out = {"id": mid, "name": info.get("name") or str(extra.get("name") or mid),
           "tools": info.get("tools"), "json": info.get("json"),
           "reasoning": info.get("reasoning", False),
           "cost_in": info.get("cost_in"), "cost_out": info.get("cost_out"),
           "context": info.get("context") or extra.get("context_length"),
           "released": info.get("released") or "",
           "deprecated": bool(info.get("deprecated")), "known": bool(info)}
    params = extra.get("supported_parameters")
    if isinstance(params, list):
        out["tools"] = "tools" in params
        out["json"] = "response_format" in params or "structured_outputs" in params
    pricing = extra.get("pricing")
    if isinstance(pricing, dict):
        try:
            out["cost_in"] = round(float(pricing.get("prompt", 0)) * 1_000_000, 4)
            out["cost_out"] = round(float(pricing.get("completion", 0)) * 1_000_000, 4)
        except (TypeError, ValueError):
            pass
    created = extra.get("created")
    if not out["released"] and isinstance(created, (int, float)) and created > 0:
        out["released"] = time.strftime("%Y-%m-%d", time.gmtime(created))
    return out


# ----------------------------------------------------------------- consultas

def _json_from(text: str) -> dict | None:
    text = re.sub(r"^```(?:json)?|```$", "", (text or "").strip(), flags=re.MULTILINE).strip()
    try:
        return json.loads(text)
    except Exception:                                        # noqa: BLE001
        m = re.search(r"\{.*\}", text, re.DOTALL)
        if m:
            try:
                return json.loads(m.group(0))
            except Exception:                                # noqa: BLE001
                return None
    return None


def resolve(file_name: str, known_artists=None, extra_hint="") -> dict | None:
    """Deduce artista/titulo de un nombre sucio. None si la IA no esta disponible."""
    if not available():
        return None
    context = ""
    if known_artists:
        sample = sorted(known_artists)[:120]
        context = ("\nArtistas que YA existen en la biblioteca (reutiliza el nombre exacto "
                   "si la cancion es de alguno):\n" + ", ".join(sample))
    user_msg = f"Nombre de archivo:\n{file_name}\n{extra_hint}{context}"
    try:
        r = complete([{"role": "system", "content": INSTRUCTIONS},
                      {"role": "user", "content": user_msg}],
                     temperature=0.1, max_tokens=400,
                     response_format={"type": "json_object"})
        data = _json_from(message_text(r.choices[0].message))
    except Exception as e:                                   # noqa: BLE001
        return {"error": describe_error(e, profile())}
    if not data:
        return None
    return {
        "artist":   str(data.get("artist", "")).strip(),
        "title":    str(data.get("title", "")).strip(),
        "feat":      str(data.get("feat", "")).strip(),
        "extra":     str(data.get("extra", "")).strip(),
        "category": str(data.get("category", "unknown")).strip().lower(),
        "confidence": float(data.get("confidence", 0.0) or 0.0),
        "source":    "ai",
    }


def ask(prompt, system="Eres un asistente musical preciso y conciso.",
        max_tokens=800, temperature=0.2) -> str | None:
    """Consulta libre en texto plano."""
    if not available():
        return None
    try:
        r = complete([{"role": "system", "content": system},
                      {"role": "user", "content": prompt}],
                     temperature=temperature, max_tokens=max_tokens)
        return message_text(r.choices[0].message)
    except Exception:                                        # noqa: BLE001
        log.info("la consulta a la IA fallo", exc_info=True)
        return None


def ask_json(prompt, system="Responde solo con JSON valido.",
             max_tokens=1200, temperature=0.15) -> dict | None:
    if not available():
        return None
    try:
        r = complete([{"role": "system", "content": system},
                      {"role": "user", "content": prompt}],
                     temperature=temperature, max_tokens=max_tokens,
                     response_format={"type": "json_object"})
        return _json_from(message_text(r.choices[0].message))
    except Exception as e:                                   # noqa: BLE001
        return {"error": describe_error(e, profile())}
