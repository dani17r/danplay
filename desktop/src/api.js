// @ts-check
// Capa de transporte.
//
// En la app de escritorio TODO pasa por Rust (invoke -> socket Unix o TCP
// local). No hay puerto abierto ni fetch a localhost, así que ningún otro
// proceso del equipo puede hablar con el núcleo.
// En el navegador (npm run dev suelto) cae a fetch contra el proxy de Vite.
//
// Lo que aquí se llama es el contrato con Rust y con Python que describe
// docs/CONTRATO-INTERNO.md. Si un nombre cambia allí, cambia aquí.

/**
 * @typedef {Object} Track  Lo mínimo que Rust necesita de una canción en cola.
 * @property {number} id
 * @property {string} title
 * @property {string} artist
 * @property {number} duration  segundos
 * @property {boolean} blur     portada difuminada
 * @property {string} [path]    dónde está el archivo, si se sabe. Con ruta y el
 *                              archivo ahí, Rust no le pregunta nada al núcleo;
 *                              si falta o ya no está, se lo pregunta por el id
 */

/**
 * @typedef {Object} DefaultPlayer  Si el sistema abre las canciones con DanPlay.
 * @property {boolean} supported   el sistema permite hacer algo desde aquí
 * @property {boolean} is_default  DanPlay las abre hoy
 * @property {boolean} direct      la app puede ponerlo ella sola
 * @property {string}  note        qué contarle al usuario, o vacío
 */

/** @typedef {'list'|'one'|'once'|'queue'} Repeat */

/**
 * @typedef {Object} MetronomeSettings  Lo que se le manda al metrónomo.
 * @property {boolean} on
 * @property {number|null} bpm    tempo a mano, con decimales (el clic va a su aire, pero
 *                                entra con el pulso de la canción); null = el de la canción
 * @property {number|null} meter  a mano: 0 sin acento, o de 2 a 12; null = el detectado
 * @property {number} shift       correr el «1» tantos pulsos
 * @property {number} mult        -1 mitad de clics, 0 tal cual, 1 el doble (también a mano)
 * @property {number} volume      0..2: hasta el doble de fuerte
 * @property {ClickSound} [sound] cómo suena el clic
 * @property {number} [count_in]  compases de cuenta al darle a play en pausa (0, 1 o 2)
 */
/** @typedef {'clasico'|'madera'|'baqueta'|'cencerro'} ClickSound */
/**
 * @typedef {Object} MetronomeState  Cómo va el metrónomo, según Rust.
 * @property {boolean} on
 * @property {number} bpm         tempo nominal (rejilla ajustada, o a mano), con el doble o la mitad
 * @property {number} meter       0 = sin acento
 * @property {number} shift
 * @property {number} mult
 * @property {number} volume
 * @property {boolean} has_grid   hay rejilla para la canción que suena
 * @property {boolean} free       va libre (sin rejilla o con tempo a mano)
 * @property {number} confidence  cuánto se fía el análisis del «1» (0..1)
 * @property {ClickSound} [sound]
 * @property {number} [count_in]
 */
/**
 * @typedef {Object} BeatGrid  El pulso y el compás de una canción.
 * @property {number} bpm
 * @property {number} meter        pulsos por compás (3 o 4; a mano, 0 sin acento o 2..12)
 * @property {number[]} beats      segundos de cada pulso
 * @property {number} first_downbeat  índice en `beats` del primer «1»
 * @property {number[]} [bars]     con Beat This!: el índice de cada «1», compás a compás
 *                                 (con compases irregulares, si los hay)
 * @property {number} phase3
 * @property {number} phase4
 * @property {number} confidence
 * @property {boolean} [closed]    acaba en su último pulso: después no hay clic
 */
/** @typedef {'repeat'|'stop'|'continue'} LoopEnd  qué pasa al acabar el último tramo */
/**
 * @typedef {Object} OutputStatus  la salida de audio abierta
 * @property {string} device   su id
 * @property {string} name
 * @property {number} channels
 * @property {number} rate
 * @property {boolean} fallback  la elegida no está y suena por la del sistema
 * @property {string|null} error
 * @property {number} xruns    veces que no llegó a tiempo
 */
/**
 * @typedef {Object} AudioDevice  una salida de audio del equipo
 * @property {string} id
 * @property {string} name
 * @property {number} channels  los que admite como mucho
 * @property {boolean} default  es la del sistema
 * @property {boolean} [busy]   ahora no se puede abrir (la usa el sistema u otro programa)
 */
/**
 * @typedef {Object} RoutingSpec  a dónde va cada cosa
 * @property {Array<{left: number, right: number}|null>} buses
 * @property {number} song
 * @property {number} click
 * @property {number} cues
 */
/**
 * @typedef {Object} PlaybackState  Lo que Rust cuenta en `danplay://state`.
 * @property {Track|null} track   lo que suena (o lo último cargado)
 * @property {number} index       posición en la cola (-1 si nada)
 * @property {number} length      tamaño de la cola
 * @property {boolean} playing
 * @property {number} position    segundos
 * @property {number} duration    segundos
 * @property {number} volume      0..1,5: por encima de 1, más alta de como viene
 * @property {number} speed       0.25..3
 * @property {Repeat} repeat
 * @property {boolean} shuffle
 * @property {boolean} has_previous
 * @property {boolean} has_next
 * @property {string} error       en castellano, vacío si no hay
 * @property {boolean} has_output false = este equipo no tiene salida de audio
 * @property {any} origin         lo que la interfaz pasó en set_queue
 * @property {boolean} pitch_preserved  la velocidad conserva el tono (ffmpeg)
 * @property {number} pitch       el tono corrido, en semitonos (0 = como está grabada; con
 *                                fracciones: 0,5 es un cuarto de tono)
 * @property {MetronomeState} metronome
 * @property {string} path        el archivo que suena de verdad (clave de la rejilla del metrónomo)
 * @property {number} loop_a      bucle A-B en segundos; 0,0 = sin bucle (con varios
 *                                tramos, del principio del primero al final del último)
 * @property {number} loop_b
 * @property {Array<[number, number]>} [loops]  los tramos que se repiten, en orden
 * @property {LoopEnd} [loop_end]  qué pasa al acabar el último tramo
 * @property {OutputStatus|null} [output]  la salida de audio abierta (null: aún no, o se soltó)
 * @property {boolean} [preparing]  la canción se está preparando (un formato que se pasa a FLAC)
 * @property {boolean} [stems]    suenan las pistas separadas de la canción, con su mezcla
 * @property {boolean} [counting_in]  se está contando: la canción entra al acabar la cuenta
 */

/**
 * @typedef {Object} Song  Una canción tal como la devuelve el núcleo.
 * @property {number} id
 * @property {string} [title]
 * @property {string} [file]
 * @property {string} [artist]
 * @property {string} [album]
 * @property {string} [feat]
 * @property {string} [genre]
 * @property {string|number} [year]
 * @property {string} [key]
 * @property {number} [bpm]
 * @property {number} [duration]
 * @property {number} [bitrate]
 * @property {number} [size]
 * @property {number} [stars]
 * @property {number|boolean} [favorite]
 * @property {boolean} [blur]
 * @property {string} [folder]
 * @property {string} [path]
 * @property {string} [lyrics]
 * @property {string} [chords]
 * @property {boolean} [cover]
 * @property {string[]} [playlists]
 * @property {string} [lyrics_synced]  letra con tiempos (LRC), si la hay
 * @property {string} [study]          modo estudio, como JSON (ver api.setStudy)
 * @property {boolean} [has_stems]     tiene sus pistas separadas (en las listas)
 * @property {boolean} [stems_best]    y ya son las mejores (no las rápidas ni las de antes)
 * @property {boolean} [ghost]         en una lista: ya no está en la biblioteca («No está»)
 */

/**
 * @typedef {Object} AiProfile  Un proveedor de IA tal como lo rellena Ajustes.
 * @property {string} [id]         el perfil guardado (vacío si es nuevo)
 * @property {string} provider     id del catálogo (`openrouter`, `ollama`, `custom`…)
 * @property {string} [name]       solo para `custom`
 * @property {string} [key]        vacío = conservar la guardada
 * @property {string} [base_url]   vacío = la del catálogo
 * @property {Object<string,string>} [fields]   huecos de la URL (región, recurso…)
 * @property {string} [model]      el rápido: identificar, fichas, juez
 * @property {string} [chat_model] el del asistente (necesita herramientas)
 * @property {Object<string,string>} [headers]
 * @property {Object<string,any>} [extra]       parámetros extra del cuerpo
 * @property {number} [timeout]
 * @property {boolean} [activate]
 */

