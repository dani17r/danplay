# -*- coding: utf-8 -*-
"""Chat con la biblioteca.

El asistente consulta el catalogo, arma listas, busca letras y acordes,
busca en YouTube y en la web, y descarga musica pasandola por la misma
tuberia de siempre (identificar, renombrar, archivar por artista).

Habla SOLO de musica. Lo que no tenga que ver con musica lo dice y ya.
"""
import json
from . import (ai, config, enrich, library, playlists, theory,
               web, youtube)

# tope por si el modelo se emociona con una lista larga
MAX_DOWNLOADS = 25

SYSTEM_PROMPT = """Eres el asistente de DanPlay, un gestor de biblioteca musical.

Hablas español, en tono cercano y directo. Respuestas breves salvo que pidan detalle.

DE QUE HABLAS
Solo de musica: canciones, artistas, discos, generos, epocas, instrumentos,
teoria (tonos, acordes, compases), produccion, historia de la musica, y la
biblioteca del usuario. Tambien de la propia app: descargar, organizar, listas.

Si te preguntan algo que no tiene que ver con musica (politica, guerras,
programacion, medicina, deportes, deberes del colegio...), no lo respondas:
di en una linea que eso se sale de lo tuyo, que solo llevas temas de musica, y
ofrece volver a lo que si sabes. No lo respondas "un poquito" ni por encima.
Si la pregunta roza la musica desde otro campo (la banda sonora de una pelicula,
el himno de un pais, un instrumento de una cultura), esa parte SI es tuya:
contesta lo musical y deja fuera el resto.

QUE PUEDES HACER
Lo mismo que el usuario puede hacer con el raton, y ademas:
- consultar la biblioteca y armar listas de reproduccion
- REPRODUCIR: poner una cancion o un repertorio entero, pausar, reanudar,
  pasar a la siguiente o volver a la anterior
- puntuar con estrellas y marcar favoritos
- corregir titulo, artista, album, año, genero, tono o bpm
- buscar la letra y la caratula y guardarlas dentro del archivo
- quitar canciones de un repertorio y borrar repertorios
- mandar una cancion a la papelera del sistema
- buscar canciones en YouTube y en la web
- DESCARGAR musica de YouTube: se baja el audio, se pasa a mp3 con su
  caratula, se identifica y se archiva en Artistas/<Artista>/ solo

Para actuar sobre una cancion necesitas su id: buscala antes con search_songs.
Si la peticion encaja con varias ("pon Mi Gozo" y hay tres versiones), enseña
las que hay y pregunta cual, en vez de elegir tu.

Usa SIEMPRE las herramientas antes de afirmar que algo esta o no en la
biblioteca, y antes de dar un dato que puedas comprobar. No lo supongas.

AL DESCARGAR
- Solo descargas si te lo piden. Nunca "de paso" ni por iniciativa propia.
- Si te pasan una lista larga o algo ambiguo ("bajame lo de Barak"), primero
  enseña que has encontrado con search_youtube y pide el visto bueno.
  Con un enlace concreto y una orden clara, tira directo.
- Antes de bajar algo, mira con search_songs si ya lo tiene. Si ya esta,
  dilo en vez de duplicarlo.
- download_music no baja lo que ya esta en la biblioteca: te lo devuelve como
  "already_there" con las coincidencias. Cuentaselo al usuario y preguntale si
  la quiere igualmente; solo entonces repites la llamada con force=true.
- Cuando termines, di que entro, bajo que artista quedo y que hubo que revisar.

AL ARMAR UNA LISTA
  1. busca los temas con search_songs
  2. crea la lista con create_playlist pasando los ids que encontraste
  3. resume que metiste y por que

LIMITES
Haz lo que te piden y nada mas. No crees listas, no descargues ni modifiques
nada que no te hayan pedido. Si algo no esta claro, pregunta antes.

Lo que no tiene vuelta atras se pregunta SIEMPRE antes, aunque parezca que
te lo estan pidiendo: delete_song (va a la papelera, pero desaparece de la
biblioteca) y delete_playlist. Di exactamente que se va a borrar y espera un
si. Puntuar, marcar favorito o corregir datos no hace falta consultarlo:
son faciles de deshacer.

Sobre los datos: el tono y los acordes que devuelven las herramientas son
aproximados. Avisa de ello cuando los des. Fechas, formaciones y quien produjo
que: compruebalo con search_web antes de soltarlo. Si no lo puedes
comprobar, dilo; no rellenes con lo que te suene.

Escribes en prosa normal, con sus mayusculas donde toca: los nombres propios
van como se escriben ("Miles Davis", "Kind of Blue"), nunca en minuscula.
La regla de "sin tildes y nada en MAYUSCULA SOSTENIDA" es de los NOMBRES DE
ARCHIVO y de las listas que crees, no de como hablas."""

