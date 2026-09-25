# -*- coding: utf-8 -*-
"""Proveedores de IA: el catalogo, los perfiles guardados y cual esta activo.

Todos hablan el mismo protocolo (el «chat completions» de OpenAI), asi que un
solo cliente sirve para OpenAI, Anthropic, Google, DeepInfra, OpenRouter, un
Ollama en este equipo o un servidor privado. Lo que cambia es la URL, si
piden clave, alguna cabecera y que parametros toleran. Eso es lo que guarda
este modulo; la conversacion con el modelo vive en `ai.py`.

Los datos de cada proveedor que el usuario haya configurado se recuerdan
TODOS (no solo el activo), en `~/.config/danplay/ai.json` con permisos 0600:
cambiar de Ollama a OpenRouter y volver no obliga a pegar la clave otra vez.

Las URLs se comprobaron contra la documentacion oficial de cada servicio el
11-09-2026. Los modelos sugeridos son solo eso, sugerencias: los nombres
caducan en meses, y la lista de verdad la trae `model_catalog` (models.dev) y
el propio proveedor (`/models`).
"""
import json
import logging
import os
import re
import secrets
from . import config

log = logging.getLogger("danplay.providers")

PROFILES_FILE = config.CONFIG_DIR / "ai.json"

GROUPS = [
    {"id": "free", "name": "Gratis, sin clave",
     "note": "Para probar sin registrarte en ningun sitio. Son servicios de terceros con limites "
             "y sin garantia: tus preguntas y los nombres de tus canciones pasan por ellos."},
    {"id": "lab", "name": "Grandes laboratorios",
     "note": "Los que entrenan los modelos. Clave de pago, salvo donde se indica."},
    {"id": "platform", "name": "Plataformas de inferencia",
     "note": "Como DeepInfra: sirven cientos de modelos abiertos y de terceros con una sola clave."},
    {"id": "cloud", "name": "Nubes corporativas",
     "note": "Si tu empresa o iglesia ya tiene cuenta en Azure, AWS, Cloudflare o Databricks."},
    {"id": "asia", "name": "Proveedores de Asia",
     "note": "Qwen, Kimi, GLM, MiniMax y compañia, desde su propia API."},
    {"id": "local", "name": "En tu equipo",
     "note": "Sin clave y sin internet: el modelo corre en tu ordenador."},
    {"id": "custom", "name": "Otro",
     "note": "Cualquier servicio compatible con la API de OpenAI: un proxy, un servidor privado, LiteLLM…"},
]

# Cabeceras con las que OpenRouter atribuye el uso a la app (opcionales).
_OPENROUTER_HEADERS = {"HTTP-Referer": "https://github.com/dani17r/danplay",
                       "X-OpenRouter-Title": "DanPlay"}


def _p(id, name, group, base_url, *, key="required", key_url="", docs="",
       models_dev=None, suggest=None, fields=None, headers=None, note="",
       quirks=None):
    return {"id": id, "name": name, "group": group, "base_url": base_url,
            "key": key, "key_url": key_url, "docs": docs,
            "models_dev": models_dev, "suggest": suggest or {},
            "fields": fields or [], "headers": headers or {}, "note": note,
            "quirks": quirks or {}}


