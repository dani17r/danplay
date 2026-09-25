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
  en el núcleo (índice, listas, estrellas), lo haya hecho quien lo haya hecho:
  también el vigilante de carpetas, cuando algo se mueve o se borra por fuera.
  Rust lo detecta comparando `revision` de `GET /api/status` en su vigilancia
  (cada 2 s); la interfaz responde con un refresco completo (lista de la
  vista, repertorios, estado, ficha abierta, cola). La primera lectura solo
  fija el punto de partida, salvo que `revision` ya no sea 0 (el núcleo cambió
  algo nada más arrancar: entonces se emite). Un núcleo sin `revision` nunca
  lo emite. Con cada cambio, Rust además pone la cola al día (ver abajo).
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
- tras un salto (`seek`) siempre, también en pausa: si no, la aguja se
  quedaba donde estaba hasta reanudar;
- nada más mientras está parado o en pausa.

La interfaz pinta el salto donde lo pidió sin esperar a este evento, y
durante 800 ms no deja que un tick que venía en camino con la posición vieja
la devuelva atrás (`usePlayback.seek`).

Al acabar una pista, **Rust** aplica el modo de repetición: `list` sigue y
da la vuelta; `one` repite; `once` se para; `queue` se para al llegar al
final. Con `shuffle` elige otra al azar distinta de la actual.

**Canciones que ya no están.** Al abrir, la sesión guardada vuelve sin las
canciones cuyo archivo ya no está donde estaba (también la actual); si no
queda ninguna, la cola arranca vacía y `track` es `null`. Con la app
abierta, cuando el núcleo avisa de un cambio (o arranca), Rust pregunta por
las canciones de la cola que no están donde creía (`POST /api/songs/locate`):
las movidas siguen con su ruta nueva y las que ya no existen salen de la
cola (`revision` de la cola sube). La actual no se quita mientras suena; si
se quita estando parada, la siguiente que quede pasa a ser la actual, cargada
en silencio. Sin núcleo no se quita nada.

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
| `reveal_in_folder` | `{ path }`: abre el explorador del sistema señalando ese archivo (con `tauri-plugin-opener`: `FileManager1.ShowItems` por D-Bus en Linux, `SHOpenFolderAndSelectItems` en Windows, `open -R` en macOS). Error en castellano si la ruta no existe. Abrir enlaces (`open_in_browser`) y la hoja del atril (`open_html`) van por el mismo plugin: ya no se pasa nada por `cmd /C start`, que en Windows ejecutaba lo que viniera tras un `&`. |

Evento `danplay://mini-visible` con `{ visible: boolean }` cuando el popup
se muestra u oculta. Evento `danplay://core` con `{ ready: boolean, message }`
cuando el núcleo Python arranca, muere o se reinicia.

La ventana `mini` se declara en `tauri.conf.json` (label `mini`, url
`index.html?mini=1`, oculta al arrancar) y tiene su propia *capability*
(`capabilities/mini.json`). Se oculta sola al perder el foco y con Escape
(la interfaz llama a `hide_mini`).

**Qué puede invocar cada ventana.** Los comandos propios se declaran en
`build.rs` (Tauri genera un permiso `allow-<comando>` por cada uno, en
`permissions/autogenerated/`) y `permissions/ventanas.toml` los junta en un
grupo por ventana: la principal, todos; la ventanita, solo sus mandos (nada
de `api` ni de Telegram); la de proyección, lo que usa, incluido `api` para
leer la letra. Un comando nuevo se añade en los dos sitios, y hay una prueba
que falla si alguno se queda sin permiso.

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
  comprobación de `Host`, y además **toda** petición a `/api/`, también los
  `GET` (hay `GET` que gastan IA), debe llevar la cabecera `X-DanPlay: 1`
  (fuerza *preflight* y corta el CSRF de peticiones simples), salvo el `GET`
  de `/api/song/{id}/audio` y `/api/song/{id}/cover`, que el navegador pide
  desde `<audio>` e `<img>`. `api.js` la añade siempre en modo navegador.
  `--host` que no sea de loopback sin token: el núcleo no arranca. No hay
  `/docs`, `/redoc` ni `/openapi.json`.
