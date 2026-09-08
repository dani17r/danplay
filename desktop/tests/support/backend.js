// Un doble del núcleo y del puente con Rust, para montar la app entera.
//
// Se construye **a partir del `api` de verdad**: cualquier método que exista
// en `src/api.js` y no esté programado aquí falla con un error que dice cuál
// es. Antes el doble se escribía a mano y se le quedaban nombres muertos
// (`quitarCarpeta`, `estadoFalso.ia`), así que las pruebas seguían en verde
// mientras la app llamaba a cosas que ya no existían.
import { vi } from 'vitest'

/** Una canción de mentira con todos los campos que la interfaz espera. */
export function song(id, extra = {}) {
  return {
    id,
    title: `Cancion ${id}`,
    artist: 'Barak',
    album: '',
    file: `cancion-${id}.mp3`,
    path: `/musica/cancion-${id}.mp3`,
    duration: 200,
    bitrate: 320000,
    size: 5_000_000,
    stars: 0,
    favorite: 0,
    blur: 0,
    feat: '',
    folder: 'Artistas/Barak',
    key: '',
    bpm: 0,
    lyrics: '',
    ...extra
  }
}

/** El estado que comparten el doble y la prueba. */
export function createState() {
  return {
    songs: [],
    playlists: [],
    addedFolders: [],
    inbox: 0,
    duplicates: { identical: [], similar: [] },
    /** Lo que aparece al escanear una carpeta recien añadida. */
    scanFinds: [song(1, { title: 'Mi Gozo' }), song(2, { title: 'Shekinah' })],
    status: {
      configured: false,
      folders: 0,
      stats: { total: 0, bytes: 0, seconds: 0 },
      library: '/musica',
      inbox: '/musica/Entrada',
      model: 'x',
      // OJO: la API dice `ai`, no `ia`. El doble viejo usaba el nombre
      // castellano y por eso nadie vio que la insignia de Ajustes decía
      // siempre «sin clave».
      ai: false,
      fingerprint: false,
      rust: true,
      ffmpeg: true,
      convert: false,
      quality: 'high',
      never_convert: ['Secuencias'],
      jobs: {}
    }
  }
}

const copy = (v) => JSON.parse(JSON.stringify(v))

/**
 * Las respuestas por defecto de cada método del `api`.
 * @param {ReturnType<typeof createState>} state
 */
