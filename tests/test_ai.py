# -*- coding: utf-8 -*-
"""La IA con cualquier proveedor: catalogo, perfiles guardados, el catalogo
de modelos (models.dev) y las tolerancias del cliente.

Nada de aqui toca la red ni la configuracion real del usuario: los perfiles
van a un archivo temporal y el «proveedor» es un cliente falso que devuelve
lo que cada prueba le diga.
"""
import json
import os
import pathlib
import sys
import pytest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

from danplay import ai, config, model_catalog, providers  # noqa: E402


@pytest.fixture
def perfiles(tmp_path, monkeypatch):
    """Perfiles en un archivo temporal, sin variables de entorno que manden."""
    monkeypatch.setattr(providers, "PROFILES_FILE", tmp_path / "ai.json")
    for v in ("DANPLAY_AI_PROVIDER", "DANPLAY_AI_KEY", "DANPLAY_AI_BASE_URL",
              "DANPLAY_AI_MODEL", "DANPLAY_AI_CHAT_MODEL", "DEEPINFRA_API_KEY"):
        monkeypatch.delenv(v, raising=False)
    monkeypatch.setattr(config, "AI_ENABLED", True)
    providers.reload()
    ai.reset_client()
    yield tmp_path / "ai.json"
    providers.reload()
    ai.reset_client()


class _Msg:
    def __init__(self, content="", tool_calls=None):
        self.content, self.tool_calls = content, tool_calls


class _FakeClient:
    """Un proveedor de mentira: `rejects` dice que parametros rechaza (con el
    texto de error que daria), `answers` lo que contesta."""

    def __init__(self, rejects=None, answer="ok", models=()):
        self.rejects = rejects or {}
        self.answer = answer
        self.calls = []
        self.model_ids = list(models)
        outer = self

        class _Completions:
            @staticmethod
            def create(**kw):
                outer.calls.append(dict(kw))
                for param, text in outer.rejects.items():
                    present = (param in kw) if param != "tool_choice" else kw.get("tool_choice") == "required"
                    if present:
                        e = Exception(text)
                        e.status_code = 400
                        raise e
                return type("r", (), {"choices": [type("c", (), {"message": _Msg(outer.answer)})()]})()

        class _Models:
            @staticmethod
            def list():
                return type("p", (), {"data": [type("m", (), {"id": i, "to_dict": (lambda i=i: {"id": i})})()
                                               for i in outer.model_ids]})()

        self.chat = type("chat", (), {"completions": _Completions})()
        self.models = _Models()


# ------------------------------------------------------------- catalogo

def test_el_catalogo_tiene_los_grupos_y_urls_bien_formadas():
    ids = {p["id"] for p in providers.CATALOG}
    for wanted in ("openai", "anthropic", "google", "deepinfra", "openrouter", "groq",
                   "ollama", "lmstudio", "azure", "bedrock", "custom"):
        assert wanted in ids, wanted
    groups = {g["id"] for g in providers.GROUPS}
    for p in providers.CATALOG:
        assert p["group"] in groups, p["id"]
        assert p["key"] in ("required", "optional", "none"), p["id"]
        if p["id"] != "custom":
            assert p["base_url"].startswith(("http://", "https://")), p["id"]
        # cada hueco de la URL tiene su campo en el formulario
        for hole in {h for h in __import__("re").findall(r"\{(\w+)\}", p["base_url"])}:
            assert any(f["name"] == hole for f in p["fields"]), f"{p['id']}: falta el campo {hole}"
    assert len(ids) == len(providers.CATALOG), "ids repetidos"


def test_los_locales_no_piden_clave_y_apuntan_a_este_equipo():
    for p in providers.CATALOG:
        if p["group"] == "local":
            assert p["key"] != "required", p["id"]
            assert "localhost" in p["base_url"], p["id"]


# -------------------------------------------------------------- perfiles

def test_sin_nada_configurado_la_ia_no_esta_disponible(perfiles):
    assert providers.active() is None
    assert not ai.available()
    assert "proveedor" in ai.unavailable_reason()