TOOLS = [
    {"type": "function", "function": {
        "name": "search_songs",
        "description": ("Busca en la biblioteca del usuario. Admite texto libre y filtros "
                        "inline: artista:barak, album:x, genero:x, tono:Bb, bpm>100, "
                        "duracion>300. Devuelve id, artista, titulo, duracion, tono, bpm, "
                        "estrellas y favorito."),
        "parameters": {"type": "object", "properties": {
            "query": {"type": "string", "description": "que buscar; vacio = todo"},
            "limit": {"type": "integer", "description": "maximo de resultados (por defecto 30)"},
            "sort": {"type": "string",
                      "enum": ["artist", "title", "duration", "bpm", "recent", "album"]}
        }, "required": ["query"]}}},
    {"type": "function", "function": {
        "name": "library_summary",
        "description": "Cuantas canciones hay, cuanto ocupan, que artistas y generos.",
        "parameters": {"type": "object", "properties": {}}}},
    {"type": "function", "function": {
        "name": "create_playlist",
        "description": "Crea una lista de reproduccion con las canciones indicadas por su id.",
        "parameters": {"type": "object", "properties": {
            "name": {"type": "string"},
            "ids": {"type": "array", "items": {"type": "integer"}},
            "note": {"type": "string", "description": "descripcion breve, opcional"}
        }, "required": ["name", "ids"]}}},
    {"type": "function", "function": {
        "name": "list_playlists",
        "description": "Lista los repertorios existentes con cuantos temas tiene cada uno.",
        "parameters": {"type": "object", "properties": {}}}},
    {"type": "function", "function": {
        "name": "add_to_playlist",
        "description": "Añade canciones a una lista que ya existe.",
        "parameters": {"type": "object", "properties": {
            "playlist_id": {"type": "integer"},
            "ids": {"type": "array", "items": {"type": "integer"}}
        }, "required": ["playlist_id", "ids"]}}},
    {"type": "function", "function": {
        "name": "get_lyrics",
        "description": "Busca la letra de una cancion de la biblioteca por su id.",
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"}}, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "music_details",
        "description": ("Tonalidad probable, progresion de acordes, año, genero y artistas "
                        "implicados de una cancion de la biblioteca. Son aproximados."),
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"}}, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "transpose_chords",
        "description": "Transpone una progresion de acordes de una tonalidad a otra.",
        "parameters": {"type": "object", "properties": {
            "chords": {"type": "string", "description": "ej: | Bb | Gm7 | Eb | F |"},
            "from_key": {"type": "string"}, "to_key": {"type": "string"}
        }, "required": ["chords", "from_key", "to_key"]}}},
    {"type": "function", "function": {
        "name": "search_youtube",
        "description": ("Busca en YouTube sin descargar nada. Sirve para enseñar al "
                        "usuario que se bajaria y que confirme. Acepta texto a buscar "
                        "o una URL de video o de lista."),
        "parameters": {"type": "object", "properties": {
            "query": {"type": "string"},
            "limit": {"type": "integer", "description": "por defecto 5"}
        }, "required": ["query"]}}},
    {"type": "function", "function": {
        "name": "download_music",
        "description": ("Descarga audio de YouTube, lo pasa a mp3 con su caratula, lo "
                        "identifica y lo archiva en Artistas/. Usala SOLO cuando te lo "
                        "hayan pedido. Cada elemento de `temas` puede ser una URL de "
                        "video, una URL de lista, o texto a buscar (se coge el primer "
                        "resultado). Tarda: unos segundos por cancion."),
        "parameters": {"type": "object", "properties": {
            "items": {"type": "array", "items": {"type": "string"},
                      "description": "URLs o titulos, uno por cancion"},
            "quality": {"type": "string", "enum": ["high", "medium", "variable"],
                        "description": "alta = 320 kbps (por defecto)"},
            "file_it": {"type": "boolean",
                        "description": "true (por defecto) archiva en Artistas/; "
                                       "false lo deja en Entrada/ para revisar"},
            "force": {"type": "boolean",
                      "description": "por defecto false: si la cancion ya esta en la "
                                     "biblioteca NO se baja y se avisa. Ponlo a true "
                                     "solo si el usuario confirma que la quiere igual "
                                     "aunque este repetida"}
        }, "required": ["items"]}}},
    {"type": "function", "function": {
        "name": "download_status",
        "description": "Como va la descarga en curso, si la hay.",
        "parameters": {"type": "object", "properties": {}}}},
    {"type": "function", "function": {
        "name": "search_web",
        "description": ("Busca en la web para comprobar datos de musica: de que año es "
                        "un disco, quien toca en el, de donde sale un genero. Devuelve "
                        "titulo, enlace y resumen. Usala en vez de suponer."),
        "parameters": {"type": "object", "properties": {
            "query": {"type": "string"},
            "limit": {"type": "integer", "description": "por defecto 5"}
        }, "required": ["query"]}}},
    {"type": "function", "function": {
        "name": "play_song",
        "description": ("Pone a sonar una cancion de la biblioteca por su id. "
                        "La cola pasa a ser la lista que se este viendo."),
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"}}, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "play_playlist",
        "description": "Pone a sonar un repertorio entero desde el principio.",
        "parameters": {"type": "object", "properties": {
            "playlist_id": {"type": "integer"}}, "required": ["playlist_id"]}}},
    {"type": "function", "function": {
        "name": "player_control",
        "description": ("Controla lo que ya esta sonando: pausar, reanudar, "
                        "siguiente, anterior o parar."),
        "parameters": {"type": "object", "properties": {
            "command": {"type": "string",
                        "enum": ["pause", "resume", "toggle", "next",
                                 "previous", "stop"]}
        }, "required": ["command"]}}},
    {"type": "function", "function": {
        "name": "set_stars",
        "description": "Puntua una cancion de 0 a 5 estrellas. Se guarda en el archivo.",
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"},
            "stars": {"type": "integer", "description": "de 0 a 5"}
        }, "required": ["id", "stars"]}}},
    {"type": "function", "function": {
        "name": "set_favorite",
        "description": "Marca o desmarca una cancion como favorita.",
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"},
            "favorite": {"type": "boolean"}
        }, "required": ["id", "favorite"]}}},
    {"type": "function", "function": {
        "name": "edit_song",
        "description": ("Corrige los datos de una cancion. Solo lo que pases se "
                        "cambia; el resto se queda igual. Se escribe tambien en "
                        "las etiquetas del archivo."),
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"},
            "title": {"type": "string"}, "artist": {"type": "string"},
            "album": {"type": "string"}, "year": {"type": "string"},
            "genre": {"type": "string"}, "key": {"type": "string"},
            "bpm": {"type": "number"}
        }, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "find_lyrics_and_cover",
        "description": ("Busca letra y caratula de una cancion de la biblioteca "
                        "y las guarda dentro del archivo."),
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"},
            "lyrics": {"type": "boolean", "description": "por defecto true"},
            "cover": {"type": "boolean", "description": "por defecto true"}
        }, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "delete_song",
        "description": ("Manda una cancion a la papelera del sistema y la saca de "
                        "la biblioteca. PREGUNTA SIEMPRE antes de usarla."),
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"}}, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "remove_from_playlist",
        "description": "Quita una cancion de un repertorio. No borra el archivo.",
        "parameters": {"type": "object", "properties": {
            "playlist_id": {"type": "integer"}, "song_id": {"type": "integer"}
        }, "required": ["playlist_id", "song_id"]}}},
    {"type": "function", "function": {
        "name": "delete_playlist",
        "description": ("Borra un repertorio. Las canciones NO se borran. "
                        "PREGUNTA antes de usarla."),
        "parameters": {"type": "object", "properties": {
            "id": {"type": "integer"}}, "required": ["id"]}}},
    {"type": "function", "function": {
        "name": "lyrics_by_name",
        "description": ("Busca la letra de una cancion que NO esta en la biblioteca, "
                        "por artista y titulo. Para las que si estan usa buscar_letra."),
        "parameters": {"type": "object", "properties": {
            "artist": {"type": "string"}, "title": {"type": "string"}
        }, "required": ["artist", "title"]}}},
]


