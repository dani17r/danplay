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

/** Lo que devuelve /api/ai/providers: catálogo corto, perfiles y activo. */
function aiOverview(state) {
  const active = state.aiProfiles[state.aiActive]
  return {
    catalog: state.aiCatalog,
    groups: [
      { id: 'free', name: 'Gratis, sin clave', note: 'para probar' },
      { id: 'lab', name: 'Grandes laboratorios', note: '' },
      { id: 'local', name: 'En tu equipo', note: '' },
      { id: 'custom', name: 'Otro', note: '' }
    ],
    profiles: JSON.parse(JSON.stringify(state.aiProfiles)),
    active: state.aiActive,
    active_profile: active
      ? { id: active.id, provider: active.provider, name: active.provider_name, model: active.model,
          chat_model: active.chat_model, base_url: active.base_url || 'https://x/v1', local: false }
      : null,
    ai_enabled: true,
    fallback: state.aiFallback !== false,
    fallbacks: Object.values(state.aiProfiles).filter((p) => p.id !== state.aiActive).map((p) => ({ id: p.id, name: p.provider_name, chat_model: p.chat_model })),
    ai_ready: !!active,
    ai_reason: active ? '' : 'no hay ningun proveedor de IA elegido',
    catalog_status: { source: 'snapshot', providers: 1, models: 2, fetched_at: 0, checked_at: 0, error: '', refreshing: false }
  }
}

