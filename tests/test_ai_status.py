"""El estado del proveedor de IA: por que falla, quien contesta de verdad y lo
que de eso se ve desde fuera (la franja del chat).

Los errores no son un doble con el texto que a cada prueba le viene bien: son
las excepciones del SDK de openai, hechas por su propio constructor
(`Error code: 400 - {...}`), con los textos que mandan de verdad Anthropic,
OpenAI, Gemini y OpenRouter. Los proveedores son clientes de mentira y la hora
es un reloj de mentira: nada sale a la red ni espera de verdad.
"""

import json
import logging
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from types import SimpleNamespace
from typing import Any

import httpx
import openai
import pytest

from danplay import ai, config, providers

# ---------------------------------------------------- los errores, de verdad
# El SDK 3.x habla con `httpx2`; para fabricar la respuesta basta lo que tiene
# de `httpx` (el SDK no comprueba el tipo): por eso `Any`.
_REQUEST: Any = httpx.Request("POST", "https://api.example.invalid/v1/chat/completions")
_SDK: Any = None


def sdk_error(status: int, body: Any) -> openai.APIStatusError:
    """La excepcion que lanza el SDK ante esa respuesta HTTP: la misma clase y
    el mismo `str(e)` («Error code: 400 - {...}»), porque la hace su propio
    constructor. Un cuerpo `str` es una respuesta que no es JSON (una pagina
    de error de un proxy)."""
    global _SDK
    if _SDK is None:
        _SDK = openai.OpenAI(api_key="no-key", base_url="http://127.0.0.1:9/v1")
    if isinstance(body, str):
        response: Any = httpx.Response(status, text=body, request=_REQUEST)
    else:
        response = httpx.Response(status, json=body, request=_REQUEST)
    return _SDK._make_status_error_from_response(response)


# Anthropic (su capa compatible con OpenAI) sin saldo: el caso del usuario
ANTHROPIC_SIN_SALDO = {
    "error": {
        "code": "invalid_request_error",
        "message": "Your credit balance is too low to access the Anthropic API. "
        "Please go to Plans & Billing to upgrade or purchase credits.",
        "type": "invalid_request_error",
        "param": None,
    }
}
ANTHROPIC_401 = {
    "type": "error",
    "error": {"type": "authentication_error", "message": "invalid x-api-key"},
}
ANTHROPIC_529 = {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}
# OpenAI: sin saldo (con un 429, que parece «espera») y limite de peticiones
OPENAI_SIN_CUOTA = {
    "error": {
        "message": "You exceeded your current quota, please check your plan and billing "
        "details. For more information on this error, read the docs: "
        "https://platform.openai.com/docs/guides/error-codes/api-errors.",
        "type": "insufficient_quota",
        "param": None,
        "code": "insufficient_quota",
    }
}
OPENAI_LIMITE = {
    "error": {
        "message": "Rate limit reached for gpt-5.6-luna in organization org-PruebaPrueba on "
        "requests per min (RPM): Limit 3, Used 3, Requested 1. Please try again in 20s. "
        "Visit https://platform.openai.com/account/rate-limits to learn more.",
        "type": "requests",
        "param": None,
        "code": "rate_limit_exceeded",
    }
}
OPENAI_CLAVE_MALA = {
    "error": {
        "message": "Incorrect API key provided: sk-proj-********************wxyz. You can find "
        "your API key at https://platform.openai.com/account/api-keys.",
        "type": "invalid_request_error",
        "param": None,
        "code": "invalid_api_key",
    }
}
OPENAI_500 = {
    "error": {
        "message": "The server had an error while processing your request. Sorry about that!",
        "type": "server_error",
        "param": None,
        "code": None,
    }
}
# Gemini: el limite por minuto (que pasa solo) con «billing» en el texto, y el
# plan sin cuota para ese modelo (`limit: 0`, que no pasa)
GEMINI_POR_MINUTO = [
    {
        "error": {
            "code": 429,
            "message": "Quota exceeded for quota metric 'Generate Content API requests per "
            "minute' and limit 'GenerateContent request limit per minute for a project' of "
            "service 'generativelanguage.googleapis.com' for consumer "
            "'project_number:123456789012'.",
            "status": "RESOURCE_EXHAUSTED",
        }
    }
]
GEMINI_GRATIS = [
    {
        "error": {
            "code": 429,
            "message": "You exceeded your current quota, please check your plan and billing "
            "details. For more information on this error, head to: "
            "https://ai.google.dev/gemini-api/docs/rate-limits.\n* Quota exceeded for metric: "
            "generativelanguage.googleapis.com/generate_content_free_tier_requests, limit: 15, "
            "model: gemini-2.5-flash\nPlease retry in 21.3s.",
            "status": "RESOURCE_EXHAUSTED",
        }
    }
]
GEMINI_SIN_CUOTA = [
    {
        "error": {
            "code": 429,
            "message": "You exceeded your current quota, please check your plan and billing "
            "details.\n* Quota exceeded for metric: "
            "generativelanguage.googleapis.com/generate_content_free_tier_requests, limit: 0, "
            "model: gemini-2.5-pro\nPlease retry in 2.1s.",
            "status": "RESOURCE_EXHAUSTED",
        }
    }
]
GEMINI_CLAVE_400 = [
    {
        "error": {
            "code": 400,
            "message": "API key not valid. Please pass a valid API key.",
            "status": "INVALID_ARGUMENT",
            "details": [{"reason": "API_KEY_INVALID", "domain": "googleapis.com"}],
        }
    }
]
GOOGLE_CLAVE_FILTRADA = [
    {
        "error": {
            "code": 403,
            "message": "Your API key was reported as leaked. Please use another API key.",
            "status": "PERMISSION_DENIED",
        }
    }
]
# OpenRouter: modera UN mensaje (403: no es la clave) y se queda sin creditos
OPENROUTER_MODERACION = {
    "error": {
        "message": "Your chosen model requires moderation and your input was flagged as "
        "violating the provider's content policy.",
        "code": 403,
        "metadata": {
            "reasons": ["harassment"],
            "flagged_input": "texto de ejemplo marcado",
            "provider_name": "ProveedorEjemplo",
            "model_slug": "proveedor/modelo",
        },
    }
}
OPENROUTER_402 = {
    "error": {
        "message": "Insufficient credits. Add more using https://openrouter.ai/credits",
        "code": 402,
    }
}
# otros: xAI sin creditos (con un 403 que habla de permisos), Groq, DeepSeek, Mistral
XAI_SIN_CREDITOS = {
    "code": "The caller does not have permission to execute the specified operation",
    "error": "Your team 00000000-test has either used all available credits or reached its "
    "monthly spending limit. To continue making API requests, please purchase more credits "
    "or raise your spending limit.",
}
GROQ_LIMITE = {
    "error": {
        "message": "Rate limit reached for model `llama-3.3-70b-versatile` in organization "
        "`org_prueba` service tier `on_demand` on tokens per minute (TPM): Limit 12000, Used "
        "11834, Requested 1455. Please try again in 3.5s.",
        "type": "tokens",
        "code": "rate_limit_exceeded",
    }
}
DEEPSEEK_402 = {
    "error": {
        "message": "Insufficient Balance",
        "type": "unknown_error",
        "param": None,
        "code": "invalid_request_error",
    }
}
MISTRAL_401 = {"message": "Unauthorized", "request_id": "00000000000000000000000000000000"}
ANTHROPIC_403 = {
    "type": "error",
    "error": {
        "type": "permission_error",
        "message": "Your API key does not have permission to use the specified resource.",
    },
}
OPENAI_CUENTA_INACTIVA = {
    "error": {
        "message": "Your account is not active, please check your billing details on our website.",
        "type": "billing_not_active",
        "param": None,
        "code": "billing_not_active",
    }
}
JSON_MAL = {
    "error": {
        "message": "Invalid JSON: unbalanced braces near a quotation mark in the request body.",
        "type": "invalid_request_error",
    }
}
SERVICIO_503 = {
    "error": {
        "message": "The model is overloaded. Please try again later.",
        "code": 503,
        "status": "UNAVAILABLE",
    }
}
PAGINA_502 = "<html><body><h1>502 Bad Gateway</h1></body></html>"
# fallos DEL MENSAJE: el respaldo no los arregla
MENSAJE_VACIO = {
    "error": {
        "message": "messages must not be empty",
        "type": "invalid_request_error",
        "param": "messages",
        "code": None,
    }
}
MODELO_NO_EXISTE = {
    "error": {
        "message": "The model `gpt-x` does not exist or you do not have access to it.",
        "type": "invalid_request_error",
        "param": None,
        "code": "model_not_found",
    }
}