# `key`: required | optional | none.  `fields`: huecos de la URL que el
# usuario rellena ({resource}, {region}…).  `quirks`: lo que se sabe de
# antemano que el servicio NO acepta, para no mandarselo y comerse el error;
# `anon_filter` deja en la lista solo lo que se puede usar sin clave.
CATALOG = [
    # --- gratis, sin clave ---
    # Comprobados el 11-09-2026 con peticiones anonimas de verdad (con
    # herramientas incluidas). Van los primeros de FREE_ORDER: «Probar gratis»
    # activa el primero que responda. Cambian sin avisar: Pollinations dejo
    # de servir anonimos ese mismo mes.
    _p("llm7", "LLM7", "free", "https://api.llm7.io/v1", key="optional",
       key_url="https://token.llm7.io", docs="https://llm7.io",
       suggest={"fast": "minimax-m2.7", "chat": "minimax-m2.7"},
       quirks={"anon_filter": {"field": "tier", "value": "turbo"}},
       note="Sin cuenta: 10 peticiones por minuto y 60 por hora, con los modelos «turbo». "
            "Un token gratuito de token.llm7.io sube el limite."),
    _p("kilo", "Kilo (rutas :free)", "free", "https://api.kilo.ai/api/gateway", key="optional",
       key_url="https://app.kilo.ai/profile", docs="https://kilo.ai", models_dev="kilo",
       suggest={"fast": "nvidia/nemotron-3.5-lightning:free",
                "chat": "nvidia/nemotron-3-super-120b-a12b:free"},
       quirks={"anon_filter": {"field": "isFree", "value": True}},
       note="Sin cuenta valen solo los modelos «:free». Algunos pueden entrenar con lo que "
            "les mandas: la lista lo marca."),
    _p("opencode-zen", "OpenCode Zen (gratis)", "free", "https://opencode.ai/zen/v1", key="optional",
       key_url="https://opencode.ai/auth", docs="https://opencode.ai/docs/zen", models_dev="opencode",
       suggest={"fast": "nemotron-3.5-lightning-free", "chat": "deepseek-v4-flash-free"},
       quirks={"anon_filter": {"suffix": "-free"}},
       note="Acceso promocional a los modelos «-free»; puede desaparecer cualquier dia."),

    # --- grandes laboratorios ---
    _p("openai", "OpenAI", "lab", "https://api.openai.com/v1",
       key_url="https://platform.openai.com/api-keys",
       docs="https://developers.openai.com/api/docs/models", models_dev="openai",
       suggest={"fast": "gpt-5.6-luna", "chat": "gpt-5.6-terra"}),
    _p("anthropic", "Anthropic (Claude)", "lab", "https://api.anthropic.com/v1/",
       key_url="https://platform.claude.com/settings/keys",
       docs="https://platform.claude.com/docs/en/api/openai-sdk", models_dev="anthropic",
       suggest={"fast": "claude-haiku-4-5", "chat": "claude-sonnet-5"},
       # su capa compatible ignora response_format: el JSON se pide en el texto
       quirks={"json_mode": False},
       # la API nativa (/v1/models) exige la version; en la compatible sobra
       headers={"anthropic-version": "2023-06-01"},
       note="Capa compatible oficial de Anthropic; las herramientas funcionan."),
    _p("google", "Google Gemini", "lab",
       "https://generativelanguage.googleapis.com/v1beta/openai/",
       key_url="https://aistudio.google.com/app/apikey",
       docs="https://ai.google.dev/gemini-api/docs/openai", models_dev="google",
       suggest={"fast": "gemini-3.5-flash-lite", "chat": "gemini-3.8-flash"},
       note="Con clave de AI Studio. Tiene nivel gratuito."),
    _p("mistral", "Mistral AI", "lab", "https://api.mistral.ai/v1",
       key_url="https://console.mistral.ai/api-keys",
       docs="https://docs.mistral.ai/getting-started/models/", models_dev="mistral",
       note="Tiene nivel gratuito."),
    _p("xai", "xAI (Grok)", "lab", "https://api.x.ai/v1",
       key_url="https://console.x.ai", docs="https://docs.x.ai/docs/models", models_dev="xai"),
    _p("deepseek", "DeepSeek", "lab", "https://api.deepseek.com/v1",
       key_url="https://platform.deepseek.com/api_keys",
       docs="https://api-docs.deepseek.com/quick_start/pricing", models_dev="deepseek",
       suggest={"fast": "deepseek-flash", "chat": "deepseek-flash"}),
    _p("cohere", "Cohere", "lab", "https://api.cohere.ai/compatibility/v1",
       key_url="https://dashboard.cohere.com/api-keys",
       docs="https://docs.cohere.com/docs/compatibility-api", models_dev="cohere",
       note="Tiene clave de prueba gratuita."),
    _p("perplexity", "Perplexity", "lab", "https://api.perplexity.ai",
       key_url="https://www.perplexity.ai/settings/api",
       docs="https://docs.perplexity.ai", models_dev="perplexity",
       note="Sus modelos buscan en la web por su cuenta."),
    _p("ai21", "AI21 Labs", "lab", "https://api.ai21.com/studio/v1",
       key_url="https://studio.ai21.com/account/api-key", docs="https://docs.ai21.com"),
    _p("inception", "Inception (Mercury)", "lab", "https://api.inceptionlabs.ai/v1",
       key_url="https://platform.inceptionlabs.ai", docs="https://docs.inceptionlabs.ai",
       models_dev="inception"),
    _p("meta", "Meta (Llama API)", "lab", "https://api.llama.com/compat/v1/",
       key_url="https://llama.developer.meta.com", docs="https://llama.developer.meta.com/docs",
       models_dev="llama"),

    # --- plataformas de inferencia ---
    _p("deepinfra", "DeepInfra", "platform", "https://api.deepinfra.com/v1/openai",
       key_url="https://deepinfra.com/dash/api_keys", docs="https://deepinfra.com/models",
       models_dev="deepinfra",
       suggest={"fast": "google/gemini-3.1-flash-lite", "chat": "Qwen/Qwen3-Next-80B-A3B-Instruct"}),
    _p("openrouter", "OpenRouter", "platform", "https://openrouter.ai/api/v1",
       key_url="https://openrouter.ai/settings/keys", docs="https://openrouter.ai/docs/quickstart",
       models_dev="openrouter", headers=_OPENROUTER_HEADERS,
       suggest={"fast": "google/gemini-3.5-flash-lite", "chat": "google/gemini-3.8-flash"},
       note="Una clave para todos los modelos del mercado. Tiene modelos gratuitos."),
    _p("together", "Together AI", "platform", "https://api.together.xyz/v1",
       key_url="https://api.together.ai/settings/api-keys",
       docs="https://docs.together.ai/docs/serverless-models", models_dev="togetherai"),
    _p("fireworks", "Fireworks AI", "platform", "https://api.fireworks.ai/inference/v1",
       key_url="https://fireworks.ai/account/api-keys", docs="https://fireworks.ai/docs/",
       models_dev="fireworks-ai"),
    _p("groq", "Groq", "platform", "https://api.groq.com/openai/v1",
       key_url="https://console.groq.com/keys", docs="https://console.groq.com/docs/models",
       models_dev="groq", suggest={"fast": "llama-3.1-8b-instant", "chat": "llama-3.3-70b-versatile"},
       note="Muy rapido. Tiene nivel gratuito."),
    _p("cerebras", "Cerebras", "platform", "https://api.cerebras.ai/v1",
       key_url="https://cloud.cerebras.ai/", docs="https://inference-docs.cerebras.ai",
       models_dev="cerebras", note="Muy rapido. Tiene nivel gratuito."),
    _p("sambanova", "SambaNova", "platform", "https://api.sambanova.ai/v1",
       key_url="https://cloud.sambanova.ai/apis", docs="https://docs.sambanova.ai",
       note="Tiene nivel gratuito."),
    _p("huggingface", "Hugging Face", "platform", "https://router.huggingface.co/v1",
       key_url="https://huggingface.co/settings/tokens",
       docs="https://huggingface.co/inference/get-started", models_dev="huggingface",
       note="Enruta a Groq, Together, Cerebras, DeepInfra… con un token de Hugging Face."),
    _p("nvidia", "NVIDIA NIM", "platform", "https://integrate.api.nvidia.com/v1",
       key_url="https://build.nvidia.com/settings/api-keys", docs="https://docs.api.nvidia.com",
       models_dev="nvidia", note="Tiene nivel gratuito."),
    _p("novita", "Novita AI", "platform", "https://api.novita.ai/openai",
       key_url="https://novita.ai/settings/key-management", docs="https://docs.novita.ai/guides/llm-api",
       models_dev="novita-ai"),
    _p("nebius", "Nebius Token Factory", "platform", "https://api.tokenfactory.nebius.com/v1",
       key_url="https://tokenfactory.nebius.com/settings/api-keys",
       docs="https://docs.tokenfactory.nebius.com/quickstart", models_dev="nebius"),
    _p("hyperbolic", "Hyperbolic", "platform", "https://api.hyperbolic.xyz/v1",
       key_url="https://app.hyperbolic.xyz/settings", docs="https://docs.hyperbolic.xyz"),
    _p("baseten", "Baseten", "platform", "https://inference.baseten.co/v1",
       key_url="https://app.baseten.co/settings/api_keys", docs="https://docs.baseten.co",
       models_dev="baseten"),
    _p("lambda", "Lambda", "platform", "https://api.lambda.ai/v1",
       key_url="https://cloud.lambda.ai/api-keys", docs="https://docs.lambda.ai/public-cloud/lambda-inference-api/"),
    _p("chutes", "Chutes", "platform", "https://llm.chutes.ai/v1",
       key_url="https://chutes.ai/", docs="https://docs.chutes.ai", models_dev="chutes"),
    _p("featherless", "Featherless", "platform", "https://api.featherless.ai/v1",
       key_url="https://featherless.ai/account/api-keys", docs="https://featherless.ai/docs"),
    _p("nscale", "Nscale", "platform", "https://inference.api.nscale.com/v1",
       key_url="https://console.nscale.com/", docs="https://docs.nscale.com"),
    _p("ovhcloud", "OVHcloud AI Endpoints", "platform",
       "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
       key_url="https://www.ovhcloud.com/en/public-cloud/ai-endpoints/",
       docs="https://help.ovhcloud.com/csm/en-public-cloud-ai-endpoints-getting-started",
       models_dev="ovhcloud"),
    _p("scaleway", "Scaleway", "platform", "https://api.scaleway.ai/v1",
       key_url="https://console.scaleway.com/iam/api-keys",
       docs="https://www.scaleway.com/en/docs/generative-apis/", models_dev="scaleway"),
    _p("github", "GitHub Models", "platform", "https://models.github.ai/inference",
       key_url="https://github.com/settings/personal-access-tokens",
       docs="https://docs.github.com/en/github-models",
       note="Con un token personal de GitHub. Tiene nivel gratuito."),
    _p("requesty", "Requesty", "platform", "https://router.requesty.ai/v1",
       key_url="https://app.requesty.ai/api-keys", docs="https://docs.requesty.ai",
       models_dev="requesty"),
    _p("vercel", "Vercel AI Gateway", "platform", "https://ai-gateway.vercel.sh/v1",
       key_url="https://vercel.com/docs/ai-gateway", docs="https://vercel.com/docs/ai-gateway",
       models_dev="vercel"),
    _p("poe", "Poe", "platform", "https://api.poe.com/v1",
       key_url="https://poe.com/api_key", docs="https://creator.poe.com/docs/external-applications/openai-compatible-api",
       models_dev="poe"),
    _p("ollama-cloud", "Ollama Cloud", "platform", "https://ollama.com/v1",
       key_url="https://ollama.com/settings/keys", docs="https://docs.ollama.com/cloud",
       models_dev="ollama-cloud"),
    _p("nanogpt", "NanoGPT", "platform", "https://nano-gpt.com/api/v1",
       key_url="https://nano-gpt.com/api", docs="https://docs.nano-gpt.com", models_dev="nano-gpt"),
    _p("aimlapi", "AI/ML API", "platform", "https://api.aimlapi.com/v1",
       key_url="https://aimlapi.com/app/keys", docs="https://docs.aimlapi.com"),
    _p("venice", "Venice", "platform", "https://api.venice.ai/api/v1",
       key_url="https://venice.ai/settings/api", docs="https://docs.venice.ai", models_dev="venice",
       note="Sin registros de lo que preguntas, segun ellos."),

    # --- nubes corporativas ---
    _p("azure", "Azure OpenAI", "cloud", "https://{resource}.openai.azure.com/openai/v1/",
       key_url="https://portal.azure.com", models_dev="azure",
       docs="https://learn.microsoft.com/azure/foundry/openai/api-version-lifecycle",
       fields=[{"name": "resource", "label": "Nombre del recurso", "placeholder": "mi-recurso"}],
       note="El modelo es el NOMBRE DEL DESPLIEGUE que creaste en Azure."),
    _p("bedrock", "Amazon Bedrock", "cloud",
       "https://bedrock-runtime.{region}.amazonaws.com/openai/v1",
       key_url="https://console.aws.amazon.com/bedrock/home#/api-keys", models_dev="amazon-bedrock",
       docs="https://docs.aws.amazon.com/bedrock/latest/userguide/inference-chat-completions.html",
       fields=[{"name": "region", "label": "Region", "placeholder": "us-east-1"}],
       note="Con una clave de API de Bedrock (no las credenciales de AWS)."),
    _p("cloudflare", "Cloudflare Workers AI", "cloud",
       "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1",
       key_url="https://dash.cloudflare.com/profile/api-tokens", models_dev="cloudflare-workers-ai",
       docs="https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/",
       fields=[{"name": "account_id", "label": "ID de la cuenta", "placeholder": "0123abcd…"}],
       note="Tiene nivel gratuito."),
    _p("databricks", "Databricks", "cloud", "https://{workspace}/serving-endpoints",
       key_url="https://docs.databricks.com/aws/en/dev-tools/auth/pat", models_dev="databricks",
       docs="https://docs.databricks.com/aws/en/machine-learning/model-serving/",
       fields=[{"name": "workspace", "label": "Dominio del espacio de trabajo",
                "placeholder": "adb-1234.5.azuredatabricks.net"}]),

    # --- Asia ---
    _p("alibaba", "Alibaba Cloud (Qwen)", "asia",
       "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
       key_url="https://bailian.console.alibabacloud.com/?apiKey=1",
       docs="https://www.alibabacloud.com/help/en/model-studio/", models_dev="alibaba"),
    _p("moonshot", "Moonshot (Kimi)", "asia", "https://api.moonshot.ai/v1",
       key_url="https://platform.moonshot.ai/console/api-keys",
       docs="https://platform.moonshot.ai/docs", models_dev="moonshotai"),
    _p("zai", "Z.ai (GLM)", "asia", "https://api.z.ai/api/paas/v4",
       key_url="https://z.ai/manage-apikey/apikey-list", docs="https://docs.z.ai", models_dev="zai"),
    _p("minimax", "MiniMax", "asia", "https://api.minimax.io/v1",
       key_url="https://platform.minimax.io/user-center/basic-information/interface-key",
       docs="https://platform.minimax.io/docs/api-reference/text-openai-api", models_dev="minimax"),
    _p("stepfun", "StepFun", "asia", "https://api.stepfun.com/v1",
       key_url="https://platform.stepfun.com/interface-key", docs="https://platform.stepfun.com/docs",
       models_dev="stepfun"),
    _p("siliconflow", "SiliconFlow", "asia", "https://api.siliconflow.com/v1",
       key_url="https://cloud.siliconflow.com/account/ak", docs="https://docs.siliconflow.com",
       models_dev="siliconflow"),
    _p("modelscope", "ModelScope", "asia", "https://api-inference.modelscope.cn/v1",
       key_url="https://modelscope.cn/my/myaccesstoken", docs="https://modelscope.cn/docs/model-service/API-Inference/intro",
       models_dev="modelscope", note="Tiene nivel gratuito."),
    _p("volcengine", "Volcengine (Doubao)", "asia", "https://ark.cn-beijing.volces.com/api/v3",
       key_url="https://console.volcengine.com/ark", docs="https://www.volcengine.com/docs/82379",
       models_dev="volcengine"),

    # --- en tu equipo ---
    _p("ollama", "Ollama", "local", "http://localhost:11434/v1", key="none",
       docs="https://docs.ollama.com/api/openai-compatibility",
       suggest={"fast": "qwen3:8b", "chat": "qwen3:8b"},
       note="Descarga antes un modelo con «ollama pull». El asistente necesita uno que sepa usar herramientas."),
    _p("lmstudio", "LM Studio", "local", "http://localhost:1234/v1", key="none",
       docs="https://lmstudio.ai/docs/developer/openai-compat", models_dev="lmstudio",
       note="Arranca el servidor en la pestaña Developer de LM Studio."),
    _p("llamacpp", "llama.cpp (llama-server)", "local", "http://localhost:8080/v1", key="optional",
       docs="https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md"),
    _p("vllm", "vLLM", "local", "http://localhost:8000/v1", key="optional",
       docs="https://docs.vllm.ai/en/latest/serving/openai_compatible_server/",
       note="Para el asistente, arranca vLLM con --enable-auto-tool-choice."),
    _p("jan", "Jan", "local", "http://localhost:1337/v1", key="optional",
       docs="https://www.jan.ai/docs/desktop/api-server"),
    _p("localai", "LocalAI", "local", "http://localhost:8080/v1", key="optional",
       docs="https://localai.io/features/openai-functions/"),
    _p("gpt4all", "GPT4All", "local", "http://localhost:4891/v1", key="none",
       docs="https://docs.gpt4all.io/gpt4all_api_server/home.html"),
    _p("koboldcpp", "KoboldCpp", "local", "http://localhost:5001/v1", key="none",
       docs="https://github.com/LostRuins/koboldcpp/wiki"),
    _p("textgen", "text-generation-webui", "local", "http://localhost:5000/v1", key="optional",
       docs="https://github.com/oobabooga/text-generation-webui/wiki/12-%E2%80%90-OpenAI-API"),
    _p("sglang", "SGLang", "local", "http://localhost:30000/v1", key="optional",
       docs="https://docs.sglang.ai/backend/openai_api_completions.html"),
    _p("docker", "Docker Model Runner", "local", "http://localhost:12434/engines/v1", key="none",
       docs="https://docs.docker.com/ai/model-runner/"),
    _p("llamafile", "llamafile", "local", "http://localhost:8080/v1", key="none",
       docs="https://github.com/Mozilla-Ocho/llamafile"),

    # --- otro ---
    _p("custom", "Compatible con OpenAI", "custom", "", key="optional",
       fields=[{"name": "name", "label": "Nombre", "placeholder": "Servidor de la iglesia"}],
       note="Pon la URL base (la que termina en /v1) y, si hace falta, la clave y cabeceras."),
]