function answers(state) {
  const find = (id) => state.songs.find((s) => s.id === id)
  const update = (id, patch) => {
    const s = find(id)
    if (s) Object.assign(s, patch)
    return s ? { ...s } : null
  }
  return {
    status: async () => {
      state.status.stats.total = state.songs.length
      return copy(state.status)
    },
    settings: async () => ({
      convert_mp3: false,
      quality: 'high',
      keep_original: false,
      write_tags: true,
      ai_enabled: true,
      model: 'x',
      library: '/musica',
      ai_key: '',
      has_ai_key: false,
      fingerprint_key: false,
      settings_file: '/a'
    }),
    saveSettings: async (d) => ({ ...d }),
    checkAi: async () => ({ ok: true }),
    folders: async () => ({ folders: [], exclusions: [], always_excluded: [] }),
    checkFolder: async () => ({ notice: null }),
    addFolder: async (path, label, force) => {
      if (state.addedFolders.includes(path) && !force) {
        return {
          action: 'already_there',
          notice: { kind: 'same', message: 'esa carpeta ya está añadida' },
          folders: []
        }
      }
      state.addedFolders.push(path)
      state.status.configured = true
      return { action: 'added', notice: null, folders: [] }
    },
    removeFolder: async () => ({ folders: [], exclusions: [], always_excluded: [] }),
    addExclusion: async () => ({ folders: [], exclusions: [], always_excluded: [] }),
    removeExclusion: async () => ({ folders: [], exclusions: [], always_excluded: [] }),
    scan: async () => {
      // escanear encuentra musica: si la prueba no dijo cual, la de siempre
      if (!state.songs.length) state.songs = state.scanFinds.map((s) => ({ ...s }))
      state.status.stats.total = state.songs.length
      return { job: {}, stats: state.status.stats }
    },
    search: async () => ({ total: state.songs.length, songs: state.songs.map((s) => ({ ...s })) }),
    quickSearch: async () => ({
      total: state.songs.length,
      songs: state.songs.map((s) => ({ ...s }))
    }),
    facets: async () => ({ artists: [], albums: [], genres: [], sorts: [], filters: [], numeric: [] }),
    song: async (id) => {
      const s = find(id)
      return s ? { ...s } : null
    },
    path: async (id) => ({ path: `/musica/cancion-${id}.mp3`, kind: 'audio/mpeg', bytes: 1 }),
    edit: async (id, d) => update(id, d),
    setStars: async (id, n) => update(id, { stars: n }),
    toggleFavorite: async (id, v) => update(id, { favorite: v ? 1 : 0 }),
    setBlur: async (id, v) => update(id, { blur: v ? 1 : 0 }),
    details: async () => ({ details: null }),
    enrich: async (id) => ({ result: {}, song: find(id) ? { ...find(id) } : null }),
    autofill: async (id) => ({ filled: {}, song: find(id) ? { ...find(id) } : null }),
    setCover: async (id) => ({ ok: true, kb: 12, song: find(id) ? { ...find(id) } : null }),
    transpose: async () => ({ text: '', latin: '', capo: [], keys: [] }),
    deleteSong: async (id) => {
      const s = find(id)
      state.songs = state.songs.filter((x) => x.id !== id)
      return { ok: true, trashed: true, name: s?.file || '' }
    },
    playlists: async () => ({ playlists: state.playlists.map((l) => ({ ...l })), favorites: 0 }),
    createPlaylist: async (name) => {
      const existing = state.playlists.find((l) => l.name === name)
      if (existing) return { id: existing.id, created: false, playlists: state.playlists }
      const id = state.playlists.length + 1
      state.playlists.push({ id, name, n: 0 })
      return { id, created: true, playlists: state.playlists }
    },
    deletePlaylist: async (id) => {
      state.playlists = state.playlists.filter((l) => l.id !== id)
      return { playlists: state.playlists }
    },
    playlistSongs: async () => ({ songs: [] }),
    addToPlaylist: async () => ({ added: 1, songs: [] }),
    removeFromPlaylist: async () => ({ songs: [] }),
    exportPlaylist: async () => ({ file: '/musica/Listas/x.m3u8' }),
    inbox: async () => ({ files: [], total: state.inbox }),
    youtube: async () => ({ available: false, reason: 'falta yt-dlp', active: false }),
    youtubeInfo: async () => ({ ok: false, reason: 'sin red', items: [] }),
    youtubeDownload: async () => ({ ok: true, active: true }),
    youtubeCancel: async () => ({ ok: true }),
    downloadHistory: async () => ({ items: [], total: 0 }),
    clearDownloadHistory: async () => ({ removed: 0 }),
    runImport: async () => ({ results: [] }),
    convertible: async () => ({ total: 0, files: [], protected: [] }),
    convert: async () => ({ converted: 0, failures: 0 }),
    duplicates: async () => copy(state.duplicates),
    resolveDuplicate: async () => ({ ok: true, kept: '/a.mp3', renamed: false, final_name: 'a.mp3', deleted: [] }),
    chat: async () => ({ text: 'hola', tools: [], actions: [], confirm: null }),
    chatConfirm: async () => ({ ok: true, result: {}, text: 'hecho' }),
    chatTools: async () => ({ model: 'x', available: false, tools: [] })
  }
}

/**
 * Construye el módulo falso a partir del real.
 * @param {any} actual  lo que devuelve `importOriginal()`
 * @param {ReturnType<typeof createState>} state
 */