# secretos falsos (y nada de nombres reales de nadie)
CLAVE = "zz-clave-falsa-0123456789abcdef"
CLAVE_RESPALDO = "zz-clave-del-respaldo-9876543210"
CABECERA = "valor-secreto-de-cabecera-77"
EXTRA = "valor-secreto-de-extra-8899"


# ------------------------------------------------------------ los dobles


class _Clock:
    """La hora, a mano."""

    def __init__(self, now: float = 1_000_000.0):
        self.now = now

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float) -> None:
        self.now += seconds


class _Fake:
    """Un proveedor de mentira. Cada llamada pasa por `behave(kwargs)`: si
    devuelve texto, contesta; si lanza, falla; si devuelve otra cosa (un flujo
    roto), se entrega tal cual. `usage` hace que cada respuesta traiga lo que
    gasto, como las de verdad."""

    def __init__(self, behave, usage: bool = False):
        self.behave = behave
        self.calls: list[dict] = []
        outer = self

        class _Completions:
            @staticmethod
            def create(**kw):
                outer.calls.append(kw)
                out = outer.behave(kw)
                if not isinstance(out, str):
                    return out
                reply = SimpleNamespace(
                    choices=[SimpleNamespace(message=SimpleNamespace(content=out, tool_calls=None))]
                )
                if usage:
                    reply.usage = SimpleNamespace(prompt_tokens=10, completion_tokens=5)
                return reply

        self.chat = SimpleNamespace(completions=_Completions)


class _FlujoRoto:
    """Un flujo que se corta nada mas empezar, con un mensaje."""

    def __init__(self, text: str):
        self.text = text

    def __iter__(self):
        raise RuntimeError(self.text)


def fails(status: int, body: Any):
    def behave(kw):
        raise sdk_error(status, body)

    return behave


def answers(text: str):
    return lambda kw: text


def server(monkeypatch, **fakes: _Fake) -> None:
    """Cada perfil habla con su cliente de mentira."""
    monkeypatch.setattr(ai, "_build_client", lambda p: fakes[p["id"]])


MSG = [{"role": "user", "content": "hola"}]


def chat_call(**kw):
    return ai.complete(MSG, purpose="chat", **kw)


@pytest.fixture
def perfiles(tmp_path, monkeypatch):
    """Perfiles en un archivo temporal, sin variables de entorno que manden y la
    base del uso de la IA en un temporal."""
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
    monkeypatch.setattr(config, "DATABASE", tmp_path / "uso.db")
    monkeypatch.setattr(ai, "VERIFIED_FILE", tmp_path / "models-verified.json")
    ai.forget_verified()
    providers.reload()
    ai.reset_client()
    yield
    providers.reload()
    ai.reset_client()
    ai.forget_verified()
    ai._stream_failures.clear()


@pytest.fixture
def reloj(perfiles, monkeypatch):
    """La hora, a mano: solo avanza si la prueba se lo dice."""
    clock = _Clock()
    monkeypatch.setattr(ai, "_now", clock)
    return clock