BY_ID = {p["id"]: p for p in CATALOG}

# En que orden se prueban los gratuitos con «Probar gratis, sin clave».
FREE_ORDER = [p["id"] for p in CATALOG if p["group"] == "free"]

# Variables de entorno que mandan sobre lo guardado. Para la linea de
# ordenes, las pruebas y quien prefiera no tocar la app.
ENV_PROVIDER, ENV_KEY, ENV_URL = "DANPLAY_AI_PROVIDER", "DANPLAY_AI_KEY", "DANPLAY_AI_BASE_URL"
ENV_MODEL, ENV_CHAT_MODEL = "DANPLAY_AI_MODEL", "DANPLAY_AI_CHAT_MODEL"

DEFAULT_TIMEOUT = 60          # segundos por peticion; los razonadores tardan
_KEEP = ("key", "model", "chat_model", "base_url", "fields", "headers", "extra",
         "timeout", "name")


def catalog() -> list[dict]:
    """El catalogo, tal cual, para la interfaz."""
    return [dict(p) for p in CATALOG]


def mask(key: str) -> str:
    """Una clave enseñable: principio y final, nunca entera."""
    key = str(key or "")
    if not key:
        return ""
    return (key[:4] + "…" + key[-4:]) if len(key) > 10 else "…"


# ----------------------------------------------------------------- almacen

