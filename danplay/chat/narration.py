"""Narrar no es hacer: detectar cuando el modelo dice que ha hecho algo que no
ha hecho (ver docs/ARQUITECTURA.md, «El asistente»).

Tres piezas: las llamadas escritas como texto en vez de hechas, las frases
que afirman una accion (la lista sale de un corpus real, tests/narracion.json)
y, cuando las frases no bastan, el propio modelo como juez de una palabra.
"""

import json
import re as _re

from .. import ai
from .tools import TOOL_NAMES

# Una llamada escrita como texto en vez de hecha: «search_songs query="x"»,
# «play_song(12)», «list_playlists: {}». Se reconoce por el nombre de una
# herramienta al principio seguido de argumentos.


PSEUDO_CALL = _re.compile(
    r"^\s*`?(?:"
    + "|".join(_re.escape(n) for n in sorted(TOOL_NAMES))
    + r")\b\s*(?:\(|\{|:|\w+\s*[:=]|$)",
    _re.IGNORECASE,
)
_PSEUDO_ARG = _re.compile(r"(\w+)\s*[:=]\s*(\"[^\"]*\"|'[^']*'|\[[^\]]*\]|\{[^}]*\}|[^\s,)]+)")


def parse_pseudo_call(text: str) -> tuple[str, dict] | None:
    """La llamada que el modelo escribio en vez de hacer, leida.

    «set_stars id=1 stars=5», «search_songs(query="barak", limit=3)»,
    «play_song: {"id": 12}»: se admite lo que un modelo suele soltar. Solo la
    primera linea; lo que no se entienda, None (y se le pide que la haga de
    verdad).
    """
    first = (text or "").strip().split("\n", 1)[0].strip().strip("`")
    m = _re.match(r"^([A-Za-z_]+)\b\s*(.*)$", first, _re.S)
    if not m or m.group(1).lower() not in TOOL_NAMES:
        return None
    name, rest = m.group(1).lower(), m.group(2).strip()
    rest = rest.strip("()").strip()
    if rest.startswith(":"):
        rest = rest[1:].strip()
    args: dict = {}
    if rest.startswith("{"):
        try:
            args = json.loads(rest)
        except Exception:  # noqa: BLE001
            return None
    else:
        for key, value in _PSEUDO_ARG.findall(rest):
            v = value.strip()
            if v[:1] in "\"'" and v[-1:] == v[:1]:
                v = v[1:-1]
            elif v.startswith(("[", "{")):
                try:
                    v = json.loads(v)
                except Exception:  # noqa: BLE001
                    pass
            elif v.lower() in ("true", "false"):
                v = v.lower() == "true"
            elif _re.fullmatch(r"-?\d+", v):
                v = int(v)
            elif _re.fullmatch(r"-?\d+\.\d+", v):
                v = float(v)
            args[key] = v
    return name, args


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


# Frases con las que el modelo cuenta que ha hecho, esta haciendo o va a hacer
# algo. Si aparecen en un turno sin ninguna llamada a herramientas, es
# narracion, no accion. La lista salio de un corpus de 300 frases (reales y
# generadas) que vive en tests/narracion.json: tocar una rama sin pasarlo es
# jugar a ciegas. Las ramas de WORD van tras un limite de palabra; las de
# LOOSE no (empiezan por parentesis, emoji o principio de frase).
PART = (
    r"(?:creada|creado|añadida|añadido|agregada|agregado|quitada|quitado|borrada|borrado|"
    r"descargada|descargado|guardada|guardado|corregida|corregido|puntuada|puntuado|"
    r"actualizada|actualizado|renombrada|renombrado|pausada|pausado|reanudada|reanudado|"
    r"eliminada|eliminado|sustituida|sustituido|reordenada|reordenado|vaciada|vaciado|"
    r"reemplazada|reemplazado|colocada|colocado|archivada|archivado|bajada|bajado)s?\b"
)
NO_SI = r"(?<!si )(?<!si ya )(?<!no )(?<!no ya )"
NEG = r"(?<!no )(?<!nada )(?<!a[uú]n no )(?<!todav[ií]a no )(?<!tampoco )"
VERBS_E = r"(?:cre|borr|descargu|agregu|puntu|quit|baj|renombr|marqu|mand|paus|elimin|cambi|orden|reorden|vaci|saqu|arregl|edit|coloqu|reemplac|lanc|inici|arranqu|program|solicit|encargu|reanud|archiv)"
VERBS_I = r"(?:añad|met|correg|ped|sustitu|mov|sub|repet|reprodu)"
ACT_NOW = r"(?:bajo|descargo|añado|agrego|creo|borro|quito|pongo|meto|corrijo|renombro|elimino|saco|muevo|dejo|arreglo|actualizo)"
OBJ = r"(?:la|lo|las|los|le|les|te la|te lo|te las|te los)"
END = r"(?=\s*(?:[,:.!;)]|✅|✔|👍|$))"