def test_guardar_un_perfil_lo_activa_y_la_clave_no_vuelve_entera(perfiles):
    pid = providers.save_profile({"provider": "openrouter", "key": "sk-or-v1-0123456789abcdef",
                                  "model": "a", "chat_model": "b"})
    assert pid == "openrouter"
    assert providers.active_id() == "openrouter"
    p = providers.active()
    assert p["base_url"] == "https://openrouter.ai/api/v1"
    assert p["key"] == "sk-or-v1-0123456789abcdef"
    assert p["headers"]["X-OpenRouter-Title"] == "DanPlay"
    public = providers.profiles()["openrouter"]
    assert "0123456789" not in json.dumps(public)
    assert public["has_key"] and public["key"].startswith("sk-o") and "…" in public["key"]
    assert ai.available()
    # y el archivo es solo del dueño
    if os.name == "posix":
        assert oct(perfiles.stat().st_mode & 0o777) == "0o600"


def test_guardar_sin_clave_conserva_la_guardada(perfiles):
    providers.save_profile({"provider": "groq", "key": "gsk_secreto123456", "model": "x"})
    providers.save_profile({"id": "groq", "provider": "groq", "model": "y", "key": ""})
    p = providers.active()
    assert p["key"] == "gsk_secreto123456" and p["model"] == "y"


def test_los_huecos_de_la_url_se_rellenan_con_los_campos(perfiles):
    providers.save_profile({"provider": "bedrock", "key": "k", "fields": {"region": "eu-west-1"},
                            "model": "m"})
    assert providers.active()["base_url"] == "https://bedrock-runtime.eu-west-1.amazonaws.com/openai/v1"
    # sin rellenar el hueco no hay URL, y por tanto no hay proveedor usable
    providers.save_profile({"provider": "azure", "key": "k", "model": "m"})
    assert providers.active() is None
    assert not ai.available()


def test_varios_servidores_propios_conviven_y_se_puede_cambiar(perfiles):
    a = providers.save_profile({"provider": "custom", "name": "Casa", "base_url": "http://casa:8080/v1",
                                "model": "m"})
    b = providers.save_profile({"provider": "custom", "name": "Casa", "base_url": "http://otra:8080/v1",
                                "model": "m"})
    assert a != b and a.startswith("custom-") and b.startswith("custom-")
    assert providers.active_id() == b
    providers.activate(a)
    assert providers.active()["base_url"] == "http://casa:8080/v1"
    providers.delete_profile(a)
    assert providers.active_id() == b


def test_un_local_sin_clave_se_activa_tal_cual(perfiles):
    providers.activate("ollama")
    p = providers.active()
    assert p["base_url"] == "http://localhost:11434/v1" and p["local"]
    assert not p["key_required"]
    assert ai.available()


def test_las_variables_de_entorno_mandan_sobre_el_archivo(perfiles, monkeypatch):
    providers.save_profile({"provider": "openai", "key": "sk-archivo", "model": "gpt"})
    monkeypatch.setenv("DANPLAY_AI_KEY", "sk-entorno")
    monkeypatch.setenv("DANPLAY_AI_MODEL", "otro")
    p = providers.active()
    assert p["key"] == "sk-entorno" and p["model"] == "otro"
    monkeypatch.setenv("DANPLAY_AI_PROVIDER", "ollama")
    assert providers.active()["provider"] == "ollama"


def test_la_clave_de_deepinfra_de_antes_se_migra_sola(perfiles, monkeypatch):
    monkeypatch.setenv("DEEPINFRA_API_KEY", "clave-vieja")
    monkeypatch.setenv("DEEPINFRA_MODEL", "modelo-viejo")
    providers.reload()
    p = providers.active()
    assert p["provider"] == "deepinfra" and p["key"] == "clave-vieja"
    assert p["model"] == "modelo-viejo"
    assert perfiles.is_file(), "queda escrita para no depender mas de la variable"


# ------------------------------------------------------ catalogo de modelos

def test_el_recorte_del_catalogo_deja_solo_conversacion():
    raw = {"x": {"name": "X", "models": {
        "chat-1": {"name": "Chat", "tool_call": True, "cost": {"input": 1, "output": 2},
                   "limit": {"context": 1000}, "release_date": "2026-01-01",
                   "modalities": {"input": ["text"], "output": ["text"]}},
        "embed-1": {"name": "Embed", "modalities": {"input": ["text"], "output": ["embedding"]}},
        "tts-1": {"name": "Voz", "modalities": {"input": ["text"], "output": ["audio"]}},
        "whisper-x": {"name": "Whisper"},
        "old": {"name": "Old", "status": "deprecated", "tool_call": False},
    }}}
    out = model_catalog._trim(raw)
    assert set(out["x"]["models"]) == {"chat-1", "old"}
    m = out["x"]["models"]["chat-1"]
    assert m["tools"] and m["cost_out"] == 2 and m["context"] == 1000
    assert out["x"]["models"]["old"]["deprecated"]