_store: dict | None = None


def _read() -> dict:
    global _store
    if _store is not None:
        return _store
    data = None
    try:
        if PROFILES_FILE.is_file():
            data = json.loads(PROFILES_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        log.warning("no se pudo leer %s", PROFILES_FILE, exc_info=True)
    if not isinstance(data, dict):
        data = _migrate_legacy()
    data.setdefault("version", 1)
    data.setdefault("active", "")
    data.setdefault("profiles", {})
    _store = data
    return data


def _write(data: dict) -> None:
    global _store
    PROFILES_FILE.parent.mkdir(parents=True, exist_ok=True)
    PROFILES_FILE.write_text(json.dumps(data, ensure_ascii=False, indent=1),
                             encoding="utf-8")
    # lleva claves: solo lo lee su dueño (en Windows el perfil ya es privado)
    if os.name == "posix":
        os.chmod(PROFILES_FILE, 0o600)
    _store = data


def _migrate_legacy() -> dict:
    """La configuracion de antes (solo DeepInfra, en danplay.env) pasa a un
    perfil. Nadie tiene que volver a pegar su clave al actualizar."""
    data = {"version": 1, "active": "", "profiles": {}}
    key = os.getenv("DEEPINFRA_API_KEY", "")
    if key:
        data["profiles"]["deepinfra"] = {
            "provider": "deepinfra", "key": key,
            "model": config.env("DEEPINFRA_MODEL", "google/gemini-3.1-flash-lite"),
            "chat_model": config.env("DEEPINFRA_CHAT_MODEL", "Qwen/Qwen3-Next-80B-A3B-Instruct"),
            "base_url": (os.getenv("DEEPINFRA_BASE_URL", "")
                         if os.getenv("DEEPINFRA_BASE_URL", "") != BY_ID["deepinfra"]["base_url"]
                         else ""),
        }
        data["active"] = "deepinfra"
        try:
            _write(data)
        except OSError:
            log.warning("no se pudo guardar la migracion de la clave", exc_info=True)
    return data


def reload() -> None:
    """Olvida lo leido (para las pruebas y para cuando cambia el archivo)."""
    global _store
    _store = None


def profiles() -> dict:
    """Los perfiles guardados, con las claves enmascaradas."""
    out = {}
    for pid, prof in _read()["profiles"].items():
        out[pid] = public(pid, prof)
    return out


def public(pid: str, prof: dict) -> dict:
    """Un perfil sin secretos: lo unico que sale del nucleo."""
    p = BY_ID.get(prof.get("provider") or pid) or BY_ID["custom"]
    out = {k: prof.get(k) for k in _KEEP if k != "key"}
    out.update(id=pid, provider=p["id"], provider_name=prof.get("name") or p["name"],
               has_key=bool(prof.get("key")), key=mask(prof.get("key", "")),
               fields=dict(prof.get("fields") or {}), headers=dict(prof.get("headers") or {}),
               extra=dict(prof.get("extra") or {}))
    return out


def active_id() -> str:
    return os.getenv(ENV_PROVIDER) or _read().get("active", "")


def active() -> dict | None:
    """El perfil activo RESUELTO (URL rellena, clave, cabeceras, modelos), o
    None si no hay ninguno. Las variables DANPLAY_AI_* mandan sobre el
    archivo, campo a campo."""
    pid = active_id()
    if not pid:
        return None
    prof = dict(_read()["profiles"].get(pid) or {})
    if not prof and pid not in BY_ID:
        return None
    prof.setdefault("provider", pid if pid in BY_ID else "custom")
    for env_name, field in ((ENV_KEY, "key"), (ENV_URL, "base_url"),
                            (ENV_MODEL, "model"), (ENV_CHAT_MODEL, "chat_model")):
        value = os.getenv(env_name)
        if value:
            prof[field] = value
    return resolve(pid, prof)


def resolve(pid: str, prof: dict) -> dict | None:
    """Convierte lo guardado (o un borrador de la interfaz) en lo que necesita
    el cliente: URL definitiva, clave, cabeceras, modelos y tiempo limite."""
    p = BY_ID.get(prof.get("provider") or pid) or BY_ID["custom"]
    fields = {k: str(v).strip() for k, v in (prof.get("fields") or {}).items()}
    base_url = str(prof.get("base_url") or "").strip() or p["base_url"]
    try:
        base_url = base_url.format(**fields) if "{" in base_url else base_url
    except (KeyError, IndexError):
        pass
    if not base_url or "{" in base_url:
        return None
    headers = dict(p["headers"])
    headers.update({str(k): str(v) for k, v in (prof.get("headers") or {}).items()
                    if str(k).strip()})
    key = str(prof.get("key") or "").strip()
    model = str(prof.get("model") or "").strip() or p["suggest"].get("fast", "")
    chat_model = str(prof.get("chat_model") or "").strip() or p["suggest"].get("chat", "") or model
    if not model:
        model = chat_model
    try:
        timeout = float(prof.get("timeout") or DEFAULT_TIMEOUT)
    except (TypeError, ValueError):
        timeout = DEFAULT_TIMEOUT
    return {"id": pid, "provider": p["id"],
            "name": prof.get("name") or fields.get("name") or p["name"],
            "base_url": base_url, "key": key, "key_required": p["key"] == "required",
            "headers": headers, "model": model, "chat_model": chat_model,
            "extra": dict(prof.get("extra") or {}), "timeout": timeout,
            "quirks": dict(p["quirks"]), "models_dev": p["models_dev"],
            "local": p["group"] == "local",
            # cuantas veces reintenta el SDK un fallo de conexion (None = lo
            # normal: 2 en la nube, 0 en local); «Probar gratis» pide 0
            "retries": prof.get("retries")}


def usable(prof: dict | None) -> tuple[bool, str]:
    """Si con este perfil se puede llamar al modelo, y si no, por que."""
    if not prof:
        return False, "no hay ningun proveedor de IA elegido"
    if prof["key_required"] and not prof["key"]:
        return False, f"falta la clave de {prof['name']}"
    if not prof["model"] and not prof["chat_model"]:
        return False, "falta elegir el modelo"
    return True, ""


_SLUG = re.compile(r"[^a-z0-9]+")


def _new_custom_id(name: str) -> str:
    base = _SLUG.sub("-", (name or "servidor").lower()).strip("-")[:24] or "servidor"
    pid = "custom-" + base
    taken = _read()["profiles"]
    while pid in taken:
        pid = f"custom-{base}-{secrets.token_hex(2)}"
    return pid


def save_profile(data: dict, activate: bool = True) -> str:
    """Guarda (o completa) un perfil y devuelve su id.

    Lo que no venga se conserva: mandar solo `model` no borra la clave. Una
    clave vacia tampoco la borra (la caja de la clave llega vacia cuando el
    usuario no la ha vuelto a escribir); para quitarla esta `delete_profile`.
    """
    store = _read()
    provider = str(data.get("provider") or "").strip()
    pid = str(data.get("id") or "").strip()
    if provider not in BY_ID and pid not in store["profiles"]:
        raise ValueError("proveedor desconocido")
    if not pid:
        pid = _new_custom_id(str(data.get("name") or (data.get("fields") or {}).get("name") or "")) \
              if provider == "custom" else provider
    prof = dict(store["profiles"].get(pid) or {})
    prof["provider"] = provider or prof.get("provider") or pid
    for k in _KEEP:
        if k not in data or data[k] is None:
            continue
        v = data[k]
        if k == "key":
            v = str(v).replace("\r", "").replace("\n", "").strip()
            if not v:
                continue
        elif k in ("fields", "headers", "extra"):
            v = {str(a): b for a, b in dict(v or {}).items() if str(a).strip()}
        elif k == "timeout":
            try:
                v = max(5.0, min(600.0, float(v)))
            except (TypeError, ValueError):
                continue
        else:
            v = str(v).strip()
        prof[k] = v
    if prof["provider"] == "custom" and not prof.get("name"):
        prof["name"] = (prof.get("fields") or {}).get("name") or "Servidor propio"
    store["profiles"][pid] = prof
    if activate or not store.get("active"):
        store["active"] = pid
    _write(store)
    return pid


def delete_profile(pid: str) -> None:
    store = _read()
    store["profiles"].pop(pid, None)
    if store.get("active") == pid:
        store["active"] = next(iter(store["profiles"]), "")
    _write(store)


def activate(pid: str) -> None:
    store = _read()
    if pid not in store["profiles"] and pid not in BY_ID:
        raise ValueError("ese proveedor no esta configurado")
    if pid not in store["profiles"]:
        # un local sin clave: vale tal cual, con sus valores por defecto
        store["profiles"][pid] = {"provider": pid}
    store["active"] = pid
    _write(store)


def fallback_enabled() -> bool:
    """Si, cuando el activo falla, se prueba con los demas configurados."""
    return bool(_read().get("fallback", True))


def set_fallback(enabled: bool) -> None:
    store = _read()
    store["fallback"] = bool(enabled)
    _write(store)


def budget() -> float:
    """Tope de gasto al mes en dolares (0 = sin aviso). Solo avisa: la IA no
    se corta sola, que a mitad de un ensayo seria peor."""
    try:
        return max(0.0, float(_read().get("budget") or 0))
    except (TypeError, ValueError):
        return 0.0


def set_budget(value) -> None:
    store = _read()
    try:
        store["budget"] = max(0.0, float(value or 0))
    except (TypeError, ValueError):
        store["budget"] = 0.0
    _write(store)


def fallbacks(active_id: str = "") -> list[dict]:
    """Los demas perfiles usables, resueltos, en el orden en que se
    guardaron. Un respaldo sin clave (un local apagado) tambien cuenta: el
    cliente lo descarta en un momento si no responde."""
    out = []
    for pid, prof in _read()["profiles"].items():
        if pid == active_id:
            continue
        r = resolve(pid, dict(prof, provider=prof.get("provider") or pid))
        if r and usable(r)[0]:
            out.append(r)
    return out


def with_saved_key(draft: dict) -> dict:
    """Un borrador de la interfaz con la clave guardada, si no trae una.

    Al probar un proveedor ya configurado, la caja de la clave llega vacia
    (nunca se le enseña entera); se usa la que hay.
    """
    d = dict(draft or {})
    if not str(d.get("key") or "").strip():
        pid = str(d.get("id") or d.get("provider") or "")
        saved = _read()["profiles"].get(pid) or {}
        if saved.get("key"):
            d["key"] = saved["key"]
    return d