WORD = [  # ramas que empiezan por palabra (van tras \b)
    r"ya "
    + OBJ
    + r" (?!creo\b)(?:cre|borr|descarg|pus|añad|agregu|guard|puntu|corr|quit|renombr|marqu|mand|elimin|cambi|met|mov|sustitu|orden|reorden|vaci|dej|sa[cq]|arregl|edit|ped|lanc|inici|arranqu|program)\w*",
    NEG
    + r"he (?:creado|borrado|descargado|añadido|agregado|puntuado|corregido|guardado|pedido|quitado|bajado|renombrado|marcado|mandado|movido|eliminado|cambiado|metido|sustituido|ordenado|reordenado|vaciado|dejado|sacado|editado|arreglado|reemplazado|colocado|pausado|reanudado|lanzado|iniciado|arrancado|programado|solicitado|archivado)",
    r"(?:ya )?lo he hecho\b",
    r"he puesto (?:a sonar|\d+ estrellas|en (?:la |tu )?(?:lista|cola|repertorio)|(?:la|el) (?:primera|primero|última|ultimo))",
    OBJ + r" he puesto (?:a sonar|(?:la|el) (?:primera|primero|última|ultimo))",
    NEG
    + r"se (?:ha|han) (?:descargado|bajado|guardado|archivado|procesado|pedido|añadido|agregado|creado|borrado|quitado|eliminado|renombrado|actualizado|corregido)",
    r"(?<!ayer )(?<!antes )(?<!nunca )se (?:descarg|baj)(?:o|ó|aron|ar[aá]n?)\b",
    VERBS_E + r"é\b",
    VERBS_I + r"[ií]\b",
    r"puse\b",
    r"(?:la|lo|las|los|le|te|ya) (?:borr|descargu|agregu|guard|puntu|quit|baj|renombr|marqu|mand|elimin|cambi|orden|vaci|dej|saqu|arregl)e\b",
    r"cre[eé] (?:la |una |el |un |tu |otra )?(?:lista|repertorio|playlist)\b",
    r"(?<!una )(?<!cada )(?<!toda )(?<!cualquier )(?:lista|repertorio|playlist)(?: \S+){0,2} (?:creada|creado|actualizada|actualizado|corregida|corregido|borrada|borrado|renombrada|renombrado|eliminada|eliminado|reordenada|reordenado|vaciada|vaciado|lista|listo)\b",
    r"(?:ya|qued[oóa]n?) (?:(?:est[aá]n?|quedan?|las?|los?) )?" + PART,
    r"(?:añadid|agregad|quitad|metid|puest|movid|sacad|colocad)[ao]s? (?:a|en|de|al) (?:la |tu |el )?\S+",
    r"(?:descargad|guardad|archivad)[ao]s? (?:a|en) (?:la |tu |el )?(?:lista|repertorio|biblioteca|cola|papelera|carpeta|artistas)",
    r"marcad[ao]s? como (?:no )?favorit",
    r"qued(?:a|an|[oó]|aron|ado) as[ií]\b",
    r"(?:se )?qued[oó] (?:con|sin)\b",
    r"ya est[aá]" + END,
    r"ya est[aá] sonando",
    r"(?:ya|ah[ií]|aqu[ií]) (?:la|lo|las|los) tienes\b",
    NO_SI + r"(?:ya )?est[aá]n? (?:ya )?(?:dentro de|metid[ao]s? en|añadid[ao]s? a)\b",
    r"(?:te )?(?:la|lo|las|los) dejo (?:list[ao]|en|con)\b",
    r"descargando\b",
    r"(?:descarg|baj)[aá]ndol[aoe]s?\b",
    r"proces[aá]ndo\w*",
    r"en proceso\b",
    r"bajando\b(?!\s+(?:medio|un|una|dos|tres|el|la)\s+(?:tono|tonos|semitono|semitonos|octava|octavas|volumen|tempo|velocidad|bpm))",
    r"descarga\s*[:…](?! ninguna)",
    r"descargas\s*:\s*(?:\n|\d|-|•|\*)",
    r"descargas? (?:pedida|solicitada|iniciada|lanzada|arrancada|programada|confirmada|en marcha|en curso|en cola|en proceso|terminada|completa|finalizada|acabada|hecha|lista)",
    r"(?:arrancando|iniciando|lanzando|empezando|comenzando|preparando|mandando|pidiendo|programando) (?:ya )?(?:la |las |tu |una )?descargas?",
    r"puse en (?:marcha|cola)",
    r"(?<!nada )en cola\b",
    r"en marcha\b",
    r"en segundo plano\b",
    r"estoy en ello",
    r"confirmo (?:la )?descarga",
    r"confirmad[oa]s?" + END,
    r"(?:voy a|paso a|procedo a) (?:descargar|crear|armar|borrar|añadir|agregar|quitar|bajar|actualizar|renombrar|eliminar|cambiar|meter|mover|ordenar|vaciar|sacar|reproducir|pausar|reanudar|pedir)",
    r"voy a poner(?:la|lo|las|los)?\b(?: (?:a sonar|en (?:la |tu )?(?:lista|cola)|m[uú]sica))",
    OBJ + r" " + ACT_NOW + r" (?:ya|ahora|en ?seguida)\b",
    r"ahora (?:mismo )?" + OBJ + r" " + ACT_NOW + r"\b",
    r"en un (?:rato|momento|minuto|par de minutos)[^.\n]{0,30}(?:la|lo|las|los) (?:tienes|ves|ver[aá]s|tendr[aá]s)\b",
    r"(?:la app|te) (?:te )?avisar[aáeé]\b",
    r"te aviso (?:cuando|en cuanto|al)\b",
    r"luego (?:la|lo|las|los) (?:añado|agrego|pongo|meto)",
    r"(?:la|lo|las|los) (?:tienes|ver[aá]s|tendr[aá]s|encuentras|dej[eé]) en artistas/",
    r"(?:qued[oó]|quedaron|guardad[ao]s?|archivad[ao]s?|est[aá]n?) en artistas/",
    NO_SI + r"(?:ya )?est[aá]n? en tu (?:repertorio|biblioteca|lista)\b",
    NO_SI
    + r"ya (?:la |lo |las |los )?(?:est[aá]n?|tienes) (?:en )?(?:tu |la )?(?:biblioteca|lista|repertorio|artistas)\b",
    r"(?:aqu[ií] (?:est[aá]|tienes)|est[aá]) tu (?:lista|repertorio)(?! de (?:acordes|notas))",
    r"ahora (?:tiene|tienes|queda|quedan) (?:\d+|las? |los? |solo |únicamente )",
    r"ahora (?:abre|empieza|cierra) ",
    r"ahora (?:suena|est[aá] sonando)",
    r"ya suena",
    r"(?<!qu[eé] est[aá] )sonando ahora(?! en)",
    r"reproduciendo\b",
    r"reanudad[ao]\b",
    r"(?<!est[aáeé]s )listo" + END,
    r"(?<!de )(?<!un )hecho" + END,
    # presente y futuro en primera persona, con objeto
    r"(?<!ya )(?<!= )(?<!s[ií] )(?<!s[ií]: )"
    + OBJ
    + r" (?:quito|pongo|añado|agrego|creo|borro|bajo|descargo|meto|saco|muevo|renombro|elimino|dejo|arreglo|actualizo|marco|corrijo|punt[uú]o|guardo|mando|env[ií]o)\b",
    r"(?:borro|creo|añado|agrego|pongo|descargo|quito|meto|renombro|elimino|actualizo|arreglo|saco|mando|env[ií]o) (?:la|las|los|el|una|un|otra|esa|esas|ese|esos|esta|estas|este|estos|mi|tu|a|en) ",
    r"(?:descargar|bajar|crear|añadir|agregar|borrar|quitar|poner|meter|renombrar|eliminar|actualizar|arreglar|guardar|mandar)[eé]\b",
    r"(?:empiezo|comienzo|inicio|arranco|lanzo|pido|mando|solicito|programo|preparo) (?:a )?(?:la |las |una |el )?(?:descarga|descargar|bajar|bajada)",
    r"(?:pedida|solicitada|enviada|mandada|lanzada|iniciada) (?:ya )?la descarga",
    r"solicitud de descarga (?:enviada|hecha|pedida|lista)",
    r"te pedir[aá] confirmaci[oó]n",
    r"(?:la app|te) pide confirmaci[oó]n",
    NO_SI
    + r"ya (?:est[aá]n?|forma parte|forman parte) (?:de |en )(?!tu |la |el |los |las |un |una |mi |esa |ese |esta |este |orden|marcha|spotify|youtube)[^\s.,;:]",
    NEG
    + r"se (?:añadi|agreg|quit|borr|cre|elimin|renombr|guard|movi|mand|envi|actualiz|corrigi)[oó]\b",
    r"(?:cambiad|mandad|enviad|movid)[ao]s? (?:el|la|los|las|a) ",
    r"(?:mandad|enviad|movid)[ao]s? a la papelera",
    r"(?:^|[.!\n]\s*)a la papelera\b",
    r"(?:he dado|le di|ya tiene|tiene ahora|le puse|puse|le pongo|le doy) \d+ estrellas",
    r"ya (?:no )?es favorita",
    r"favorita ya\b",
    r"suena ahora\b",
    r"qued(?:a|an|[oó]) con \d+",
    r"ya tienen? (?:las |los |\d)",
    r"guard[eé] la letra",
    r"(?:la |lo |las |los )?dej[eé] (?:con|en|como|lista)",
    r"(?:^|[.!\n]\s*)(?:lista|repertorio) [^:\n]{1,40}:\s*\S",
    r"(?:^|[.!\n]\s*)siguiente(?: canci[oó]n| tema)?\s*[:.]",
    r"cuando (?:termine|acabe|est[eé])[^.\n]{0,40}(?:la|lo|las|los) (?:añado|agrego|meto|pongo|creo|armo|añadir[eé]|agregar[eé]|meter[eé]|pondr[eé]|crear[eé]|armar[eé])",
    r"en cuanto (?:termine|acabe|est[eé])[^.\n]{0,30}(?:la|lo|las|los) (?:añado|agrego|meto|pongo|creo|armo)",
    r"guardad[ao]s? en el archivo",
    r"he buscado la letra",
    r"letra (?:y car[aá]tula )?guardadas?\b",
    r"movid[ao]s? \S+ al (?:final|principio)",
]


