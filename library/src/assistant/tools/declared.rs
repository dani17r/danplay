//! Lo que se le declara al modelo de cada herramienta (el `TOOLS` de
//! chat/tools.py), en el mismo orden y con las mismas palabras: cada
//! declaración va en cada llamada y se paga.
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

/// Una herramienta, sin repetir el andamiaje veinte veces.
fn t(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    let mut parameters = Map::new();
    parameters.insert("type".into(), "object".into());
    parameters.insert(
        "properties".into(),
        if properties.is_null() {
            json!({})
        } else {
            properties
        },
    );
    if !required.is_empty() {
        parameters.insert("required".into(), json!(required));
    }
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": Value::Object(parameters),
        },
    })
}

fn int() -> Value {
    json!({"type": "integer"})
}

fn string() -> Value {
    json!({"type": "string"})
}

fn ids() -> Value {
    json!({"type": "array", "items": {"type": "integer"}})
}

/// Las de listas aceptan el nombre (lo normal) o el id, y algo más.
fn list_with(extra: Value) -> Value {
    let mut out = Map::new();
    out.insert("name".into(), string());
    out.insert("playlist_id".into(), int());
    if let Value::Object(extra) = extra {
        out.extend(extra);
    }
    Value::Object(out)
}

static DECLARED: LazyLock<Vec<Value>> = LazyLock::new(build);

/// Las herramientas que se le declaran al modelo.
pub fn declared() -> &'static [Value] {
    &DECLARED
}

