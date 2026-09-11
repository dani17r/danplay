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
                return type("p", (), {"data": [type("m", (), {"id": i, "to_dict": staticmethod(lambda i=i: {"id": i})})()
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


# --------------------------------------------------------- gratis, sin clave

def test_los_gratuitos_van_primero_y_no_exigen_clave():
    assert providers.GROUPS[0]["id"] == "free"
    assert providers.FREE_ORDER and providers.FREE_ORDER[0] == "llm7"
    for pid in providers.FREE_ORDER:
        p = providers.BY_ID[pid]
        assert p["group"] == "free" and p["key"] != "required", pid
        assert p["suggest"].get("chat") and p["suggest"].get("fast"), pid
        assert p["quirks"].get("anon_filter"), f"{pid}: sin filtro, la lista enseñaria modelos de pago"


def test_probar_gratis_activa_el_primero_que_responde(perfiles, monkeypatch):
    class _Call:
        id = "1"
        function = type("f", (), {"name": "saluda", "arguments": "{}"})()

    def client_for(p):
        fake = _FakeClient()
        if p["id"] == "llm7":
            def down(**kw):
                raise Exception("Connection error.")
            fake.chat.completions.create = down
        else:
            def create(**kw):
                msg = _Msg("", [_Call()]) if kw.get("tools") else _Msg("ok")
                return type("r", (), {"choices": [type("c", (), {"message": msg})()]})()
            fake.chat.completions.create = create
        return fake
    monkeypatch.setattr(ai, "_build_client", client_for)
    r = ai.try_free()
    assert r["ok"] and r["chosen"] == "kilo" and r["tools_ok"]
    assert [t["id"] for t in r["tried"]] == ["llm7", "kilo"]
    assert "conexion" in r["tried"][0]["reason"]
    assert providers.active_id() == "kilo"
    assert providers.active()["chat_model"] == providers.BY_ID["kilo"]["suggest"]["chat"]
    assert ai.available(), "sin clave, pero listo"


def test_probar_gratis_sin_ninguno_vivo_lo_dice_y_no_activa_nada(perfiles, monkeypatch):
    def client_for(p):
        fake = _FakeClient()

        def down(**kw):
            raise Exception("Connection error.")
        fake.chat.completions.create = down
        return fake
    monkeypatch.setattr(ai, "_build_client", client_for)
    r = ai.try_free()
    assert not r["ok"] and len(r["tried"]) == len(providers.FREE_ORDER)
    assert "ninguno" in r["reason"]
    assert providers.active_id() == ""


def test_sin_clave_la_lista_solo_enseña_lo_que_sirve_a_anonimos(perfiles, monkeypatch):
    rows = [{"id": "minimax-m2.7", "tier": "turbo"}, {"id": "gpt-6-astra", "tier": "pro"}]
    fake = _FakeClient()
    fake.models = type("M", (), {"list": staticmethod(lambda: type("p", (), {
        "data": [type("m", (), {"id": r["id"], "to_dict": staticmethod(lambda r=r: dict(r))})() for r in rows]})())})()
    monkeypatch.setattr(ai, "_build_client", lambda p: fake)
    sin = ai.list_models({"provider": "llm7"})
    assert [m["id"] for m in sin["models"]] == ["minimax-m2.7"]
    con = ai.list_models({"provider": "llm7", "key": "token"})
    assert {m["id"] for m in con["models"]} == {"minimax-m2.7", "gpt-6-astra"}


# ---------------------------------------------------------------------- TOON

def test_toon_sigue_la_especificacion():
    from danplay import toon
    enc = toon.encode
    assert enc({"user": {"id": 123, "name": "Ada"}}) == "user:\n  id: 123\n  name: Ada"
    assert enc({"tags": ["admin", "ops", "dev"]}) == "tags[3]: admin,ops,dev"
    assert enc({"items": [{"sku": "A1", "qty": 2, "price": 9.99},
                          {"sku": "B2", "qty": 1, "price": 14.5}]}) == \
        "items[2]{sku,qty,price}:\n  A1,2,9.99\n  B2,1,14.5"
    assert enc({"items": [1, {"a": 1}, "text"]}) == "items[3]:\n  - 1\n  - a: 1\n  - text"
    assert enc({"e": [], "o": {}}) == "e: []\no:"
    # numeros, booleanos y nulos tal cual; los flotantes enteros sin «.0»
    assert enc({"a": 2.0, "b": -3.14, "c": None, "d": True, "e": float("nan")}) == \
        "a: 2\nb: -3.14\nc: null\nd: true\ne: null"
    # comillas SOLO cuando hace falta, con sus escapes
    assert enc({"a": "Hello world", "b": "123", "c": "true", "d": " x", "e": "",
                "f": "a:b", "g": "x,y", "h": "-1x", "i": 'say "hi"', "j": "l1\nl2",
                "k": "Barak - Mi Gozo (En Vivo)"}) == (
        'a: Hello world\nb: "123"\nc: "true"\nd: " x"\ne: ""\nf: "a:b"\ng: "x,y"\n'
        'h: "-1x"\ni: "say \\"hi\\""\nj: "l1\\nl2"\nk: Barak - Mi Gozo (En Vivo)')
    assert enc({"my-key": [1, 2, 3]}) == '"my-key"[3]: 1,2,3'
    # una lista de objetos desiguales o anidados va en forma de lista
    assert enc({"d": [{"ok": True, "m": []}, {"ok": False, "m": [{"id": 1}]}]}) == \
        "d[2]:\n  - ok: true\n    m: []\n  - ok: false\n    m[1]{id}:\n      1"
    # otro delimitador se declara en la cabecera y vale en todas partes
    assert enc({"items": [{"sku": "A1", "name": "x,y"}]}, "|") == "items[1|]{sku|name}:\n  A1|x,y"
    with pytest.raises(ValueError):
        enc({}, ";")


def test_las_herramientas_llegan_al_modelo_en_toon_y_pesan_menos(perfiles, monkeypatch):
    from danplay import chat, toon
    providers.save_profile({"provider": "ollama", "model": "m", "chat_model": "m"})
    songs = [{"id": i, "artist": "Barak", "title": f"Tema {i}", "album": "Gozo", "duration": 240,
              "key": "Bb", "bpm": 120, "stars": 0, "favorite": False} for i in range(1, 21)]
    result = {"total": 20, "songs": songs}
    monkeypatch.setattr(chat, "run_tool", lambda name, args: result)

    class _Call:
        id = "1"
        function = type("f", (), {"name": "search_songs", "arguments": '{"query": "barak"}'})()
    fake = _FakeClient()
    turns = [_Msg("", [_Call()]), _Msg("Tienes 20 temas de Barak.")]

    def create(**kw):
        fake.calls.append(dict(kw))
        return type("r", (), {"choices": [type("c", (), {"message": turns.pop(0)})()]})()
    fake.chat.completions.create = create
    _fake(monkeypatch, fake)
    chat.reply([{"role": "user", "text": "¿que tengo de Barak?"}])
    tool_msg = next(m for m in fake.calls[1]["messages"] if m.get("role") == "tool")
    assert tool_msg["content"].startswith("total: 20\nsongs[20]{id,artist,title,")
    assert tool_msg["content"] == toon.encode(result)
    assert len(tool_msg["content"]) < 0.6 * len(json.dumps(result, ensure_ascii=False)), \
        "la tabla tiene que pesar bastante menos que el JSON"


def test_una_ficha_de_ia_vacia_no_se_reutiliza(monkeypatch):
    from danplay import enrich
    vacia = json.dumps({"likely_key": "", "progression": "", "confidence": 0.2})
    buena = json.dumps({"likely_key": "Bb", "progression": "| Bb | Gm |", "confidence": 0.8})
    assert enrich.cached_details({"chords": vacia}) is None
    assert enrich.cached_details({"chords": buena})["likely_key"] == "Bb"
    assert enrich.cached_details({"chords": ""}) is None
    assert enrich.cached_details({"chords": "esto no es json"}) is None
    # con una vacia guardada se vuelve a preguntar, y lo nuevo se guarda
    asked = []
    monkeypatch.setattr(enrich, "details", lambda song: asked.append(song["id"]) or
                        {"likely_key": "G", "progression": "| G |", "confidence": 0.7})
    saved = {}
    monkeypatch.setattr(enrich.library, "update", lambda cid, **f: saved.update(f))
    d, cached = enrich.details_for({"id": 7, "chords": vacia})
    assert asked == [7] and not cached and d["likely_key"] == "G"
    assert json.loads(saved["chords"])["likely_key"] == "G"
    d, cached = enrich.details_for({"id": 8, "chords": buena})
    assert cached and asked == [7], "la buena se reutiliza sin preguntar"


# ------------------------------------------------------- en trozos y respaldo

def _chunk(content=None, tool=None, usage=None):
    """Un trozo como los del SDK: delta con texto o con parte de una herramienta."""
    from types import SimpleNamespace as NS
    delta = NS(content=content, tool_calls=None)
    if tool:
        idx, tid, name, args = tool
        delta.tool_calls = [NS(index=idx, id=tid, function=NS(name=name, arguments=args))]
    return NS(choices=[NS(delta=delta)] if (content is not None or tool) else [], usage=usage)


def test_la_respuesta_en_trozos_se_junta_y_se_va_enseñando():
    from types import SimpleNamespace as NS
    seen = []
    stream = iter([_chunk("Hola"), _chunk(" Dani"), _chunk(usage=NS(prompt_tokens=10, completion_tokens=2))])
    msg, usage = ai._collect(stream, on_text=seen.append)
    assert msg.content == "Hola Dani" and msg.tool_calls is None
    assert seen == ["Hola", "Hola Dani"]
    assert usage.prompt_tokens == 10


def test_las_herramientas_en_trozos_se_recomponen_y_el_preambulo_se_retira():
    seen = []
    stream = iter([_chunk("Voy a buscar"), _chunk(tool=(0, "c1", "search_songs", '{"que')),
                   _chunk(tool=(0, None, None, 'ry": "x"}')), _chunk(tool=(1, "c2", "list_playlists", "{}"))])
    msg, _ = ai._collect(stream, on_text=seen.append)
    assert seen[-1] == "", "lo enseñado era un preambulo: se retira"
    assert [c.function.name for c in msg.tool_calls] == ["search_songs", "list_playlists"]
    assert msg.tool_calls[0].function.arguments == '{"query": "x"}'
    assert msg.tool_calls[0].id == "c1"
    # sin argumentos llega "": devuelto asi al servidor era un 400
    msg, _ = ai._collect(iter([_chunk(tool=(0, "c3", "library_summary", ""))]))
    assert msg.tool_calls[0].function.arguments == "{}"


def test_el_razonamiento_abierto_no_se_enseña_a_medias():
    seen = []
    stream = iter([_chunk("<think>pienso"), _chunk(" mas</think>"), _chunk("Respuesta")])
    msg, _ = ai._collect(stream, on_text=seen.append)
    assert seen[0] == "" and seen[-1] == "Respuesta"
    assert ai.message_text(msg) == "Respuesta"


def test_cancelar_corta_la_respuesta_y_cierra_el_flujo():
    import threading
    closed = []

    class _Stream:
        def __iter__(self):
            yield _chunk("Hola")
            yield _chunk(" mundo")

        def close(self):
            closed.append(True)
    flag = threading.Event()

    def on_text(t):
        flag.set()
    with pytest.raises(ai.Canceled):
        ai._collect(_Stream(), on_text=on_text, cancel=flag)
    assert closed == [True]


def test_si_el_activo_esta_caido_responde_el_respaldo(perfiles, monkeypatch):
    providers.save_profile({"provider": "openai", "key": "k", "model": "gpt-5.6-luna",
                            "chat_model": "gpt-5.6-luna"})
    providers.save_profile({"provider": "groq", "key": "k2", "model": "llama-3.1-8b-instant",
                            "chat_model": "llama-3.3-70b-versatile", "activate": False})
    providers.activate("openai")
    calls = []

    def client_for(p):
        fake = _FakeClient(answer=f"desde {p['id']}")
        if p["id"] == "openai":
            def down(**kw):
                calls.append(("openai", kw["model"]))
                e = Exception("Error code: 503 - service unavailable"); e.status_code = 503
                raise e
            fake.chat.completions.create = down
        else:
            orig = fake.chat.completions.create
            fake.chat.completions.create = lambda **kw: calls.append(("groq", kw["model"])) or orig(**kw)
        return fake
    monkeypatch.setattr(ai, "_build_client", client_for)
    monkeypatch.setattr(ai, "_get_client", lambda: client_for(ai.profile()))
    ai.begin_turn()
    r = ai.complete([{"role": "user", "content": "hola"}], purpose="chat")
    assert ai.message_text(r.message) == "desde groq"
    assert r.via["fallback"] and r.via["id"] == "groq" and r.via["model"] == "llama-3.3-70b-versatile"
    assert calls == [("openai", "gpt-5.6-luna"), ("groq", "llama-3.3-70b-versatile")]
    # mientras el activo siga marcado como caido, se va directo al respaldo
    r = ai.complete([{"role": "user", "content": "otra"}], purpose="fast")
    assert calls[-1] == ("groq", "llama-3.1-8b-instant") and len(calls) == 3
    # sin respaldo activado, el fallo se cuenta tal cual
    providers.set_fallback(False)
    ai.reset_client()
    with pytest.raises(Exception, match="503"):
        ai.complete([{"role": "user", "content": "hola"}], purpose="chat")


def test_un_error_del_mensaje_no_dispara_el_respaldo(perfiles, monkeypatch):
    providers.save_profile({"provider": "openai", "key": "k", "model": "m", "chat_model": "m"})
    providers.save_profile({"provider": "groq", "key": "k2", "model": "g", "activate": False})
    providers.activate("openai")
    tried = []

    def client_for(p):
        fake = _FakeClient()

        def bad(**kw):
            tried.append(p["id"])
            e = Exception("Error code: 400 - messages must not be empty"); e.status_code = 400
            raise e
        fake.chat.completions.create = bad
        return fake
    monkeypatch.setattr(ai, "_build_client", client_for)
    monkeypatch.setattr(ai, "_get_client", lambda: client_for(ai.profile()))
    with pytest.raises(Exception, match="400"):
        ai.complete([], purpose="chat")
    assert tried == ["openai"], "un 400 es cosa del mensaje: el respaldo no lo arregla"


def test_el_uso_se_apunta_por_turno_y_en_la_base(perfiles, monkeypatch, tmp_path):
    from types import SimpleNamespace as NS
    from danplay import library
    monkeypatch.setattr(config, "DATABASE", tmp_path / "uso.db")
    providers.save_profile({"provider": "openai", "key": "k", "model": "gpt-6-astra",
                            "chat_model": "gpt-6-astra"})
    fake = _FakeClient(answer="ok")
    orig = fake.chat.completions.create

    def create(**kw):
        r = orig(**kw)
        r.usage = NS(prompt_tokens=1000, completion_tokens=500)
        return r
    fake.chat.completions.create = create
    _fake(monkeypatch, fake)
    ai.begin_turn()
    ai.ask("hola")
    ai.ask("otra")
    usage, via = ai.turn_summary()
    assert usage["calls"] == 2 and usage["prompt"] == 2000 and usage["completion"] == 1000
    # gpt-6-astra: 10 $/M entrada y 50 $/M salida en el catalogo
    assert usage["priced"] and abs(usage["cost"] - (2000 / 1e6 * 10 + 1000 / 1e6 * 50)) < 1e-9
    assert via["model"] == "gpt-6-astra" and not via["fallback"]
    s = library.ai_usage_summary()
    assert s["today"]["calls"] == 2 and s["month"]["prompt"] == 2000
    assert abs(s["today"]["cost"] - usage["cost"]) < 1e-6


def test_la_identificacion_pide_esquema_si_el_modelo_lo_admite(perfiles, monkeypatch):
    providers.save_profile({"provider": "openai", "key": "k", "model": "gpt-6-astra",
                            "chat_model": "gpt-6-astra"})
    assert ai.json_format(ai.SONG_SCHEMA)["type"] == "json_schema"
    fake = _FakeClient(answer='{"artist":"Barak","title":"Mi Gozo","feat":"","extra":"","category":"song","confidence":0.9}')
    _fake(monkeypatch, fake)
    r = ai.resolve("BARAK mi gozo.mp3")
    assert r["artist"] == "Barak"
    assert fake.calls[-1]["response_format"]["type"] == "json_schema"
    # un modelo sin ficha: json_object a secas
    providers.save_profile({"provider": "ollama", "model": "raro", "chat_model": "raro"})
    assert ai.json_format(ai.SONG_SCHEMA) == {"type": "json_object"}
    # y si el servidor rechaza el esquema, se baja a json_object antes que quitarlo
    providers.save_profile({"provider": "openai", "key": "k", "model": "gpt-6-astra", "chat_model": "gpt-6-astra"})
    fake2 = _FakeClient(rejects={"response_format": "400: json_schema is not supported by this model"},
                        answer='{"artist":"X","title":"Y","feat":"","extra":"","category":"song","confidence":0.5}')
    real_create = fake2.chat.completions.create

    def create(**kw):
        if (kw.get("response_format") or {}).get("type") == "json_object":
            fake2.calls.append(dict(kw))
            return type("r", (), {"choices": [type("c", (), {"message": _Msg(fake2.answer)})()]})()
        return real_create(**kw)
    fake2.chat.completions.create = create
    _fake(monkeypatch, fake2)
    assert ai.resolve("x.mp3")["artist"] == "X"
    assert fake2.calls[-1]["response_format"] == {"type": "json_object"}


# ------------------------------------------------------- lo que ve el usuario

def test_el_estado_real_cuenta_lo_que_ve_selecciona_y_suena():
    from danplay import chat
    lines = chat._screen_note({
        "view": {"kind": "playlist", "name": "Domingo"}, "total": 3,
        "songs": [{"id": 1, "artist": "Barak", "title": "Mi Gozo"}, {"id": 2, "artist": "New Wine", "title": "Shekinah"}],
        "selected": [{"id": 2, "artist": "New Wine", "title": "Shekinah"}],
        "playing": {"id": 1, "artist": "Barak", "title": "Mi Gozo", "paused": True}})
    text = "\n".join(lines)
    assert "«Domingo» (repertorio): 3 canciones" in text
    assert "songs[2]{id,artist,title}:" in text and "1,Barak,Mi Gozo" in text
    assert "seleccionadas 1 canciones: id 2 «New Wine - Shekinah»" in text
    assert "en pausa: id 1 «Barak - Mi Gozo»" in text
    assert chat._screen_note(None) == [] and chat._screen_note({}) == []


def test_el_contexto_llega_al_modelo_en_el_estado_real(perfiles, monkeypatch):
    from danplay import chat
    providers.save_profile({"provider": "ollama", "model": "m", "chat_model": "m"})
    fake = _FakeClient(answer="Vale.")
    _fake(monkeypatch, fake)
    chat.reply([{"role": "user", "text": "pon la segunda"}],
               context={"view": {"kind": "all", "name": "Todas"}, "total": 2,
                        "songs": [{"id": 5, "artist": "A", "title": "Uno"}, {"id": 6, "artist": "B", "title": "Dos"}]})
    system_notes = [m["content"] for m in fake.calls[0]["messages"] if m["role"] == "system"]
    assert any("6,B,Dos" in n for n in system_notes)


# --------------------------------------------------------- tonos y la hoja

def test_los_tonos_vecinos():
    from danplay import theory
    r = theory.related_keys("G")
    assert r["relative"] == "Em" and r["neighbors"] == ["D", "C"]
    assert theory.related_keys("F#m")["neighbors"] == ["C#m", "Bm"]
    assert theory.related_keys("Bb")["relative"] == "Gm"
    assert theory.related_keys("nada") is None


def test_las_conversaciones_se_guardan_y_se_buscan(monkeypatch, tmp_path):
    from danplay import chats
    monkeypatch.setattr(config, "DATABASE", tmp_path / "chats.db")
    c = chats.create()
    assert c["id"] and chats.list_all()[0]["n"] == 0
    chats.append(c["id"], [{"role": "me", "text": "¿que tengo de Barak?"},
                           {"role": "ai", "text": "Tienes 18 temas.", "tools": [{"name": "search_songs", "summary": "18"}],
                            "usage": {"prompt": 10, "completion": 5, "cost": 0.0001}}])
    got = chats.get(c["id"])
    assert got["title"] == "¿que tengo de Barak?"
    assert got["messages"][1]["tools"][0]["name"] == "search_songs"
    assert got["messages"][1]["usage"]["prompt"] == 10
    assert "hidden" not in got["messages"][0]
    hits = chats.search("barak")
    assert hits and hits[0]["chat_id"] == c["id"] and "Barak" in hits[0]["snippet"]
    assert chats.search("%") == []
    assert chats.rename(c["id"], "Domingo") and chats.get(c["id"])["title"] == "Domingo"
    assert chats.delete(c["id"]) and chats.get(c["id"]) is None
    assert chats.title_from("x" * 100).endswith("…")


def test_una_llamada_escrita_como_texto_se_ejecuta_igual(perfiles, monkeypatch):
    """Algun modelo, en vez de llamar a la herramienta, escribe la llamada:
    «set_stars id=1 stars=5». Si se entiende, se ejecuta como si la hubiera
    hecho; si no, se le obliga a usar herramientas de verdad."""
    from danplay import chat
    providers.save_profile({"provider": "ollama", "model": "m", "chat_model": "m"})
    assert chat.PSEUDO_CALL.match('search_songs query="artist:Barak" sort="duration"')
    assert chat.PSEUDO_CALL.match("play_song(12)") and chat.PSEUDO_CALL.match("list_playlists")
    assert not chat.PSEUDO_CALL.match("Busca con search_songs si quieres.")
    assert not chat.PSEUDO_CALL.match("Tienes 18 canciones de Barak.")
    assert chat.parse_pseudo_call("set_stars id=1 stars=5") == ("set_stars", {"id": 1, "stars": 5})
    assert chat.parse_pseudo_call('create_playlist name="Domingo" ids=[1, 2]') == \
        ("create_playlist", {"name": "Domingo", "ids": [1, 2]})
    assert chat.parse_pseudo_call("nada que ver") is None
    ran = []
    monkeypatch.setattr(chat, "run_tool", lambda name, args: ran.append((name, args)) or
                        {"ok": True, "stars": 5, "song": "Barak - Mi Gozo"})
    turns = [_Msg("set_stars id=1 stars=5"), _Msg("Listo: **Mi Gozo** con 5 estrellas.")]
    fake = _FakeClient()

    def create(**kw):
        fake.calls.append(dict(kw))
        return type("r", (), {"choices": [type("c", (), {"message": turns.pop(0)})()]})()
    fake.chat.completions.create = create
    _fake(monkeypatch, fake)
    r = chat.reply([{"role": "user", "text": "ponle 5 estrellas a Mi Gozo"}])
    assert ran == [("set_stars", {"id": 1, "stars": 5})]
    assert r["text"] == "Listo: **Mi Gozo** con 5 estrellas."
    assert [t["name"] for t in r["tools"]] == ["set_stars"] and not r.get("narrated")
    # y el historial del turno siguiente lleva la llamada como hecha de verdad
    assert fake.calls[1]["messages"][-1]["role"] == "tool"
    assert fake.calls[1]["messages"][-2]["tool_calls"][0]["function"]["name"] == "set_stars"
    # lo que no se entiende (un JSON roto) sigue el camino de siempre: herramientas obligadas
    assert chat.parse_pseudo_call('set_stars: {"id": 1,') is None
    turns[:] = [_Msg('set_stars: {"id": 1,'), _Msg("", [type("C", (), {"id": "1", "function": type("f", (), {"name": "search_songs", "arguments": "{}"})()})()]), _Msg("Fin.")]
    fake.calls.clear()
    monkeypatch.setattr(chat, "run_tool", lambda name, args: {"total": 0, "songs": []})
    chat.reply([{"role": "user", "text": "busca algo"}])
    assert fake.calls[1]["tool_choice"] == "required"


def test_un_flujo_roto_a_medias_se_repite_entero(perfiles, monkeypatch):
    """DeepInfra manda de vez en cuando un evento vacio y el SDK rompe el
    flujo: la peticion se repite sin trozos y la respuesta llega igual."""
    providers.save_profile({"provider": "ollama", "model": "m", "chat_model": "m"})
    fake = _FakeClient(answer="entera")
    orig = fake.chat.completions.create

    class _Broken:
        def __iter__(self):
            yield _chunk("Hol")
            raise Exception("Input is a zero-length, empty document")

        def close(self):
            pass

    def create(**kw):
        if kw.get("stream"):
            fake.calls.append(dict(kw))            # el falso de siempre apunta las demas
            return _Broken()
        return orig(**kw)
    fake.chat.completions.create = create
    _fake(monkeypatch, fake)
    seen = []
    r = ai.complete([{"role": "user", "content": "hola"}], purpose="chat", on_text=seen.append)
    assert ai.message_text(r.message) == "entera"
    assert seen[-1] == "", "lo enseñado a medias se retira"
    assert [bool(c.get("stream")) for c in fake.calls] == [True, False]
    # a la tercera seguida, ese modelo deja de pedirse en trozos
    ai.complete([{"role": "user", "content": "b"}], purpose="chat", on_text=seen.append)
    ai.complete([{"role": "user", "content": "c"}], purpose="chat", on_text=seen.append)
    fake.calls.clear()
    ai.complete([{"role": "user", "content": "d"}], purpose="chat", on_text=seen.append)
    assert [bool(c.get("stream")) for c in fake.calls] == [False]
