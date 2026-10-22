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

Y cuando el proveedor elegido falla, no lo calla: apunta POR QUE (sin saldo,
clave rechazada, limite de peticiones, caido: `classify_failure`) y quien
contesto en su lugar. `status()` lo cuenta a la interfaz sin repetir ni una
palabra del proveedor, que puede llevar fragmentos de la clave
(`sanitize_provider_message`).
"""

import json
import logging
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from types import SimpleNamespace
from typing import Any

import danplay_core as _rust

from . import config, model_catalog, providers

log = logging.getLogger(__name__)

# el cliente del SDK de OpenAI (o el de mentira de las pruebas)
_client: Any = None
_client_for: tuple | None = None  # con que perfil se construyo
_client_lock = threading.Lock()
# clientes de los perfiles de respaldo, por su firma
_clients: dict[tuple, Any] = {}

# Lo que un servidor ya rechazo, por (proveedor, modelo): no se repite.
_unsupported: dict[tuple[str, str], set[str]] = {}

# Un proveedor que falla se da por caido un rato: en ese rato se va directo al
# respaldo, sin gastar otra llamada (y su tiempo limite) en el. Cuanto dura
# depende de POR QUE fallo (`classify_failure`): lo que se arregla solo, poco;
# lo que necesita a la persona (recargar saldo, cambiar la clave), mucho. Con
# 60 s para todo, un principal sin saldo costaba una llamada fallida cada
# minuto de charla.
_down_until: dict[str, float] = {}
DOWN_FOR = 60  # sin respuesta, saturado, o un 403 que no dice por que
RATE_FOR = 120  # limite de peticiones (por minuto): pasa solo
LONG_FOR = 20 * 60  # sin saldo o clave rechazada: no pasa solo

# Lo que se sabe de por que fallo cada proveedor y quien contesto la ultima
# vez en el chat. Lo lee `status()`, que llama OTRO hilo (la ruta que alimenta
# la franja de la interfaz): por eso es un registro global con cerrojo y no
# algo por hilo como `_turn`. NO guarda el texto del proveedor (puede repetir
# trozos de la clave): solo el tipo de fallo, el codigo HTTP y cuando.
_status_lock = threading.Lock()
_problems: dict[str, dict] = {}  # id -> {"kind", "code", "at"}
_last_chat_via: dict | None = None

# Flujos que se rompieron a medias, por (proveedor, modelo). Algun servicio
# manda de vez en cuando un evento vacio que el SDK no traga: la peticion se
# repite sin flujo, y si pasa tres veces seguidas se deja de pedir en trozos
# a ese modelo en esta sesion.
_stream_failures: dict[tuple[str, str], int] = {}
STREAM_FAILURES_MAX = 3

# Lo gastado en el turno en curso (por hilo: cada conversacion va en el suyo).
# Cuenta TODAS las llamadas (la conversacion, el juez...); `via`, solo la
# ultima de conversacion: lo que la interfaz enseña como «quien contesto» no
# puede cambiar porque el juez haya pasado por otro modelo.
_turn = threading.local()


class ToolsUnsupported(Exception):
    """El modelo no sabe usar herramientas: el asistente no puede con el."""


class Canceled(Exception):
    """La persona paro la respuesta a medias."""


class Reply:
    """Lo que devuelve `complete`: el mensaje ya recibido entero (aunque
    llegara en trozos), lo que costo y quien respondio."""

    def __init__(self, message, usage, via):
        self.message, self.usage, self.via = message, usage, via


# Sin perfil no hay con quien hablar; `_get_client` lo dice. Este vacio solo
# sirve para que `complete` monte la peticion igual (las pruebas sustituyen
# el cliente por uno falso y no tienen ningun proveedor configurado).
_NO_PROFILE: providers.Profile = {
    "id": "",
    "provider": "",
    "name": "",
    "base_url": "",
    "key": "",
    "key_required": False,
    "headers": {},
    "model": "",
    "chat_model": "",
    "extra": {},
    "timeout": providers.DEFAULT_TIMEOUT,
    "quirks": {},
    "models_dev": None,
    "local": False,
    "retries": None,
}


# Lo que se le dice al modelo para identificar un archivo (en Rust:
# biblioteca/src/ia/peticion.rs)
INSTRUCTIONS = _rust.ai_instructions()


# ------------------------------------------------------------------ perfil


def profile() -> providers.Profile | None:
    """El perfil activo resuelto, o None."""
    return providers.active()


def available() -> bool:
    if not config.AI_ENABLED:
        return False
    ok, _ = providers.usable(profile())
    return ok


def warm_in_background(delay: float = 2.0) -> threading.Thread | None:
    """Carga el SDK de OpenAI en segundo plano, si hay IA configurada.

    Importarlo es lo mas caro de la primera respuesta del chat: miles de
    modulos de tipos (medido: 49 s en un disco lento por FUSE; en uno normal,
    segundos). Lo pagaba el primer mensaje de la persona, que esperaba con la
    pantalla quieta. Se hace un poco despues de arrancar, para no competir con
    la primera pantalla. Sin IA configurada no se carga nada.
    """
    try:
        if not available():
            return None
    except Exception:  # noqa: BLE001
        return None

    def run() -> None:
        time.sleep(delay)
        try:
            import openai  # noqa: F401
            from openai.resources.chat import completions  # noqa: F401
        except Exception:
            log.debug("no se pudo precargar el SDK de openai", exc_info=True)

    t = threading.Thread(target=run, name="danplay-ia-precarga", daemon=True)
    t.start()
    return t


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
    """Olvida el cliente: se vuelve a crear con el perfil actual. Y con el, lo
    que se sabia de quien fallaba y de quien contestaba: si cambia el perfil o
    sus ajustes (o la persona recarga el saldo y lo guarda), lo de antes ya no
    vale."""
    global _client, _client_for, _last_chat_via
    with _client_lock:
        _client, _client_for = None, None
        _clients.clear()
    _unsupported.clear()
    with _status_lock:
        _down_until.clear()
        _problems.clear()
        _last_chat_via = None


def begin_turn() -> None:
    """Empieza a contar lo que gasta un turno (varias llamadas: las vueltas
    con herramientas, el juez…). Por hilo."""
    _turn.usage = {
        "calls": 0,
        "prompt": 0,
        "completion": 0,
        "cost": 0.0,
        "priced": True,
        "requests": 0,
    }
    _turn.via = None


def turn_requests() -> int:
    """Cuantas veces se ha llamado a `complete` en el turno en curso (0 sin
    turno): cada vuelta, reintento, empujon y consulta al juez, salgan bien o
    mal y pasen por cuantos proveedores pasen. A diferencia de `calls` (solo
    las que devolvieron lo gastado), no se escapa ninguna: es lo que mide el
    presupuesto de llamadas de un turno."""
    acc = getattr(_turn, "usage", None)
    return int(acc.get("requests", 0)) if acc else 0


def turn_summary() -> tuple[dict | None, dict | None]:
    """(uso del turno, quien respondio en su ultima llamada de CONVERSACION), o
    (None, None). El uso suma todas las llamadas; `via`, solo las de
    `purpose="chat"`: el juez o la identificacion de un archivo no cambian
    quien se dice que ha contestado."""
    return getattr(_turn, "usage", None), getattr(_turn, "via", None)


def price_of(p: providers.Profile | None, model: str, prompt: int, completion: int) -> float | None:
    """Lo que cuestan esos tokens segun el catalogo, o None si no se sabe."""
    if not p or p.get("local"):
        return 0.0 if p and p.get("local") else None
    info = model_catalog.lookup(p.get("models_dev"), model)
    return _rust.ai_price_of(False, info, int(prompt), int(completion))


def _account(p: providers.Profile, model: str, purpose: str, usage) -> None:
    """Apunta el gasto de una llamada: en el turno y en la base."""
    if usage is None:
        return
    prompt = int(getattr(usage, "prompt_tokens", 0) or 0)
    completion = int(getattr(usage, "completion_tokens", 0) or 0)
    cost = price_of(p, model, prompt, completion)
    acc = getattr(_turn, "usage", None)
    if acc is not None:
        acc["calls"] += 1
        acc["prompt"] += prompt
        acc["completion"] += completion
        if cost is None:
            acc["priced"] = False
        else:
            acc["cost"] += cost
    try:
        from . import library

        library.log_ai_usage(p["id"], model, purpose, prompt, completion, cost)
    except Exception:
        log.debug("no se pudo apuntar el uso de la IA", exc_info=True)


def _build_client(p: providers.Profile):
    from openai import OpenAI

    # Los servidores locales no piden clave, pero el SDK exige alguna: un
    # texto cualquiera vale (Ollama documenta justo eso).
    # Reintentar un fallo de conexion tiene sentido con la nube; con un
    # servidor local apagado solo añade segundos antes de decir que no esta.
    retries = p.get("retries")
    if retries is None:
        retries = 0 if p.get("local") else 2
    return OpenAI(
        api_key=p["key"] or "no-key",
        base_url=p["base_url"],
        default_headers=p["headers"] or None,
        timeout=p["timeout"],
        max_retries=int(retries),
    )


def _signature(p: providers.Profile) -> tuple:
    return (
        p["id"],
        p["base_url"],
        p["key"],
        json.dumps(p["headers"], sort_keys=True),
        p["timeout"],
    )


def _get_client():
    """El cliente para el perfil activo; se recrea si el perfil cambio."""
    global _client, _client_for
    p = profile()
    if not p:
        raise RuntimeError("no hay proveedor de IA configurado")
    signature = _signature(p)
    with _client_lock:
        if _client is None or _client_for != signature:
            _client = _build_client(p)
            _client_for = signature
        return _client


def _client_for_profile(p: providers.Profile):
    """El cliente de un perfil de respaldo (se guardan por firma)."""
    signature = _signature(p)
    with _client_lock:
        c = _clients.get(signature)
        if c is None:
            c = _clients[signature] = _build_client(p)
        return c


# ---------------------------------------------------------------- peticion


# Lo que no se toca nunca al quitar parametros (y puede llevar objetos del
# SDK, que Rust no conoce): el resto de la peticion va a Rust y vuelve.
_UNTOUCHED = ("messages", "tools")


def _view(kwargs: dict) -> dict:
    return {k: v for k, v in kwargs.items() if k not in _UNTOUCHED}


def _strip(kwargs: dict, what: str) -> None:
    """Quita `what` de la peticion (o lo cambia por lo que si vale)."""
    new = _rust.ai_strip(_view(kwargs), what)
    for k in [k for k in kwargs if k not in _UNTOUCHED and k not in new]:
        del kwargs[k]
    kwargs.update(new)


def _droppable(kwargs: dict, what: str) -> bool:
    """Si quitar `what` cambia algo en la peticion (si no, reintentar es un bucle)."""
    return _rust.ai_droppable(_view(kwargs), what)


def _first_droppable(kwargs: dict, text: str) -> str | None:
    """El primer parametro que el error dice que sobra y que, quitado, cambia
    algo (las pistas, en Rust: `peticion::hints`)."""
    return _rust.ai_first_droppable(_view(kwargs), text)


def _known_limits(p: providers.Profile, model: str) -> set[str]:
    """Lo que se sabe sin preguntar: por el catalogo y por el proveedor."""
    out = set(_unsupported.get((p["id"], model), set()))
    info = model_catalog.lookup(p.get("models_dev"), model)
    out.update(_rust.ai_known_limits(p["quirks"], info))
    return out


# ----------------------------------------------- por que falla un proveedor

# Cuatro tipos de fallo del SERVICIO. Lo que dice el TEXTO sin lugar a dudas
# manda sobre el codigo HTTP, porque el codigo engaña: OpenAI manda
# `insufficient_quota` (sin saldo) con un 429 (que casi siempre es «espera un
# poco»), Gemini manda con un 429 tanto «limite por minuto» como «tu plan no
# tiene cuota para este modelo», y OpenRouter contesta un 403 cuando modera UN
# mensaje, no porque la clave este mal.
#   billing  sin saldo o sin cuota en el plan: lo arregla la persona
#   auth     clave rechazada: lo arregla la persona
#   rate     demasiadas peticiones: pasa solo
#   down     caido, saturado, sin conexion, tiempo agotado, o un 403 sin motivo
KINDS = ("billing", "auth", "rate", "down")


def _error_text(e: object) -> str:
    try:
        return str(e)
    except Exception:  # noqa: BLE001
        return type(e).__name__


def _status_of(e: object) -> int | None:
    """El codigo HTTP que trae el fallo (el `status_code` del SDK), si es un
    numero de verdad."""
    code = getattr(e, "status_code", None)
    if isinstance(code, int) and not isinstance(code, bool):
        return code
    return None


def classify_failure(e: BaseException) -> tuple[str, int | None]:
    """(tipo, codigo HTTP) de un fallo del SERVICIO. Tipo "" si el fallo es del
    mensaje (parametros que no valen, un modelo que no existe…) y probar con
    otro proveedor no lo arreglaria. Ver `KINDS` para los tipos (y las reglas,
    en Rust: biblioteca/src/ia/fallos.rs)."""
    kind, code = _rust.ai_classify(_error_text(e), _status_of(e))
    return kind, code


def _is_down(e: Exception) -> bool:
    """Si el fallo es del servicio y no del mensaje: toca probar el respaldo."""
    return bool(classify_failure(e)[0])


def _hold_for(kind: str) -> float:
    """Cuanto se da por caido un proveedor segun por que ha fallado."""
    return _rust.ai_hold_for(kind)


def _now() -> float:
    return time.time()


def _held_kind(pid: str) -> str:
    """El tipo de fallo por el que `pid` esta dado por caido ahora, o ""."""
    with _status_lock:
        if _down_until.get(pid, 0) <= _now():
            return ""
        return (_problems.get(pid) or {}).get("kind") or "down"


def _note_failure(pid: str, kind: str, code: int | None) -> None:
    """Apunta que `pid` ha fallado por `kind`. El plazo cuenta desde ESTE
    instante, el del fallo, y no desde antes de la llamada: con un tiempo
    limite de 60 s y los reintentos del SDK, un proveedor caido tarda minutos
    en fallar, y una marca puesta con la hora de antes nacia ya caducada (el
    turno siguiente volvia a esperarle)."""
    now = _now()
    with _status_lock:
        _down_until[pid] = now + _hold_for(kind)
        _problems[pid] = {"kind": kind, "code": code, "at": now}


def _note_success(pid: str, purpose: str, via: dict) -> None:
    """Apunta que `pid` ha contestado: ya no esta caido. Y, si era la
    conversacion, que ha sido el quien ha contestado (solo ella: ver
    `_turn`)."""
    global _last_chat_via
    with _status_lock:
        _down_until.pop(pid, None)
        _problems.pop(pid, None)
        if purpose == "chat":
            _last_chat_via = dict(via)
    if purpose == "chat":
        _turn.via = via


def _forget_problem(pid: str) -> None:
    """Olvida lo que se sabia del fallo de `pid`, porque la persona ha
    comprobado que responde o quiere volver a probarlo. Si la ultima respuesta
    del chat fue de un respaldo, tambien se olvida: ya no dice nada de lo que
    pasara en el siguiente mensaje."""
    global _last_chat_via
    with _status_lock:
        _down_until.pop(pid, None)
        _problems.pop(pid, None)
        if _last_chat_via and _last_chat_via.get("fallback"):
            _last_chat_via = None


def status() -> dict:
    """El estado del proveedor para la interfaz (la franja bajo la cabecera del
    chat), sin ningun texto del proveedor:

        {"primary": {"id", "name", "model", "problem": {"kind", "code", "at"} | None},
         "answering": {"id", "name", "model", "fallback", "reason"} | None}

    `primary` es el perfil activo y su modelo de conversacion; `problem`, por
    que no se le esta llamando ahora (tipo, codigo HTTP o None y cuando; solo
    mientras dure la marca: pasado el plazo se vuelve a probar). `answering`,
    quien contesto la ultima respuesta del chat (`reason` es el tipo de fallo
    que provoco el respaldo, o "tools" si el modelo del principal no usa
    herramientas), o None si aun no ha contestado nadie."""
    p = profile()
    problem = None
    with _status_lock:
        if p and _down_until.get(p["id"], 0) > _now():
            hit = _problems.get(p["id"])
            if hit:
                problem = {"kind": hit["kind"], "code": hit["code"], "at": hit["at"]}
        answering = dict(_last_chat_via) if _last_chat_via else None
    return {
        "primary": {
            "id": p["id"] if p else "",
            "name": p["name"] if p else "",
            "model": p["chat_model"] if p else "",
            "problem": problem,
        },
        "answering": answering,
    }


def recheck() -> dict:
    """«Reintentar con el principal»: olvida que el activo fallo (y que un
    respaldo contesto la ultima vez) para que el siguiente mensaje se pruebe
    con el. Devuelve `status()`."""
    p = profile()
    if p:
        _forget_problem(p["id"])
    return status()


def complete(
    messages: list[dict],
    *,
    model: str = "",
    tools=None,
    tool_choice=None,
    response_format=None,
    temperature=None,
    max_tokens=None,
    timeout=None,
    purpose: str = "fast",
    on_text=None,
    cancel=None,
) -> Reply:
    """Una peticion al modelo, con las tolerancias descritas arriba.

    Devuelve un `Reply` con el mensaje entero. Con `on_text` la respuesta se
    pide en trozos y se va llamando con el texto acumulado (vacio si lo que
    parecia respuesta resulto ser el preambulo de una herramienta); `cancel`
    es un Event que corta a medias (levanta `Canceled`).

    Si el proveedor activo esta caido, sin credito o saturado, se prueba con
    los demas configurados (si el respaldo esta activado), cada uno con su
    modelo. Al que falla se le apunta por que (`classify_failure`) y se le da
    por caido un rato, contado desde el instante del fallo: las llamadas
    siguientes van directas al respaldo. `Reply.via` dice quien contesto y, si
    fue un respaldo, en `reason` el tipo de fallo que lo provoco.
    `ToolsUnsupported` si se pidieron herramientas y ningun modelo las admite;
    cualquier otro fallo sale como la excepcion del SDK.
    """
    acc = getattr(_turn, "usage", None)
    if acc is not None:
        acc["requests"] = acc.get("requests", 0) + 1
    active = profile() or _NO_PROFILE
    candidates = [active]
    if providers.fallback_enabled():
        candidates += providers.fallbacks(active["id"])
    errors: list[Exception] = []
    # por que no contesto el elegido: es el `reason` de quien lo sustituya
    cause = ""
    for i, p in enumerate(candidates):
        last = i == len(candidates) - 1
        held = _held_kind(p["id"])
        if held and not last:
            cause = cause or held
            continue
        m = model or (p["chat_model"] if purpose == "chat" else p["model"])
        try:
            reply = _complete_with(
                p,
                m,
                messages,
                tools,
                tool_choice,
                response_format,
                temperature,
                max_tokens,
                timeout,
                purpose,
                on_text,
                cancel,
                active=(i == 0),
                reason=cause if i else "",
            )
        except Canceled:
            raise
        except ToolsUnsupported as e:
            cause = cause or "tools"
            errors.append(e)
        except Exception as e:
            kind, code = classify_failure(e)
            if not kind:
                raise  # es del mensaje, no del servicio
            _note_failure(p["id"], kind, code)
            cause = cause or kind
            log.info(
                "%s no responde (%s: %s); se prueba el respaldo",
                p["name"],
                kind,
                sanitize_provider_message(e, p)[:80],
            )
            errors.append(e)
        else:
            _note_success(p["id"], purpose, reply.via)
            return reply
    # el error que se cuenta es el del proveedor elegido, que es el que
    # el usuario tiene que arreglar
    raise errors[0] if errors else RuntimeError("no hay proveedor de IA configurado")


def _complete_with(
    p,
    model,
    messages,
    tools,
    tool_choice,
    response_format,
    temperature,
    max_tokens,
    timeout,
    purpose,
    on_text,
    cancel,
    active=True,
    reason="",
) -> Reply:
    kwargs: dict = {"model": model, "messages": messages}
    if tools:
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
    if on_text is not None:
        kwargs["stream"] = True
        kwargs["stream_options"] = {"include_usage": True}
    for what in _known_limits(p, model):
        _strip(kwargs, what)

    client = _get_client() if active else _client_for_profile(p)
    key = (p["id"], model)
    if tools and "tools" in _unsupported.get(key, ()):
        raise ToolsUnsupported(model)
    for attempt in range(6):
        streaming = bool(kwargs.get("stream"))
        try:
            raw = client.chat.completions.create(**kwargs)
            try:
                msg, usage = _collect(raw, on_text if streaming else None, cancel)
            except Canceled:
                raise
            except Exception as e:
                # el flujo se rompio a medias (un evento vacio, un corte):
                # la misma peticion, entera y de una vez
                if not streaming or attempt == 5:
                    raise
                n = _stream_failures[key] = _stream_failures.get(key, 0) + 1
                log.info(
                    "%s corto el flujo con %s (%s); se repite sin trozos",
                    p["id"],
                    model,
                    sanitize_provider_message(e, p)[:80],
                )
                if n >= STREAM_FAILURES_MAX:
                    _unsupported.setdefault(key, set()).add("stream")
                _strip(kwargs, "stream")
                if on_text:
                    on_text("")
                continue
            if streaming:
                _stream_failures.pop(key, None)
            _account(p, model, purpose, usage)
            via = {
                "id": p["id"],
                "name": p["name"],
                "model": model,
                "fallback": not active,
                "reason": reason,
            }
            return Reply(msg, usage, via)
        except Canceled:
            raise
        except Exception as e:
            status = getattr(e, "status_code", None)
            text = str(e)
            if status not in (400, 404, 422) and not _rust.ai_retryable_text(text):
                raise
            # Primero lo que se puede quitar (tool_choice=required, JSON…):
            # un «tool_choice not supported» habla del parametro, no de que
            # el modelo no sepa usar herramientas.
            dropped = _first_droppable(kwargs, text)
            if dropped and attempt < 5:
                log.info("%s rechaza %s con %s; se reintenta sin el", p["id"], dropped, model)
                _unsupported.setdefault(key, set()).add(dropped)
                _strip(kwargs, dropped)
                continue
            if tools and _rust.ai_tools_hint(text):
                _unsupported.setdefault(key, set()).add("tools")
                raise ToolsUnsupported(model) from e
            raise
    raise RuntimeError("sin respuesta")  # no se llega: el bucle sale antes


def _thinking_open(text: str) -> bool:
    """Un <think> abierto sin cerrar: lo que hay es razonamiento, no respuesta."""
    return _rust.ai_thinking_open(text)


def _collect(raw, on_text=None, cancel=None):
    """Recibe la respuesta, entera o en trozos, y devuelve (mensaje, uso).

    En trozos, junta el texto y las llamadas a herramientas (que llegan
    partidas por indice), avisa a `on_text` con el texto acumulado —sin el
    razonamiento entre <think>— y, si tras el texto aparece una herramienta,
    avisa con vacio: lo de antes era un preambulo, no la respuesta.
    """
    if hasattr(raw, "choices"):
        return raw.choices[0].message, getattr(raw, "usage", None)
    parts: list[str] = []
    calls: dict[int, dict] = {}
    usage = None
    retracted = False
    try:
        for chunk in raw:
            if cancel is not None and cancel.is_set():
                raise Canceled()
            u = getattr(chunk, "usage", None)
            if u:
                usage = u
            choices = getattr(chunk, "choices", None) or []
            if not choices:
                continue
            delta = getattr(choices[0], "delta", None)
            if delta is None:
                continue
            content = getattr(delta, "content", None)
            if content:
                parts.append(content)
                if on_text and not calls:
                    joined = "".join(parts)
                    on_text("" if _thinking_open(joined) else strip_thoughts(joined))
            for tc in getattr(delta, "tool_calls", None) or []:
                slot = calls.setdefault(
                    getattr(tc, "index", 0) or 0, {"id": "", "name": "", "arguments": ""}
                )
                if getattr(tc, "id", None):
                    slot["id"] = tc.id
                fn = getattr(tc, "function", None)
                if fn is not None:
                    if getattr(fn, "name", None):
                        slot["name"] = fn.name
                    if getattr(fn, "arguments", None):
                        slot["arguments"] += fn.arguments
                if on_text and parts and not retracted:
                    on_text("")
                    retracted = True
    finally:
        close = getattr(raw, "close", None)
        if callable(close):
            try:
                close()
            except Exception:  # noqa: BLE001
                pass
    # una llamada sin argumentos llega con "" y, devuelta asi al servidor en
    # el turno siguiente, es «un documento vacio»: un 400. Siempre JSON.
    tool_calls = [
        SimpleNamespace(
            id=c["id"] or f"call_{i}",
            type="function",
            function=SimpleNamespace(name=c["name"], arguments=c["arguments"] or "{}"),
        )
        for i, c in sorted(calls.items())
    ] or None
    return SimpleNamespace(content="".join(parts), tool_calls=tool_calls), usage


def message_text(msg) -> str:
    """El texto de una respuesta, sin el razonamiento que algunos dejan escapar."""
    text = getattr(msg, "content", None) or ""
    if isinstance(text, list):  # partes (texto + otras)
        text = "".join(
            getattr(x, "text", "") or (x.get("text", "") if isinstance(x, dict) else "")
            for x in text
        )
    return strip_thoughts(text)


def strip_thoughts(t: str) -> str:
    """Algunos modelos dejan escapar su razonamiento entre <think>."""
    return _rust.ai_strip_thoughts(t or "")


# ------------------------------------------------------------------ prueba


# Lo que dice un proveedor en un error no es de fiar: algunos repiten la clave
# entera o por trozos («Incorrect API key provided: sk-…abcd»), las cabeceras o
# la URL con `?key=`. Ese texto acaba en la pantalla, en el registro y en la
# conversacion guardada, asi que antes pasa por `sanitize_provider_message`.
MAX_PROVIDER_MESSAGE = 160


def _known_secrets(p: providers.Profile | None) -> list[str]:
    """Lo que no debe salir NUNCA en un texto: la clave y los valores de las
    cabeceras y de los parametros extra (ahi tambien va un «Authorization» o un
    «api-key») del perfil que se pasa, del activo y de los de respaldo (los
    saca Rust de lo guardado, sin pasar los perfiles por aqui)."""
    try:
        return _rust.ai_known_secrets_store(providers.PROFILES_FILE, dict(p) if p else None)
    except Exception:
        log.debug("no se pudieron leer los perfiles para limpiar un texto", exc_info=True)
        return _rust.ai_known_secrets([dict(p)] if p else [])


def sanitize_provider_message(text: object, p: providers.Profile | None = None) -> str:
    """Lo que dice un proveedor (o una excepcion), apto para enseñarlo y para el
    registro: sin secretos y en UNA linea de 160 caracteres como mucho.

    Quita la clave y los valores de cabeceras y extras del perfil `p` (y del
    activo y los de respaldo), las claves con forma de clave (`sk-…`, `AIza…`),
    `Bearer …`, `clave=valor` y `Authorization: …`; y deja cada URL en su
    servidor, sin `usuario:clave@`, ruta ni parametros. Nunca lanza: esto corre
    justo cuando algo ya ha ido mal.
    """
    if text is None:
        return ""
    try:
        s = text if isinstance(text, str) else _error_text(text)
        return _rust.ai_sanitize(s, _known_secrets(p))
    except Exception:  # noqa: BLE001
        return "error del proveedor"


def describe_error(e: Exception, p: providers.Profile | None = None) -> str:
    """Un fallo del proveedor, en una frase que el usuario entienda."""
    return _rust.ai_describe(
        _error_text(e), _status_of(e), dict(p) if p else None, _known_secrets(p)
    )


def _one_token(client, model: str, p: providers.Profile) -> None:
    """La llamada mas barata posible: un token. Si falla, lanza el error."""
    kwargs = {"model": model, "messages": [{"role": "user", "content": "ok"}], "max_tokens": 1}
    if "temperature" not in _known_limits(p, model):
        kwargs["temperature"] = 0
    try:
        client.chat.completions.create(**kwargs)
    except Exception as e:
        low = str(e).lower()
        if "max_completion_tokens" in low or "temperature" in low:
            kwargs.pop("temperature", None)
            kwargs["max_completion_tokens"] = kwargs.pop("max_tokens", 1)
            client.chat.completions.create(**kwargs)
        else:
            raise


def _ping(client, model: str, p: providers.Profile) -> tuple[bool, str, int]:
    """Un token. Vale para clave, URL y modelo."""
    t0 = time.monotonic()
    try:
        _one_token(client, model, p)
        return True, "", int((time.monotonic() - t0) * 1000)
    except Exception as e:  # noqa: BLE001
        return False, describe_error(e, p), int((time.monotonic() - t0) * 1000)


_PROBE_TOOL = _rust.ai_probe_tool()


def _probe_tools(client, model: str, p: providers.Profile) -> tuple[bool, str]:
    """¿Sabe llamar a una herramienta? Es lo que necesita el asistente."""
    kwargs = {
        "model": model,
        "tools": _PROBE_TOOL,
        "tool_choice": "auto",
        "max_tokens": 60,
        "messages": [{"role": "user", "content": "Saluda a Dani usando la herramienta."}],
    }
    for what in _known_limits(p, model):
        _strip(kwargs, what)
    waited = 0
    for _ in range(4):
        try:
            r = client.chat.completions.create(**kwargs)
            msg = r.choices[0].message
            if getattr(msg, "tool_calls", None):
                return True, ""
            return False, (
                "responde sin llamar a la herramienta: el asistente puede fallar con este modelo"
            )
        except Exception as e:  # noqa: BLE001
            text = str(e)
            # Los gratuitos admiten una peticion por segundo y esta va justo
            # detras de la prueba de la clave: se espera y se repite.
            if (
                getattr(e, "status_code", None) == 429
                or "rate limit" in text.lower()
                or "too many" in text.lower()
            ) and waited < 2:
                waited += 1
                time.sleep(2.5)
                continue
            dropped = _first_droppable(kwargs, text)
            if dropped:
                _strip(kwargs, dropped)
                continue
            if _rust.ai_tools_hint(text) or _rust.ai_mentions_tools(text):
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
    if p is None or not ok:
        return {"ok": False, "reason": why}
    try:
        client = _build_client(p)
    except Exception as e:  # noqa: BLE001
        return {"ok": False, "reason": describe_error(e, p)}
    ok, reason, ms = _ping(client, p["model"], p)
    out = {
        "ok": ok,
        "reason": reason,
        "provider": p["name"],
        "model": p["model"],
        "chat_model": p["chat_model"],
        "latency_ms": ms,
    }
    if not ok:
        return out
    if p["chat_model"] != p["model"]:
        ok2, reason2, _ = _ping(client, p["chat_model"], p)
        if not ok2:
            out.update(ok=False, reason=f"modelo de conversacion: {reason2}")
            return out
    tools_ok, tools_reason = _probe_tools(client, p["chat_model"], p)
    out.update(tools_ok=tools_ok, tools_reason=tools_reason)
    if draft is None:
        # el activo responde: lo que se sabia de su fallo ya no vale
        _forget_problem(p["id"])
    return out


def list_models(draft: dict | None = None, recheck: bool = False) -> dict:
    """Los modelos que se pueden usar con ESA clave, cruzados con el catalogo
    (precio, herramientas, obsoleto). `draft` como en `check`.

    Lo que lista el proveedor no basta: se quitan los que no conversan (voz,
    imagen, embeddings) y, con clave, se prueba cada uno (`_usable`). Los
    que no contestan van aparte, en `hidden`, con el motivo. `recheck`
    vuelve a probarlos aunque se sepa de hace poco.
    """
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
    except Exception as e:  # noqa: BLE001
        return {"ok": False, "reason": describe_error(e, p), "models": []}
    # sin clave, solo lo que el servicio sirve a anonimos (LLM7 lo llama
    # «turbo», Kilo marca `isFree`, OpenCode Zen termina en «-free»)
    anon = p["quirks"].get("anon_filter") if not p["key"] else None
    prefix = p["quirks"].get("strip_prefix") or ""
    models = []
    seen = set()
    for m in raw:
        mid = getattr(m, "id", None) or (m.get("id") if isinstance(m, dict) else None)
        if not mid:
            continue
        mid = str(mid)
        if prefix and mid.startswith(prefix):
            mid = mid[len(prefix) :]
        if mid in seen:
            continue
        seen.add(mid)
        extra: dict = {}
        for getter in ("to_dict", "model_dump"):
            fn = getattr(m, getter, None)
            if callable(fn):
                try:
                    got = fn()
                except Exception:  # noqa: BLE001
                    continue
                extra = got if isinstance(got, dict) else {}
                break
        if anon and not _anonymous_ok(mid, extra, anon):
            continue
        if not model_catalog.is_chat(mid, str(extra.get("name") or "")):
            continue
        models.append(_merge(mid, extra, p))
    models, hidden, verified = _usable(models, p, recheck)
    models.sort(key=lambda x: (x["released"] or "", x["id"]), reverse=True)
    return {
        "ok": True,
        "models": models,
        "hidden": hidden,
        "verified": verified,
        "source": "provider",
        "provider": p["name"],
    }


# ------------------------------------------------- que modelos sirven de verdad

# Lo que un proveedor lista no es lo que tu clave puede usar. Google enseña
# a una clave gratuita sesenta modelos, y una buena parte contesta 404 («no
# longer available to new users»), 429 con «limit: 0» (tu plan no tiene
# cuota para ese modelo) o 400 («only supports Interactions API»). Asi que
# se prueba cada uno con un token y se enseñan los que contestan.
#
# Lo probado se recuerda un dia en disco, por proveedor y clave (la clave no
# se guarda, solo una huella): abrir el modal no gasta cuota cada vez, que
# en los niveles gratuitos se cuenta por peticiones al dia.
VERIFIED_FILE = config.DATA_DIR / "models-verified.json"
VERIFY_TTL = 24 * 3600
# el que no contesto (saturado, lento) se enseña, y se vuelve a probar pronto
UNSURE_TTL = 15 * 60
# OpenRouter o DeepInfra listan cientos: probarlos todos cuesta dinero y
# minutos, y lo que listan con clave es lo que se puede pagar. Ahi no.
VERIFY_MAX = 80
VERIFY_WORKERS = 8
VERIFY_TIMEOUT = 12

_verified: dict[str, dict] | None = None
_verified_lock = threading.Lock()


def _verdict(e: Exception) -> tuple[bool | None, str]:
    """Que dice un fallo de la prueba: True, se puede usar (solo esta
    ocupado); False, con esta clave no; None, no se sabe (caido, sin red)."""
    status = getattr(e, "status_code", None)
    if status is not None and not (isinstance(status, int) and not isinstance(status, bool)):
        status = -1  # algo que no es un codigo: ni ese ni ninguno
    return _rust.ai_verdict(status, str(e), _known_secrets(None))


def _gone_reason(text: str) -> str:
    """Por que no sirve, en español si se reconoce; si no, el «message» del
    error del proveedor sin el resto del volcado."""
    return _rust.ai_gone_reason(text, _known_secrets(None))


def _verify_key(p: providers.Profile, model: str) -> str:
    return _rust.ai_verify_key(p["base_url"], p["key"], model)


def _load_verified() -> dict[str, dict]:
    global _verified
    if _verified is None:
        try:
            d = json.loads(VERIFIED_FILE.read_text(encoding="utf-8"))
            _verified = d if isinstance(d, dict) else {}
        except (OSError, ValueError):
            _verified = {}
    return _verified


def _save_verified() -> None:
    now = time.time()
    with _verified_lock:
        d = {k: v for k, v in _load_verified().items() if now - v.get("at", 0) < VERIFY_TTL}
    try:
        VERIFIED_FILE.parent.mkdir(parents=True, exist_ok=True)
        tmp = VERIFIED_FILE.with_suffix(".tmp")
        tmp.write_text(json.dumps(d), encoding="utf-8")
        tmp.replace(VERIFIED_FILE)
    except OSError:
        log.warning("no se pudo guardar que modelos funcionan", exc_info=True)


def forget_verified() -> None:
    """Olvida lo probado (para las pruebas)."""
    global _verified
    with _verified_lock:
        _verified = None


def _usable(models: list[dict], p: providers.Profile, recheck: bool = False):
    """Separa los que contestan de los que no. Devuelve (los que sirven,
    los ocultos con su motivo, si se probaron).

    Sin clave no se prueba (los gratuitos admiten una peticion por segundo y
    ya se filtran por lo que sirven a anonimos), ni en un servidor local (lo
    que lista es lo descargado, y probar cargaria cada modelo en memoria),
    ni con listas enormes.
    """
    if not models or p.get("local") or not p["key"] or len(models) > VERIFY_MAX:
        return models, [], False
    now = time.time()
    with _verified_lock:
        known = _load_verified()
        cached = {
            m["id"]: known[k]
            for m in models
            if (k := _verify_key(p, m["id"])) in known
            and now - known[k].get("at", 0)
            < (UNSURE_TTL if known[k].get("ok") is None else VERIFY_TTL)
            and not recheck
        }
    todo = [m["id"] for m in models if m["id"] not in cached]
    found: dict[str, tuple[bool | None, str]] = {}
    if todo:
        client = _build_client(
            {**p, "retries": 0, "timeout": min(float(p["timeout"]), VERIFY_TIMEOUT)}  # type: ignore[typeddict-item]
        )

        def probe(mid: str) -> tuple[str, bool | None, str]:
            try:
                _one_token(client, mid, p)
                return mid, True, ""
            except Exception as e:  # noqa: BLE001
                return mid, *_verdict(e)

        with ThreadPoolExecutor(max_workers=VERIFY_WORKERS) as pool:
            for mid, ok, why in pool.map(probe, todo):
                found[mid] = (ok, why)
        with _verified_lock:
            known = _load_verified()
            for mid, (ok, why) in found.items():
                # «ocupado» no se recuerda (mañana no lo estara); el que no
                # contesto, un rato, para no esperarlo en cada apertura
                known[_verify_key(p, mid)] = {"ok": ok, "why": "" if ok else why, "at": now}
        _save_verified()
    usable, hidden = [], []
    for m in models:
        if m["id"] in cached:
            ok, why = cached[m["id"]]["ok"], cached[m["id"]].get("why", "")
        else:
            ok, why = found[m["id"]]
        if ok is False:
            hidden.append({"id": m["id"], "name": m["name"], "reason": why})
            continue
        if why:
            # ocupado o sin respuesta: se puede elegir, pero se avisa
            m = {**m, "note": why}
        usable.append(m)
    return usable, hidden, True


def _anonymous_ok(mid: str, extra: dict, rule: dict) -> bool:
    return _rust.ai_anonymous_ok(mid, _jsonable(extra), dict(rule))


def _jsonable(value):
    """Lo que trae el /models de un proveedor, en tipos de JSON (un SDK puede
    meter fechas u otros objetos: van como texto)."""
    try:
        return json.loads(json.dumps(value, default=str))
    except (TypeError, ValueError):
        return {}


def _merge(mid: str, extra: dict, p: providers.Profile) -> dict:
    """La ficha de un modelo: lo que dice el proveedor mas lo que sabe el
    catalogo. OpenRouter cuenta precio y parametros en su propio /models."""
    info = model_catalog.lookup(p.get("models_dev"), mid)
    return _rust.ai_merge(mid, _jsonable(extra), info)


def try_free(order=None, timeout: float = 20) -> dict:
    """«Probar gratis, sin clave»: el primero de los gratuitos que responda
    de verdad se guarda y se activa. Devuelve cual, o por que ninguno."""
    tried = []
    for pid in order or providers.FREE_ORDER:
        entry = providers.BY_ID.get(pid)
        if not entry:
            continue
        draft = {"id": pid, "provider": pid, "timeout": timeout, "retries": 0}
        r = check(draft)
        tried.append(
            {
                "id": pid,
                "name": entry["name"],
                "ok": bool(r.get("ok")),
                "reason": r.get("reason") or r.get("tools_reason") or "",
                "tools_ok": r.get("tools_ok"),
            }
        )
        if r.get("ok"):
            providers.save_profile(
                {
                    "id": pid,
                    "provider": pid,
                    "model": r["model"],
                    "chat_model": r["chat_model"],
                    "timeout": timeout,
                }
            )
            reset_client()
            return {
                "ok": True,
                "chosen": pid,
                "name": entry["name"],
                "model": r["model"],
                "chat_model": r["chat_model"],
                "tools_ok": r.get("tools_ok"),
                "tried": tried,
            }
    return {
        "ok": False,
        "chosen": "",
        "tried": tried,
        "reason": "ahora mismo ninguno de los servicios gratuitos responde; prueba mas "
        "tarde, pon una clave o usa un modelo en tu equipo",
    }


# ----------------------------------------------------------------- consultas

# La forma exacta de lo que devuelve la identificacion. Con un modelo que
# admite salida con esquema (lo dice el catalogo) se le exige; con los demas
# se pide «un objeto JSON» y se extrae del texto.
SONG_SCHEMA = _rust.ai_song_schema()


def json_format(schema: dict | None = None) -> dict:
    """El `response_format` mas fuerte que admite el modelo activo."""
    p = profile()
    info = model_catalog.lookup(p.get("models_dev"), p["model"]) if schema and p else None
    return _rust.ai_json_format(schema, info)


def _json_from(text: str) -> dict | None:
    """El objeto JSON de una respuesta, o None.

    Solo un OBJETO: un modelo puede contestar una lista, una cadena o `null`
    (JSON valido las tres), y quien llama hace `.get` sobre lo que recibe;
    eso era un 500 en la ficha y en «Completar con IA».
    """
    return _rust.ai_json_from(text or "")


def resolve(file_name: str, known_artists=None, extra_hint="") -> dict | None:
    """Deduce artista/titulo de un nombre sucio. None si la IA no esta disponible."""
    if not available():
        return None
    context = ""
    if known_artists:
        sample = sorted(known_artists)[:120]
        context = (
            "\nArtistas que YA existen en la biblioteca (reutiliza el nombre exacto "
            "si la cancion es de alguno):\n" + ", ".join(sample)
        )
    user_msg = f"Nombre de archivo:\n{file_name}\n{extra_hint}{context}"
    try:
        r = complete(
            [{"role": "system", "content": INSTRUCTIONS}, {"role": "user", "content": user_msg}],
            temperature=0.1,
            max_tokens=400,
            response_format=json_format(SONG_SCHEMA),
            purpose="identify",
        )
        data = _json_from(message_text(r.message))
    except Exception as e:  # noqa: BLE001
        return {"error": describe_error(e, profile())}
    if not data:
        return None
    return _rust.ai_identified(data)


def ask(
    prompt,
    system="Eres un asistente musical preciso y conciso.",
    max_tokens=800,
    temperature=0.2,
    purpose="ask",
) -> str | None:
    """Consulta libre en texto plano."""
    if not available():
        return None
    try:
        r = complete(
            [{"role": "system", "content": system}, {"role": "user", "content": prompt}],
            temperature=temperature,
            max_tokens=max_tokens,
            purpose=purpose,
        )
        return message_text(r.message)
    except Exception as e:  # noqa: BLE001
        # sin `exc_info`: el traceback repite el texto del proveedor, con lo que
        # lleve dentro (ver `sanitize_provider_message`)
        log.info(
            "la consulta a la IA fallo (%s): %s", type(e).__name__, sanitize_provider_message(e)
        )
        return None


def ask_json(
    prompt,
    system="Responde solo con JSON valido.",
    max_tokens=1200,
    temperature=0.15,
    schema: dict | None = None,
    purpose="ask_json",
) -> dict | None:
    if not available():
        return None
    try:
        r = complete(
            [{"role": "system", "content": system}, {"role": "user", "content": prompt}],
            temperature=temperature,
            max_tokens=max_tokens,
            response_format=json_format(schema),
            purpose=purpose,
        )
        return _json_from(message_text(r.message))
    except Exception as e:  # noqa: BLE001
        return {"error": describe_error(e, profile())}