export function buildApiDouble(actual, state) {
  const planned = answers(state)
  const double = {}
  for (const [key, value] of Object.entries(actual.api)) {
    if (typeof value !== 'function') {
      double[key] = value
      continue
    }
    const answer = planned[key]
    double[key] =
      key === 'coverUrl' || key === 'audioUrl' || key === 'coverUrlAlt' || key === 'audioUrlAlt'
        ? vi.fn(value)
        : vi.fn(
            answer ||
              (async () => {
                throw new Error(
                  `la prueba no ha programado api.${key}: añádelo en tests/support/backend.js`
                )
              })
          )
  }
  double.inTauri = false
  return double
}

/**
 * El doble de `app`: lo que la aplicación pide a Rust sobre sí misma.
 *
 * Con valores neutros: sin bandeja y sin ser el reproductor predeterminado,
 * que es el estado en el que arranca cualquier prueba. `defaultPlayer` hace
 * falta aunque la prueba no vaya de eso: App.vue lo consulta al montarse.
 */
export function createAppDouble(over = {}) {
  return {
    showWindow: vi.fn(async () => {}),
    quit: vi.fn(async () => {}),
    trayAvailable: vi.fn(async () => false),
    defaultPlayer: vi.fn(async () => ({
      supported: true,
      is_default: false,
      direct: true,
      note: ''
    })),
    makeDefaultPlayer: vi.fn(async () => ({
      supported: true,
      is_default: true,
      direct: true,
      note: ''
    })),
    ...over
  }
}

/**
 * Un puente de reproducción falso con la misma forma que el de Rust.
 * Guarda lo que se le manda para poder comprobarlo, y publica el estado por
 * el mismo camino que el de verdad (`onState`).
 */
export function createPlaybackDouble() {
  const listeners = new Set()
  const state = {
    track: null,
    index: -1,
    length: 0,
    playing: false,
    position: 0,
    duration: 0,
    volume: 0.9,
    speed: 1,
    repeat: 'list',
    shuffle: false,
    has_previous: false,
    has_next: false,
    error: '',
    has_output: true,
    origin: null
  }
  let items = []
  let origin = null

  const push = () => {
    const snapshot = { ...state }
    for (const fn of listeners) fn(snapshot)
  }
  /** Simula lo que Rust contaría. Las pruebas lo llaman para mover el estado. */
  const emit = (patch) => {
    Object.assign(state, patch)
    push()
  }

  const blank = { ...state }
  const bridge = {
    available: true,
    setQueue: vi.fn(async (list, start, from) => {
      items = list
      origin = from
      const i = start == null ? 0 : list.findIndex((t) => t.id === start)
      const index = i < 0 ? 0 : i
      emit({
        track: list[index] || null,
        index: list.length ? index : -1,
        length: list.length,
        playing: !!list.length,
        position: 0,
        duration: list[index]?.duration || 0,
        origin: from,
        has_previous: list.length > 1,
        has_next: list.length > 1
      })
    }),
    next: vi.fn(async () => {}),
    previous: vi.fn(async () => {}),
    jump: vi.fn(async () => {}),
    setRepeat: vi.fn(async (mode) => emit({ repeat: mode })),
    setShuffle: vi.fn(async (on) => emit({ shuffle: on })),
    toggle: vi.fn(async () => emit({ playing: !state.playing })),
    stop: vi.fn(async () => emit({ track: null, index: -1, playing: false })),
    seek: vi.fn(async (seconds) => emit({ position: seconds })),
    setVolume: vi.fn(async (value) => emit({ volume: value })),
    setSpeed: vi.fn(async (value) => emit({ speed: value })),
    state: vi.fn(async () => ({ ...state })),
    queueItems: vi.fn(async () => ({ items, origin })),
    onState: vi.fn(async (fn) => {
      listeners.add(fn)
      return () => listeners.delete(fn)
    })
  }
  /** Vuelve a empezar: sin oyentes y sin nada sonando. */
  function reset() {
    listeners.clear()
    Object.assign(state, blank)
    items = []
    origin = null
  }
  return { bridge, emit, state, reset }
}