def _song_brief(c):
    return {"id": c["id"], "artist": c["artist"], "title": c["title"],
            "album": c["album"], "duration": round(c["duration"]),
            "key": c["key"], "bpm": round(c["bpm"]) if c["bpm"] else 0,
            "stars": c.get("stars", 0), "favorite": bool(c.get("favorite"))}


def run_tool(name, args) -> dict:
    """Ejecuta una herramienta y devuelve el resultado en JSON."""
    try:
        if name == "search_songs":
            rows = library.search(args.get("query", ""), None,
                             args.get("sort", "artist"), int(args.get("limit", 30)))
            return {"total": len(rows), "songs": [_song_brief(c) for c in rows]}

        if name == "library_summary":
            e = library.stats_of()
            f = library.facets()
            return {"songs": e["total"], "hours": round(e["seconds"] / 3600, 1),
                    "gigabytes": round(e["bytes"] / 2**30, 2),
                    "artists": [a["value"] for a in f["artists"][:40]],
                    "genres": [g["value"] for g in f["genres"][:20]],
                    "without_artist": e["without_artist"]}

        if name == "create_playlist":
            lid = playlists.create(args["name"], args.get("note", ""))
            n = playlists.add(lid, [int(i) for i in args.get("ids", [])])
            return {"playlist_id": lid, "name": args["name"], "añadidas": n}

        if name == "list_playlists":
            return {"playlists": [{"id": l["id"], "name": l["name"], "items": l["n"],
                                "minutos": round(l["seconds"] / 60)} for l in playlists.list_all()]}

        if name == "add_to_playlist":
            n = playlists.add(int(args["playlist_id"]), [int(i) for i in args.get("ids", [])])
            return {"añadidas": n}

        if name == "get_lyrics":
            c = library.by_id(int(args["id"]))
            if not c:
                return {"error": "no existe esa cancion"}
            if c["lyrics"]:
                return {"source": "stored", "lyrics": c["lyrics"][:4000]}
            r = enrich.lyrics(c["artist"], c["title"], c["album"], c["duration"])
            if r:
                library.update(c["id"], lyrics=r["lyrics"])
                return {"source": r["source"], "lyrics": r["lyrics"][:4000]}
            return {"error": "no se encontro la letra"}

        if name == "music_details":
            c = library.by_id(int(args["id"]))
            if not c:
                return {"error": "no existe esa cancion"}
            if c["chords"]:
                try:
                    return {"cached": True, **json.loads(c["chords"])}
                except Exception:
                    pass
            d = enrich.details(c)
            if d and not d.get("error"):
                library.update(c["id"], chords=json.dumps(d, ensure_ascii=False))
                return d
            return {"error": "no se pudieron obtener los detalles"}

        if name == "transpose_chords":
            out = theory.transpose_to(args["chords"], args["from_key"], args["to_key"])
            return {"chords": out, "latin": theory.to_latin(out),
                    "capo": theory.suggested_capo(args["to_key"])}

        if name == "search_youtube":
            f = youtube.info(args.get("query", ""), int(args.get("limit", 5)))
            if not f.get("ok"):
                return {"error": f.get("reason", "no se pudo consultar YouTube")}
            return {"playlist": f.get("playlist", False), "name": f.get("name", ""),
                    "items": [{"title": t_["title"], "channel": t_["channel"],
                               "duration": round(t_["duration"]), "url": t_["url"]}
                              for t_ in f["items"]]}

        if name == "download_music":
            if not youtube.available():
                return {"error": youtube.unavailable_reason()}
            items = [str(x).strip() for x in (args.get("items") or []) if str(x).strip()]
            if not items:
                return {"error": "no me has dicho que bajar"}
            trimmed = len(items) > MAX_DOWNLOADS
            items = items[:MAX_DOWNLOADS]
            quality = args.get("quality") or config.MP3_QUALITY
            file_it = args.get("file_it", True)
            force = bool(args.get("force", False))
            done_items = []
            for x in items:
                # de uno en uno: `run_job` publica el avance en youtube.STATE y
                # la pagina de Descargas lo enseña mientras el chat trabaja
                for r in youtube.run_job(x, quality=quality, file_it=file_it,
                                         results=1, force=force,
                                         source="assistant"):
                    done_items.append({"ok": r.get("ok", False),
                                       "already_there": r.get("already_there", False),
                                       "matches": r.get("matches", []),
                                       "requested": x,
                                       "title": r.get("title") or r.get("source", ""),
                                       "artist": r.get("artist", ""),
                                       "song": r.get("song", ""),
                                       "action": r.get("action", ""),
                                       "identified_by": r.get("identified_by", ""),
                                       "target": r.get("target", ""),
                                       "reason": r.get("reason", "")})
            return {"downloaded": sum(1 for h in done_items if h["ok"]),
                    "already_there": sum(1 for h in done_items if h["already_there"]),
                    "failures": sum(1 for h in done_items
                                    if not h["ok"] and not h["already_there"]),
                    "trimmed_to": MAX_DOWNLOADS if trimmed else None,
                    "detail": done_items}

        if name == "download_status":
            e = youtube.STATE
            return {"active": e["active"], "phase": e["phase"], "song": e["name"],
                    "percent": e["percent"], "index": e["index"],
                    "total": e["total"], "error": e["error"]}

        if name == "search_web":
            r = web.search(args.get("query", ""), int(args.get("limit", 5)))
            if not r:
                return {"results": [],
                        "note": "no se pudo comprobar; no te lo inventes, dilo"}
            return {"results": r}

        # --- reproduccion: la ejecuta la interfaz, aqui solo se pide ---
        if name == "play_song":
            c = library.by_id(int(args["id"]))
            if not c:
                return {"error": "no existe esa cancion"}
            return {"ok": True, "playing": f"{c['artist']} - {c['title']}",
                    "action": {"kind": "play_song", "song_id": c["id"]}}

        if name == "play_playlist":
            lid = int(args["playlist_id"])
            cs = playlists.songs(lid)
            if not cs:
                return {"error": "esa lista no existe o esta vacia"}
            return {"ok": True, "songs": len(cs),
                    "action": {"kind": "play_playlist", "playlist_id": lid}}

        if name == "player_control":
            cmd = str(args.get("command", "")).lower()
            if cmd not in ("pause", "resume", "toggle", "next", "previous", "stop"):
                return {"error": f"no se que es «{cmd}»"}
            return {"ok": True, "command": cmd,
                    "action": {"kind": "player", "command": cmd}}

        # --- cambios sobre la biblioteca ---
        if name == "set_stars":
            n = max(0, min(5, int(args.get("stars", 0))))
            if not playlists.rate(int(args["id"]), n):
                return {"error": "no existe esa cancion"}
            c = library.by_id(int(args["id"]))
            return {"ok": True, "stars": n, "song": f"{c['artist']} - {c['title']}"}

        if name == "set_favorite":
            v = bool(args.get("favorite", True))
            if not playlists.favorite(int(args["id"]), v):
                return {"error": "no existe esa cancion"}
            c = library.by_id(int(args["id"]))
            return {"ok": True, "favorite": v, "song": f"{c['artist']} - {c['title']}"}

        if name == "edit_song":
            fields = {k: v for k, v in args.items()
                      if k != "id" and v not in (None, "")}
            if not fields:
                return {"error": "no me has dicho que cambiar"}
            c = library.edit(int(args["id"]), **fields)
            if not c:
                return {"error": "no existe esa cancion"}
            return {"ok": True, "changed": list(fields),
                    "song": _song_brief(c)}

        if name == "find_lyrics_and_cover":
            r = enrich.enrich(int(args["id"]),
                              with_lyrics=args.get("lyrics", True),
                              with_cover=args.get("cover", True),
                              with_details=False)
            return r or {"error": "no se pudo"}

        if name == "delete_song":
            return library.trash(int(args["id"]))

        if name == "remove_from_playlist":
            playlists.remove_song(int(args["playlist_id"]), int(args["song_id"]))
            return {"ok": True}

        if name == "delete_playlist":
            playlists.remove(int(args["id"]))
            return {"ok": True}

        if name == "lyrics_by_name":
            r = enrich.lyrics(args.get("artist", ""), args.get("title", ""))
            if r:
                return {"source": r["source"], "lyrics": r["lyrics"][:4000]}
            return {"error": "no se encontro la letra"}

        return {"error": f"herramienta desconocida: {name}"}
    except Exception as e:
        return {"error": str(e)}