- **El núcleo muere con la app**: Rust lo arranca con
  `DANPLAY_PARENT_PID=<su pid>` y el núcleo se apaga en cuanto ese proceso ya
  no existe (en Windows con la API de procesos: allí `os.kill(pid, 0)` mata).

Rutas nuevas o cambiadas (Python):

- `GET /api/song/{id}/cover?size=N` → miniatura JPEG de N px (96, 192 o 320),
  generada con ffmpeg y cacheada en disco en `DATA_DIR/covers/`. Sin `size`,
  la imagen original. Rust reenvía la *query* tal cual desde
  `danplay://cover/<id>?size=N` y añade `Cache-Control`.
- `GET /api/song/{id}/path` → `{ path, kind, bytes }` donde `kind` es el MIME
  real por extensión (`audio/mpeg`, `audio/flac`, `audio/ogg`, `audio/mp4`,
  `audio/wav`, `audio/x-ms-wma`, `audio/opus`).
- `POST /api/songs/locate` `{ ids }` → `{ paths: { "<id>": ruta | null } }`:
  dónde está ahora cada canción (`null` si su archivo ya no está). Ids
  negativos: archivos abiertos desde fuera. Lo usa la cola de Rust.
- `GET /api/jobs/{name}` → `{ name, active, done, total, message, result,
  error, started, ended }`: cómo va una tarea larga (ver §4) y, al acabar, su
  `result` o su `error` en castellano.
- `GET /api/search` → `{ total, count, songs }`: `count` es cuántas cumplen la
  consulta y los filtros sin contar `limit` ni `from_key` (y `limit` admite
  hasta 20000). Las canciones de las listas (`/search`, las de un repertorio,
  `/external`, favoritos) son **ligeras**: sin `lyrics`, `lyrics_synced`,
  `chords` ni `study`, y con `has_lyrics`, `has_synced_lyrics`, `has_chords`,
  `has_study`. La ficha entera, en `GET /api/song/{id}`.
- `POST /api/duplicates/scan` → la búsqueda de duplicados como tarea larga
  (`duplicados`); su `result` es el informe que antes daba
  `GET /api/duplicates`, que ya no existe.
- `POST /api/duplicates/resolve` → `dry_run` es `true` por defecto; solo rutas
  dentro de las carpetas gestionadas y con extensión de audio; borrar = papelera.
- `GET /api/youtube` → además de `{ available, reason }`: `version` (la de
  yt-dlp que se usa), `bundled_version` (la que viaja con la app),
  `js_runtime` (`deno | node | bun | quickjs | null`) y `js_runtime_hint`
  (qué instalar si es `null`).
- `POST /api/youtube/update` → pone yt-dlp al día como tarea larga (`yt-dlp`),
  con `result: { previous, version, updated }`.
- `POST /api/external/play` → `400` si el archivo no es de audio.
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

### El chat en vivo, el contexto y las conversaciones

- `POST /api/chat` y `POST /api/chat/start` aceptan `context`: lo que la
  persona tiene delante — `{ view: {kind, name, id?}, songs: [{id, artist,
  title}] (las primeras 20 de la lista, en su orden), total, selected:
  [{id, artist, title}], playing: {id, artist, title, paused} | null }`. El
  núcleo lo mete en el ESTADO REAL de ese turno, así que «la segunda»,
  «esta» o «las seleccionadas» significan algo.
- `POST /api/chat/start` → `{ id }`; la respuesta se prepara en un hilo.
  `GET /api/chat/poll/{id}` → `{ text, tools, done, result }`: `text` es lo
  que lleva escrito el modelo (vacío si lo que parecía respuesta era el
  preámbulo de una herramienta, o se retiró por narración), `tools` las
  herramientas ya ejecutadas, y con `done` llega `result`, idéntico a lo que
  devuelve `/api/chat`. `POST /api/chat/cancel/{id}` corta: el resultado
  trae `canceled: true` y el texto que hubiera. La interfaz sondea cada
  250 ms por el puente de siempre; no hay flujo abierto ni cambios en Rust.