def two_providers(extra: dict | None = None, headers: dict | None = None) -> None:
    """Anthropic de principal (el del usuario) y DeepInfra de respaldo."""
    providers.save_profile(
        {
            "provider": "anthropic",
            "key": CLAVE,
            "model": "claude-sonnet-5-5",
            "chat_model": "claude-sonnet-5-5",
            "extra": extra or {},
            "headers": headers or {},
        }
    )
    providers.save_profile(
        {
            "provider": "deepinfra",
            "key": CLAVE_RESPALDO,
            "model": "google/gemini-3.1-flash-lite",
            "chat_model": "Qwen/Qwen3-Next-80B-A3B-Instruct",
        },
        activate=False,
    )
    providers.activate("anthropic")


@pytest.fixture
def api_client():
    from fastapi.testclient import TestClient

    from danplay import api

    return TestClient(api.app)


# ---------------------------------------------------- clasificar un fallo

CASOS = [
    pytest.param(400, ANTHROPIC_SIN_SALDO, "billing", id="anthropic-400-credit-balance"),
    pytest.param(429, OPENAI_SIN_CUOTA, "billing", id="openai-429-insufficient_quota"),
    pytest.param(429, OPENAI_LIMITE, "rate", id="openai-429-limite-de-peticiones"),
    pytest.param(429, GEMINI_POR_MINUTO, "rate", id="gemini-429-quota-exceeded-per-minute"),
    pytest.param(429, GEMINI_GRATIS, "rate", id="gemini-429-con-billing-en-el-texto"),
    pytest.param(429, GEMINI_SIN_CUOTA, "billing", id="gemini-429-limit-0"),
    pytest.param(429, OPENAI_CUENTA_INACTIVA, "billing", id="openai-429-billing_not_active"),
    pytest.param(402, OPENROUTER_402, "billing", id="openrouter-402"),
    pytest.param(403, OPENROUTER_MODERACION, "down", id="openrouter-403-moderacion"),
    pytest.param(401, OPENAI_CLAVE_MALA, "auth", id="openai-401-clave"),
    pytest.param(401, ANTHROPIC_401, "auth", id="anthropic-401-x-api-key"),
    pytest.param(403, GOOGLE_CLAVE_FILTRADA, "auth", id="google-403-clave-filtrada"),
    pytest.param(400, GEMINI_CLAVE_400, "auth", id="gemini-400-api-key-not-valid"),
    pytest.param(403, XAI_SIN_CREDITOS, "billing", id="xai-403-creditos-agotados"),
    pytest.param(429, GROQ_LIMITE, "rate", id="groq-429-tokens-por-minuto"),
    pytest.param(402, DEEPSEEK_402, "billing", id="deepseek-402-insufficient-balance"),
    pytest.param(401, MISTRAL_401, "auth", id="mistral-401"),
    pytest.param(403, ANTHROPIC_403, "auth", id="anthropic-403-permission_error"),
    pytest.param(500, OPENAI_500, "down", id="500"),
    pytest.param(502, PAGINA_502, "down", id="502-pagina-html"),
    pytest.param(503, SERVICIO_503, "down", id="503-saturado"),
    pytest.param(529, ANTHROPIC_529, "down", id="529-overloaded"),
    pytest.param(400, MENSAJE_VACIO, "", id="400-del-mensaje"),
    pytest.param(400, JSON_MAL, "", id="400-con-palabras-que-parecen-de-dinero"),
    pytest.param(404, MODELO_NO_EXISTE, "", id="404-modelo-inexistente"),
]


@pytest.mark.parametrize(("status", "body", "kind"), CASOS)
def test_cada_error_real_se_clasifica_por_su_texto_antes_que_por_su_codigo(status, body, kind):
    e = sdk_error(status, body)
    assert isinstance(e, openai.APIStatusError) and e.status_code == status
    assert ai.classify_failure(e) == (kind, status)
    assert ai._is_down(e) is bool(kind), "solo lo del servicio manda al respaldo"


def test_el_tiempo_agotado_y_la_conexion_caida_son_down_y_sin_codigo():
    timeout = openai.APITimeoutError(request=_REQUEST)
    conexion = openai.APIConnectionError(message="Connection error.", request=_REQUEST)
    assert str(timeout) == "Request timed out." and str(conexion) == "Connection error."
    assert ai.classify_failure(timeout) == ("down", None)
    assert ai.classify_failure(conexion) == ("down", None)
    # y los que no son del SDK
    assert ai.classify_failure(TimeoutError("timed out")) == ("down", None)
    assert ai.classify_failure(ConnectionRefusedError("Connection refused")) == ("down", None)
    assert ai.classify_failure(ValueError("el modelo devolvio algo raro")) == ("", None)


def test_el_codigo_tambien_se_lee_del_texto_si_la_excepcion_no_lo_trae():
    assert ai.classify_failure(Exception("Error code: 503 - service unavailable")) == ("down", 503)
    assert ai.classify_failure(Exception("429 Too Many Requests")) == ("rate", 429)
    assert ai.classify_failure(Exception("402 Payment Required")) == ("billing", 402)
    assert ai.classify_failure(Exception("500ms de espera")) == ("", None), "no es un codigo"