def reply(messages: list[dict], max_vueltas=5) -> dict:
    """Conversa usando herramientas. `messages` son {role, text} del historial."""
    if not ai.available():
        return {"error": "la IA no esta configurada; pon tu clave de DeepInfra en Ajustes"}

    history = [{"role": "system", "content": SYSTEM_PROMPT}]
    for m in messages[-24:]:
        role = "assistant" if m.get("role") == "ai" else "user"
        history.append({"role": role, "content": m.get("text", "")})

    cliente = ai._get_client()
    used = []
    # Reproducir no se puede hacer desde aqui: el audio lo maneja Rust y la
    # cola vive en la interfaz. Las herramientas de reproduccion devuelven una
    # `action` y la app la ejecuta al recibir la respuesta.
    actions = []
    for _ in range(max_vueltas):
        try:
            r = cliente.chat.completions.create(
                model=config.DEEPINFRA_CHAT_MODEL, messages=history,
                tools=TOOLS, tool_choice="auto",
                temperature=0.4, max_tokens=1400)
        except Exception as e:
            return {"error": f"no pude hablar con el modelo: {e}"}

        msg = r.choices[0].message
        calls = getattr(msg, "tool_calls", None)
        if not calls:
            return {"text": _strip_thoughts(msg.content or ""),
                    "tools": used, "actions": actions}

        history.append({"role": "assistant", "content": msg.content or "",
                     "tool_calls": [{"id": c.id, "type": "function",
                                     "function": {"name": c.function.name,
                                                  "arguments": c.function.arguments}}
                                    for c in calls]})
        for c in calls:
            try:
                args = json.loads(c.function.arguments or "{}")
            except Exception:
                args = {}
            res = run_tool(c.function.name, args)
            if res.get("action"):
                actions.append(res["action"])
            used.append({"name": c.function.name, "args": args,
                         "summary": _summarize(c.function.name, res)})
            history.append({"role": "tool", "tool_call_id": c.id,
                         "content": json.dumps(res, ensure_ascii=False)[:12000]})

    return {"text": "Me he enredado con las consultas. ¿Puedes reformularlo?",
            "tools": used, "actions": actions}