- El resultado lleva además `usage: { calls, prompt, completion, cost |
  null }` (el coste según el catálogo; `null` si el precio no se conoce) y
  `via: { id, name, model, fallback }`: quién respondió y si fue un
  respaldo porque el activo falló.
- Conversaciones guardadas: `GET /api/chats` → `{ chats: [{id, title,
  updated, n}] }`; `POST /api/chats {title?}` → la nueva; `GET
  /api/chats/{id}` → `{ id, title, messages }` con cada mensaje como lo pinta
  la interfaz (`role, text, tools?, app?, event?, hidden?, narrated?,
  error?, usage?, via?, canceled?`); `POST /api/chats/{id}/messages
  {messages}` añade al final (el título sale del primer mensaje del usuario
  si no tenía); `PATCH /api/chats/{id} {title}`; `DELETE /api/chats/{id}`;
  `GET /api/chats/search?q=` → `{ hits: [{chat_id, title, role, snippet,
  at}] }`.
- `GET /api/ai/usage` → `{ today, month, total }`, cada uno `{ calls,
  prompt, completion, cost, unpriced }`. `POST /api/ai/fallback { enabled }`
  enciende o apaga el respaldo; `GET /api/ai/providers` trae `fallback` y
  `fallbacks: [{id, name, chat_model}]`.
- `POST /api/playlists/{id}/sheet { with_lyrics }` → `{ file }`: la hoja para
  el atril (HTML en `Listas/`); la app la abre con el navegador por el
  comando de Rust `open_html`, que solo acepta `.html` que existan.
- `GET /api/ai/usage` añade `by_provider` (este mes), `budget` y
  `over_budget`; `POST /api/ai/budget { dollars }` fija el tope (0 = sin
  aviso). El resultado de `/api/chat` lleva `budget: { limit, month, over }`
  cuando hay tope: la interfaz avisa, no corta.
- `GET /api/chats/{id}/export` → `{ markdown }`.
- `PUT /api/song/{id}/study { loop?: [a, b], speed?, markers?: [{t, end?,
  label, notes?}], notes? }` → la ficha; lo que no venga se quita. Un
  marcador es un tramo (`t`–`end`) con nombre y sus notas; sin `end` (o con
  un `end` que no vaya detrás de `t`) es un instante suelto. Va al índice
  (`study`, JSON) y a la etiqueta `ESTUDIO` del archivo, y vuelve al
  escanear. Se va con la canción: la fila del índice se borra al mandarla a
  la papelera o cuando el escaneo la da por desaparecida.
- `GET /api/song/{id}/waveform?buckets=800` → `{ peaks, rms, buckets }`: la
  forma de onda para la línea de tiempo del estudio, `buckets` columnas con
  pico y RMS entre 0 y 1 (normalizados al pico más alto). Vale para canciones
  de fuera de la biblioteca. La calcula `danplay_core.waveform` (Rust) o, si
  no está compilado, ffmpeg a PCM crudo; se guarda en `DATA_DIR/waveforms/`,
  un archivo por canción nombrado por su ruta (dentro van ruta, mtime y
  columnas: si no coinciden se recalcula). Se borra con la canción
  (`library.forget`, `forget_path`, y las que el escaneo da por perdidas) y
  el escaneo poda las que no correspondan a ninguna canción del índice.
  `404` sin archivo, `501` sin nada con que decodificar, `422` si no se pudo.
- Reproducción: `set_loop(a, b)` (sin valores, lo quita); el estado trae
  `loop_a`, `loop_b` (0,0 = sin bucle) y `pitch_preserved` (la velocidad
  conserva el tono: ffmpeg `atempo`; `false` = sin ffmpeg, cambia el tono).
