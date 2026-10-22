//! El catálogo de proveedores (lo que era `providers.CATALOG`), con sus
//! comentarios. Salió de providers.py; ahora se cambia aquí.
use super::p;
use serde_json::{Value, json};

/// Cabeceras con las que OpenRouter atribuye el uso a la app (opcionales).
fn openrouter_headers() -> Value {
    json!({"HTTP-Referer": "https://github.com/dani17r/danplay", "X-OpenRouter-Title": "DanPlay"})
}

/// Los grupos en que se enseñan, en orden.
pub fn groups() -> Value {
    json!([
        {"id": "free", "name": "Gratis, sin clave", "note": "Para probar sin registrarte en ningun sitio. Son servicios de terceros con limites y sin garantia: tus preguntas y los nombres de tus canciones pasan por ellos."},
        {"id": "lab", "name": "Grandes laboratorios", "note": "Los que entrenan los modelos. Clave de pago, salvo donde se indica."},
        {"id": "platform", "name": "Plataformas de inferencia", "note": "Como DeepInfra: sirven cientos de modelos abiertos y de terceros con una sola clave."},
        {"id": "cloud", "name": "Nubes corporativas", "note": "Si tu empresa o iglesia ya tiene cuenta en Azure, AWS, Cloudflare o Databricks."},
        {"id": "asia", "name": "Proveedores de Asia", "note": "Qwen, Kimi, GLM, MiniMax y compañia, desde su propia API."},
        {"id": "local", "name": "En tu equipo", "note": "Sin clave y sin internet: el modelo corre en tu ordenador."},
        {"id": "custom", "name": "Otro", "note": "Cualquier servicio compatible con la API de OpenAI: un proxy, un servidor privado, LiteLLM…"},
    ])
}