@pytest.mark.parametrize(
    ("status", "body", "kind", "hold"),
    [
        pytest.param(400, ANTHROPIC_SIN_SALDO, "billing", 20 * 60, id="sin-saldo-20-min"),
        pytest.param(401, OPENAI_CLAVE_MALA, "auth", 20 * 60, id="clave-20-min"),
        pytest.param(429, GEMINI_POR_MINUTO, "rate", 120, id="limite-2-min"),
        pytest.param(503, SERVICIO_503, "down", 60, id="caido-60-s"),
        pytest.param(403, OPENROUTER_MODERACION, "down", 60, id="403-sin-motivo-60-s"),
    ],
)
def test_cuanto_se_da_por_caido_depende_de_por_que_fallo(
    reloj, monkeypatch, status, body, kind, hold
):
    two_providers()
    principal = _Fake(fails(status, body))
    server(monkeypatch, anthropic=principal, deepinfra=_Fake(answers("desde el respaldo")))
    r = chat_call()
    assert ai.message_text(r.message) == "desde el respaldo"
    assert ai._down_until["anthropic"] == reloj() + hold
    assert ai.status()["primary"]["problem"] == {"kind": kind, "code": status, "at": reloj()}
    assert r.via["fallback"] and r.via["reason"] == kind


# --------------------------------------------- la marca nace al fallar


def test_la_marca_nace_al_fallar_y_no_antes_de_la_llamada(reloj, monkeypatch):
    """Con un tiempo limite de 60 s y los reintentos del SDK, un proveedor
    caido tarda minutos en fallar. La marca se ponia con la hora de ANTES de
    la llamada y nacia ya caducada: el turno siguiente volvia a esperarle."""
    two_providers()

    def tarda_y_cae(kw):
        reloj.advance(200)  # el tiempo limite y los reintentos del SDK
        raise sdk_error(503, SERVICIO_503)

    principal = _Fake(tarda_y_cae)
    server(monkeypatch, anthropic=principal, deepinfra=_Fake(answers("desde el respaldo")))
    r = chat_call()
    assert r.via["fallback"]
    fallo = reloj()  # el instante en que acabo de fallar
    assert ai._down_until["anthropic"] == fallo + ai.DOWN_FOR
    assert ai._down_until["anthropic"] > reloj(), "no nace caducada"
    chat_call()
    assert len(principal.calls) == 1, "el segundo turno va directo al respaldo"


def test_sin_saldo_el_segundo_turno_va_directo_al_respaldo_hasta_pasados_20_minutos(
    reloj, monkeypatch
):
    """El caso del usuario: Anthropic sin saldo contestaba el respaldo en TODOS
    los mensajes, y cada uno perdia ademas una llamada fallida."""
    two_providers()
    principal = _Fake(fails(400, ANTHROPIC_SIN_SALDO))
    respaldo = _Fake(answers("hola, soy el respaldo"))
    server(monkeypatch, anthropic=principal, deepinfra=respaldo)

    r1 = chat_call()
    assert len(principal.calls) == 1 and len(respaldo.calls) == 1
    assert r1.via == {
        "id": "deepinfra",
        "name": providers.BY_ID["deepinfra"]["name"],
        "model": "Qwen/Qwen3-Next-80B-A3B-Instruct",
        "fallback": True,
        "reason": "billing",
    }
    reloj.advance(30)
    r2 = chat_call()
    assert len(principal.calls) == 1, "ni se le llama: ya se sabe que no tiene saldo"
    assert r2.via["fallback"] and r2.via["reason"] == "billing", "lo dice la marca"
    reloj.advance(19 * 60)  # 19 min y 30 s desde el fallo
    chat_call()
    assert len(principal.calls) == 1
    reloj.advance(60)  # ya pasaron los 20 minutos: se vuelve a probar
    assert ai.status()["primary"]["problem"] is None, "la marca caduco: no se afirma nada"
    chat_call()
    assert len(principal.calls) == 2
    assert ai.status()["primary"]["problem"]["kind"] == "billing", (
        "y como sigue igual, se vuelve a marcar"
    )


def test_cuando_el_principal_vuelve_a_contestar_se_olvida_el_fallo(reloj, monkeypatch):
    two_providers()
    principal = _Fake(fails(429, GEMINI_POR_MINUTO))
    server(monkeypatch, anthropic=principal, deepinfra=_Fake(answers("respaldo")))
    chat_call()
    assert ai.status()["primary"]["problem"]["kind"] == "rate"
    reloj.advance(121)
    principal.behave = answers("ya estoy")
    r = chat_call()
    assert not r.via["fallback"] and r.via["reason"] == ""
    st = ai.status()
    assert st["primary"]["problem"] is None and not st["answering"]["fallback"]
    assert "anthropic" not in ai._down_until and "anthropic" not in ai._problems


def test_un_error_del_mensaje_no_marca_a_nadie(perfiles, monkeypatch):
    two_providers()
    principal = _Fake(fails(400, MENSAJE_VACIO))
    respaldo = _Fake(answers("no deberia llegar"))
    server(monkeypatch, anthropic=principal, deepinfra=respaldo)
    with pytest.raises(openai.BadRequestError):
        chat_call()
    assert not respaldo.calls and not ai._down_until and not ai._problems
    assert ai.status()["primary"]["problem"] is None


def test_sin_respaldo_el_principal_se_sigue_llamando_pero_el_problema_se_ve(reloj, monkeypatch):
    two_providers()
    providers.set_fallback(False)
    principal = _Fake(fails(400, ANTHROPIC_SIN_SALDO))
    server(monkeypatch, anthropic=principal)
    for _ in range(2):
        with pytest.raises(openai.BadRequestError):
            chat_call()
    assert len(principal.calls) == 2, "es el unico: se prueba siempre"
    st = ai.status()
    assert st["primary"]["problem"] == {"kind": "billing", "code": 400, "at": reloj()}
    assert st["answering"] is None


def test_si_el_modelo_del_principal_no_usa_herramientas_contesta_el_respaldo_con_ese_motivo(
    perfiles, monkeypatch
):
    two_providers()
    sin_tools = {
        "error": {
            "message": "registry.ollama.ai/library/gemma does not support tools",
            "code": None,
        }
    }
    principal = _Fake(fails(400, sin_tools))
    server(monkeypatch, anthropic=principal, deepinfra=_Fake(answers("con herramientas")))
    r = chat_call(tools=[{"type": "function"}])
    assert r.via["fallback"] and r.via["reason"] == "tools"
    st = ai.status()
    assert st["primary"]["problem"] is None, "no es un fallo del servicio: no se le da por caido"
    assert st["answering"]["reason"] == "tools"