/**
 * @typedef {Object} ChatPlanRow  Una fila de la tarjeta de descarga: lo pedido y lo encontrado.
 * @property {number} i            su índice: es lo que va en `selection`
 * @property {number|null} n       su número en la lista pegada
 * @property {string} query        lo pedido
 * @property {string} kind         `video`, `lista`, `texto`…
 * @property {string} url          SOLO la URL canónica de un vídeo (o vacía)
 * @property {string} title        lo encontrado en YouTube
 * @property {string} channel
 * @property {number} duration     en segundos
 * @property {string} state        buscando | encontrada | dudosa | no_encontrada | rechazada | sin_buscar
 * @property {boolean} checked     marcada por defecto
 * @property {string} badge        «no se parece», «ya la tienes»…
 * @property {string} note
 */

/**
 * @typedef {Object} ChatPlan  Lo que pinta la tarjeta de descarga (`GET /api/chat/plan/{id}`).
 * @property {'buscando'|'listo'|'parcial'|'error'} state
 * @property {ChatPlanRow[]} items
 * @property {{n:number|null, title:string, song:{id:number, artist:string, title:string}|null}[]} already
 * @property {{i:number, n:number|null, query:string, reason:string}[]} [unresolved]
 * @property {string} effect       «Al terminar: añado 2 a «domingo»»
 * @property {number} trimmed      cuántas se quedaron fuera por el tope
 * @property {string} [reason]     por qué se dejó de buscar
 */

/**
 * @typedef {Object} ChatConfirm  Lo que el asistente propone y espera tu visto bueno (§3, §5).
 * El servidor lo registra y le da `id`; la interfaz confirma SOLO con ese id
 * (y las casillas): `args` es para una interfaz antigua y no se usa ni se guarda.
 * @property {string} [id]          sin id, un núcleo antiguo: se confirma con `(tool, args)`
 * @property {'download_music'|'delete_song'|'delete_playlist'} tool
 * @property {string} summary
 * @property {string} [effect]      lo que pasará al terminar
 * @property {string} [plan_state]  si trae plan: buscando | listo | parcial | error
 * @property {ChatPlan} [plan]      las filas, según llegan (solo en la interfaz)
 * @property {Object<string, any>} [args]
 */

/**
 * @typedef {Object} ChatSetlistSong
 * @property {number} id
 * @property {string} artist
 * @property {string} title
 */

/**
 * @typedef {Object} ChatSetlistRow  Una canción de la lista pegada contra la biblioteca.
 * @property {number|null} n
 * @property {string} title         lo pedido
 * @property {'found'|'other_version'|'ambiguous'|'probable'|'missing'|'ignored'} status
 * @property {ChatSetlistSong|null} song   la elegida
 * @property {ChatSetlistSong[]} alts
 * @property {string} note
 * @property {{url:string}|null} link
 * @property {boolean} in_list
 * @property {string} kind
 * @property {boolean} [downloaded]  se bajó desde la tarjeta y ya está en la lista
 */

/**
 * @typedef {Object} ChatSetlist  La tabla de una lista metida o comprobada (`fill_playlist`).
 * @property {{id:number, name:string, created:boolean}} playlist
 * @property {'add'|'set'|'check'} mode
 * @property {boolean} applied
 * @property {ChatSetlistRow[]} rows
 * @property {number[]} added
 * @property {number[]} already_in
 * @property {ChatSetlistSong[]} removed
 * @property {ChatSetlistSong[]} extras_in_list
 * @property {boolean} order_differs
 * @property {number[]|null} previous   lo que había antes, para «Deshacer»
 * @property {string[]} ignored
 * @property {number} truncated
 * @property {string} [mode_note]
 * @property {string} [ask_note]
 */

/**
 * @typedef {Object} ChatVia  Quién contestó.
 * @property {string} id
 * @property {string} name
 * @property {string} model
 * @property {boolean} fallback    un respaldo, no el elegido
 * @property {string} [reason]     por qué el elegido no: billing | auth | rate | down | tools
 */

/**
 * @typedef {Object} ChatReply
 * @property {string} [text]
 * @property {string} [error]
 * @property {{name:string, summary:string, detail?:string}[]} [tools]
 * @property {{kind:string, [k:string]: any}[]} [actions]
 * @property {ChatConfirm|null} [confirm]
 * @property {ChatSetlist|null} [setlist]
 * @property {boolean} [canceled]
 * @property {{calls:number, prompt:number, completion:number, cost:number|null}} [usage]
 * @property {ChatVia} [via]  quién respondió
 * @property {boolean} [narrated]  dijo haber hecho algo sin llamar a ninguna herramienta
 * @property {{limit:number, month:number, over:boolean}} [budget]  el tope de gasto, si lo hay
 */

/**
 * Un mensaje del chat tal como se pinta y se guarda.
 * @typedef {Object} ChatMessage
 * @property {'me'|'ai'} role
 * @property {string} text
 * @property {{name: string, summary: string, detail?: string}[]} [tools]
 * @property {boolean} [app]       lo escribió la app, no el modelo
 * @property {string} [event]      un aviso estructural: download_done, confirm_accepted,
 *                                 confirm_declined, confirm_failed
 * @property {string} [ref]        el id de la tarjeta a la que se refiere ese aviso
 * @property {{n: number, song: ChatSetlistSong}[]} [filled]  al terminar la descarga:
 *   qué fila de la tabla de esa tarjeta quedó con qué canción
 * @property {ChatConfirm} [confirm]  la tarjeta de permiso que trajo esta respuesta
 * @property {ChatSetlist} [setlist]  la tabla de la lista que trajo esta respuesta
 * @property {boolean} [hidden]    va al núcleo pero no se pinta
 * @property {boolean} [narrated]  dice haber hecho algo sin herramienta
 * @property {boolean} [error]
 * @property {any} [usage]
 * @property {ChatVia|null} [via]
 * @property {boolean} [canceled]
 */

/**
 * @typedef {Object} JobSnapshot  Una tarea larga del núcleo, tal como va (contrato A).
 * @property {string} name
 * @property {boolean} active     sigue en marcha
 * @property {number} done
 * @property {number} total       0 si no se sabe cuánto hay
 * @property {string} message     qué está haciendo, en castellano
 * @property {any} result         al acabar, lo que devolvía la ruta cuando era síncrona
 * @property {string} error       en castellano, o vacío
 * @property {number} [started]
 * @property {number} [ended]
 */

/**
 * @typedef {Object} ChatContext  Lo que la persona tiene delante, para el asistente.
 * @property {{kind:string, name:string, id?:number}} [view]
 * @property {{id:number, artist:string, title:string}[]} [songs]   las primeras de la lista, en su orden
 * @property {number} [total]
 * @property {{id:number, artist:string, title:string}[]} [selected]
 * @property {{id:number, artist:string, title:string, paused:boolean}|null} [playing]
 */

export const inTauri =
  typeof window !== 'undefined' && !!(/** @type {any} */ (window).__TAURI_INTERNALS__)

/** @type {null | ((cmd: string, args?: Object) => Promise<any>)} */
let invoke = null
/** @type {null | ((name: string, fn: (e: {payload: any}) => void) => Promise<() => void>)} */
let listen = null
if (inTauri) {
  const core = await import('@tauri-apps/api/core')
  invoke = core.invoke
  const events = await import('@tauri-apps/api/event')
  listen = events.listen
}

/**
 * Un fallo del núcleo o del puente. Se imprime como su mensaje a secas:
 * antes cada sitio quitaba a mano el prefijo «Error:» que añade el
 * navegador, con una expresión regular repetida diez veces, y se olvidaba
 * en la mitad de los sitios.
 */
export class ApiError extends Error {
  /**
   * @param {string} message
   * @param {number} [status]
   */
  constructor(message, status = 0) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }

  toString() {
    return this.message
  }
}