/** El estado que comparten el doble y la prueba. */
export function createState() {
  return {
    songs: [],
    /** proveedores de IA: un catálogo mínimo, sin perfiles guardados */
    aiCatalog: [
      { id: 'llm7', name: 'LLM7', group: 'free', base_url: 'https://api.llm7.io/v1', key: 'optional', key_url: '',
        docs: '', models_dev: null, suggest: { fast: 'minimax-m2.7', chat: 'minimax-m2.7' }, fields: [], headers: {},
        note: 'sin cuenta', quirks: {} },
      { id: 'openai', name: 'OpenAI', group: 'lab', base_url: 'https://api.openai.com/v1', key: 'required',
        key_url: 'https://platform.openai.com/api-keys', docs: '', models_dev: 'openai',
        suggest: { fast: 'chico', chat: 'grande' }, fields: [], headers: {}, note: '', quirks: {} },
      { id: 'ollama', name: 'Ollama', group: 'local', base_url: 'http://localhost:11434/v1', key: 'none',
        key_url: '', docs: '', models_dev: null, suggest: {}, fields: [], headers: {}, note: 'sin clave', quirks: {} },
      { id: 'bedrock', name: 'Amazon Bedrock', group: 'lab', key: 'required', key_url: '', docs: '',
        base_url: 'https://bedrock-runtime.{region}.amazonaws.com/openai/v1', models_dev: null, suggest: {},
        fields: [{ name: 'region', label: 'Region', placeholder: 'us-east-1' }], headers: {}, note: '', quirks: {} },
      { id: 'custom', name: 'Compatible con OpenAI', group: 'custom', base_url: '', key: 'optional', key_url: '',
        docs: '', models_dev: null, suggest: {}, fields: [{ name: 'name', label: 'Nombre', placeholder: '' }],
        headers: {}, note: '', quirks: {} }
    ],
    aiProfiles: {},
    aiActive: '',
    aiFallback: true,
    /** conversaciones guardadas del asistente */
    chats: [],
    aiModels: [
      { id: 'grande', name: 'Grande', tools: true, cost_in: 1, cost_out: 5, context: 128000, released: '2026-09-01', deprecated: false, known: true },
      { id: 'chico', name: 'Chico', tools: true, cost_in: 0.1, cost_out: 0.4, context: 32000, released: '2026-08-01', deprecated: false, known: true },
      { id: 'viejo', name: 'Viejo', tools: false, cost_in: 1, cost_out: 1, context: 8000, released: '2024-01-01', deprecated: true, known: true }
    ],
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
    aiProviders: async () => aiOverview(state),
    aiSaveProfile: async (d) => {
      const id = d.id || (d.provider === 'custom' ? 'custom-' + (d.name || 'x').toLowerCase() : d.provider)
      state.aiProfiles[id] = {
        id, provider: d.provider, provider_name: d.name || d.provider,
        has_key: !!(d.key || state.aiProfiles[id]?.has_key), key: d.key ? d.key.slice(0, 4) + '…' : '',
        model: d.model || '', chat_model: d.chat_model || '', base_url: d.base_url || '',
        fields: d.fields || {}, headers: d.headers || {}, extra: d.extra || {}, timeout: d.timeout || 60
      }
      if (d.activate !== false) state.aiActive = id
      return { ...aiOverview(state), saved: id }
    },
    aiDeleteProfile: async (id) => {
      delete state.aiProfiles[id]
      if (state.aiActive === id) state.aiActive = Object.keys(state.aiProfiles)[0] || ''
      return aiOverview(state)
    },
    aiActivate: async (id) => {
      state.aiActive = id
      return aiOverview(state)
    },
    aiCheck: async (d) => ({ ok: true, model: d.model, chat_model: d.chat_model, latency_ms: 120, tools_ok: true, tools_reason: '' }),
    aiFree: async () => {
      state.aiProfiles.llm7 = {
        id: 'llm7', provider: 'llm7', provider_name: 'LLM7', has_key: false, key: '',
        model: 'minimax-m2.7', chat_model: 'minimax-m2.7', base_url: '', fields: {}, headers: {}, extra: {}, timeout: 20
      }
      state.aiActive = 'llm7'
      return { ...aiOverview(state), free: { ok: true, chosen: 'llm7', name: 'LLM7', model: 'minimax-m2.7', chat_model: 'minimax-m2.7', tools_ok: true, tried: [{ id: 'llm7', name: 'LLM7', ok: true, reason: '' }] } }
    },
    aiModels: async () => ({
      ok: true, source: 'provider', provider: 'Prueba',
      models: state.aiModels, catalog: state.aiModels, suggest: { chat: 'grande', fast: 'chico' },
      catalog_status: { models: 2, providers: 1, checked_at: 0, refreshing: false, error: '' }
    }),
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
    chatStart: async () => ({ id: 'job1' }),
    chatPoll: async () => ({ text: 'hola', tools: [], done: true,
                             result: { text: 'hola', tools: [], actions: [], confirm: null } }),
    chatCancel: async () => ({ ok: true }),
    chats: async () => ({ chats: state.chats.map((c) => ({ id: c.id, title: c.title, updated: c.updated, n: c.messages.length })) }),
    chatCreate: async (title = '') => {
      const c = { id: state.chats.length + 1, title, updated: Date.now() / 1000, messages: [] }
      state.chats.unshift(c)
      return { id: c.id, title: c.title, updated: c.updated, n: 0 }
    },
    chatGet: async (id) => {
      const c = state.chats.find((x) => x.id === id)
      return c ? { id: c.id, title: c.title, messages: c.messages.map((m) => ({ ...m })) } : null
    },
    chatAppend: async (id, messages) => {
      const c = state.chats.find((x) => x.id === id)
      if (!c) throw new Error('no existe esa conversacion')
      c.messages.push(...messages.map((m) => ({ ...m })))
      if (!c.title) c.title = (messages.find((m) => m.role === 'me' && !m.hidden)?.text || '').slice(0, 60)
      return { n: messages.length }
    },
    chatRename: async (id, title) => { const c = state.chats.find((x) => x.id === id); if (c) c.title = title; return { ok: true } },
    chatDelete: async (id) => { state.chats = state.chats.filter((x) => x.id !== id); return { ok: true } },
    chatSearch: async (q) => ({
      hits: state.chats.flatMap((c) => c.messages.filter((m) => (m.text || '').toLowerCase().includes(q.toLowerCase()))
        .map((m) => ({ chat_id: c.id, title: c.title, role: m.role, snippet: m.text, at: 0 })))
    }),
    aiUsage: async () => ({ today: { calls: 2, prompt: 3000, completion: 400, cost: 0.0021, unpriced: 0 },
                            month: { calls: 20, prompt: 30000, completion: 4000, cost: 0.021, unpriced: 0 },
                            total: { calls: 20, prompt: 30000, completion: 4000, cost: 0.021, unpriced: 0 } }),
    aiFallback: async (enabled) => { state.aiFallback = enabled; return aiOverview(state) },
    setStudy: async (id, study) => {
      const s = find(id)
      const raw = study && Object.keys(study).length ? JSON.stringify(study) : ''
      if (s) s.study = raw
      return s ? { ...s } : null
    },
    playlistSheet: async (id) => ({ file: `/musica/Listas/lista-${id}.html` }),
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
  // Los escuchadores no son respuestas programables: se guardan para poder
  // dispararlos desde la prueba, como hace Rust.
  double.onExternal = vi.fn(async (fn) => {
    double.fireExternal = fn
    return () => (double.fireExternal = null)
  })
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
    shareTargets: vi.fn(async () => ({ telegram: true })),
    sendToTelegram: vi.fn(async () => {}),
    revealInFolder: vi.fn(async () => {}),
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
    origin: null,
    revision: 0
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
        revision: state.revision + 1,
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
    setLoop: vi.fn(async (a, b) => emit({ loop_a: a == null ? 0 : a, loop_b: b == null ? 0 : b })),
    state: vi.fn(async () => ({ ...state })),
    queueItems: vi.fn(async () => ({ items, origin })),
    onState: vi.fn(async (fn) => {
      listeners.add(fn)
      return () => listeners.delete(fn)
    })
  }
  /**
   * Lo que pasa cuando la cola la cambia RUST, no la interfaz: al abrir una
   * canción con DanPlay desde el explorador de archivos, o desde la bandeja.
   * La interfaz no se entera por haberlo pedido ella, solo por el estado.
   */
  const replaceQueueFromRust = (list) => {
    items = list
    origin = null
    emit({
      revision: state.revision + 1,
      track: list[0] || null,
      index: list.length ? 0 : -1,
      length: list.length,
      playing: !!list.length,
      position: 0,
      duration: list[0]?.duration || 0,
      origin: null,
      has_previous: false,
      has_next: list.length > 1
    })
  }

  /** Vuelve a empezar: sin oyentes y sin nada sonando. */
  function reset() {
    listeners.clear()
    Object.assign(state, blank)
    items = []
    origin = null
  }
  return { bridge, emit, state, reset, replaceQueueFromRust }
}