# ------------------------------------------------- la forma de status()


def test_status_sin_ningun_perfil_esta_vacio(perfiles):
    assert ai.status() == {
        "primary": {"id": "", "name": "", "model": "", "problem": None},
        "answering": None,
    }


def test_status_tiene_la_forma_del_contrato(reloj, monkeypatch):
    two_providers()
    server(
        monkeypatch,
        anthropic=_Fake(fails(400, ANTHROPIC_SIN_SALDO)),
        deepinfra=_Fake(answers("respaldo")),
    )
    nombre = providers.BY_ID["anthropic"]["name"]
    assert ai.status() == {
        "primary": {
            "id": "anthropic",
            "name": nombre,
            "model": "claude-sonnet-5-5",
            "problem": None,
        },
        "answering": None,
    }
    r = chat_call()
    st = ai.status()
    assert set(st) == {"primary", "answering"}
    assert set(st["primary"]) == {"id", "name", "model", "problem"}
    assert st["primary"]["problem"] == {"kind": "billing", "code": 400, "at": reloj()}
    assert set(st["answering"]) == {"id", "name", "model", "fallback", "reason"}
    assert st["answering"] == r.via
    json.dumps(st)  # se serializa tal cual
    # lo que devuelve es una copia: tocarla no cambia el registro
    st["answering"]["id"] = "otro"
    st["primary"]["problem"]["kind"] = "otro"
    assert ai.status()["answering"]["id"] == "deepinfra"
    assert ai.status()["primary"]["problem"]["kind"] == "billing"


def test_status_se_ve_desde_otro_hilo(perfiles, monkeypatch):
    """`_turn` es por hilo: el estado que lee la ruta de la franja (otro hilo)
    no puede salir de ahi."""
    two_providers()
    server(
        monkeypatch,
        anthropic=_Fake(fails(400, ANTHROPIC_SIN_SALDO)),
        deepinfra=_Fake(answers("respaldo")),
    )
    ai.begin_turn()
    r = chat_call()
    visto: dict = {}

    def otro_hilo():
        visto["status"] = ai.status()
        visto["turno"] = ai.turn_summary()

    t = threading.Thread(target=otro_hilo)
    t.start()
    t.join(10)
    assert not t.is_alive()
    assert visto["turno"] == (None, None), "lo del turno es de ESE hilo"
    assert visto["status"]["answering"] == r.via
    assert visto["status"]["primary"]["problem"]["kind"] == "billing"


def test_el_registro_aguanta_lectores_y_escritores_a_la_vez(perfiles):
    two_providers()
    errores: list[BaseException] = []
    parar = threading.Event()

    def lee():
        try:
            while not parar.is_set():
                st = ai.status()
                assert set(st) == {"primary", "answering"}
        except Exception as e:  # noqa: BLE001
            errores.append(e)

    lectores = [threading.Thread(target=lee) for _ in range(4)]
    for t in lectores:
        t.start()
    for _ in range(300):
        ai._note_failure("anthropic", "billing", 400)
        ai.recheck()
    parar.set()
    for t in lectores:
        t.join(10)
    assert not errores and not any(t.is_alive() for t in lectores)


# ----------------------------- quien contesta: solo la conversacion cuenta


def test_el_juez_y_la_identificacion_no_cambian_quien_contesta(reloj, monkeypatch):
    two_providers()
    principal = _Fake(fails(400, ANTHROPIC_SIN_SALDO), usage=True)
    respaldo = _Fake(answers("respaldo"), usage=True)
    server(monkeypatch, anthropic=principal, deepinfra=respaldo)
    ai.begin_turn()
    chat = chat_call()
    assert chat.via["fallback"]
    # pasa el plazo y el principal vuelve: lo siguiente (el juez, una
    # identificacion) lo contesta el, y NO es lo que contesto la conversacion
    reloj.advance(25 * 60)
    principal.behave = answers("SI")
    for purpose in ("judge", "identify"):
        r = ai.complete(MSG, purpose=purpose)
        assert not r.via["fallback"] and r.via["id"] == "anthropic"
    st = ai.status()
    assert st["answering"] == chat.via and st["answering"]["fallback"]
    usage, via = ai.turn_summary()
    assert via == chat.via, "lo que el bucle manda a la interfaz tampoco cambia"
    assert usage is not None and usage["calls"] == 3, "pero el gasto del turno las cuenta todas"
    # y la proxima respuesta de la conversacion si lo cambia
    r = chat_call()
    assert not r.via["fallback"]
    assert ai.status()["answering"] == r.via and ai.turn_summary()[1] == r.via


def test_el_contador_de_peticiones_del_turno_no_se_deja_ninguna(reloj, monkeypatch):
    """El presupuesto de llamadas de un turno tiene que ver tambien la consulta
    al juez (se hace dentro de otro modulo), las que fallan y las que pasan de
    un proveedor a otro. `calls` solo cuenta las que devolvieron lo gastado."""
    two_providers()
    principal = _Fake(fails(400, ANTHROPIC_SIN_SALDO))
    respaldo = _Fake(answers("respaldo"))  # sin `usage`: algunos servidores no lo mandan
    server(monkeypatch, anthropic=principal, deepinfra=respaldo)
    sin_turno: list[int] = []
    t = threading.Thread(target=lambda: sin_turno.append(ai.turn_requests()))
    t.start()
    t.join(10)
    assert sin_turno == [0], "un hilo sin turno no ha pedido nada"
    ai.begin_turn()
    assert ai.turn_requests() == 0
    chat_call()  # el principal falla, contesta el respaldo: UNA peticion del turno
    chat_call()
    ai.complete(MSG, purpose="judge")
    assert ai.turn_requests() == 3
    respaldo.behave = fails(400, MENSAJE_VACIO)
    with pytest.raises(openai.BadRequestError):
        chat_call()
    assert ai.turn_requests() == 4, "tambien la que acaba en error"
    usage, _ = ai.turn_summary()
    assert usage is not None and usage["calls"] == 0
    ai.begin_turn()
    assert ai.turn_requests() == 0, "cada turno empieza de cero"