/// Todos los proveedores, en el orden en que se enseñan.
/// `key`: required | optional | none.  `fields`: huecos de la URL que el
/// usuario rellena ({resource}, {region}…).  `quirks`: lo que se sabe de
/// antemano que el servicio NO acepta, para no mandarselo y comerse el error;
/// `anon_filter` deja en la lista solo lo que se puede usar sin clave;
/// `strip_prefix`, lo que sobra delante de los ids que da su /models.
#[allow(clippy::too_many_lines, reason = "es una lista, de una vez")]
pub fn catalog() -> Vec<Value> {
    let openrouter = openrouter_headers();
    vec![
        // --- gratis, sin clave ---
        // Comprobados el 11-09-2026 con peticiones anonimas de verdad (con
        // herramientas incluidas). Van los primeros de FREE_ORDER: «Probar gratis»
        // activa el primero que responda. Cambian sin avisar: Pollinations dejo
        // de servir anonimos ese mismo mes.
        p(
            "llm7",
            "LLM7",
            "free",
            "https://api.llm7.io/v1",
            json!({
                "key": "optional",
                "key_url": "https://token.llm7.io",
                "docs": "https://llm7.io",
                "suggest": {"fast": "minimax-m2.7", "chat": "minimax-m2.7"},
                "quirks": {"anon_filter": {"field": "tier", "value": "turbo"}},
                "note": "Sin cuenta: 10 peticiones por minuto y 60 por hora, con los modelos «turbo». Un token gratuito de token.llm7.io sube el limite.",
            }),
        ),
        p(
            "kilo",
            "Kilo (rutas :free)",
            "free",
            "https://api.kilo.ai/api/gateway",
            json!({
                "key": "optional",
                "key_url": "https://app.kilo.ai/profile",
                "docs": "https://kilo.ai",
                "models_dev": "kilo",
                "suggest": {"fast": "nvidia/nemotron-3.5-lightning:free", "chat": "nvidia/nemotron-3-super-120b-a12b:free"},
                "quirks": {"anon_filter": {"field": "isFree", "value": true}},
                "note": "Sin cuenta valen solo los modelos «:free». Algunos pueden entrenar con lo que les mandas: la lista lo marca.",
            }),
        ),
        p(
            "opencode-zen",
            "OpenCode Zen (gratis)",
            "free",
            "https://opencode.ai/zen/v1",
            json!({
                "key": "optional",
                "key_url": "https://opencode.ai/auth",
                "docs": "https://opencode.ai/docs/zen",
                "models_dev": "opencode",
                "suggest": {"fast": "nemotron-3.5-lightning-free", "chat": "deepseek-v4-flash-free"},
                "quirks": {"anon_filter": {"suffix": "-free"}},
                "note": "Acceso promocional a los modelos «-free»; puede desaparecer cualquier dia.",
            }),
        ),
        // --- grandes laboratorios ---
        p(
            "openai",
            "OpenAI",
            "lab",
            "https://api.openai.com/v1",
            json!({
                "key_url": "https://platform.openai.com/api-keys",
                "docs": "https://developers.openai.com/api/docs/models",
                "models_dev": "openai",
                "suggest": {"fast": "gpt-5.6-luna", "chat": "gpt-5.6-terra"},
            }),
        ),
        p(
            "anthropic",
            "Anthropic (Claude)",
            "lab",
            "https://api.anthropic.com/v1/",
            json!({
                "key_url": "https://platform.claude.com/settings/keys",
                "docs": "https://platform.claude.com/docs/en/api/openai-sdk",
                "models_dev": "anthropic",
                "suggest": {"fast": "claude-haiku-4-5", "chat": "claude-sonnet-5"},
                // su capa compatible ignora response_format: el JSON se pide en el texto
                "quirks": {"json_mode": false},
                // la API nativa (/v1/models) exige la version; en la compatible sobra
                "headers": {"anthropic-version": "2023-06-01"},
                "note": "Capa compatible oficial de Anthropic; las herramientas funcionan.",
            }),
        ),
        p(
            "google",
            "Google Gemini",
            "lab",
            "https://generativelanguage.googleapis.com/v1beta/openai/",
            json!({
                "key_url": "https://aistudio.google.com/app/apikey",
                "docs": "https://ai.google.dev/gemini-api/docs/openai",
                "models_dev": "google",
                "suggest": {"fast": "gemini-3.5-flash-lite", "chat": "gemini-3.8-flash"},
                "note": "Con clave de AI Studio. Tiene nivel gratuito.",
                // su /models dice «models/gemini-3.8-flash»; acepta los dos, y sin el
                // prefijo se cruza con el catalogo
                "quirks": {"strip_prefix": "models/"},
            }),
        ),
        p(
            "mistral",
            "Mistral AI",
            "lab",
            "https://api.mistral.ai/v1",
            json!({
                "key_url": "https://console.mistral.ai/api-keys",
                "docs": "https://docs.mistral.ai/getting-started/models/",
                "models_dev": "mistral",
                "note": "Tiene nivel gratuito.",
            }),
        ),
        p(
            "xai",
            "xAI (Grok)",
            "lab",
            "https://api.x.ai/v1",
            json!({
                "key_url": "https://console.x.ai",
                "docs": "https://docs.x.ai/docs/models",
                "models_dev": "xai",
            }),
        ),
        p(
            "deepseek",
            "DeepSeek",
            "lab",
            "https://api.deepseek.com/v1",
            json!({
                "key_url": "https://platform.deepseek.com/api_keys",
                "docs": "https://api-docs.deepseek.com/quick_start/pricing",
                "models_dev": "deepseek",
                "suggest": {"fast": "deepseek-flash", "chat": "deepseek-flash"},
            }),
        ),
        p(
            "cohere",
            "Cohere",
            "lab",
            "https://api.cohere.ai/compatibility/v1",
            json!({
                "key_url": "https://dashboard.cohere.com/api-keys",
                "docs": "https://docs.cohere.com/docs/compatibility-api",
                "models_dev": "cohere",
                "note": "Tiene clave de prueba gratuita.",
            }),
        ),
        p(
            "perplexity",
            "Perplexity",
            "lab",
            "https://api.perplexity.ai",
            json!({
                "key_url": "https://www.perplexity.ai/settings/api",
                "docs": "https://docs.perplexity.ai",
                "models_dev": "perplexity",
                "note": "Sus modelos buscan en la web por su cuenta.",
            }),
        ),
        p(
            "ai21",
            "AI21 Labs",
            "lab",
            "https://api.ai21.com/studio/v1",
            json!({
                "key_url": "https://studio.ai21.com/account/api-key",
                "docs": "https://docs.ai21.com",
            }),
        ),
        p(
            "inception",
            "Inception (Mercury)",
            "lab",
            "https://api.inceptionlabs.ai/v1",
            json!({
                "key_url": "https://platform.inceptionlabs.ai",
                "docs": "https://docs.inceptionlabs.ai",
                "models_dev": "inception",
            }),
        ),
        p(
            "meta",
            "Meta (Llama API)",
            "lab",
            "https://api.llama.com/compat/v1/",
            json!({
                "key_url": "https://llama.developer.meta.com",
                "docs": "https://llama.developer.meta.com/docs",
                "models_dev": "llama",
            }),
        ),
        // --- plataformas de inferencia ---
        p(
            "deepinfra",
            "DeepInfra",
            "platform",
            "https://api.deepinfra.com/v1/openai",
            json!({
                "key_url": "https://deepinfra.com/dash/api_keys",
                "docs": "https://deepinfra.com/models",
                "models_dev": "deepinfra",
                "suggest": {"fast": "google/gemini-3.1-flash-lite", "chat": "Qwen/Qwen3-Next-80B-A3B-Instruct"},
            }),
        ),
        p(
            "openrouter",
            "OpenRouter",
            "platform",
            "https://openrouter.ai/api/v1",
            json!({
                "key_url": "https://openrouter.ai/settings/keys",
                "docs": "https://openrouter.ai/docs/quickstart",
                "models_dev": "openrouter",
                "headers": openrouter.clone(),
                "suggest": {"fast": "google/gemini-3.5-flash-lite", "chat": "google/gemini-3.8-flash"},
                "note": "Una clave para todos los modelos del mercado. Tiene modelos gratuitos.",
            }),
        ),
        p(
            "together",
            "Together AI",
            "platform",
            "https://api.together.xyz/v1",
            json!({
                "key_url": "https://api.together.ai/settings/api-keys",
                "docs": "https://docs.together.ai/docs/serverless-models",
                "models_dev": "togetherai",
            }),
        ),
        p(
            "fireworks",
            "Fireworks AI",
            "platform",
            "https://api.fireworks.ai/inference/v1",
            json!({
                "key_url": "https://fireworks.ai/account/api-keys",
                "docs": "https://fireworks.ai/docs/",
                "models_dev": "fireworks-ai",
            }),
        ),
        p(
            "groq",
            "Groq",
            "platform",
            "https://api.groq.com/openai/v1",
            json!({
                "key_url": "https://console.groq.com/keys",
                "docs": "https://console.groq.com/docs/models",
                "models_dev": "groq",
                "suggest": {"fast": "llama-3.1-8b-instant", "chat": "llama-3.3-70b-versatile"},
                "note": "Muy rapido. Tiene nivel gratuito.",
            }),
        ),
        p(
            "cerebras",
            "Cerebras",
            "platform",
            "https://api.cerebras.ai/v1",
            json!({
                "key_url": "https://cloud.cerebras.ai/",
                "docs": "https://inference-docs.cerebras.ai",
                "models_dev": "cerebras",
                "note": "Muy rapido. Tiene nivel gratuito.",
            }),
        ),
        p(
            "sambanova",
            "SambaNova",
            "platform",
            "https://api.sambanova.ai/v1",
            json!({
                "key_url": "https://cloud.sambanova.ai/apis",
                "docs": "https://docs.sambanova.ai",
                "note": "Tiene nivel gratuito.",
            }),
        ),
        p(
            "huggingface",
            "Hugging Face",
            "platform",
            "https://router.huggingface.co/v1",
            json!({
                "key_url": "https://huggingface.co/settings/tokens",
                "docs": "https://huggingface.co/inference/get-started",
                "models_dev": "huggingface",
                "note": "Enruta a Groq, Together, Cerebras, DeepInfra… con un token de Hugging Face.",
            }),
        ),
        p(
            "nvidia",
            "NVIDIA NIM",
            "platform",
            "https://integrate.api.nvidia.com/v1",
            json!({
                "key_url": "https://build.nvidia.com/settings/api-keys",
                "docs": "https://docs.api.nvidia.com",
                "models_dev": "nvidia",
                "note": "Tiene nivel gratuito.",
            }),
        ),
        p(
            "novita",
            "Novita AI",
            "platform",
            "https://api.novita.ai/openai",
            json!({
                "key_url": "https://novita.ai/settings/key-management",
                "docs": "https://docs.novita.ai/guides/llm-api",
                "models_dev": "novita-ai",
            }),
        ),
        p(
            "nebius",
            "Nebius Token Factory",
            "platform",
            "https://api.tokenfactory.nebius.com/v1",
            json!({
                "key_url": "https://tokenfactory.nebius.com/settings/api-keys",
                "docs": "https://docs.tokenfactory.nebius.com/quickstart",
                "models_dev": "nebius",
            }),
        ),
        p(
            "hyperbolic",
            "Hyperbolic",
            "platform",
            "https://api.hyperbolic.xyz/v1",
            json!({
                "key_url": "https://app.hyperbolic.xyz/settings",
                "docs": "https://docs.hyperbolic.xyz",
            }),
        ),
        p(
            "baseten",
            "Baseten",
            "platform",
            "https://inference.baseten.co/v1",
            json!({
                "key_url": "https://app.baseten.co/settings/api_keys",
                "docs": "https://docs.baseten.co",
                "models_dev": "baseten",
            }),
        ),
        p(
            "lambda",
            "Lambda",
            "platform",
            "https://api.lambda.ai/v1",
            json!({
                "key_url": "https://cloud.lambda.ai/api-keys",
                "docs": "https://docs.lambda.ai/public-cloud/lambda-inference-api/",
            }),
        ),
        p(
            "chutes",
            "Chutes",
            "platform",
            "https://llm.chutes.ai/v1",
            json!({
                "key_url": "https://chutes.ai/",
                "docs": "https://docs.chutes.ai",
                "models_dev": "chutes",
            }),
        ),
        p(
            "featherless",
            "Featherless",
            "platform",
            "https://api.featherless.ai/v1",
            json!({
                "key_url": "https://featherless.ai/account/api-keys",
                "docs": "https://featherless.ai/docs",
            }),
        ),
        p(
            "nscale",
            "Nscale",
            "platform",
            "https://inference.api.nscale.com/v1",
            json!({
                "key_url": "https://console.nscale.com/",
                "docs": "https://docs.nscale.com",
            }),
        ),
        p(
            "ovhcloud",
            "OVHcloud AI Endpoints",
            "platform",
            "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
            json!({
                "key_url": "https://www.ovhcloud.com/en/public-cloud/ai-endpoints/",
                "docs": "https://help.ovhcloud.com/csm/en-public-cloud-ai-endpoints-getting-started",
                "models_dev": "ovhcloud",
            }),
        ),
        p(
            "scaleway",
            "Scaleway",
            "platform",
            "https://api.scaleway.ai/v1",
            json!({
                "key_url": "https://console.scaleway.com/iam/api-keys",
                "docs": "https://www.scaleway.com/en/docs/generative-apis/",
                "models_dev": "scaleway",
            }),
        ),
        p(
            "github",
            "GitHub Models",
            "platform",
            "https://models.github.ai/inference",
            json!({
                "key_url": "https://github.com/settings/personal-access-tokens",
                "docs": "https://docs.github.com/en/github-models",
                "note": "Con un token personal de GitHub. Tiene nivel gratuito.",
            }),
        ),
        p(
            "requesty",
            "Requesty",
            "platform",
            "https://router.requesty.ai/v1",
            json!({
                "key_url": "https://app.requesty.ai/api-keys",
                "docs": "https://docs.requesty.ai",
                "models_dev": "requesty",
            }),
        ),
        p(
            "vercel",
            "Vercel AI Gateway",
            "platform",
            "https://ai-gateway.vercel.sh/v1",
            json!({
                "key_url": "https://vercel.com/docs/ai-gateway",
                "docs": "https://vercel.com/docs/ai-gateway",
                "models_dev": "vercel",
            }),
        ),
        p(
            "poe",
            "Poe",
            "platform",
            "https://api.poe.com/v1",
            json!({
                "key_url": "https://poe.com/api_key",
                "docs": "https://creator.poe.com/docs/external-applications/openai-compatible-api",
                "models_dev": "poe",
            }),
        ),
        p(
            "ollama-cloud",
            "Ollama Cloud",
            "platform",
            "https://ollama.com/v1",
            json!({
                "key_url": "https://ollama.com/settings/keys",
                "docs": "https://docs.ollama.com/cloud",
                "models_dev": "ollama-cloud",
            }),
        ),
        p(
            "nanogpt",
            "NanoGPT",
            "platform",
            "https://nano-gpt.com/api/v1",
            json!({
                "key_url": "https://nano-gpt.com/api",
                "docs": "https://docs.nano-gpt.com",
                "models_dev": "nano-gpt",
            }),
        ),
        p(
            "aimlapi",
            "AI/ML API",
            "platform",
            "https://api.aimlapi.com/v1",
            json!({
                "key_url": "https://aimlapi.com/app/keys",
                "docs": "https://docs.aimlapi.com",
            }),
        ),
        p(
            "venice",
            "Venice",
            "platform",
            "https://api.venice.ai/api/v1",
            json!({
                "key_url": "https://venice.ai/settings/api",
                "docs": "https://docs.venice.ai",
                "models_dev": "venice",
                "note": "Sin registros de lo que preguntas, segun ellos.",
            }),
        ),
        // --- nubes corporativas ---
        p(
            "azure",
            "Azure OpenAI",
            "cloud",
            "https://{resource}.openai.azure.com/openai/v1/",
            json!({
                "key_url": "https://portal.azure.com",
                "models_dev": "azure",
                "docs": "https://learn.microsoft.com/azure/foundry/openai/api-version-lifecycle",
                "fields": [{"name": "resource", "label": "Nombre del recurso", "placeholder": "mi-recurso"}],
                "note": "El modelo es el NOMBRE DEL DESPLIEGUE que creaste en Azure.",
            }),
        ),
        p(
            "bedrock",
            "Amazon Bedrock",
            "cloud",
            "https://bedrock-runtime.{region}.amazonaws.com/openai/v1",
            json!({
                "key_url": "https://console.aws.amazon.com/bedrock/home#/api-keys",
                "models_dev": "amazon-bedrock",
                "docs": "https://docs.aws.amazon.com/bedrock/latest/userguide/inference-chat-completions.html",
                "fields": [{"name": "region", "label": "Region", "placeholder": "us-east-1"}],
                "note": "Con una clave de API de Bedrock (no las credenciales de AWS).",
            }),
        ),
        p(
            "cloudflare",
            "Cloudflare Workers AI",
            "cloud",
            "https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1",
            json!({
                "key_url": "https://dash.cloudflare.com/profile/api-tokens",
                "models_dev": "cloudflare-workers-ai",
                "docs": "https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/",
                "fields": [{"name": "account_id", "label": "ID de la cuenta", "placeholder": "0123abcd…"}],
                "note": "Tiene nivel gratuito.",
            }),
        ),
        p(
            "databricks",
            "Databricks",
            "cloud",
            "https://{workspace}/serving-endpoints",
            json!({
                "key_url": "https://docs.databricks.com/aws/en/dev-tools/auth/pat",
                "models_dev": "databricks",
                "docs": "https://docs.databricks.com/aws/en/machine-learning/model-serving/",
                "fields": [{"name": "workspace", "label": "Dominio del espacio de trabajo", "placeholder": "adb-1234.5.azuredatabricks.net"}],
            }),
        ),
        // --- Asia ---
        p(
            "alibaba",
            "Alibaba Cloud (Qwen)",
            "asia",
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            json!({
                "key_url": "https://bailian.console.alibabacloud.com/?apiKey=1",
                "docs": "https://www.alibabacloud.com/help/en/model-studio/",
                "models_dev": "alibaba",
            }),
        ),
        p(
            "moonshot",
            "Moonshot (Kimi)",
            "asia",
            "https://api.moonshot.ai/v1",
            json!({
                "key_url": "https://platform.moonshot.ai/console/api-keys",
                "docs": "https://platform.moonshot.ai/docs",
                "models_dev": "moonshotai",
            }),
        ),
        p(
            "zai",
            "Z.ai (GLM)",
            "asia",
            "https://api.z.ai/api/paas/v4",
            json!({
                "key_url": "https://z.ai/manage-apikey/apikey-list",
                "docs": "https://docs.z.ai",
                "models_dev": "zai",
            }),
        ),
        p(
            "minimax",
            "MiniMax",
            "asia",
            "https://api.minimax.io/v1",
            json!({
                "key_url": "https://platform.minimax.io/user-center/basic-information/interface-key",
                "docs": "https://platform.minimax.io/docs/api-reference/text-openai-api",
                "models_dev": "minimax",
            }),
        ),
        p(
            "stepfun",
            "StepFun",
            "asia",
            "https://api.stepfun.com/v1",
            json!({
                "key_url": "https://platform.stepfun.com/interface-key",
                "docs": "https://platform.stepfun.com/docs",
                "models_dev": "stepfun",
            }),
        ),
        p(
            "siliconflow",
            "SiliconFlow",
            "asia",
            "https://api.siliconflow.com/v1",
            json!({
                "key_url": "https://cloud.siliconflow.com/account/ak",
                "docs": "https://docs.siliconflow.com",
                "models_dev": "siliconflow",
            }),
        ),
        p(
            "modelscope",
            "ModelScope",
            "asia",
            "https://api-inference.modelscope.cn/v1",
            json!({
                "key_url": "https://modelscope.cn/my/myaccesstoken",
                "docs": "https://modelscope.cn/docs/model-service/API-Inference/intro",
                "models_dev": "modelscope",
                "note": "Tiene nivel gratuito.",
            }),
        ),
        p(
            "volcengine",
            "Volcengine (Doubao)",
            "asia",
            "https://ark.cn-beijing.volces.com/api/v3",
            json!({
                "key_url": "https://console.volcengine.com/ark",
                "docs": "https://www.volcengine.com/docs/82379",
                "models_dev": "volcengine",
            }),
        ),
        // --- en tu equipo ---
        p(
            "ollama",
            "Ollama",
            "local",
            "http://localhost:11434/v1",
            json!({
                "key": "none",
                "docs": "https://docs.ollama.com/api/openai-compatibility",
                "suggest": {"fast": "qwen3:8b", "chat": "qwen3:8b"},
                "note": "Descarga antes un modelo con «ollama pull». El asistente necesita uno que sepa usar herramientas.",
            }),
        ),
        p(
            "lmstudio",
            "LM Studio",
            "local",
            "http://localhost:1234/v1",
            json!({
                "key": "none",
                "docs": "https://lmstudio.ai/docs/developer/openai-compat",
                "models_dev": "lmstudio",
                "note": "Arranca el servidor en la pestaña Developer de LM Studio.",
            }),
        ),
        p(
            "llamacpp",
            "llama.cpp (llama-server)",
            "local",
            "http://localhost:8080/v1",
            json!({
                "key": "optional",
                "docs": "https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md",
            }),
        ),
        p(
            "vllm",
            "vLLM",
            "local",
            "http://localhost:8000/v1",
            json!({
                "key": "optional",
                "docs": "https://docs.vllm.ai/en/latest/serving/openai_compatible_server/",
                "note": "Para el asistente, arranca vLLM con --enable-auto-tool-choice.",
            }),
        ),
        p(
            "jan",
            "Jan",
            "local",
            "http://localhost:1337/v1",
            json!({
                "key": "optional",
                "docs": "https://www.jan.ai/docs/desktop/api-server",
            }),
        ),
        p(
            "localai",
            "LocalAI",
            "local",
            "http://localhost:8080/v1",
            json!({
                "key": "optional",
                "docs": "https://localai.io/features/openai-functions/",
            }),
        ),
        p(
            "gpt4all",
            "GPT4All",
            "local",
            "http://localhost:4891/v1",
            json!({
                "key": "none",
                "docs": "https://docs.gpt4all.io/gpt4all_api_server/home.html",
            }),
        ),
        p(
            "koboldcpp",
            "KoboldCpp",
            "local",
            "http://localhost:5001/v1",
            json!({
                "key": "none",
                "docs": "https://github.com/LostRuins/koboldcpp/wiki",
            }),
        ),
        p(
            "textgen",
            "text-generation-webui",
            "local",
            "http://localhost:5000/v1",
            json!({
                "key": "optional",
                "docs": "https://github.com/oobabooga/text-generation-webui/wiki/12-%E2%80%90-OpenAI-API",
            }),
        ),
        p(
            "sglang",
            "SGLang",
            "local",
            "http://localhost:30000/v1",
            json!({
                "key": "optional",
                "docs": "https://docs.sglang.ai/backend/openai_api_completions.html",
            }),
        ),
        p(
            "docker",
            "Docker Model Runner",
            "local",
            "http://localhost:12434/engines/v1",
            json!({
                "key": "none",
                "docs": "https://docs.docker.com/ai/model-runner/",
            }),
        ),
        p(
            "llamafile",
            "llamafile",
            "local",
            "http://localhost:8080/v1",
            json!({
                "key": "none",
                "docs": "https://github.com/Mozilla-Ocho/llamafile",
            }),
        ),
        // --- otro ---
        p(
            "custom",
            "Compatible con OpenAI",
            "custom",
            "",
            json!({
                "key": "optional",
                "fields": [{"name": "name", "label": "Nombre", "placeholder": "Servidor de la iglesia"}],
                "note": "Pon la URL base (la que termina en /v1) y, si hace falta, la clave y cabeceras.",
            }),
        ),
    ]
}
