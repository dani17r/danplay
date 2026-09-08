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
  id: number, title: string, artist: string, duration: number, blur: boolean
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

Rust obtiene la **ruta del archivo** él mismo (`GET /api/song/{id}/path` por
el socket) al empezar cada pista. La interfaz no manda rutas.

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

## 4. Ajustes (Python ↔ Vue), nombres correctos

- `/api/status`: `ai` (no `ia`).
- `/api/convert` body: `{ dry_run, quality, keep }`.
- `/api/folders` respuesta `action`: `added | already_there | replaced | confirm`.
- Calidad: `high | medium | variable`.
