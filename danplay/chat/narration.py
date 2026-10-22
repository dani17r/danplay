"""Narrar no es hacer: detectar cuando el modelo dice que ha hecho algo que no
ha hecho (ver docs/ARQUITECTURA.md, «El asistente»).

Tres piezas: las llamadas escritas como texto en vez de hechas, las frases
que afirman una accion (la lista sale de un corpus real, tests/narracion.json)
y, cuando las frases no bastan, el propio modelo como juez de una palabra.

Las dos primeras las hace Rust (biblioteca/src/asistente/narracion.rs, con
las mismas expresiones, rama a rama); esto es la puerta de Python, y la
pregunta al juez, que necesita la IA.
"""

import re

import danplay_core as _rust

from .. import ai
from .tools import TOOL_NAMES

# las herramientas que se reconocen escritas como texto
_rust.narration_set_tools(sorted(TOOL_NAMES))

# Los sustitutos sueltos no llegan a Rust: pasan a «�».
_SURROGATE = re.compile("[\ud800-\udfff]")


def _txt(text) -> str:
    s = text if isinstance(text, str) else str(text or "")
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        return _SURROGATE.sub("�", s)
    return s


class _PseudoCall:
    """Una llamada escrita como texto en vez de hecha: «search_songs
    query="x"», «play_song(12)», «list_playlists: {}». Se reconoce por el
    nombre de una herramienta al principio seguido de argumentos. Delante
    puede llevar lo que algun modelo le pone: una viñeta, comillas invertidas
    o una etiqueta («búsqueda: search_songs …», «llamada: …», «tool: …»).

    `PSEUDO_CALL.match(texto)` dice si el texto empieza asi (verdadero o
    None, como `re.match`)."""

    def match(self, text) -> bool | None:
        return True if _rust.narration_is_pseudo_call(_txt(text or "")) else None


PSEUDO_CALL = _PseudoCall()


def parse_pseudo_call(text: str) -> tuple[str, dict] | None:
    """La llamada que el modelo escribio en vez de hacer, leida.

    «set_stars id=1 stars=5», «search_songs(query="barak", limit=3)»,
    «play_song: {"id": 12}»: se admite lo que un modelo suele soltar. Solo la
    primera linea; lo que no se entienda, None (y se le pide que la haga de
    verdad).
    """
    found = _rust.narration_parse_pseudo_call(_txt(text or ""))
    return None if found is None else (found[0], found[1])


NUDGE = (
    "ATENCION: en este turno ninguna herramienta ha hecho nada, asi que nada "
    "de lo que acabas de decir que hiciste o esta en marcha ha ocurrido. Si el "
    "usuario te habia PEDIDO hacer algo, hazlo ahora con las herramientas "
    "(los ids, con search_songs). Si solo estabas informando u ofreciendo, "
    "comprueba el estado real con una consulta (list_playlists, "
    "playlist_songs, search_songs o download_status) y responde con lo que "
    "devuelva, SIN crear ni cambiar nada. Añadir o quitar de una lista, "
    "corregirla, puntuar o marcar favorito NO necesitan confirmacion: hazlo, "
    "no preguntes. Cuenta solo lo que las herramientas hayan devuelto."
)


def claims_action(text: str) -> bool:
    """Si el texto afirma haber hecho, estar haciendo o ir a hacer algo en la app.

    Antes de mirar: fuera lo entrecomillado (letras citadas: «guardé tu
    palabra»), las marcas de markdown, las preguntas («¿la creo?») y las
    condicionales («si dices que si, la app te avisara»), que no son
    afirmaciones. Amplia a proposito: sin herramientas en el turno, un falso
    positivo cuesta una llamada de mas; un falso negativo es una lista que
    no existe. Los casos que se toleran estan en tests/narracion.json.
    """
    return _rust.narration_claims_action(_txt(text or ""))


# Las notas que la app añade al historial. Si el modelo las imita en su
# propio texto, se borran y cuentan como afirmacion falsa.
def has_markers(text: str) -> bool:
    return _rust.narration_has_markers(_txt(text or ""))


def strip_markers(text: str) -> str:
    return _rust.narration_strip_markers(_txt(text or ""))


def _judge_claims(text: str, user_text: str = "", downloaded=False) -> bool:
    """Segunda opinion: el propio modelo, como clasificador de una palabra.

    Las frases de `claims_action` nunca las cubren todas («Descargando:»,
    «Añadida a la lista» se colaron). Se pregunta cuando en el turno ninguna
    herramienta ha hecho nada y el texto habla de la app; el conocimiento
    musical no pasa por aqui. Ve tambien la peticion del usuario, para
    distinguir una oferta («puedo crear la lista») de una afirmacion. Si el
    juez falla o tarda, no se le culpa: se sigue con lo que digan las frases.
    """
    plain = _rust.narration_for_the_judge(_txt(text or ""))
    if plain is None:
        return False
    try:
        r = ai.complete(
            [
                {"role": "system", "content": "Eres un clasificador. Contesta solo SI o NO."},
                {
                    "role": "user",
                    "content": (
                        "¿Este mensaje de un asistente AFIRMA que YA ha hecho, esta haciendo "
                        "ahora o va a hacer ahora mismo una accion en la aplicacion (descargar, "
                        "crear o cambiar una lista, añadir o quitar canciones, borrar, puntuar, "
                        "poner musica)? Preguntar u ofrecer hacerlo NO cuenta. Informar de un dato "
                        "o responder conocimiento musical NO cuenta."
                        + (
                            " La descarga en si YA ocurrio de verdad: decir que las canciones "
                            "estan descargadas o en la biblioteca NO cuenta; cuenta cualquier OTRA "
                            "accion (añadir a una lista, poner a sonar, borrar)."
                            if downloaded
                            else ""
                        )
                        + "\n\n"
                        f"Peticion del usuario:\n{(user_text or '')[:400]}\n\n"
                        f"Mensaje del asistente:\n{plain[:1500]}"
                    ),
                },
            ],
            purpose="judge",
            temperature=0,
            max_tokens=3,
            timeout=15,
        )
        answer = ai.message_text(r.message).strip().upper().rstrip(".!")
        return answer in ("SI", "SÍ")
    except Exception:  # noqa: BLE001
        return False


# La persona pide HACER algo (crear, borrar, poner, descargar, corregir…).
# Solo entonces merece la pena molestar al juez: «¿de que año es?» o «di
# solo: listo» no pueden acabar en «ya la cree», y consultarle costaba una
# llamada mas en cada turno de charla normal.
def wants_action(text: str) -> bool:
    """Si el mensaje de la persona pide hacer algo en la app."""
    return _rust.narration_wants_action(_txt(text or ""))


def _answers_an_offer(messages: list[dict]) -> bool:
    """El ultimo mensaje es un si del usuario a una pregunta del asistente.

    Ahi es donde mas narraba: «¿la bajo?» — «si» — «Descargando…» sin llamar
    a nada. En ese caso la primera vuelta va obligada a usar herramientas.
    """
    if not messages:
        return False
    seen = [
        (
            m.get("role") == "ai",
            _txt(str(m.get("text") or "")),
            bool(m.get("app")),
            bool(m.get("tools")),
        )
        for m in messages
    ]
    return _rust.narration_answers_an_offer(seen)