/**
 * Texto legible de cualquier fallo, venga de donde venga.
 * @param {unknown} e
 * @returns {string}
 */
export function errorMessage(e) {
  if (e instanceof ApiError) return e.message
  if (e && typeof e === 'object' && 'message' in e) return String(e.message)
  return String(e).replace(/^Error:\s*/, '')
}

/**
 * Lo que el núcleo pone en el cuerpo de un error. FastAPI manda
 * `{"detail": "..."}`; si no es eso, se enseña el texto tal cual.
 * @param {string} text
 * @param {number} status
 */
function describeFailure(text, status) {
  try {
    const parsed = JSON.parse(text)
    if (typeof parsed?.detail === 'string') return parsed.detail
  } catch {
    /* no era JSON */
  }
  return text.trim() || `HTTP ${status}`
}

/**
 * Un fallo que llega por el puente de Rust. Rust lo manda como
 * `<código>: <cuerpo>`, y el cuerpo es el JSON de FastAPI: se saca el
 * `detail` para que el usuario lea «ya hay una descarga en marcha» y no
 * `409: {"detail":"ya hay una descarga en marcha"}`.
 * @param {unknown} e
 */
export function fromBridge(e) {
  const raw = errorMessage(e)
  const m = raw.match(/^(\d{3}):\s*([\s\S]*)$/)
  if (!m) return new ApiError(raw)
  const status = Number(m[1])
  return new ApiError(describeFailure(m[2], status), status)
}

/**
 * @param {'GET'|'POST'|'PUT'|'PATCH'|'DELETE'} method
 * @param {string} path  ruta sin el prefijo /api
 * @param {Object} [body]
 * @returns {Promise<any>}
 */
async function request(method, path, body) {
  if (inTauri && invoke) {
    try {
      const txt = await invoke('api', {
        method,
        path: '/api' + path,
        body: body ? JSON.stringify(body) : null
      })
      return txt ? JSON.parse(txt) : null
    } catch (e) {
      throw fromBridge(e)
    }
  }
  // En modo desarrollo el núcleo exige esta cabecera en TODAS las peticiones
  // (§2), también en los GET: hay GET con efectos (los que gastan IA) y una
  // página cualquiera abierta en el navegador podría lanzarlos. Una cabecera
  // propia obliga al navegador a preguntar antes (preflight), y ahí CORS
  // corta. Los medios que pide el propio navegador (<audio>, <img>) no pasan
  // por aquí: van por `mediaUrl`.
  /** @type {Record<string, string>} */
  const headers = { 'X-DanPlay': '1' }
  if (body) headers['Content-Type'] = 'application/json'
  const r = await fetch('/api' + path, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined
  })
  if (!r.ok) throw new ApiError(describeFailure(await r.text(), r.status), r.status)
  return r.json()
}

const GET = (r) => request('GET', r)
const POST = (r, c) => request('POST', r, c)
const PATCH = (r, c) => request('PATCH', r, c)
const PUT = (r, c) => request('PUT', r, c)
const DEL = (r) => request('DELETE', r)

// La portada no viaja por el puente: la sirve Rust con un protocolo propio
// (`danplay://cover/<id>`). El audio, dentro de la app, ni eso: lo reproduce
// Rust. Solo en el navegador se pide el audio por URL, al núcleo, para el
// <audio> del reproductor web.
//
// OJO: Tauri expone los protocolos propios de forma distinta según el sistema.
// En Linux y Windows es  http://<esquema>.localhost/...
// y solo en macOS/iOS es  <esquema>://localhost/...
const isApple =
  typeof navigator !== 'undefined' &&
  /Mac|iPhone|iPad|iPod/.test(navigator.platform || navigator.userAgent || '')

/**
 * @param {'audio'|'cover'} kind
 * @param {number|string} id
 * @param {string} [query]  cadena de consulta ya montada, sin el «?»
 */
export function mediaUrl(kind, id, query = '') {
  const q = query ? '?' + query : ''
  if (!inTauri) return `/api/song/${id}/${kind}${q}`
  return isApple
    ? `danplay://localhost/${kind}/${id}${q}`
    : `http://danplay.localhost/${kind}/${id}${q}`
}

/**
 * La otra forma del protocolo. Si el WebView rechaza una, probamos con esta.
 * @param {'audio'|'cover'} kind
 * @param {number|string} id
 * @param {string} [query]
 */
export function mediaUrlAlt(kind, id, query = '') {
  if (!inTauri) return null
  const q = query ? '?' + query : ''
  return isApple
    ? `http://danplay.localhost/${kind}/${id}${q}`
    : `danplay://localhost/${kind}/${id}${q}`
}

/** Tamaños de miniatura que el núcleo genera y cachea (§2). */
export const COVER_SIZES = [96, 192, 320]

/**
 * @param {number} [size]
 * @returns {string} «size=96» o cadena vacía para la imagen original
 */
function coverQuery(size) {
  return size && COVER_SIZES.includes(size) ? `size=${size}` : ''
}

/**
 * Abre el selector de carpetas del sistema. Devuelve la ruta o null.
 * @param {string} [title]
 * @returns {Promise<string|null>}
 */
export async function pickFolder(title = 'Elige tu carpeta de música') {
  if (!inTauri) return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  const r = await open({ directory: true, multiple: false, title })
  return typeof r === 'string' ? r : Array.isArray(r) ? r[0] : null
}

/**
 * Abre el selector de imágenes del sistema. Devuelve la ruta o null.
 * @param {string} [title]
 * @returns {Promise<string|null>}
 */
export async function pickImage(title = 'Elige una imagen para la carátula') {
  if (!inTauri) return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  const r = await open({
    multiple: false,
    title,
    filters: [{ name: 'Imágenes', extensions: ['jpg', 'jpeg', 'png', 'webp'] }]
  })
  return typeof r === 'string' ? r : Array.isArray(r) ? r[0] : null
}

/**
 * Abre el diálogo del sistema para elegir dónde guardar un archivo. Devuelve
 * la ruta o null (cancelado, o fuera de la app).
 * @param {{ title?: string, defaultPath?: string, filters?: Array<{name: string, extensions: string[]}> }} [options]
 * @returns {Promise<string|null>}
 */
export async function pickSavePath(options = {}) {
  if (!inTauri) return null
  const { save } = await import('@tauri-apps/plugin-dialog')
  const r = await save({ title: 'Guardar', ...options })
  return typeof r === 'string' && r ? r : null
}

/** Fuera de la app no hay nada que escuchar: se devuelve un «dejar de escuchar» vacío. */
const noListener = async () => () => {}

/**
 * Reproducción y cola (§1). La cola vive en Rust: aquí solo se mandan
 * órdenes y se escucha lo que Rust cuenta. Fuera de Tauri estas funciones
 * no se llaman: `usePlayback` usa entonces su propio reproductor web.
 */
