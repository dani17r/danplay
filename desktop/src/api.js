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
 * @property {string} [path]    solo para archivos abiertos desde fuera de la
 *                              biblioteca; con ruta, Rust no la pregunta al núcleo
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
 * @typedef {Object} PlaybackState  Lo que Rust cuenta en `danplay://state`.
 * @property {Track|null} track   lo que suena (o lo último cargado)
 * @property {number} index       posición en la cola (-1 si nada)
 * @property {number} length      tamaño de la cola
 * @property {boolean} playing
 * @property {number} position    segundos
 * @property {number} duration    segundos
 * @property {number} volume      0..1
 * @property {number} speed       0.25..3
 * @property {Repeat} repeat
 * @property {boolean} shuffle
 * @property {boolean} has_previous
 * @property {boolean} has_next
 * @property {string} error       en castellano, vacío si no hay
 * @property {boolean} has_output false = este equipo no tiene salida de audio
 * @property {any} origin         lo que la interfaz pasó en set_queue
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
 */

/**
 * @typedef {Object} ChatConfirm  Una herramienta destructiva pendiente (§3).
 * @property {string} id
 * @property {string} tool
 * @property {Object<string, any>} args
 * @property {string} summary
 */

/**
 * @typedef {Object} ChatReply
 * @property {string} [text]
 * @property {string} [error]
 * @property {{name:string, summary:string}[]} [tools]
 * @property {{kind:string, [k:string]: any}[]} [actions]
 * @property {ChatConfirm|null} [confirm]
 */

export const inTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

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
 * @param {'GET'|'POST'|'PATCH'|'DELETE'} method
 * @param {string} path  ruta sin el prefijo /api
 * @param {Object} [body]
 * @returns {Promise<any>}
 */