def test_la_foto_incluida_en_la_app_existe_y_conoce_a_los_grandes():
    assert model_catalog.SNAPSHOT.is_file(), "falta danplay/data/models-snapshot.json"
    d = json.loads(model_catalog.SNAPSHOT.read_text(encoding="utf-8"))
    for pid in ("openai", "anthropic", "google", "deepinfra", "openrouter"):
        assert pid in d["providers"], pid
        assert d["providers"][pid]["models"], pid


def test_la_recomendacion_evita_obsoletos_experimentales_y_sin_herramientas():
    models = [
        {"id": "a-exp", "name": "A exp", "tools": True, "deprecated": False, "released": "2026-09-01",
         "cost_in": 0.1, "cost_out": 0.4},
        {"id": "b-flash", "name": "B Flash", "tools": True, "deprecated": False, "released": "2026-08-01",
         "cost_in": 0.1, "cost_out": 0.4},
        {"id": "c-big", "name": "C", "tools": True, "deprecated": False, "released": "2026-08-15",
         "cost_in": 2, "cost_out": 8},
        {"id": "d-old", "name": "D", "tools": True, "deprecated": True, "released": "2026-09-02",
         "cost_in": 0.01, "cost_out": 0.02},
        {"id": "e-notools", "name": "E", "tools": False, "deprecated": False, "released": "2026-09-03",
         "cost_in": 0.01, "cost_out": 0.02},
    ]
    r = model_catalog.recommend(models)
    assert r == {"chat": "c-big", "fast": "b-flash"}
    assert model_catalog.recommend([]) == {"chat": "", "fast": ""}


def test_sin_red_el_catalogo_no_rompe_nada(monkeypatch, tmp_path):
    monkeypatch.setattr(model_catalog, "CACHE", tmp_path / "no.json")
    monkeypatch.setattr(model_catalog, "SOURCE", "http://127.0.0.1:9/nada")
    model_catalog.forget()
    try:
        s = model_catalog.refresh(force=True, timeout=1)
        assert s["error"], "tiene que contar que no pudo"
        assert s["models"] > 0, "y seguir con la foto de la app"
    finally:
        model_catalog.forget()


# ------------------------------------------------------- el cliente tolera

def _fake(monkeypatch, fake):
    monkeypatch.setattr(ai, "_get_client", lambda: fake)
    monkeypatch.setattr(ai, "_build_client", lambda p: fake)


def test_si_el_proveedor_rechaza_json_se_reintenta_sin_el_y_se_recuerda(perfiles, monkeypatch):
    providers.save_profile({"provider": "ollama", "model": "m", "chat_model": "m"})
    fake = _FakeClient(rejects={"response_format": "400: response_format is not supported"},
                       answer='{"artist": "Barak", "title": "Mi Gozo", "confidence": 0.9}')
    _fake(monkeypatch, fake)
    r = ai.resolve("BARAK mi gozo.mp3")
    assert r["artist"] == "Barak" and r["source"] == "ai"
    assert len(fake.calls) == 2 and "response_format" not in fake.calls[1]
    ai.resolve("otra.mp3")
    assert len(fake.calls) == 3 and "response_format" not in fake.calls[2], "ya no lo vuelve a mandar"


def test_tool_choice_obligatorio_baja_a_auto_si_no_lo_admiten(perfiles, monkeypatch):
    providers.save_profile({"provider": "lmstudio", "model": "m", "chat_model": "m"})
    fake = _FakeClient(rejects={"tool_choice": "400: tool_choice 'required' is not supported"})
    _fake(monkeypatch, fake)
    ai.complete([{"role": "user", "content": "hola"}], tools=[{"type": "function"}],
                tool_choice="required", purpose="chat")
    assert fake.calls[-1]["tool_choice"] == "auto"