export const playback = {
  available: inTauri,
  /**
   * Sustituye la cola y empieza por `start` (id) o por el primero.
   * @param {Track[]} items
   * @param {number|null} start
   * @param {any} origin
   */
  setQueue: (items, start, origin) => invoke('set_queue', { items, start, origin }),
  next: () => invoke('queue_next'),
  previous: () => invoke('queue_previous'),
  /** @param {number} id */
  jump: (id) => invoke('queue_jump', { id }),
  /** @param {Repeat} mode */
  setRepeat: (mode) => invoke('set_repeat', { mode }),
  /** @param {boolean} on */
  setShuffle: (on) => invoke('set_shuffle', { on }),
  toggle: () => invoke('toggle_pause'),
  stop: () => invoke('stop'),
  /** @param {number} seconds */
  seek: (seconds) => invoke('seek', { seconds }),
  /** @param {number} value 0..1,5 (por encima de 1, más alta de como viene) */
  setVolume: (value) => invoke('set_volume', { value }),
  /** @param {number} value */
  setSpeed: (value) => invoke('set_speed', { value }),
  /**
   * Repetir de A a B (segundos); sin valores, lo quita. No mueve la canción:
   * el tramo entra cuando la canción está (o llega) dentro, y con tramo la
   * canción no se acaba (al final vuelve a A). Con `segments` son varios
   * tramos seguidos; `end` dice qué pasa al acabar el último: 'repeat'
   * (vuelta al primero), 'stop' (se para al principio del primero) o
   * 'continue' (suenan una vez y la canción sigue).
   * @param {number|null} a @param {number|null} b
   * @param {{ segments?: Array<[number, number]>, end?: LoopEnd }} [opts]
   */
  setLoop: (a, b, opts) =>
    opts
      ? invoke('set_loop', { a, b, segments: opts.segments ?? null, end: opts.end ?? 'repeat' })
      : invoke('set_loop', { a, b }),
  /**
   * A dónde va cada cosa: los destinos (pares de canales de la salida, o uno
   * en mono: `{left, right}` con los dos iguales) y el de la canción, el
   * clic y las voces guía (cada pista lleva el suyo en `setStems`).
   * @param {RoutingSpec} routing
   */
  setRouting: (routing) => invoke('set_routing', { routing }),
  /**
   * Qué salida de audio abrir (su id; null, la del sistema) y con cuántos
   * canales.
   * @param {{device: string|null, channels: number}} output
   */
  setOutput: (output) => invoke('set_output', { output }),
  /** @returns {Promise<AudioDevice[]>} las salidas del equipo, la del sistema primero */
  outputDevices: () => invoke('output_devices'),
  /**
   * Las voces guía de la canción `song` (la ruta de la que suena): cada una,
   * un archivo y el segundo en que entra.
   * @param {string} song @param {Array<{at: number, path: string, gain?: number}>} cues
   * @param {number} [volume]
   */
  setCues: (song, cues, volume = 1) => invoke('set_cues', { song, cues, volume }),
  /**
   * El tono corrido, en semitonos (-12..12, con fracciones: 0,5 es un cuarto
   * de tono). Solo hace algo con ffmpeg.
   */
  setPitch: (semitones) => invoke('set_pitch', { semitones }),
  setCrossfade: (seconds) => invoke('set_crossfade', { seconds }),
  /** @param {MetronomeSettings} settings */
  setMetronome: (settings) => invoke('set_metronome', { settings }),
  /**
   * Que suenen las pistas separadas de la canción `song` (la ruta de la que
   * suena) en vez de ella: cada una con su volumen, su panorama y si suena.
   * Sin `tracks`, la canción otra vez. Con las mismas pistas, solo cambia la
   * mezcla, al momento.
   * @param {string} song
   * @param {Array<{path: string, gain: number, pan: number, on: boolean}>|null} tracks
   */
  setStems: (song, tracks) => invoke('set_stems', { song, tracks }),
  /**
   * La rejilla de un archivo calculada fuera (Beat This!, en el núcleo):
   * Rust se queda con ella para el metrónomo, como si la hubiera analizado.
   * @param {string} path @param {BeatGrid} grid
   * @returns {Promise<BeatGrid>}
   */
  setBeatGrid: (path, grid) => invoke('set_beat_grid', { path, grid }),
  /**
   * Analiza el pulso y el compás del archivo (un par de segundos; Rust se
   * queda con la rejilla para el metrónomo).
   * @param {string} path @param {number|null} [hintBpm]
   * @returns {Promise<BeatGrid>}
   */
  analyzeBeats: (path, hintBpm = null) => invoke('analyze_beats', { path, hintBpm }),
  /** @returns {Promise<PlaybackState>} */
  state: () => invoke('playback_state'),
  /** @returns {Promise<{items: Track[], origin: any}>} */
  queueItems: () => invoke('queue_items'),
  /**
   * Escucha `danplay://state`. Devuelve cómo dejar de escuchar.
   * @param {(s: PlaybackState) => void} fn
   */
  onState: inTauri ? (fn) => listen('danplay://state', (e) => fn(e.payload)) : noListener
}

/** La ventana principal y la app entera (§1, «ventana mini y bandeja»). */
export const app = {
  /** Trae la ventana principal al frente (y la muestra si estaba oculta). */
  showWindow: () => (inTauri ? invoke('show_window') : Promise.resolve()),
  /**
   * La forma de la ventana (`shape.rs`): `normal`, `completa`, `maximizada`,
   * `izquierda`, `derecha`, `columna`, `cuadrito` o `barra`. Devuelve la que
   * queda. Fuera de la app solo hay pantalla completa (la del navegador).
   * @param {string} shape
   * @returns {Promise<string>}
   */
  setWindowShape: (shape) => {
    if (inTauri) return invoke('set_window_shape', { shape })
    if (shape === 'completa') {
      return document.documentElement.requestFullscreen().then(() => 'completa')
    }
    if (shape === 'normal' && document.fullscreenElement) {
      return document.exitFullscreen().then(() => 'normal')
    }
    return Promise.reject(new Error('Solo en la aplicación de escritorio'))
  },
  /** @returns {Promise<[string, boolean]>} la forma de ahora y si va siempre encima */
  windowShape: () =>
    inTauri
      ? invoke('window_shape')
      : Promise.resolve([document.fullscreenElement ? 'completa' : 'normal', false]),
  /** «Siempre encima», en cualquier forma. @param {boolean} on */
  setWindowOnTop: (on) =>
    inTauri
      ? invoke('set_window_on_top', { on })
      : Promise.reject(new Error('Solo en la aplicación de escritorio')),
  /** Se puede grabar con el micrófono (solo en la app de escritorio). */
  canRecord: inTauri,
  /** Empieza a grabar con el micrófono del sistema (para una guía). */
  recordStart: () => invoke('record_start'),
  /** Cuánto suena lo que entra ahora (0..1), para el vúmetro. @returns {Promise<number>} */
  recordLevel: () => invoke('record_level'),
  /** Para de grabar: devuelve el archivo (WAV) que quedó. @returns {Promise<string>} */
  recordStop: () => invoke('record_stop'),
  /**
   * Abre el explorador de archivos del sistema señalando ese archivo (o esa
   * carpeta). Fuera de la app no hay explorador que abrir.
   * @param {string} path
   */
  revealInFolder: (path) =>
    inTauri
      ? invoke('reveal_in_folder', { path })
      : Promise.reject(new Error('Solo en la aplicación de escritorio')),
  /**
   * Abre una página web en el navegador del sistema (solo http/https). Fuera
   * de la app, una pestaña nueva.
   * @param {string} url
   */
  openInBrowser: (url) =>
    inTauri
      ? invoke('open_in_browser', { url })
      : Promise.resolve(window.open(url, '_blank') && undefined),
  /**
   * Abre un .html de la biblioteca (la hoja para el atril) con el navegador
   * del sistema, para leerlo e imprimirlo. Solo dentro de la app.
   * @param {string} path
   */
  openHtml: (path) =>
    inTauri
      ? invoke('open_html', { path })
      : Promise.reject(new Error('Solo en la aplicación de escritorio')),
  /**
   * A dónde se puede enviar una canción desde este equipo.
   * @returns {Promise<{telegram: boolean}>}
   */
  shareTargets: () => (inTauri ? invoke('share_targets') : Promise.resolve({ telegram: false })),
  /**
   * Abre Telegram Desktop con esos archivos listos para enviar (`-sendpath`):
   * uno, una selección o un repertorio. El chat lo elige la persona allí; no
   * se manda nada solo.
   * @param {string[]} paths
   */
  sendToTelegram: (paths) =>
    inTauri
      ? invoke('send_to_telegram', { paths: [].concat(paths) })
      : Promise.reject(new Error('Solo en la aplicación de escritorio')),
  /** Cierra DanPlay del todo, lo mismo que «Salir» en la bandeja. */
  quit: () => (inTauri ? invoke('quit_app') : Promise.resolve()),
  /**
   * ¿Hay bandeja donde quedarse? Si no, cerrar la ventana cierra la app.
   * @returns {Promise<boolean>}
   */
  trayAvailable: () => (inTauri ? invoke('tray_available') : Promise.resolve(false)),
  /**
   * ¿Abre el sistema las canciones con DanPlay?
   *
   * `direct` dice si la propia app puede ponerlo: en Linux sí, en Windows no
   * (desde Windows 8 la elección la tiene que confirmar el usuario en Ajustes,
   * y ningún programa puede saltársela).
   * @returns {Promise<DefaultPlayer>}
   */
  defaultPlayer: () =>
    inTauri
      ? invoke('default_player')
      : Promise.resolve({ supported: false, is_default: false, direct: false, note: '' }),
  /**
   * Pide ser el reproductor predeterminado. En Windows, además, abre la
   * ventana de Ajustes donde se confirma.
   * @returns {Promise<DefaultPlayer>}
   */
  makeDefaultPlayer: () => (inTauri ? invoke('make_default_player') : Promise.resolve(null))
}