import re as _re

def _strip_thoughts(t: str) -> str:
    """Algunos modelos dejan escapar su razonamiento entre <think>."""
    t = _re.sub(r"<think>.*?</think>", "", t, flags=_re.DOTALL | _re.IGNORECASE)
    t = _re.sub(r"</?think>", "", t, flags=_re.IGNORECASE)
    return t.strip()


def _summarize(name, res) -> str:
    if res.get("error"):
        return "error: " + str(res["error"])[:80]
    if name == "search_songs":
        return f"{res.get('total', 0)} resultados"
    if name == "create_playlist":
        return f"lista «{res.get('name')}» con {res.get('añadidas', 0)} temas"
    if name == "add_to_playlist":
        return f"{res.get('añadidas', 0)} añadidas"
    if name == "list_playlists":
        return f"{len(res.get('playlists', []))} listas"
    if name == "get_lyrics":
        return f"letra ({res.get('source', '?')})"
    if name == "library_summary":
        return f"{res.get('songs', 0)} canciones"
    if name == "search_youtube":
        return f"{len(res.get('items', []))} en YouTube"
    if name == "download_music":
        n = res.get("downloaded", 0)
        ya, f = res.get("already_there", 0), res.get("failures", 0)
        partes = [f"{n} descargada(s)"]
        if ya:
            partes.append(f"{ya} ya la tenias")
        if f:
            partes.append(f"{f} con fallo")
        return ", ".join(partes)
    if name == "download_status":
        return res.get("phase") or ("en marcha" if res.get("active") else "parada")
    if name == "search_web":
        return f"{len(res.get('results', []))} resultados en la web"
    if name == "lyrics_by_name":
        return f"letra ({res.get('source', '?')})"
    if name == "play_song":
        return f"sonando: {res.get('playing', '')}"[:60]
    if name == "play_playlist":
        return f"lista con {res.get('songs', 0)} temas"
    if name == "player_control":
        return {"pause": "pausado", "resume": "reanudado", "toggle": "play/pausa",
                "next": "siguiente", "previous": "anterior",
                "stop": "parado"}.get(res.get("command"), "ok")
    if name == "set_stars":
        return f"{res.get('stars', 0)} estrellas"
    if name == "set_favorite":
        return "favorita" if res.get("favorite") else "ya no es favorita"
    if name == "edit_song":
        return "cambiado: " + ", ".join(res.get("changed", []))
    if name == "find_lyrics_and_cover":
        hechos = [k for k in ("lyrics", "cover") if res.get(k)]
        return ", ".join(hechos) if hechos else "no se encontro nada"
    if name == "delete_song":
        return f"a la papelera: {res.get('name', '')}"[:60]
    if name in ("remove_from_playlist", "delete_playlist"):
        return "hecho"
    return "ok"