def test_un_fallo_del_principal_en_otra_llamada_tambien_se_apunta_pero_no_cambia_la_respuesta(
    perfiles, monkeypatch
):
    """El principal falla en una identificacion: la franja ya puede avisar, pero
    nadie ha contestado aun en el chat."""
    two_providers()
    server(
        monkeypatch,
        anthropic=_Fake(fails(401, OPENAI_CLAVE_MALA)),
        deepinfra=_Fake(answers("{}")),
    )
    ai.complete(MSG, purpose="identify")
    st = ai.status()
    assert st["primary"]["problem"]["kind"] == "auth" and st["answering"] is None


# ------------------------- recheck, reset_client y check limpian la marca


def _con_el_principal_sin_saldo(monkeypatch):
    two_providers()
    principal = _Fake(fails(400, ANTHROPIC_SIN_SALDO))
    respaldo = _Fake(answers("respaldo"))
    server(monkeypatch, anthropic=principal, deepinfra=respaldo)
    chat_call()
    assert ai.status()["primary"]["problem"] and ai.status()["answering"]["fallback"]
    return principal, respaldo


def test_recheck_borra_la_marca_y_el_siguiente_mensaje_prueba_el_principal(perfiles, monkeypatch):
    principal, _ = _con_el_principal_sin_saldo(monkeypatch)
    st = ai.recheck()
    assert st["primary"]["problem"] is None and st["answering"] is None
    assert st == ai.status()
    principal.behave = answers("ya hay saldo")
    r = chat_call()
    assert len(principal.calls) == 2 and not r.via["fallback"]


def test_reset_client_olvida_el_fallo_y_quien_contesto(perfiles, monkeypatch):
    principal, _ = _con_el_principal_sin_saldo(monkeypatch)
    ai.reset_client()  # lo que hace guardar ajustes, cambiar de perfil…
    assert ai.status()["primary"]["problem"] is None and ai.status()["answering"] is None
    assert not ai._down_until and not ai._problems
    chat_call()
    assert len(principal.calls) == 2


def test_un_check_correcto_del_activo_borra_la_marca_pero_uno_de_borrador_no(perfiles, monkeypatch):
    principal, _ = _con_el_principal_sin_saldo(monkeypatch)
    principal.behave = answers("ok")  # la persona recargo el saldo
    # probar lo que hay en el formulario sin guardar no dice nada del activo
    borrador = ai.check({"provider": "anthropic", "key": "zz-otra-clave-de-prueba-1", "model": "m"})
    assert borrador["ok"] and ai.status()["primary"]["problem"]["kind"] == "billing"
    r = ai.check()  # el boton «Probar» de Ajustes
    assert r["ok"]
    st = ai.status()
    assert st["primary"]["problem"] is None and st["answering"] is None
    # y un check que falla no borra nada
    principal.behave = fails(400, ANTHROPIC_SIN_SALDO)
    chat_call()
    assert ai.status()["primary"]["problem"]["kind"] == "billing"
    assert not ai.check()["ok"]
    assert ai.status()["primary"]["problem"]["kind"] == "billing"


def test_la_ruta_recheck_borra_la_marca_y_devuelve_el_estado(perfiles, monkeypatch, api_client):
    principal, _ = _con_el_principal_sin_saldo(monkeypatch)
    r = api_client.post("/api/ai/recheck")
    assert r.status_code == 200
    nombre = providers.BY_ID["anthropic"]["name"]
    assert r.json() == {
        "primary": {
            "id": "anthropic",
            "name": nombre,
            "model": "claude-sonnet-5-5",
            "problem": None,
        },
        "answering": None,
    }
    chat_call()
    assert len(principal.calls) == 2
    # la ruta no cambia ningun ajuste
    assert providers.active_id() == "anthropic" and providers.fallback_enabled()


# ------------------------- con el SDK real, por un socket de verdad


class _Loopback:
    """Un servidor HTTP de mentira en 127.0.0.1 que habla como un proveedor:
    `/p/v1` (el principal) contesta 400 de Anthropic sin saldo, `/f/v1` (el
    respaldo) contesta bien. Cuenta las peticiones de cada uno."""

    def __init__(self):
        self.hits = {"p": 0, "f": 0}
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, format, *args):
                pass

            def do_POST(self):
                self.rfile.read(int(self.headers.get("content-length") or 0))
                which = "p" if self.path.startswith("/p/") else "f"
                outer.hits[which] += 1
                if which == "p":
                    self.reply(400, ANTHROPIC_SIN_SALDO)
                    return
                msg = {"role": "assistant", "content": "respaldo ok"}
                self.reply(
                    200,
                    {
                        "id": "x",
                        "object": "chat.completion",
                        "created": 1,
                        "model": "m",
                        "choices": [{"index": 0, "finish_reason": "stop", "message": msg}],
                        "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5},
                    },
                )

            def reply(self, status, body):
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.httpd.server_address[1]

    def __enter__(self):
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()
        return self

    def __exit__(self, *exc):
        self.httpd.shutdown()
        self.httpd.server_close()