/**
 * La ventana de proyección: la letra en grande, para el proyector. Es una
 * ventana aparte que se arrastra a la otra pantalla; cerrarla la esconde.
 */
export const projection = {
  show: () =>
    inTauri
      ? invoke('show_projection')
      : Promise.resolve(window.open('/?projection=1', 'danplay-projection') && undefined),
  hide: () => (inTauri ? invoke('hide_projection') : Promise.resolve(window.close())),
  /** Pantalla completa de ESTA ventana (solo dentro de la app). @param {boolean} on */
  fullscreen: async (on) => {
    if (!inTauri) return
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    await getCurrentWindow().setFullscreen(!!on)
  },
  isFullscreen: async () => {
    if (!inTauri) return false
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    return getCurrentWindow().isFullscreen()
  }
}

/** El popup del mini reproductor. */
export const mini = {
  hide: () => (inTauri ? invoke('hide_mini') : Promise.resolve()),
  toggle: () => (inTauri ? invoke('toggle_mini') : Promise.resolve()),
  /**
   * Avisa cuando el popup se muestra u oculta.
   * @param {(e: {visible: boolean}) => void} fn
   */
  onVisible: inTauri ? (fn) => listen('danplay://mini-visible', (e) => fn(e.payload)) : noListener
}

/** Cada cuánto mira el navegador si el núcleo ha cambiado (como `core::watch` en Rust). */
export const WATCH_MS = 2000

/**
 * Lo que en la app hace Rust (`core::watch`), en el navegador: mirar
 * `/api/status` cada dos segundos y avisar cuando `revision` cambia (algo de
 * lo que se enseña cambió en el núcleo, lo hiciera quien lo hiciera: el
 * vigilante de carpetas, el asistente) o cuando el núcleo pasa de no
 * contestar a contestar. Sin esto, en `npm run dev` la pantalla no se
 * enteraba de nada de lo que pasaba por detrás.
 *
 * Solo mira mientras alguien escucha. La primera lectura fija el punto de
 * partida: la página acaba de cargar lo que hay.
 */
function createBrowserWatch() {
  /** @type {Set<(e: {revision: number}) => void>} */
  const changed = new Set()
  /** @type {Set<(e: {ready: boolean, message: string}) => void>} */
  const status = new Set()
  /** @type {ReturnType<typeof setTimeout> | null} */
  let timer = null
  /** @type {number|null|undefined} */
  let revision
  /** @type {boolean|null} */
  let ready = null

  async function tick() {
    timer = null
    let now
    try {
      now = await request('GET', '/status')
    } catch {
      now = null
    }
    if (!changed.size && !status.size) return
    const alive = !!now
    // solo los cambios: de no contestar a contestar (o al revés)
    if (ready !== null && alive !== ready) {
      const e = { ready: alive, message: alive ? '' : 'el núcleo no contesta' }
      for (const fn of status) fn(e)
    }
    ready = alive
    if (now && typeof now.revision === 'number') {
      if (revision != null && now.revision !== revision) {
        for (const fn of changed) fn({ revision: now.revision })
      }
      revision = now.revision
    }
    timer = setTimeout(tick, WATCH_MS)
  }

  /**
   * @template T
   * @param {Set<T>} set
   * @returns {(fn: T) => Promise<() => void>}
   */
  const subscribe = (set) => async (fn) => {
    set.add(fn)
    if (!timer) timer = setTimeout(tick, 0)
    return () => {
      set.delete(fn)
      if (!changed.size && !status.size && timer) {
        clearTimeout(timer)
        timer = null
        revision = undefined
        ready = null
      }
    }
  }
  return { onChanged: subscribe(changed), onStatus: subscribe(status) }
}
const browserWatch = inTauri ? null : createBrowserWatch()

/** El núcleo Python, visto desde Rust (o, en el navegador, sondeando). */
export const core = {
  /**
   * Avisa cuando el núcleo arranca, muere o se reinicia.
   * @param {(e: {ready: boolean, message: string}) => void} fn
   * @returns {Promise<() => void>}
   */
  onStatus: (fn) =>
    inTauri && listen
      ? listen('danplay://core', (e) => fn(e.payload))
      : (browserWatch?.onStatus(fn) ?? noListener()),
  /**
   * Avisa cuando algo de lo que se enseña ha cambiado en el núcleo (el
   * índice, las listas, las estrellas), venga de donde venga: el asistente,
   * una descarga que termina, la línea de órdenes, el vigilante de carpetas.
   * Rust lo detecta por la `revision` de `/api/status` (§1); en el navegador
   * se mira aquí mismo. Devuelve cómo dejar de escuchar.
   * @param {(e: {revision: number}) => void} fn
   * @returns {Promise<() => void>}
   */
  onChanged: (fn) =>
    inTauri && listen
      ? listen('danplay://changed', (e) => fn(e.payload))
      : (browserWatch?.onChanged(fn) ?? noListener())
}

/** Cada cuánto se pregunta por una tarea larga. */
export const JOB_POLL_MS = 500

/** Los nombres de las tareas largas del núcleo (contrato A). */
export const JOBS = {
  scan: 'escaneo',
  import: 'importacion',
  convert: 'conversion',
  duplicates: 'duplicados',
  ytdlp: 'yt-dlp',
  separate: 'separacion',
  mix: 'mezcla',
  phone: 'celular',
  sequence: 'secuencia'
}

/** @param {number} ms */
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/**
 * Una tarea larga del núcleo, de principio a fin.
 *
 * Escanear, importar, convertir o buscar duplicados puede tardar minutos, y
 * el puente de Rust corta a los 60 s: esperaban dentro de la petición y la
 * interfaz daba error aunque el trabajo siguiera. Ahora `start` la arranca
 * (el núcleo contesta al momento con cómo va) y aquí se pregunta cada poco
 * hasta que acaba, contando el avance a quien quiera enseñarlo. Si ya había
 * una igual en marcha, se sigue esa.
 * @param {() => Promise<any>} start  el POST que la arranca
 * @param {string} name               su nombre en el núcleo (ver `JOBS`)
 * @param {{ onProgress?: (job: JobSnapshot) => void, interval?: number,
 *           poll?: (name: string) => Promise<JobSnapshot> }} [options]
 * @returns {Promise<any>} lo que dejó la tarea (`result`)
 */
export async function runJob(start, name, options = {}) {
  const { onProgress, interval = JOB_POLL_MS, poll = (n) => api.job(n) } = options
  const first = await start()
  /** @type {JobSnapshot} */
  let job = first?.job || (await poll(name))
  onProgress?.(job)
  let failures = 0
  while (job.active) {
    await sleep(interval)
    try {
      job = await poll(name)
      failures = 0
    } catch (e) {
      // un fallo suelto (el núcleo ocupado, un reinicio) no tira la tarea,
      // que sigue en el núcleo; varios seguidos, sí
      if (++failures >= 3) throw e
      continue
    }
    onProgress?.(job)
  }
  if (job.error) throw new ApiError(job.error)
  return job.result
}

