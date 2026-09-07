// Capa de transporte.
//
// En la app de escritorio TODO pasa por Rust (invoke -> socket Unix). No hay
// puerto TCP abierto ni fetch a localhost, asi que ningun otro proceso del
// equipo puede hablar con el nucleo.
// En el navegador (npm run dev suelto) cae a fetch contra el proxy de Vite.

const inTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

let invoke = null
if (inTauri) {
  const mod = await import('@tauri-apps/api/core')
  invoke = mod.invoke
}

async function request (method, path, body) {
  if (inTauri) {
    const txt = await invoke('api', {
      method,
      path: '/api' + path,
      body: body ? JSON.stringify(body) : null
    })
    return txt ? JSON.parse(txt) : null
  }
  const r = await fetch('/api' + path, {
    method: method,
    headers: { 'Content-Type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined
  })
  if (!r.ok) throw new Error(`${r.status} ${await r.text()}`)
  return r.json()
}

const GET = (r) => request('GET', r)
const POST = (r, c) => request('POST', r, c)
const PATCH = (r, c) => request('PATCH', r, c)
const DEL = (r) => request('DELETE', r)

// El audio y la portada no viajan por el puente: los sirve Rust con un protocolo
// propio (el audio se lee del disco, con soporte de Range).
//
// OJO: Tauri expone los protocolos propios de forma distinta segun el sistema.
// En Linux y Windows es  http://<esquema>.localhost/...
// y solo en macOS/iOS es  <esquema>://localhost/...
const isApple = typeof navigator !== 'undefined' &&
  /Mac|iPhone|iPad|iPod/.test(navigator.platform || navigator.userAgent || '')

export function mediaUrl (kind, id) {
  if (!inTauri) return `/api/song/${id}/${kind}`
  return isApple ? `danplay://localhost/${kind}/${id}`
                 : `http://danplay.localhost/${kind}/${id}`
}

/** La otra forma del protocolo. Si el WebView rechaza una, probamos con esta. */
export function mediaUrlAlt (kind, id) {
  if (!inTauri) return null
  return isApple ? `http://danplay.localhost/${kind}/${id}`
                 : `danplay://localhost/${kind}/${id}`
}
const media = mediaUrl

/** Abre el selector de carpetas del sistema. Devuelve la ruta o null. */
export async function pickFolder (title = 'Elige tu carpeta de musica') {
  if (!inTauri) return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  const r = await open({ directory: true, multiple: false, title: title })
  return typeof r === 'string' ? r : (Array.isArray(r) ? r[0] : null)
}

/** Abre el selector de imagenes del sistema. Devuelve la ruta o null. */
export async function pickImage (title = 'Elige una imagen para la caratula') {
  if (!inTauri) return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  const r = await open({
    multiple: false, title,
    filters: [{ name: 'Imagenes', extensions: ['jpg', 'jpeg', 'png', 'webp'] }]
  })
  return typeof r === 'string' ? r : (Array.isArray(r) ? r[0] : null)
}

/** Reproduccion nativa (solo dentro de la app). */
export const native = {
  available: inTauri,
  play: (path) => invoke('play', { path }),
  togglePlay: () => invoke('toggle_pause'),
  stop: () => invoke('stop'),
  seek: (seconds) => invoke('seek', { seconds }),
  volume: (value) => invoke('set_volume', { value }),
  speed: (value) => invoke('set_speed', { value }),
  status: () => invoke('audio_state')
}

/**
 * Bandeja del sistema y ventanita del reproductor (solo dentro de la app).
 *
 * El menu de la bandeja y la ventanita viven fuera de la ventana principal,
 * asi que no ven nada de lo que pasa en ella: hay que contarselo. Rust guarda
 * lo ultimo que le dijimos, para que la ventanita sepa que poner nada mas
 * abrirse sin tener que esperar al siguiente cambio.
 */
export const tray = {
  available: inTauri,
  /** Lo que suena ahora, para pintar el menu y la ventanita. */
  setNowPlaying: (data) => inTauri ? invoke('set_now_playing', { data }) : Promise.resolve(),
  nowPlaying: () => inTauri ? invoke('now_playing') : Promise.resolve(null),
  openMini: () => inTauri ? invoke('show_mini') : Promise.resolve(),
  closeMini: () => inTauri ? invoke('hide_mini') : Promise.resolve(),
  showApp: () => inTauri ? invoke('show_window') : Promise.resolve(),

  /** Manda una orden (cambiar de cancion) a quien tenga la cola. */
  async send (action, extra = {}) {
    if (!inTauri) return
    const { emit } = await import('@tauri-apps/api/event')
    await emit('danplay://command', { action, ...extra })
  },
  /** Escucha esas ordenes. Devuelve como dejar de escuchar. */
  async onCommand (fn) {
    if (!inTauri) return () => {}
    const { listen } = await import('@tauri-apps/api/event')
    return listen('danplay://command', (e) => fn(e.payload || {}))
  },
  /** Avisa a la ventanita de que ha cambiado lo que suena. */
  async onChanged (fn) {
    if (!inTauri) return () => {}
    const { listen } = await import('@tauri-apps/api/event')
    return listen('danplay://now', (e) => fn(e.payload || {}))
  }
}

export const api = {
  inTauri,
  path: (id) => GET(`/song/${id}/path`),
  status: () => GET('/status'),
  settings: () => GET('/settings'),
  checkAi: () => POST('/settings/check-ai'),
  saveSettings: (d) => POST('/settings', d),

  folders: () => GET('/folders'),
  checkFolder: (path) => POST('/check-folder', { path }),
  addFolder: (path, label, force = false) =>
    POST('/folders', { path, label, force }),
  removeFolder: (r) => DEL(`/folders?path=${encodeURIComponent(r)}`),
  addExclusion: (pattern, kind = 'glob', note = '') =>
    POST('/exclusions', { pattern, kind, note }),
  removeExclusion: (p) => DEL(`/exclusions?pattern=${encodeURIComponent(p)}`),
  scan: () => POST('/scan'),

  search: (p) => GET('/search?' + new URLSearchParams(p)),
  facets: () => GET('/facets'),
  song: (id) => GET(`/song/${id}`),
  edit: (id, d) => PATCH(`/song/${id}`, d),
  setStars: (id, n) => POST(`/song/${id}/stars`, { stars: n }),
  toggleFavorite: (id, v) => POST(`/song/${id}/favorite`, { favorite: v }),
  setBlur: (id, v) => POST(`/song/${id}/blur`, { blur: v }),
  details: (id) => GET(`/song/${id}/details`),
  enrich: (id, o = {}) => POST(`/song/${id}/enrich`, o),
  autofill: (id) => POST(`/song/${id}/autofill`),
  setCover: (id, path) => POST(`/song/${id}/cover`, { path }),
  transpose: (d) => POST('/transpose', d),

  deleteSong: (id) => DEL(`/song/${id}`),

  playlists: () => GET('/playlists'),
  createPlaylist: (name) => POST('/playlists', { name }),
  deletePlaylist: (id) => DEL(`/playlists/${id}`),
  playlistSongs: (id) => GET(`/playlists/${id}/songs`),
  addToPlaylist: (id, ids) => POST(`/playlists/${id}/songs`, { ids }),
  removeFromPlaylist: (l, c) => DEL(`/playlists/${l}/songs/${c}`),
  exportPlaylist: (id) => POST(`/playlists/${id}/export`),

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
  convert: (d) => POST('/convert', d),
  duplicates: () => GET('/duplicates'),
  chat: (messages) => POST('/chat', { messages }),
  chatTools: () => GET('/chat/tools'),
  resolveDuplicate: (keep, remove) =>
    POST('/duplicates/resolve', { keep, remove }),

  audioUrl: (id) => media('audio', id),
  coverUrl: (id) => media('cover', id),
  audioUrlAlt: (id) => mediaUrlAlt('audio', id),
  coverUrlAlt: (id) => mediaUrlAlt('cover', id)
}