def test_con_el_sdk_real_el_principal_sin_saldo_recibe_una_peticion_cada_20_minutos(reloj):
    """El caso del usuario, de punta a punta y sin dobles de cliente: el SDK de
    verdad contra un servidor local que contesta 400 «credit balance». Antes
    cada mensaje pagaba una peticion fallida (1 s) al principal."""
    with _Loopback() as srv:
        for name, path, key in (("Principal", "p", CLAVE), ("Respaldo", "f", CLAVE_RESPALDO)):
            providers.save_profile(
                {
                    "provider": "custom",
                    "name": name,
                    "key": key,
                    "base_url": f"http://127.0.0.1:{srv.port}/{path}/v1",
                    "model": "m",
                    "chat_model": "m",
                },
                activate=(path == "p"),
            )
        ai.reset_client()
        replies = [chat_call() for _ in range(3)]
        assert [ai.message_text(r.message) for r in replies] == ["respaldo ok"] * 3
        assert srv.hits == {"p": 1, "f": 3}, "tres mensajes: el principal solo en el primero"
        assert all(r.via["fallback"] and r.via["reason"] == "billing" for r in replies)
        assert ai.status()["primary"]["problem"] == {
            "kind": "billing",
            "code": 400,
            "at": reloj(),
        }
        reloj.advance(20 * 60 + 1)
        chat_call()
        assert srv.hits == {"p": 2, "f": 4}, "pasados los 20 minutos se vuelve a probar"


# --------------------------------------------------- /api/chat/tools


def test_chat_tools_devuelve_los_campos_nuevos_y_lo_de_siempre(reloj, monkeypatch, api_client):
    two_providers()
    server(
        monkeypatch,
        anthropic=_Fake(fails(400, ANTHROPIC_SIN_SALDO)),
        deepinfra=_Fake(answers("respaldo")),
    )
    antes = api_client.get("/api/chat/tools").json()
    assert antes["primary_problem"] is None and antes["answering"] is None
    chat_call()
    d = api_client.get("/api/chat/tools").json()
    # lo de siempre
    assert d["model"] == "claude-sonnet-5-5" and d["provider"] == ai.provider_name()
    assert d["available"] is True and d["reason"] == ""
    assert {t["name"] for t in d["tools"]} >= {"search_songs", "create_playlist"}
    # lo nuevo, sin ningun texto del proveedor
    assert d["primary_problem"] == {"kind": "billing", "code": 400, "at": reloj()}
    assert d["answering"] == {
        "id": "deepinfra",
        "name": providers.BY_ID["deepinfra"]["name"],
        "model": "Qwen/Qwen3-Next-80B-A3B-Instruct",
        "fallback": True,
        "reason": "billing",
    }
    assert "credit" not in json.dumps({k: d[k] for k in ("primary_problem", "answering")})


# ------------------------------------------ el bucle del chat, de punta a punta


def test_el_bucle_cuenta_quien_contesto_y_por_que(perfiles, monkeypatch):
    from danplay import chat

    two_providers()
    principal = _Fake(fails(400, ANTHROPIC_SIN_SALDO))
    server(monkeypatch, anthropic=principal, deepinfra=_Fake(answers("Hola, ¿que cancion buscas?")))
    r = chat.reply([{"role": "user", "text": "hola"}])
    assert "error" not in r and r["text"]
    assert (
        r["via"]["fallback"] and r["via"]["id"] == "deepinfra" and r["via"]["reason"] == "billing"
    )
    assert ai.status()["answering"] == r["via"]
    r = chat.reply([{"role": "user", "text": "hola otra vez"}])
    assert len(principal.calls) == 1, "el segundo mensaje ya no pierde una llamada fallida"
    assert r["via"]["fallback"] and r["via"]["reason"] == "billing"


# ------------------------------------------- nada del proveedor sale de aqui

FUGAS = [
    pytest.param(CLAVE, (CLAVE,), id="la-clave-del-perfil"),
    pytest.param(CABECERA, (CABECERA,), id="un-valor-de-cabecera"),
    pytest.param(EXTRA, (EXTRA,), id="un-valor-de-extra"),
    pytest.param(CLAVE_RESPALDO, (CLAVE_RESPALDO,), id="la-clave-del-respaldo"),
    pytest.param("sk-live-ABCDEFGH12345678", ("ABCDEFGH12345678",), id="sk"),
    pytest.param("sk-proj-********************wxyz.", ("wxyz", "sk-proj"), id="sk-enmascarada"),
    pytest.param("AIzaSyDUMMYKEYDUMMYKEYDUMMYKEY1234567", ("DUMMYKEYDUMMY",), id="AIza"),
    pytest.param("Bearer tokenfalso.abc-123", ("tokenfalso",), id="bearer"),
    pytest.param("Authorization: Basic dXN1YXJpbzpjbGF2ZQ==", ("dXN1YXJpbzpjbGF2ZQ",), id="basic"),
    pytest.param(
        "see https://usuario:contrasena@servidor.example/v1/chat?key=ZZ99887766",
        ("contrasena", "ZZ99887766"),
        id="url-con-usuario-y-clave",
    ),
    pytest.param(
        "api_key=otra-clave-falsa-0001", ("otra-clave-falsa-0001",), id="clave-igual-valor"
    ),
]


def _con_fuga(monkeypatch, fuga: str, respaldo: bool = True) -> tuple[_Fake, _Fake]:
    """El principal falla (500) con un mensaje que repite `fuga`."""
    two_providers(extra={"metadato": EXTRA}, headers={"X-Privado": CABECERA})
    if not respaldo:
        providers.set_fallback(False)
    principal = _Fake(fails(500, {"error": {"message": f"upstream failure: {fuga}", "code": None}}))
    secundario = _Fake(answers("desde el respaldo"))
    server(monkeypatch, anthropic=principal, deepinfra=secundario)
    return principal, secundario