LOOSE = [  # ramas sin \b delante
    r"(?<!\w )(?<!\w)(?<!: )(?<!fue )(?<!fueron )(?<!sido )(?<!era )(?:"
    + PART
    + r"|est[aá] sonando\b)",
    r"(?:\A|[.!\n:]\s*)(?:sonando(?! ahora en| en las)|en pausa|pausad[ao]|detenid[ao]|parad[ao])\b",
    r"\(ids?:? ?\d+",
    r"[✅✔☑]",
]


_CLAIMS = _re.compile(r"(?:\b(?:" + "|".join(WORD) + r")|" + "|".join(LOOSE) + r")", _re.IGNORECASE)
_QUOTED = _re.compile(r'[«"“][^«»"“”]{0,120}[»"”]')
_QUESTION = _re.compile(r"¿[^?]*\?|(?:^|(?<=[.!\n,;:]))[^.!?\n,;:]*\?", _re.MULTILINE)
_CONDITIONAL = _re.compile(
    r"(?:^|(?<=[.!\n]))[^.!\n]*\b(?:cuando (?:digas|quieras|me lo pidas|me lo digas|me digas)|si (?:dices|me dices|quieres|me lo pides|lo pides|prefieres|confirmas|aceptas|me das)|har[ií]a(?:mos)?|podr[ií]a(?:mos)?|ser[ií]a)\b[^.!\n]*",
    _re.IGNORECASE | _re.MULTILINE,
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
    t = _QUOTED.sub(" ", text or "")
    plain = _re.sub(r"[*_`«»\"']", "", t)
    plain = _QUESTION.sub(" ", plain)
    plain = _CONDITIONAL.sub(" ", plain)
    return bool(_CLAIMS.search(plain))


# Las notas que la app añade al historial. Si el modelo las imita en su
# propio texto, se borran y cuentan como afirmacion falsa.
_MARKERS = _re.compile(
    r"\s*\[(?:nota de la app|aviso de la app|herramientas que usaste|en este mensaje no usaste)[^\]]*\]\s*",
    _re.IGNORECASE,
)


def has_markers(text: str) -> bool:
    return bool(_MARKERS.search(text or ""))


def strip_markers(text: str) -> str:
    return _MARKERS.sub(" ", text or "").strip()


# Palabras de la app. Una respuesta sin ninguna de ellas es conocimiento
# musical o conversacion, y no hace falta molestar al juez.
_APPISH = _re.compile(
    r"lista|repertorio|playlist|descarg|baj(a|o|ando|ar)|biblioteca|añad|agreg|quit|borr|"
    r"papelera|estrella|favorit|son(ar|ando)|suena|pausa|siguiente|anterior|cola|"
    r"car[aá]tula|letra|archiv|artistas/|hecho|listo|✅|"
    r"\bcre[eé]|cread|\bpus[eo]|\bpongo|\bmet[ií]|\bmeto|\bmov[ií]|renombr|elimin|"
    r"actualiz|correg|corrij|guard|puntu|marc[oó]|marcad",
    _re.IGNORECASE,
)


def _judge_claims(text: str, user_text: str = "", downloaded=False) -> bool:
    """Segunda opinion: el propio modelo, como clasificador de una palabra.

    Las frases de `_CLAIMS` nunca las cubren todas («Descargando:», «Añadida
    a la lista» se colaron). Se pregunta cuando en el turno ninguna
    herramienta ha hecho nada y el texto habla de la app; el conocimiento
    musical no pasa por aqui. Ve tambien la peticion del usuario, para
    distinguir una oferta («puedo crear la lista») de una afirmacion. Si el
    juez falla o tarda, no se le culpa: se sigue con lo que digan las frases.
    """
    plain = _QUESTION.sub(" ", text or "").strip()
    if not plain or not _APPISH.search(plain):
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
_WANTS_ACTION = _re.compile(
    r"\b(?:arregl|haz|hac[ée]|añad|añ[áa]d|agreg|agr[ée]g|met[ea]|m[ée]tel|cre[aá]|arm[aá]|borr|elimin|"
    r"pon|p[óo]n|descarg|b[aá]j|marc|puntu|punt[úu]|dale|corrig|corrij|quit|q[uí]t|renombr|"
    r"dej|cambi|guard|sac|orden|actualiz|repit|reprodu|paus|salt|mand|env[ií]|edit|mu[eé]v|"
    r"sub[ei]|coloc|reemplaz|sustitu|export|escrib|proyect|quiero que|puedes|podr[ií]as|hazme|hazlo|"
    r"siguiente|anterior|favorit|estrella)\w*",
    _re.IGNORECASE,
)


def wants_action(text: str) -> bool:
    """Si el mensaje de la persona pide hacer algo en la app."""
    return bool(_WANTS_ACTION.search(text or ""))


# Un si a una pregunta suya. Con «no» dentro, no es un si.
_YES = _re.compile(
    r"^\W*(s[ií]\b|ok\b|okay|okis|dale|vale|venga|claro|hazlo|adelante|confirmo|perfecto|"
    r"exacto|correcto|eso|esa|ese|b[aá]jal[ao]s?|desc[aá]rgal[ao]s?|ponl[ao]s?|cr[eé]al[ao]|"
    r"agr[eé]gal[ao]s?|a[ñn][aá]del[ao]s?|qu[ií]tal[ao]s?|b[oó]rral[ao]s?|hazl[ao])",
    _re.IGNORECASE,
)


def _answers_an_offer(messages: list[dict]) -> bool:
    """El ultimo mensaje es un si del usuario a una pregunta del asistente.

    Ahi es donde mas narraba: «¿la bajo?» — «si» — «Descargando…» sin llamar
    a nada. En ese caso la primera vuelta va obligada a usar herramientas.
    """
    if not messages or messages[-1].get("role") == "ai":
        return False
    answer = str(messages[-1].get("text") or "").strip()
    if not answer or _re.search(r"\bno\b", answer, _re.IGNORECASE):
        return False
    if not _YES.search(answer):
        return False
    # La pregunta suya puede no ser el mensaje justo anterior: entre medias
    # la app mete los suyos («Cancelado, no he tocado nada.»), que no cuentan.
    # Pero si la app ya EJECUTO lo que preguntaba (un «Listo, esta en la
    # papelera» con su herramienta apuntada), el «si» llega tarde: obligarle a
    # usar herramientas le hacia repetir el borrado.
    asked = []
    for m in reversed(messages[:-1]):
        if m.get("role") != "ai":
            continue
        if m.get("app"):
            if m.get("tools"):
                return False
            continue
        asked.append(m)
        if len(asked) == 2:
            break
    return any("?" in str(m.get("text") or "") for m in asked)
