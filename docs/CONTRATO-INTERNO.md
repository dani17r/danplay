# Contratos internos entre capas

Referencia para quien toque una capa sin ver las otras. Cada apartado dice
quién lo implementa (Rust, Python o Vue) y quién lo consume. Lo que no está
aquí no es contrato: se puede cambiar libremente dentro de su capa.

## 1. Reproducción y cola (Rust → Vue)

La cola de reproducción vive en Rust (`desktop/src-tauri/src/queue.rs`). La
interfaz manda la lista y las órdenes; Rust decide qué suena y lo cuenta por
un evento. No hay sondeo desde JS.

### Tipos

```ts
type Track = {
  id: number, title: string, artist: string, duration: number, blur: boolean,
  path?: string             // donde esta el archivo, si quien lo manda lo sabe
}
type Repeat = 'list' | 'one' | 'once' | 'queue'
type PlaybackState = {
  track: Track | null,      // lo que suena (o lo ultimo cargado)
  index: number,            // posicion en la cola (-1 si nada)
  length: number,           // tamaño de la cola
  playing: boolean,
  position: number,         // segundos
  duration: number,         // segundos (de Rust si la sabe, si no la del indice)
  volume: number,           // 0..1
  speed: number,            // 0.25..3
  repeat: Repeat,
  shuffle: boolean,
  has_previous: boolean,
  has_next: boolean,
  error: string,            // en castellano, vacio si no hay
  has_output: boolean,      // false = este equipo no tiene salida de audio
  origin: any | null        // lo que la interfaz paso en set_queue (opaco para Rust)
}
```

La **ruta del archivo** viaja en `Track.path` cuando quien manda la canción la
sabe: la interfaz la trae del índice con cada canción, y «Abrir con DanPlay»
la recibe en la orden. Si el archivo está ahí, Rust lo pone a sonar sin
preguntarle nada al núcleo. Si no viene, o el archivo ya no está donde
estaba, Rust se la pide al núcleo (`GET /api/song/{id}/path`, con un tope de
8 s) al empezar la pista. Si tampoco así se localiza, `error` lo dice en
castellano y, si la canción se acabó sola, se prueba con la siguiente.

### Eventos del núcleo (Rust → Vue)

- `danplay://core` → `{ ready, message }`: el núcleo arrancó, murió o se
  reinició.
- `danplay://changed` → `{ revision }`: algo de lo que se enseña ha cambiado
  en el núcleo (índice, listas, estrellas), lo haya hecho quien lo haya hecho.
  Rust lo detecta comparando `revision` de `GET /api/status` en su vigilancia
  (cada 2 s); la interfaz responde con un refresco completo (lista de la
  vista, repertorios, estado, ficha abierta, cola). Un núcleo sin `revision`
  nunca lo emite.
- `danplay://recent` → la lista del reproductor cambió (se abrió algo desde
  fuera).

### Comandos (`invoke`)

| Comando | Argumentos | Efecto |
| --- | --- | --- |
| `set_queue` | `{ items: Track[], start: number \| null, origin: any }` | Sustituye la cola y empieza por `start` (o por el primero). Si `start` es el que ya suena, vuelve a empezar. |
| `queue_next` | — | Siguiente (a mano siempre avanza; da la vuelta al final). |
| `queue_previous` | — | Anterior (da la vuelta al principio). |
| `queue_jump` | `{ id }` | Salta a esa canción de la cola; si es la actual, la reinicia. |
| `set_repeat` | `{ mode: Repeat }` | |
| `set_shuffle` | `{ on: boolean }` | |
| `toggle_pause` | — | Pausa o sigue. Si la pista acabó, la vuelve a poner. |
| `stop` | — | Para y vacía «lo que suena» (la cola se conserva). |
| `seek` | `{ seconds }` | |
| `set_volume` | `{ value }` | 0..1 |
| `set_speed` | `{ value }` | |
| `playback_state` | — | Devuelve `PlaybackState` (para pintar nada más montar). |
| `queue_items` | — | Devuelve `{ items: Track[], origin }`. |

### Evento

`danplay://state` con `PlaybackState` como carga. Se emite:

- al instante en cada cambio (orden recibida, pista que empieza o acaba, error);
- cada 250 ms mientras suena (para la barra de progreso);
- nada mientras está parado o en pausa.

Al acabar una pista, **Rust** aplica el modo de repetición: `list` sigue y
da la vuelta; `one` repite; `once` se para; `queue` se para al llegar al
final. Con `shuffle` elige otra al azar distinta de la actual.

### Ventana mini y bandeja (Rust → Vue)