@pytest.mark.parametrize(("fuga", "agujas"), FUGAS)
def test_ni_status_ni_la_ruta_ni_r_error_ni_los_logs_llevan_lo_del_proveedor(
    perfiles, monkeypatch, caplog, api_client, fuga, agujas
):
    from danplay import chat

    caplog.set_level(logging.DEBUG)
    # 1) el principal falla, contesta el respaldo: el estado y la ruta
    _con_fuga(monkeypatch, fuga)
    chat_call()
    visible = json.dumps(ai.status()) + api_client.get("/api/chat/tools").text
    # 2) el flujo se corta a mitad con el mismo mensaje: el aviso del registro
    ai.reset_client()
    rota = _Fake(lambda kw: _FlujoRoto(f"upstream failure: {fuga}") if kw.get("stream") else "ok")
    server(monkeypatch, anthropic=rota, deepinfra=_Fake(answers("x")))
    ai.complete(MSG, purpose="chat", on_text=lambda t: None)
    # 3) sin respaldo: el bucle del chat da su error, y la consulta suelta lo registra
    ai.reset_client()
    _con_fuga(monkeypatch, fuga, respaldo=False)
    r = chat.reply([{"role": "user", "text": "hola"}])
    assert "upstream" in r["error"], "el error sale (saneado), no se calla"
    assert ai.ask("hola") is None
    ident = ai.resolve("pista.mp3")
    assert ident is not None and "upstream" in ident["error"]
    for aguja in agujas:
        assert aguja not in visible
        assert aguja not in r["error"]
        assert aguja not in ident["error"]
        assert aguja not in caplog.text
    # y que los avisos se emitieron (si no, la prueba no probaria nada)
    for aviso in ("no responde (down:", "corto el flujo", "la consulta a la IA fallo"):
        assert aviso in caplog.text


def test_describe_error_ya_no_confunde_un_limite_con_falta_de_saldo_ni_un_403_con_la_clave(
    perfiles,
):
    two_providers()
    p = ai.profile()
    assert p is not None
    nombre = p["name"]
    assert "pide esperar" in ai.describe_error(sdk_error(429, GEMINI_GRATIS), p)
    assert ai.describe_error(sdk_error(429, OPENAI_SIN_CUOTA), p) == (
        "la clave es valida pero no tiene credito"
    )
    assert ai.describe_error(sdk_error(401, OPENAI_CLAVE_MALA), p) == f"{nombre} rechaza la clave"
    moderado = ai.describe_error(sdk_error(403, OPENROUTER_MODERACION), p)
    assert "clave" in moderado and "rechaza la clave" not in moderado and "403" in moderado
    assert "conexion" in ai.describe_error(openai.APIConnectionError(request=_REQUEST), p)


# ---------------------------------------------- sanitize_provider_message


def test_sanitize_quita_lo_secreto_y_deja_el_resto(perfiles):
    two_providers(extra={"metadato": EXTRA, "reasoning": {"effort": "low"}})
    f = ai.sanitize_provider_message
    # la URL se queda en su servidor: sin usuario:clave@, ruta ni parametros
    assert f(
        "fallo en https://usuario:contrasena@servidor.example:8443/v1/chat?key=abc12345 ya"
    ) == ("fallo en servidor.example:8443 ya")
    assert f("mira https://platform.openai.com/account/api-keys.") == "mira platform.openai.com."
    # Bearer, claves con forma de clave, clave=valor y «Authorization»
    assert f("Bearer abc.def-ghi") == "Bearer ***"
    assert f("x sk-live-ABCDEFGH12345678 y AIzaSyDUMMYKEYDUMMYKEY1234567 z") == "x *** y *** z"
    assert f("a?key=AIzaSyDUMMYKEYDUMMYKEY1234567&b=1") == "a?key=***&b=1"
    assert f("token=abc123 y password: hunter2xx") == "token=*** y password: ***"
    assert f("Authorization: Bearer abc123") == "Authorization: ***"
    # lo del perfil activo y el de respaldo, y los valores de cabeceras/extras
    assert f(f"clave {CLAVE} y {CLAVE_RESPALDO}") == "clave *** y ***"
    assert f(f"extra {EXTRA}") == "extra ***"
    # un valor corto (lo de «reasoning»: low) no es un secreto
    assert f("el esfuerzo low sigue") == "el esfuerzo low sigue"
    # con un perfil de fuera (un borrador de Ajustes), tambien el suyo
    borrador = ai.providers.resolve(
        "openai", {"provider": "openai", "key": "zz-clave-de-borrador-1"}
    )
    assert f("clave zz-clave-de-borrador-1", borrador) == "clave ***"


def test_sanitize_siempre_una_linea_de_160_como_mucho_y_sin_invisibles(perfiles):
    f = ai.sanitize_provider_message
    assert f("uno\n\tdos\r\n\x07tres") == "uno dos tres"
    largo = f("palabra " * 100)
    assert len(largo) == 160 and largo.endswith("…") and "\n" not in largo
    # ceros de ancho y cambios de sentido de escritura: fuera
    sucio = "pa" + chr(0x200B) + "la" + chr(0x202E) + "bra" + chr(0xFEFF)
    assert f(sucio) == "palabra"
    # lo que no cabe nunca se corta a medias de un secreto: se limpia antes
    assert "ABCDEFGH" not in f("x" * 150 + " sk-live-ABCDEFGH12345678")
    # y es estable
    una_vez = f("fallo en https://u:p@h.example/x?key=zzz1234567 con Bearer abcdef")
    assert f(una_vez) == una_vez


def test_sanitize_no_lanza_con_lo_que_sea(perfiles):
    f = ai.sanitize_provider_message

    class Roto:
        def __str__(self):
            raise RuntimeError("no se puede")

    assert f(None) == ""
    assert f(123) == "123"
    assert f(b"bytes") == "b'bytes'"
    assert f(Roto()) == "Roto"
    assert f(ValueError("algo " + "x" * 50_000)).startswith("algo xxx")
    assert f("https://[::1") == "[url]", "una URL rota no tumba nada"