#[allow(clippy::too_many_lines, reason = "una declaración por herramienta")]
fn build() -> Vec<Value> {
    vec![
        t(
            "fill_playlist",
            "Mete canciones en un repertorio, o lo comprueba, de una vez: busca cada una en la biblioteca, añade las que hay en el orden de la lista y, para las que faltan, la app le enseña al usuario una tarjeta para decidir si bajarlas (no lo preguntes tu). Si el usuario pego una lista, la app ya la leyo: NO copies las canciones, llamala solo con playlist. El informe lo da la app.",
            json!({
                "playlist": {"type": "string", "description": "nombre de la lista"},
                "mode": {
                    "type": "string",
                    "enum": ["add", "check", "set"],
                    "description": "add: añadir (por defecto). check: solo comprobar («verifica»). set: dejarla EXACTAMENTE con estas, solo si el usuario lo dijo",
                },
                "items": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "solo si NO pego una lista: «Titulo», «Artista - Titulo» o un enlace",
                },
                "only": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "numeros de la lista pegada a los que limitarse",
                },
                "overrides": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"n": int(), "id": int()},
                        "required": ["n", "id"],
                    },
                    "description": "la cancion n de la lista es la del id (otra version que eligio)",
                },
                "create": {"type": "boolean", "description": "true solo si pidio una lista NUEVA"},
                "download_missing": {
                    "type": "boolean",
                    "description": "por defecto true: la app ofrece bajar lo que falte",
                },
            }),
            &[],
        ),
        t(
            "find_songs",
            "¿Estan en la biblioteca? Varias canciones de una vez, con erratas, sin tildes, con o sin artista. Si el usuario pego una lista, llamala sin items (la app ya la leyo). Devuelve por cancion su id, si esta, y las dudas con alternativas. No cambia nada.",
            json!({
                "items": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "«Titulo», «Artista - Titulo» o un enlace",
                }
            }),
            &[],
        ),
        t(
            "search_songs",
            "Busca en la biblioteca. Texto libre (titulo, artista) y filtros: artista:barak, titulo:gozo, album:x, genero:x, tono:Bb, bpm>100, duracion>300. Devuelve id, artista, titulo, album, duracion, tono, bpm, estrellas, favorito.",
            json!({
                "query": {"type": "string", "description": "vacio = todo"},
                "limit": {"type": "integer", "description": "por defecto 30"},
                "sort": {
                    "type": "string",
                    "enum": ["artist", "title", "duration", "bpm", "recent", "album"],
                },
                "desc": {
                    "type": "boolean",
                    "description": "true = de mayor a menor (las mas largas, mas bpm, mas nuevas)",
                },
            }),
            &["query"],
        ),
        t(
            "library_summary",
            "Cuantas canciones hay, cuanto ocupan, artistas y generos.",
            Value::Null,
            &[],
        ),
        t(
            "create_playlist",
            "Crea un repertorio con esos ids (de search_songs o del aviso de descarga; los inexistentes se rechazan). Si ya existe una lista con ese nombre, se añaden a esa.",
            json!({"name": string(), "ids": ids(), "note": string()}),
            &["name", "ids"],
        ),
        t(
            "list_playlists",
            "Los repertorios que existen: id, nombre y cuantos temas.",
            Value::Null,
            &[],
        ),
        t(
            "playlist_songs",
            "Que canciones tiene un repertorio, en orden, con ids. Miralo antes de corregir una lista.",
            list_with(Value::Null),
            &[],
        ),
        t(
            "add_to_playlist",
            "Añade canciones a un repertorio que ya existe.",
            list_with(json!({"ids": ids()})),
            &["ids"],
        ),
        t(
            "set_playlist_songs",
            "Deja un repertorio EXACTAMENTE con estos ids, en este orden. Asi se corrige una lista. No borra archivos.",
            list_with(json!({"ids": ids()})),
            &["ids"],
        ),
        t(
            "rename_playlist",
            "Cambia el nombre o la nota de un repertorio.",
            list_with(json!({"new_name": string(), "note": string()})),
            &[],
        ),
        t(
            "get_lyrics",
            "Letra de una cancion: por id si esta en la biblioteca, o por artista y titulo si no.",
            json!({"id": int(), "artist": string(), "title": string()}),
            &[],
        ),
        t(
            "get_chords",
            "Tono y acordes de una cancion, leidos de un cifrado publicado (Ultimate Guitar, LaCuerda), con el enlace: por id si esta en la biblioteca, o por artista y titulo si no. El tono solo si el cifrado lo dice.",
            json!({"id": int(), "artist": string(), "title": string()}),
            &[],
        ),
        t(
            "music_details",
            "Album, año, genero, artistas implicados y contexto de una cancion de la biblioteca, segun la IA (aproximados). Para tono y acordes, get_chords.",
            json!({"id": int()}),
            &["id"],
        ),
        t(
            "transpose_chords",
            "Transpone una progresion de acordes de un tono a otro.",
            json!({
                "chords": {"type": "string", "description": "ej: | Bb | Gm7 | Eb | F |"},
                "from_key": string(),
                "to_key": string(),
            }),
            &["chords", "from_key", "to_key"],
        ),
        t(
            "search_youtube",
            "Busca en YouTube sin descargar, para enseñar que se bajaria. Texto, URL de video o de lista.",
            json!({"query": string(), "limit": {"type": "integer", "description": "por defecto 5"}}),
            &["query"],
        ),
        t(
            "download_music",
            "Descarga audio de YouTube (mp3 con caratula, identificado y archivado en Artistas/). SOLO si te lo han pedido. `items`: URLs de video, URLs de lista o textos a buscar. La app pide confirmacion al usuario y la ejecuta en segundo plano: llamala una vez con todos los temas, sin preguntar tu.",
            json!({
                "items": {"type": "array", "items": {"type": "string"}},
                "quality": {
                    "type": "string",
                    "enum": ["high", "medium", "variable"],
                    "description": "por defecto high (320 kbps)",
                },
                "file_it": {
                    "type": "boolean",
                    "description": "por defecto true; false la deja en Entrada/",
                },
                "force": {
                    "type": "boolean",
                    "description": "por defecto false: lo que ya esta en la biblioteca no se baja. true solo si el usuario la quiere como otra version",
                },
            }),
            &["items"],
        ),
        t(
            "download_status",
            "Como va la descarga en curso, si la hay.",
            Value::Null,
            &[],
        ),
        t(
            "search_web",
            "Comprueba datos de musica en la web (año de un disco, quien toca, origen de un genero). Devuelve titulo, enlace y resumen. Antes que suponer.",
            json!({"query": string(), "limit": {"type": "integer", "description": "por defecto 5"}}),
            &["query"],
        ),
        t(
            "play",
            "Pone a sonar una cancion (id) o un repertorio entero (name o playlist_id).",
            {
                let mut p = Map::new();
                p.insert("id".into(), int());
                p.insert("name".into(), string());
                p.insert("playlist_id".into(), int());
                Value::Object(p)
            },
            &[],
        ),
        t(
            "player_control",
            "Controla lo que suena.",
            json!({
                "command": {
                    "type": "string",
                    "enum": ["pause", "resume", "toggle", "next", "previous", "stop"],
                }
            }),
            &["command"],
        ),
        t(
            "edit_song",
            "Corrige datos de una cancion (solo lo que pases): titulo, artista, album, año, genero, tono, bpm, estrellas (0-5) o favorito. Se escribe en las etiquetas del archivo.",
            json!({
                "id": int(),
                "title": string(),
                "artist": string(),
                "album": string(),
                "year": string(),
                "genre": string(),
                "key": string(),
                "bpm": {"type": "number"},
                "stars": {"type": "integer", "description": "0 a 5"},
                "favorite": {"type": "boolean"},
            }),
            &["id"],
        ),
        t(
            "find_lyrics_and_cover",
            "Busca letra y caratula de una cancion de la biblioteca y las guarda en el archivo.",
            json!({"id": int(), "lyrics": {"type": "boolean"}, "cover": {"type": "boolean"}}),
            &["id"],
        ),
        t(
            "delete_song",
            "Manda una cancion a la papelera del sistema. La app pide confirmacion al usuario: llamala sin preguntar tu.",
            json!({"id": int()}),
            &["id"],
        ),
        t(
            "remove_from_playlist",
            "Quita canciones de un repertorio. No borra archivos.",
            list_with(json!({"song_ids": ids(), "song_id": int()})),
            &[],
        ),
        t(
            "delete_playlist",
            "Borra un repertorio entero (las canciones no). Solo si piden borrar la lista; para corregirla, set_playlist_songs. La app pide confirmacion al usuario: llamala sin preguntar tu.",
            json!({"name": string(), "id": int()}),
            &[],
        ),
        t(
            "related_keys",
            "Los tonos vecinos de uno (relativo, dominante, subdominante) y con que cejilla se toca facil: para armar un set sin saltos de tono.",
            json!({"key": {"type": "string", "description": "ej: Bb, F#m"}}),
            &["key"],
        ),
        t(
            "setlist_sheet",
            "Escribe la hoja para el atril de un repertorio (HTML en la carpeta Listas/): canciones con tono, bpm, cejilla y acordes; con la letra si se pide.",
            list_with(json!({"with_lyrics": {"type": "boolean"}})),
            &[],
        ),
    ]
}