| Comando | Efecto |
| --- | --- |
| `show_window` | Trae la ventana principal al frente (y la muestra si estaba oculta). |
| `hide_mini` | Oculta el popup del mini reproductor. |
| `toggle_mini` | Muestra u oculta el popup. |
| `quit_app` | Cierra DanPlay del todo (lo mismo que «Salir» en la bandeja). |
| `tray_available` | `boolean`: hay bandeja donde quedarse. Si es `false`, cerrar la ventana cierra la app. |
| `share_targets` | → `{ telegram: boolean }`: a dónde se puede enviar una canción desde este equipo. Se busca Telegram Desktop donde lo pone cada sistema (PATH, snap y flatpak en Linux; `%APPDATA%`, Archivos de programa y el alias de la Store en Windows; `/Applications` en macOS). Sin Telegram, la opción no se enseña. |
| `send_to_telegram` | `{ paths: string[] }`: abre Telegram con esos archivos listos para enviar (una canción, una selección o un repertorio) (`-sendpath`; `open -a` en macOS; con `--file-forwarding` en flatpak). El chat lo elige la persona allí: no se manda nada solo. |
| `reveal_in_folder` | `{ path }`: abre el explorador del sistema señalando ese archivo (D-Bus `FileManager1.ShowItems` o `xdg-open` en Linux, `explorer /select,` en Windows, `open -R` en macOS). Error en castellano si la ruta no existe. |

Evento `danplay://mini-visible` con `{ visible: boolean }` cuando el popup
se muestra u oculta. Evento `danplay://core` con `{ ready: boolean, message }`
cuando el núcleo Python arranca, muere o se reinicia.

La ventana `mini` se declara en `tauri.conf.json` (label `mini`, url
`index.html?mini=1`, oculta al arrancar) y tiene su propia *capability*
(`capabilities/mini.json`). Se oculta sola al perder el foco y con Escape
(la interfaz llama a `hide_mini`).

Ya no existen: `play`, `audio_state`, `set_now_playing`, `now_playing`,
`show_mini`, ni los eventos `danplay://command` y `danplay://now`.

## 2. Núcleo Python ↔ Rust (transporte)

- **Linux y macOS**: socket Unix `0600`, como hasta ahora (`--uds RUTA`).
- **Windows**: TCP en `127.0.0.1` con puerto elegido por Rust
  (`serve --host 127.0.0.1 --port N`) y un secreto de un solo arranque en la
  variable de entorno `DANPLAY_TOKEN` (32 bytes en hex). Con esa variable
  presente, el núcleo exige `Authorization: Bearer <token>` en **todas** las
  rutas `/api/` y responde `401` sin él. Rust la manda en cada petición.
- **Modo desarrollo** (`danplay serve` sin `--uds` ni token): se mantiene la
  comprobación de `Host`, y además toda petición que no sea `GET`/`HEAD`
  debe llevar la cabecera `X-DanPlay: 1` (fuerza *preflight* y corta el CSRF
  de peticiones simples). `api.js` la añade en modo navegador.

Rutas nuevas o cambiadas (Python):

- `GET /api/song/{id}/cover?size=N` → miniatura JPEG de N px (96, 192 o 320),
  generada con ffmpeg y cacheada en disco en `DATA_DIR/covers/`. Sin `size`,
  la imagen original. Rust reenvía la *query* tal cual desde
  `danplay://cover/<id>?size=N` y añade `Cache-Control`.
- `GET /api/song/{id}/path` → `{ path, kind, bytes }` donde `kind` es el MIME
  real por extensión (`audio/mpeg`, `audio/flac`, `audio/ogg`, `audio/mp4`,
  `audio/wav`, `audio/x-ms-wma`, `audio/opus`).
- `POST /api/duplicates/resolve` → `dry_run` es `true` por defecto; solo rutas
  dentro de las carpetas gestionadas y con extensión de audio; borrar = papelera.
- `PATCH /api/song/{id}` → solo `artist, title, album, year, genre, key, bpm,
  lyrics`. Otras claves: `422`.
- `PATCH /api/playlists/{id}` → `{ name?, note? }`: renombra la lista o
  cambia su nota. `409` si el nombre ya es de otra; `400` sin nada que cambiar.
- `POST /api/chat` → ver §3.

## 3. Asistente: confirmaciones (Python ↔ Vue)

Las herramientas destructivas (`delete_song`, `delete_playlist`,
`download_music`) **nunca se ejecutan dentro de la conversación**. Cuando el
modelo las llama, el núcleo le devuelve `{"needs_confirmation": true,
"summary": "..."}` y la respuesta HTTP lleva:

```json
{ "text": "...", "tools": [...], "actions": [...],
  "confirm": { "id": "c1", "tool": "delete_song", "args": {"id": 12},
               "summary": "Mandar a la papelera «Barak - Mi Gozo.mp3»" } }
```

`confirm` es `null` si no hay nada pendiente (como mucho una por respuesta).
La interfaz muestra un diálogo con `summary`; si el usuario acepta, llama a

`POST /api/chat/confirm` con `{ "tool": "...", "args": {...} }` → ejecuta la
herramienta y devuelve `{ "ok": true, "result": {...}, "text": "resumen en
castellano" }`. Si rechaza, no se llama a nada.