export const api = {
  inTauri,
  /** @param {number} id */
  path: (id) => GET(`/song/${id}/path`),
  status: () => GET('/status'),
  settings: () => GET('/settings'),
  checkAi: () => POST('/settings/check-ai'),
  saveSettings: (d) => POST('/settings', d),
  /** Si se aceptaron los términos, cuáles y cuándo (el texto, en data/terminos.js). */
  terms: () => GET('/terms'),
  acceptTerms: (version) => POST('/terms/accept', { version }),

  /**
   * Proveedores de IA: el catálogo, los perfiles guardados (claves
   * enmascaradas), cuál está activo y el estado del catálogo de modelos
   * (models.dev). Con `refresh` espera a consultarlo; si no, lo hace en
   * segundo plano.
   * @param {boolean} [refresh]
   */
  aiProviders: (refresh = false) => GET('/ai/providers' + (refresh ? '?refresh=1' : '')),
  /** @param {AiProfile} d  guarda (y activa, salvo `activate: false`) */
  aiSaveProfile: (d) => POST('/ai/profile', d),
  /** @param {string} id */
  aiDeleteProfile: (id) => DEL(`/ai/profile/${encodeURIComponent(id)}`),
  /** @param {string} id */
  aiActivate: (id) => POST('/ai/activate', { id }),
  /** Lo que gasta la IA: hoy, este mes y en total (tokens y coste). */
  aiUsage: () => GET('/ai/usage'),
  /** @param {boolean} enabled  si al fallar el activo se usan los demás */
  aiFallback: (enabled) => POST('/ai/fallback', { enabled }),
  /** @param {number} dollars  avisar al pasar de tantos dólares al mes (0 = sin aviso) */
  aiBudget: (dollars) => POST('/ai/budget', { dollars }),
  /** @param {AiProfile} d  prueba lo del formulario sin guardarlo */
  aiCheck: (d) => POST('/ai/check', d),
  /**
   * «Probar gratis, sin clave»: el núcleo prueba los servicios gratuitos por
   * orden y activa el primero que responda. Devuelve lo mismo que
   * `aiProviders` más `free: {ok, chosen, name, model, tried, reason}`.
   */
  aiFree: () => POST('/ai/free'),
  /** @param {AiProfile} d  los modelos de ese proveedor con esa clave */
  aiModels: (d) => POST('/ai/models', d),
  /**
   * «Reintentar con el principal»: el núcleo olvida que el proveedor activo
   * falló y el siguiente mensaje se prueba con él. No cambia ningún ajuste.
   * @returns {Promise<{primary: {id:string, name:string, model:string, problem: {kind:string, code:number|null, at:number}|null}, answering: ChatVia|null}>}
   */
  aiRecheck: () => POST('/ai/recheck'),

  folders: () => GET('/folders'),
  /** @param {string} path */
  checkFolder: (path) => POST('/check-folder', { path }),
  /**
   * Respuesta: `action` es `added | already_there | replaced | confirm |
   * relocated` (§4). `relocated`: era una carpeta gestionada que se movió.
   * @param {string} path
   * @param {string} [label]
   * @param {boolean} [force]
   */
  addFolder: (path, label = '', force = false) => POST('/folders', { path, label, force }),
  /** @param {string} r */
  removeFolder: (r) => DEL(`/folders?path=${encodeURIComponent(r)}`),
  /**
   * Una carpeta gestionada que ya no está (`from`), en su sitio nuevo (`to`):
   * sus canciones vuelven con su id, sus listas y sus notas.
   * @param {string} from
   * @param {string} to
   */
  relocateFolder: (from, to) => POST('/folders/relocate', { from, to }),
  addExclusion: (pattern, kind = 'glob', note = '') => POST('/exclusions', { pattern, kind, note }),
  removeExclusion: (p) => DEL(`/exclusions?pattern=${encodeURIComponent(p)}`),

  /**
   * Cómo va una tarea larga (ver `runJob`).
   * @param {string} name
   * @returns {Promise<JobSnapshot>}
   */
  job: (name) => GET(`/jobs/${encodeURIComponent(name)}`),
  /** @type {typeof runJob} */
  runJob: (start, name, options) => runJob(start, name, options),
  /** Arranca el escaneo (tarea «escaneo»): contesta al momento con `{job}`. */
  scan: () => POST('/scan'),

  /**
   * Una página de la búsqueda. Trae `count` (cuántas cumplen la consulta, sin
   * límite) y canciones ligeras: sin letra, acordes ni estudio, que se piden
   * con `song` (en su lugar, `has_lyrics`, `has_chords`…). `limit` hasta
   * 20000; `from_key` es desde cuál.
   * @param {Object<string, any>} p
   * @returns {Promise<{total: number, count?: number, songs: Song[]}>}
   */
  search: (p) => GET('/search?' + new URLSearchParams(p)),
  // búsqueda suelta para el desplegable: no toca la lista de la página
  quickSearch: (q, limit = 40) =>
    GET('/search?' + new URLSearchParams({ q, limit: String(limit) })),
  facets: () => GET('/facets'),
  /** @param {number} id @returns {Promise<Song>} */
  song: (id) => GET(`/song/${id}`),
  /** Solo `artist, title, album, year, genre, key, bpm, lyrics`; otras claves dan 422 (§2). */
  edit: (id, d) => PATCH(`/song/${id}`, d),
  setStars: (id, n) => POST(`/song/${id}/stars`, { stars: n }),
  toggleFavorite: (id, v) => POST(`/song/${id}/favorite`, { favorite: v }),
  setBlur: (id, v) => POST(`/song/${id}/blur`, { blur: v }),
  /** A su carpeta: `category` es `track` (Pistas), `tutorial` o `sequence`. Mismo id. */
  moveSong: (id, category) => POST(`/song/${id}/move`, { category }),
  details: (id) => GET(`/song/${id}/details`),
  enrich: (id, o = {}) => POST(`/song/${id}/enrich`, o),
  autofill: (id) => POST(`/song/${id}/autofill`),
  /**
   * El cifrado de la cancion, de una pagina de acordes (Ultimate Guitar,
   * LaCuerda) y con su enlace. Lo guardado, o se busca; `refresh` lo busca otra vez.
   * @returns {Promise<{sheet: object|null, tried: string[], failed: string[], song: object}>}
   */
  chords: (id, refresh = false) => POST(`/song/${id}/chords`, { refresh }),
  /** @param {{to_key?: string, semitones?: number}} d el tono, o los semitonos si no lo dice */
  chordsTransposed: (id, d) => POST(`/song/${id}/chords/transposed`, d),
  setCover: (id, path) => POST(`/song/${id}/cover`, { path }),
  /** @param {{text:string, from_key?:string, to_key?:string, semitones?:number}} d */
  transpose: (d) => POST('/transpose', d),

  /** A la papelera; con `fromLists`, sale también de sus listas (si no,
   * se queda en ellas como «No está»). */
  deleteSong: (id, { fromLists = false } = {}) =>
    DEL(`/song/${id}${fromLists ? '?from_lists=true' : ''}`),

  playlists: () => GET('/playlists'),
  createPlaylist: (name) => POST('/playlists', { name }),
  deletePlaylist: (id) => DEL(`/playlists/${id}`),
  /** Renombra una lista o cambia su nota. `409` si el nombre ya es de otra. */
  editPlaylist: (id, fields) => PATCH(`/playlists/${id}`, fields),
  playlistSongs: (id) => GET(`/playlists/${id}/songs`),
  addToPlaylist: (id, ids) => POST(`/playlists/${id}/songs`, { ids }),
  removeFromPlaylist: (l, c) => DEL(`/playlists/${l}/songs/${c}`),
  /**
   * Deja la lista en ese orden: TODOS los ids, tal como tienen que quedar.
   * Es lo que hay detrás de arrastrar una canción arriba o abajo dentro de
   * un repertorio. Devuelve la lista ya ordenada.
   * @param {number} id @param {number[]} ids
   * @returns {Promise<{songs: Song[]}>}
   */
  reorderPlaylist: (id, ids) => POST(`/playlists/${id}/order`, { ids }),
  /**
   * Deja la lista EXACTAMENTE con esas canciones y en ese orden, de una vez (lo
   * que sobra sale, lo que falta entra). Lo usan «Dejar solo estas» y
   * «Deshacer» de la tabla de una lista en el chat. Una lista vacía da 400.
   * @param {number} id @param {number[]} ids
   * @returns {Promise<{songs: Song[]}>}
   */
  setPlaylistSongs: (id, ids) => PUT(`/playlists/${id}/songs`, { ids }),
  /**
   * El modo estudio de una canción: bucle [a, b], velocidad, marcadores
   * [{t, label}] y notas. Se guarda en el índice y en el archivo. Vacío lo quita.
   * @param {number} id @param {{loop?: number[], speed?: number, markers?: {t:number,label:string}[], notes?: string}} study
   * @returns {Promise<Song>}
   */
  setStudy: (id, study) => PUT(`/song/${id}/study`, study),
  /**
   * La forma de onda para el modo estudio: `buckets` columnas con el pico y
   * el RMS entre 0 y 1. Falla con 501 si no hay con qué decodificar.
   * @param {number} id @param {number} [buckets]
   * @returns {Promise<{peaks: number[], rms: number[], buckets: number}>}
   */
  waveform: (id, buckets = 800) => GET(`/song/${id}/waveform?buckets=${buckets}`),
  /**
   * La forma de onda de un trozo, con detalle (para la onda acercada): del
   * segundo `start` al `end`, en `buckets` columnas, a la misma escala que la
   * de toda la canción. @param {number} id @param {number} start
   * @param {number} end @param {number} buckets
   * @returns {Promise<{peaks: number[], rms: number[]}>}
   */
  waveformRange: (id, start, end, buckets) =>
    GET(
      `/song/${id}/waveform?start=${start.toFixed(3)}&end=${end.toFixed(3)}&buckets=${Math.round(buckets)}`
    ),
  /**
   * El pulso y el «1» de la canción con Beat This! (la primera vez, unos
   * segundos). 503 si aquí no se puede: la app sigue con su análisis.
   * @param {number} id
   * @returns {Promise<BeatGrid>}
   */
  beatGrid: (id) => GET(`/song/${id}/beats`),
  /** sin esperar: la rejilla, o `{pending, progress}` mientras se calcula */
  beatGridNow: (id) => GET(`/song/${id}/beats?wait=false`),
  /** deja calculándose la de una canción que vendrá después */
  beatGridPrepare: (id) => POST(`/song/${id}/beats/prepare`),
  /**
   * Vuelve a analizar el clic: de toda la canción o de un tramo, arreglando
   * los huecos y los saltos de fase ('repair'), a tempo fijo ('steady'),
   * con la batería separada ('drums') o rellenando los silencios ('fill').
   * Devuelve la rejilla nueva y lo que se arregló.
   * @param {number} id
   * @param {{mode: 'repair'|'steady'|'drums'|'fill', start?: number, end?: number}} opts
   * @returns {Promise<{grid: BeatGrid, report: {fixed: number, bpm: number, notes: string[]}}>}
   */
  reanalyzeBeats: (id, opts) => POST(`/song/${id}/beats/reanalyze`, opts),
  // ---- las guías habladas
  /**
   * Las voces con que se dicen las guías escritas: las que hay (y las que
   * se pueden bajar, con lo que pesan).
   * @returns {Promise<{voices: Array<{id: string, name: string, lang: string, gender: string, engine: string, installed: boolean, size: number, progress?: number, error?: string}>, available: string}>}
   */
  /**
   * Lo que hay en una carpeta de secuencia (o en la de ese archivo): título,
   * artista, tono, tempo, partes y pistas con su papel. 422 si no lo es.
   * @param {string} path
   * @returns {Promise<{folder: string, title: string, artist: string, key: string, bpm: number, project: boolean, cover: boolean, sections: string[], tracks: Array<{name: string, role: string}>}>}
   */
  sequenceInfo: (path) => GET(`/sequence?path=${encodeURIComponent(path)}`),
  /** Abrir una secuencia (tarea «secuencia»). @param {string} path */
  importSequence: (path) => POST('/sequence/import', { path }),
  // ---- la música del teléfono (solo en la app del celular: movil/src/musica.rs)
  /** Las carpetas con música, con cuántas canciones y si se indexan. */
  phoneFolders: () => GET('/phone/folders'),
  /** Elegir las carpetas que se indexan. @param {string[]} folders */
  setPhoneFolders: (folders) => PUT('/phone/folders', { folders }),
  /** Volver a mirar las carpetas (solo se lee lo nuevo). */
  phoneScan: () => POST('/phone/scan'),
  /** La música del teléfono. @param {Record<string,string>} [params] */
  phoneSongs: (params = {}) => GET('/phone/songs?' + new URLSearchParams(params)),
  /** Las canciones de la carpeta de esa (para la cola). @param {number} id */
  phoneFolder: (id) => GET(`/phone/folder/${id}`),
  cueVoices: () => GET('/cues/voices'),
  /**
   * Bajar una voz (en segundo plano: se sigue en `cueVoices`).
   * @param {string} id @returns {Promise<{installed: boolean, progress?: number}>}
   */
  installCueVoice: (id) => POST(`/cues/voices/${encodeURIComponent(id)}/install`),
  /**
   * Una guía escrita, dicha con una voz: el archivo que se hace (con
   * `preview`, uno de prueba que no se guarda con la canción).
   * @param {number} id @param {{text: string, voice: string, preview?: boolean}} body
   * @returns {Promise<{file: string, seconds: number}>}
   */
  makeCue: (id, body) => POST(`/song/${id}/cues`, body),
  /**
   * Una guía grabada con el micrófono (el archivo que dejó la app), a su
   * sitio. @param {number} id @param {string} path
   * @returns {Promise<{file: string, seconds: number}>}
   */
  importCue: (id, path) => POST(`/song/${id}/cues/import`, { path }),
  /** Escuchar una guía (su archivo). @param {string} file */
  previewCue: async (file) => {
    const r = await POST('/cues/audio', { file })
    if (!r?.data) return
    const audio = new Audio(`data:${r.mime || 'audio/wav'};base64,${r.data}`)
    await audio.play()
  },
  /** «Transformar»: lo que no está en su sitio o con su nombre (las seguras primero) */
  tidyList: () => GET('/tidy'),
  /** «Transformar» una canción: `{artist, title, tags}` o `{to}` (deshacer) */
  tidy: (id, body) => POST(`/song/${id}/tidy`, body),
  // ---- separar en pistas (bateria, voces, bajo...)
  /**
   * Si se puede separar aqui, lo que pesa el separador (y lo que falta por
   * bajar), la cola y lo ultimo que acabo.
   */
  separation: () => GET('/separate'),
  /**
   * A la cola de separacion (tarea «separacion»), con la mejor calidad: si ya
   * tiene las pistas rapidas, solo se mejoran. La primera vez baja el
   * separador. @param {number} id
   */
  separate: (id, opts = {}) => POST(`/song/${id}/separate${opts.max ? '?quality=max' : ''}`),
  /** Para lo que se este separando y vacia la cola. */
  cancelSeparation: () => DEL('/separate'),
  /** Quita de la cola una cancion que aun espera. @param {number} id */
  unqueueSeparation: (id) => DEL(`/separate/queue/${id}`),
  /** Borra lo bajado del separador (se vuelve a bajar al separar). */
  removeSeparator: () => DEL('/separate/weights'),
  /** Las pistas separadas de la cancion, cada una con su ruta y su onda (404 si no tiene). */
  stems: (id) => GET(`/song/${id}/stems`),
  /** Manda a la papelera las pistas separadas de la cancion. @param {number} id */
  deleteStems: (id) => DEL(`/song/${id}/stems`),
  /**
   * Guarda una mezcla de las pistas (tarea «mezcla»), con el clic si se da.
   * @param {number} id
   * @param {{tracks: Array<{source: string, gain?: number, pan?: number}>, path: string,
   *          format?: 'mp3'|'flac'|'wav', speed?: number, pitch?: number,
   *          click?: {beats: number[], accents: boolean[], sound?: ClickSound, volume?: number}}} d
   */
  exportMix: (id, d) => POST(`/song/${id}/stems/mix`, d),
  /**
   * Prepara las canciones para la app del celular, cada una en su `.danpista`
   * (tarea «celular»): `{done: [{id, path, name, size, click}], failed: [{id, title, error}]}`.
   * @param {number[]} ids
   */
  toPhone: (ids) => POST('/celular', { ids }),
  /**
   * La hoja para el atril del repertorio: un HTML en Listas/, para abrir en
   * el navegador e imprimir.
   * @param {number} id @param {boolean} [withLyrics]
   * @returns {Promise<{file: string}>}
   */
  playlistSheet: (id, withLyrics = false) =>
    POST(`/playlists/${id}/sheet`, { with_lyrics: withLyrics }),
  exportPlaylist: (id) => POST(`/playlists/${id}/export`),

  // La lista del reproductor: lo que has abierto desde FUERA de DanPlay.
  // No entra en la biblioteca; ver `danplay/external.py`.
  /** @returns {Promise<{songs: Song[]}>} de lo último a lo más antiguo */
  externalList: () => GET('/external'),
  /** Apunta que esa ruta acaba de sonar. @param {string} path */
  externalPlayed: (path) => POST('/external/play', { path }),
  /** Descarta la lista entera. Los archivos no se tocan. */
  externalClear: () => DEL('/external'),
  /** Quita una sola. @param {number} id */
  externalForget: (id) => DEL(`/external/${id}`),
  /** La guarda como lista de DanPlay. @param {string} name */
  externalSave: (name) => POST('/external/save', { name }),
  /**
   * Avisa cuando la lista cambia porque se ha abierto algo desde fuera.
   * Devuelve cómo dejar de escuchar.
   * @param {() => void} fn
   */
  onExternal: inTauri ? (fn) => listen('danplay://recent', () => fn()) : noListener,
  /**
   * Se abrió desde fuera una pista de una secuencia (el «Click Track.wav» de
   * una carpeta de MultiTracks): llega la carpeta de la secuencia.
   * @param {(folder: string) => void} fn
   */
  onSequence: inTauri
    ? (fn) => listen('danplay://sequence', (e) => fn(String(e.payload || '')))
    : noListener,

  inbox: () => GET('/inbox'),

  // descargas de YouTube: la descarga arranca y vuelve enseguida; el avance
  // se consulta con youtube() cada poco.
  /** El estado de la descarga, y qué yt-dlp hay y con qué motor de JavaScript. */
  youtube: () => GET('/youtube'),
  /**
   * Trae el yt-dlp más nuevo (tarea «yt-dlp»); `result`: `{previous, version, updated}`.
   * YouTube cambia a menudo y el que va dentro de la app se queda viejo.
   */
  youtubeUpdate: () => POST('/youtube/update'),
  youtubeInfo: (query, results = 5) => POST('/youtube/info', { query, results }),
  youtubeDownload: (d) => POST('/youtube/download', d),
  youtubeCancel: () => POST('/youtube/cancel'),
  downloadHistory: (limit = 60, offset = 0) =>
    GET(`/downloads/history?limit=${limit}&offset=${offset}`),
  clearDownloadHistory: () => DEL('/downloads/history'),
  /** Donde se guarda lo descargado: `{ path, ready, reason, suggested }` (§2). */
  downloadFolder: () => GET('/downloads/folder'),
  /** La elegida: una de tus carpetas de música; dentro se crean Artistas/, Pistas/… si faltan. */
  setDownloadFolder: (path) => PUT('/downloads/folder', { path }),

  /** Importa la Entrada (tarea «importacion»; también en prueba, con `dry_run`). */
  runImport: (d) => POST('/import', d),
  convertible: () => GET('/convertible'),
  /**
   * Con `dry_run` contesta al momento con lo que haría; sin él, arranca la
   * tarea «conversion».
   * @param {{dry_run: boolean, quality: 'high'|'medium'|'variable', keep: boolean}} d
   */
  convert: (d) => POST('/convert', d),
  /** Busca duplicados (tarea «duplicados»; `result`: `{identical, similar}`). */
  duplicatesScan: () => POST('/duplicates/scan'),
  /** @returns {Promise<ChatReply>} */
  /**
   * Una respuesta entera de una vez. `context` es lo que la persona tiene
   * delante (vista, selección, lo que suena): ver ChatContext.
   * @param {Object[]} messages
   * @param {ChatContext} [context]
   * @returns {Promise<ChatReply>}
   */
  chat: (messages, context = null) => POST('/chat', context ? { messages, context } : { messages }),
  /**
   * La misma respuesta, pero en vivo: se arranca aquí y se va leyendo con
   * `chatPoll` (texto según sale, herramientas según terminan) hasta `done`.
   * @param {Object[]} messages
   * @param {ChatContext} [context]
   * @returns {Promise<{id: string}>}
   */
  chatStart: (messages, context = null) =>
    POST('/chat/start', context ? { messages, context } : { messages }),
  /** @param {string} id @returns {Promise<{text: string, tools: Object[], done: boolean, result: ChatReply|null}>} */
  chatPoll: (id) => GET(`/chat/poll/${encodeURIComponent(id)}`),
  /** @param {string} id */
  chatCancel: (id) => POST(`/chat/cancel/${encodeURIComponent(id)}`),

  /** Las conversaciones guardadas, la más reciente primero. */
  chats: () => GET('/chats'),
  /** @param {string} [title] */
  chatCreate: (title = '') => POST('/chats', { title }),
  /** @param {number} id  la conversación con sus mensajes */
  chatGet: (id) => GET(`/chats/${id}`),
  /** @param {number} id @param {Object[]} messages  añade al final */
  chatAppend: (id, messages) => POST(`/chats/${id}/messages`, { messages }),
  /** @param {number} id @param {string} title */
  chatRename: (id, title) => PATCH(`/chats/${id}`, { title }),
  /** @param {number} id */
  chatDelete: (id) => DEL(`/chats/${id}`),
  /** @param {string} q  busca en todas las conversaciones */
  chatSearch: (q) => GET(`/chats/search?q=${encodeURIComponent(q)}`),
  /** @param {number} id  la conversación como markdown: {markdown} */
  chatExport: (id) => GET(`/chats/${id}/export`),
  chatTools: () => GET('/chat/tools'),
  /**
   * Acepta lo que el asistente propuso (§3, §5). Con `{id, selection?}`: el id
   * de la propuesta registrada en el servidor y los índices (`i`) de las filas
   * marcadas; lo que se ejecuta lo pone el servidor, nunca esto. 410 si ya no
   * vale, 409 si hay otra descarga o falta la carpeta (la propuesta sigue
   * viva), 503 sin yt-dlp, 400 si con esas casillas no queda nada.
   *
   * Por compatibilidad con un núcleo antiguo (una propuesta sin id):
   * `chatConfirm(tool, args)`.
   * @param {{id: string, selection?: number[]} | string} arg
   * @param {Object<string, any>} [args]
   * @returns {Promise<{ok: boolean, result: any, text: string}>}
   */
  chatConfirm: (arg, args) =>
    POST(
      '/chat/confirm',
      typeof arg === 'string'
        ? { tool: arg, args }
        : Array.isArray(arg.selection)
          ? { id: arg.id, selection: arg.selection }
          : { id: arg.id }
    ),
  /**
   * Si una propuesta se puede aceptar todavía. La tarjeta solo habilita sus
   * botones con `live`.
   * @param {string} id
   * @returns {Promise<{state: 'live'|'used'|'superseded'|'expired'|'unknown', tool: string, summary: string}>}
   */
  chatConfirmStatus: (id) => GET(`/chat/confirm/${encodeURIComponent(id)}`),
  /**
   * Las filas de la tarjeta de descarga según se resuelven en YouTube. 404 si
   * esa propuesta no tiene plan.
   * @param {string} id
   * @returns {Promise<ChatPlan>}
   */
  chatPlan: (id) => GET(`/chat/plan/${encodeURIComponent(id)}`),
  /**
   * Deja de buscar en YouTube las filas que faltan. La propuesta sigue viva:
   * «Descargar» vale con lo resuelto.
   * @param {string} id
   */
  chatPlanCancel: (id) => POST(`/chat/plan/${encodeURIComponent(id)}/cancel`),
  /** `dry_run` es true por defecto en el núcleo: hay que pedir el borrado a propósito (§2). */
  resolveDuplicate: (keep, remove, dry_run = false) =>
    POST('/duplicates/resolve', { keep, remove, dry_run }),

  /**
   * El audio de una canción para el <audio> del navegador. Dentro de la app no
   * hay URL de audio (lo reproduce Rust): `null`.
   * @param {number|string} id
   */
  audioUrl: (id) => (inTauri ? null : mediaUrl('audio', id)),
  /**
   * Portada. Con `size` (96, 192 o 320) llega una miniatura JPEG cacheada;
   * sin él, la imagen original, que solo hace falta en el panel de detalle.
   * @param {number|string} id
   * @param {number} [size]
   */
  coverUrl: (id, size) => mediaUrl('cover', id, coverQuery(size)),
  coverUrlAlt: (id, size) => mediaUrlAlt('cover', id, coverQuery(size))
}
