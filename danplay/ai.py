# -*- coding: utf-8 -*-
"""El cerebro: DeepInfra via endpoint compatible con OpenAI.

Se usa solo cuando la huella acustica falla y el heuristico no esta seguro.
"""
import json, re
from . import config

_client = None

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


def available() -> bool:
    return bool(config.AI_ENABLED and config.DEEPINFRA_API_KEY)


def check() -> dict:
    """Comprueba que la clave sirve de verdad, no solo que este puesta.

    Hace la llamada mas barata posible (un token). Distingue entre «clave
    rechazada» y «no hay internet», porque para el usuario no es lo mismo.
    """
    if not config.AI_ENABLED:
        return {"ok": False, "reason": "la IA esta apagada en Ajustes"}
    if not config.DEEPINFRA_API_KEY:
        return {"ok": False, "reason": "falta la clave de DeepInfra"}
    try:
        _get_client().chat.completions.create(
            model=config.DEEPINFRA_MODEL,
            messages=[{"role": "user", "content": "ok"}],
            max_tokens=1, temperature=0)
        return {"ok": True, "model": config.DEEPINFRA_MODEL}
    except Exception as e:                                  # noqa: BLE001
        texto = str(e)
        bajo = texto.lower()
        if any(x in bajo for x in ("401", "403", "unauthorized", "invalid api key",
                                   "authentication")):
            return {"ok": False, "reason": "la clave no es valida"}
        if any(x in bajo for x in ("429", "quota", "rate limit", "insufficient")):
            return {"ok": False, "reason": "la clave es valida pero no tiene credito"}
        if any(x in bajo for x in ("connection", "timeout", "resolve", "network")):
            return {"ok": False, "reason": "no hay conexion con DeepInfra"}
        if "model" in bajo and "not" in bajo:
            return {"ok": False,
                    "reason": f"el modelo «{config.DEEPINFRA_MODEL}» no esta disponible"}
        return {"ok": False, "reason": texto[:160]}


def _get_client():
    global _client
    if _client is None:
        from openai import OpenAI
        _client = OpenAI(api_key=config.DEEPINFRA_API_KEY,
                          base_url=config.DEEPINFRA_BASE_URL)
    return _client


def _json_from(text: str) -> dict | None:
    text = re.sub(r"^```(?:json)?|```$", "", text.strip(), flags=re.MULTILINE).strip()
    try:
        return json.loads(text)
    except Exception:
        m = re.search(r"\{.*\}", text, re.DOTALL)
        if m:
            try:
                return json.loads(m.group(0))
            except Exception:
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
        r = _get_client().chat.completions.create(
            model=config.DEEPINFRA_MODEL,
            messages=[{"role": "system", "content": INSTRUCTIONS},
                      {"role": "user", "content": user_msg}],
            temperature=0.1, max_tokens=400,
            response_format={"type": "json_object"},
        )
        data = _json_from(r.choices[0].message.content or "")
    except Exception as e:
        return {"error": str(e)}
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
        r = _get_client().chat.completions.create(
            model=config.DEEPINFRA_MODEL,
            messages=[{"role": "system", "content": system},
                      {"role": "user", "content": prompt}],
            temperature=temperature, max_tokens=max_tokens)
        return (r.choices[0].message.content or "").strip()
    except Exception:
        return None


def ask_json(prompt, system="Responde solo con JSON valido.",
                   max_tokens=1200, temperature=0.15) -> dict | None:
    if not available():
        return None
    try:
        r = _get_client().chat.completions.create(
            model=config.DEEPINFRA_MODEL,
            messages=[{"role": "system", "content": system},
                      {"role": "user", "content": prompt}],
            temperature=temperature, max_tokens=max_tokens,
            response_format={"type": "json_object"})
        return _json_from(r.choices[0].message.content or "")
    except Exception as e:
        return {"error": str(e)}