- `set_pitch(semitones)` (−12..12): el tono corrido, por ffmpeg
  (`rubberband=tempo:pitch` si lo trae, que es lo normal; si no,
  `asetrate`+`aresample`+`atempo`). Reabre la canción donde iba, como la
  velocidad. El estado trae `pitch`. Sin ffmpeg no hace nada.
- `analyze_beats(path, hint_bpm)` → `BeatGrid { bpm, meter, beats[],
  first_downbeat, phase3, phase4, confidence }`: el pulso y el compás del
  archivo, calculados en Rust (`beats.rs`: envolvente de ataques por FFT,
  tempo por autocorrelación, rejilla por programación dinámica, el «1» por
  bombo y cambios de acorde). Un par de segundos; Rust guarda la rejilla
  por ruta durante la sesión y el metrónomo la usa.
- `set_metronome({ on, bpm, meter, shift, mult, volume })`: se manda
  entero. `bpm`/`meter` en null = lo detectado; con `bpm` a mano el clic va
  libre. `shift` corre el «1» tantos pulsos; `mult` −1/0/1 = mitad/tal
  cual/doble de pulsos. El clic es un sink aparte del mezclador (su volumen
  y su marcha, independientes de la canción); sonando con la canción se
  reengancha a su rejilla en cada play, salto, cambio de velocidad y vuelta
  del bucle, y sigue la velocidad del estudio. El estado trae `metronome
  { on, bpm, meter, shift, mult, volume, has_grid, free, confidence }` y
  `path` (el archivo que suena de verdad: la clave de la rejilla).
- `PUT /api/song/{id}/study` admite además `pitch` (−12..12, 0 no se
  guarda) y `metronome { bpm?, meter?, shift?, mult? }` (solo lo ajustado a
  mano sobre lo detectado).

## 4. Ajustes (Python ↔ Vue), nombres correctos

- `/api/status`: `ai` (no `ia`). `missing_folders`: rutas de las carpetas
  gestionadas que ya no están donde estaban (sus canciones están apartadas).
  `configured` sigue siendo «hay alguna carpeta dada de alta», exista o no.
- `/api/convert` body: `{ dry_run, quality, keep }`. Con `dry_run` contesta
  al momento; sin él es la tarea larga `conversion`.
- **Tareas largas**: `POST /api/scan` (`escaneo`), `POST /api/import`
  (`importacion`, también con `dry_run`), `POST /api/convert` sin `dry_run`
  (`conversion`), `POST /api/duplicates/scan` (`duplicados`) y
  `POST /api/youtube/update` (`yt-dlp`) arrancan en segundo plano y contestan
  `202 { job }` al momento. Si ya hay una en marcha con ese nombre, `202` con
  esa y `already_running: true` (antes `409`). Se sigue con
  `GET /api/jobs/{name}`; `api.runJob(start, name, { onProgress })` lo hace
  (sondeo cada ~500 ms) y devuelve `result` o lanza el `error`.
  `/api/status` conserva `jobs` con todas.
- `/api/folders` respuesta `action`: `added | already_there | replaced |
  confirm | relocated`. `relocated`: la carpeta era una gestionada que se
  movió; se cambia su ruta en vez de añadir otra y sus canciones vuelven con su
  id (`notice.other` es la ruta de antes).
- `GET /api/folders`: cada carpeta lleva `exists` (si sigue en su sitio).
- `POST /api/folders/relocate` `{ from, to }` → lo mismo a mano: `from` es una
  carpeta gestionada que ya no existe y `to` dónde está ahora. Devuelve
  `{ from, to, back, folders, exclusions, always_excluded }` (`back`: cuántas
  canciones volvieron). `400` si `from` sigue existiendo o `to` no existe.
- Calidad: `high | medium | variable`.

## 5. Proveedor de IA (Python ↔ Vue)

Una clave guardada solo se reutiliza hacia **la URL con la que se guardó**:
probar un perfil con otra `base_url` sin escribir la clave no manda la
guardada a ese servidor. Los valores de `headers` y `extra` salen
enmascarados como la clave; si vuelven sin cambiar, se conserva lo guardado.

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