async function request(method, path, body) {
  if (inTauri) {
    try {
      const txt = await invoke('api', {
        method,
        path: '/api' + path,
        body: body ? JSON.stringify(body) : null
      })
      return txt ? JSON.parse(txt) : null
    } catch (e) {
      throw new ApiError(errorMessage(e))
    }
  }
  /** @type {Record<string, string>} */
  const headers = { 'Content-Type': 'application/json' }
  // En modo desarrollo el núcleo exige esta cabecera en todo lo que no sea
  // GET: fuerza un preflight y corta el CSRF de peticiones simples (§2).
  if (method !== 'GET') headers['X-DanPlay'] = '1'
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
const DEL = (r) => request('DELETE', r)

// El audio y la portada no viajan por el puente: los sirve Rust con un protocolo
// propio (el audio se lee del disco, con soporte de Range).
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
  /** @param {number} value 0..1 */
  setVolume: (value) => invoke('set_volume', { value }),
  /** @param {number} value */
  setSpeed: (value) => invoke('set_speed', { value }),
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

/** El núcleo Python, visto desde Rust. */
export const core = {
  /**
   * Avisa cuando el núcleo arranca, muere o se reinicia.
   * @param {(e: {ready: boolean, message: string}) => void} fn
   */
  onStatus: inTauri ? (fn) => listen('danplay://core', (e) => fn(e.payload)) : noListener
}

export const api = {
  inTauri,
  /** @param {number} id */
  path: (id) => GET(`/song/${id}/path`),
  status: () => GET('/status'),
  settings: () => GET('/settings'),
  checkAi: () => POST('/settings/check-ai'),
  saveSettings: (d) => POST('/settings', d),

  folders: () => GET('/folders'),
  /** @param {string} path */
  checkFolder: (path) => POST('/check-folder', { path }),
  /**
   * Respuesta: `action` es `added | already_there | replaced | confirm` (§4).
   * @param {string} path
   * @param {string} [label]
   * @param {boolean} [force]
   */
  addFolder: (path, label = '', force = false) => POST('/folders', { path, label, force }),
  /** @param {string} r */
  removeFolder: (r) => DEL(`/folders?path=${encodeURIComponent(r)}`),
  addExclusion: (pattern, kind = 'glob', note = '') => POST('/exclusions', { pattern, kind, note }),
  removeExclusion: (p) => DEL(`/exclusions?pattern=${encodeURIComponent(p)}`),
  scan: () => POST('/scan'),

  /** @param {Object<string, any>} p */
  search: (p) => GET('/search?' + new URLSearchParams(p)),
  // búsqueda suelta para el desplegable: no toca la lista de la página
  quickSearch: (q, limit = 40) => GET('/search?' + new URLSearchParams({ q, limit })),
  facets: () => GET('/facets'),
  /** @param {number} id @returns {Promise<Song>} */
  song: (id) => GET(`/song/${id}`),
  /** Solo `artist, title, album, year, genre, key, bpm, lyrics`; otras claves dan 422 (§2). */
  edit: (id, d) => PATCH(`/song/${id}`, d),
  setStars: (id, n) => POST(`/song/${id}/stars`, { stars: n }),
  toggleFavorite: (id, v) => POST(`/song/${id}/favorite`, { favorite: v }),
  setBlur: (id, v) => POST(`/song/${id}/blur`, { blur: v }),
  details: (id) => GET(`/song/${id}/details`),
  enrich: (id, o = {}) => POST(`/song/${id}/enrich`, o),
  autofill: (id) => POST(`/song/${id}/autofill`),
  setCover: (id, path) => POST(`/song/${id}/cover`, { path }),
  /** @param {{text:string, from_key?:string, to_key?:string, semitones?:number}} d */
  transpose: (d) => POST('/transpose', d),

  deleteSong: (id) => DEL(`/song/${id}`),

  playlists: () => GET('/playlists'),
  createPlaylist: (name) => POST('/playlists', { name }),
  deletePlaylist: (id) => DEL(`/playlists/${id}`),
  playlistSongs: (id) => GET(`/playlists/${id}/songs`),
  addToPlaylist: (id, ids) => POST(`/playlists/${id}/songs`, { ids }),
  removeFromPlaylist: (l, c) => DEL(`/playlists/${l}/songs/${c}`),
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

  inbox: () => GET('/inbox'),

  // descargas de YouTube: la descarga arranca y vuelve enseguida; el avance
  // se consulta con youtube() cada poco.
  youtube: () => GET('/youtube'),
  youtubeInfo: (query, results = 5) => POST('/youtube/info', { query, results }),
  youtubeDownload: (d) => POST('/youtube/download', d),
  youtubeCancel: () => POST('/youtube/cancel'),
  downloadHistory: (limit = 60) => GET(`/downloads/history?limit=${limit}`),
  clearDownloadHistory: () => DEL('/downloads/history'),

  runImport: (d) => POST('/import', d),
  convertible: () => GET('/convertible'),
  /** @param {{dry_run: boolean, quality: 'high'|'medium'|'variable', keep: boolean}} d */
  convert: (d) => POST('/convert', d),
  duplicates: () => GET('/duplicates'),
  /** @returns {Promise<ChatReply>} */
  chat: (messages) => POST('/chat', { messages }),
  chatTools: () => GET('/chat/tools'),
  /**
   * Ejecuta una herramienta que el asistente dejó pendiente de confirmar (§3).
   * @param {string} tool
   * @param {Object<string, any>} args
   * @returns {Promise<{ok: boolean, result: any, text: string}>}
   */
  chatConfirm: (tool, args) => POST('/chat/confirm', { tool, args }),
  /** `dry_run` es true por defecto en el núcleo: hay que pedir el borrado a propósito (§2). */
  resolveDuplicate: (keep, remove, dry_run = false) =>
    POST('/duplicates/resolve', { keep, remove, dry_run }),

  /** @param {number|string} id */
  audioUrl: (id) => mediaUrl('audio', id),
  /**
   * Portada. Con `size` (96, 192 o 320) llega una miniatura JPEG cacheada;
   * sin él, la imagen original, que solo hace falta en el panel de detalle.
   * @param {number|string} id
   * @param {number} [size]
   */
  coverUrl: (id, size) => mediaUrl('cover', id, coverQuery(size)),
  audioUrlAlt: (id) => mediaUrlAlt('audio', id),
  coverUrlAlt: (id, size) => mediaUrlAlt('cover', id, coverQuery(size))
}