def test_un_modelo_sin_herramientas_se_dice_claro(perfiles, monkeypatch):
    providers.save_profile({"provider": "ollama", "model": "gemma", "chat_model": "gemma"})
    fake = _FakeClient(rejects={"tools": "400: registry.ollama.ai/library/gemma does not support tools"})
    _fake(monkeypatch, fake)
    with pytest.raises(ai.ToolsUnsupported):
        ai.complete([{"role": "user", "content": "hola"}], tools=[{"type": "function"}], purpose="chat")
    # y el asistente lo cuenta en castellano en vez de un error seco
    from danplay import chat
    r = chat.reply([{"role": "user", "text": "hola"}])
    assert "herramientas" in r["error"] and "gemma" in r["error"]


def test_los_razonadores_de_openai_no_reciben_temperatura(perfiles, monkeypatch):
    providers.save_profile({"provider": "openai", "key": "sk", "model": "gpt-6-astra",
                            "chat_model": "gpt-6-astra"})
    fake = _FakeClient(answer="ok")
    _fake(monkeypatch, fake)
    ai.ask("hola")
    assert "temperature" not in fake.calls[-1], "el catalogo dice que no la admite"


def test_los_parametros_extra_del_perfil_viajan_en_cada_peticion(perfiles, monkeypatch):
    providers.save_profile({"provider": "openrouter", "key": "k", "model": "m",
                            "extra": {"reasoning": {"effort": "low"}}})
    fake = _FakeClient(answer="ok")
    _fake(monkeypatch, fake)
    ai.ask("hola")
    assert fake.calls[-1]["extra_body"] == {"reasoning": {"effort": "low"}}


def test_probar_distingue_clave_mala_de_sin_conexion(perfiles, monkeypatch):
    providers.save_profile({"provider": "groq", "key": "k", "model": "m"})
    bad = Exception("Error code: 401 - invalid api key"); bad.status_code = 401
    fake = _FakeClient(rejects={"model": ""})
    fake.rejects = {}

    def boom(**kw):
        raise bad
    fake.chat.completions.create = boom
    _fake(monkeypatch, fake)
    r = ai.check()
    assert not r["ok"] and "clave" in r["reason"] and "Groq" in r["reason"]

    providers.save_profile({"provider": "ollama", "model": "m"})
    off = Exception("Connection error.")
    fake2 = _FakeClient()
    fake2.chat.completions.create = lambda **kw: (_ for _ in ()).throw(off)
    _fake(monkeypatch, fake2)
    r = ai.check()
    assert not r["ok"] and "arrancado" in r["reason"]


def test_probar_comprueba_que_el_modelo_de_conversacion_usa_herramientas(perfiles, monkeypatch):
    providers.save_profile({"provider": "groq", "key": "k", "model": "chico", "chat_model": "grande"})
    calls = []

    class _Call:
        id = "1"
        function = type("f", (), {"name": "saluda", "arguments": "{}"})()

    def create(**kw):
        calls.append(kw)
        msg = _Msg("", [_Call()]) if kw.get("tools") else _Msg("ok")
        return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()
    fake = _FakeClient()
    fake.chat.completions.create = create
    _fake(monkeypatch, fake)
    r = ai.check()
    assert r["ok"] and r["tools_ok"]
    assert [c["model"] for c in calls] == ["chico", "grande", "grande"]
    assert calls[0]["max_tokens"] == 1, "la prueba de clave es la mas barata posible"


def test_la_lista_de_modelos_cruza_con_el_catalogo(perfiles, monkeypatch):
    providers.save_profile({"provider": "openai", "key": "k", "model": "m"})
    fake = _FakeClient(models=["gpt-6-astra", "modelo-desconocido"])
    _fake(monkeypatch, fake)
    r = ai.list_models()
    assert r["ok"]
    by = {m["id"]: m for m in r["models"]}
    assert by["gpt-6-astra"]["known"] and by["gpt-6-astra"]["tools"]
    assert not by["modelo-desconocido"]["known"]


def test_un_borrador_sin_clave_usa_la_guardada(perfiles):
    providers.save_profile({"provider": "mistral", "key": "clave-guardada", "model": "m"})
    d = providers.with_saved_key({"id": "mistral", "provider": "mistral", "model": "otro"})
    assert d["key"] == "clave-guardada"
    p = providers.resolve("mistral", d)
    assert p["model"] == "otro" and p["key"] == "clave-guardada"