`download_music` es la excepción: tarda minutos, así que el confirm la
**arranca en segundo plano** y vuelve al momento con
`result: { active: true, items: [...], force: bool }`. Los `args` son los de
la herramienta tal cual (`items`, `quality`, `file_it`, `force`); la
interfaz no los toca. A partir de ahí el chat sigue la descarga por
`GET /api/youtube` (el mismo estado que la página de Descargas: `active`,
`phase`, `index`, `total`, `results`) y, cuando `active` pasa a `false`,
cuenta en la conversación qué entró, qué ya estaba (`already_there`, con sus
`matches`) y qué falló. `409` si ya hay una descarga en marcha; `400` si
no hay nada que bajar; `503` si falta yt-dlp o ffmpeg.

Las burbujas del asistente se pintan como **markdown** (negritas, listas,
títulos, tablas, código) con un conversor propio que escapa todo el HTML
antes de marcar nada (`desktop/src/utils/markdown.js`). Los enlaces no se
convierten en `<a>`: se enseñan como texto con la dirección al lado.

## 4. Ajustes (Python ↔ Vue), nombres correctos

- `/api/status`: `ai` (no `ia`).
- `/api/convert` body: `{ dry_run, quality, keep }`.
- `/api/folders` respuesta `action`: `added | already_there | replaced | confirm`.
- Calidad: `high | medium | variable`.

## 5. Proveedor de IA (Python ↔ Vue)

La IA vale con cualquier servicio que hable el *chat completions* de OpenAI
(todos, hoy). Qué proveedor, con qué clave y qué modelos lo guarda el núcleo
en `~/.config/danplay/ai.json` (0600), **un perfil por proveedor
configurado** y uno activo. Las claves entran por `POST` y salen siempre
enmascaradas (`sk-o…f3a1`); una caja de clave vacía significa «conserva la
guardada».

- `GET /api/ai/providers[?refresh=1]` → `{ catalog, groups, profiles, active,
  active_profile, ai_enabled, ai_ready, ai_reason, catalog_status }`.
  `catalog` es la lista de servicios conocidos (`id, name, group, base_url,
  key: required|optional|none, key_url, docs, fields, suggest, note`);
  `fields` son los huecos de la URL (`{region}`, `{resource}`…) que el
  formulario pide. Sin `refresh`, el núcleo comprueba models.dev en segundo
  plano (petición condicional con ETag: sin cambios, cero bytes); con
  `refresh=1`, espera a hacerlo.
- `POST /api/ai/profile` con `AiProfile` (`id?, provider, name?, key?,
  base_url?, fields?, model, chat_model, headers?, extra?, timeout?,
  activate=true`) → guarda y devuelve lo mismo que `GET` más `saved` (el
  id). Un `custom` nuevo recibe id `custom-<nombre>`; puede haber varios.
- `DELETE /api/ai/profile/{id}` → borra clave y ajustes de ese perfil.
- `POST /api/ai/activate` `{ id }` → cambia el activo (un local sin perfil se
  crea con sus valores por defecto).
- `POST /api/ai/free` (sin cuerpo) → «Probar gratis, sin clave»: prueba por
  orden los del grupo `free` (`providers.FREE_ORDER`), guarda y activa el
  primero que responda. Devuelve lo mismo que `GET` más `free: { ok, chosen,
  name, model, chat_model, tools_ok, tried: [{id, name, ok, reason}],
  reason }`. Sin clave, `/api/ai/models` de esos proveedores devuelve solo lo
  que sirven a anónimos (`quirks.anon_filter`).
- `POST /api/ai/check` con el mismo cuerpo → prueba **sin guardar**:
  `{ ok, reason, provider, model, chat_model, latency_ms, tools_ok,
  tools_reason }`. Hace la llamada más barata posible (un token) con el
  modelo rápido, otra con el de conversación si es distinto, y una tercera
  con una herramienta de prueba para saber si el asistente podrá usarlo.
- `POST /api/ai/models` con el mismo cuerpo → `{ ok, reason, source, models,
  catalog, suggest, catalog_status }`. `models` es lo que lista el proveedor
  con esa clave (`/models`) cruzado con el catálogo (`tools, cost_in,
  cost_out, context, released, deprecated, known`); `catalog` lo que sabe
  models.dev de ese proveedor aunque no responda; `suggest` `{fast, chat}`
  recomendados (herramientas, reciente, barato, ni obsoleto ni experimental).
- `/api/settings` conserva `model`, `ai_key`, `has_ai_key` (van al perfil
  activo) y añade `chat_model`, `provider`, `provider_name`, `ai_ready`,
  `ai_reason`, `ai_file`. `/api/status` y `/api/chat/tools` añaden
  `provider`.
- `POST /api/settings/check-ai` prueba el perfil activo (mismo resultado que
  `/api/ai/check`).

Variables de entorno que mandan sobre el archivo, para la línea de órdenes:
`DANPLAY_AI_PROVIDER`, `DANPLAY_AI_KEY`, `DANPLAY_AI_BASE_URL`,
`DANPLAY_AI_MODEL`, `DANPLAY_AI_CHAT_MODEL`. Las `DEEPINFRA_*` de antes se
leen una sola vez para migrar la clave al perfil `deepinfra`.
