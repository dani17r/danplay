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

/**
 * Una canción como la mandan las LISTAS (contrato C): sin los textos pesados
 * (letra, acordes, estudio), con `has_*` en su lugar. La ficha completa sigue
 * en `song`.
 */
export function light(s) {
  const { lyrics, lyrics_synced, chords, study, stems, ...rest } = s
  return {
    ...rest,
    has_lyrics: !!lyrics,
    has_synced_lyrics: !!lyrics_synced,
    has_chords: !!chords,
    has_study: !!study,
    has_stems: !!(stems || rest.has_stems),
    stems_best: !!rest.stems_best
  }
}

/**
 * Las pistas separadas de una cancion, como las devuelve /api/song/{id}/stems.
 * @param {number} id
 * @param {string[]} [sources]
 * @param {''|'rapida'|'mejor'} [quality]  las de la pasada rápida, las buenas o las de 1.16.0
 */
export function stemsOf(
  id,
  sources = ['drums', 'vocals', 'bass', 'guitar', 'piano', 'other'],
  quality = 'mejor'
) {
  const names = {
    drums: ['Bateria', 'Batería'],
    vocals: ['Voces', 'Voces'],
    bass: ['Bajo', 'Bajo'],
    guitar: ['Guitarra', 'Guitarra'],
    piano: ['Piano', 'Piano'],
    other: ['Otros', 'Otros']
  }
  const folder = `/musica/Separadas/cancion-${id}`
  return {
    folder,
    model: quality === 'mejor' ? 'htdemucs_6s+htdemucs_ft' : 'htdemucs_6s',
    created: 1,
    quality,
    best: quality === 'mejor',
    dropped: [],
    complete: true,
    tracks: sources.map((source) => ({
      source,
      name: names[source][1],
      file: `${names[source][0]}.flac`,
      path: `${folder}/${names[source][0]}.flac`,
      exists: true,
      wave: { peaks: [0.2, 0.8, 0.5], rms: [0.1, 0.4, 0.2] }
    }))
  }
}

/**
 * Acaba la pasada en marcha, como lo haria el nucleo: tras la rapida, la
 * cancion ya tiene sus pistas y su mejora va a la cola (detras de las
 * rapidas que esperen); tras la buena, ya son las mejores. Pasa la siguiente
 * de la cola (o se para) y queda el aviso de lo que acabo.
 * @param {ReturnType<typeof createState>} state
 * @param {{ok?: boolean, error?: string}} [how]
 */
export function finishSeparation(state, how = {}) {
  const item = state.separation.current
  if (!item) return
  const ok = how.ok ?? true
  const s = state.songs.find((x) => x.id === item.id)
  if (ok) {
    const quality = item.stage === 'refine' ? 'mejor' : 'rapida'
    state.stems[item.id] = stemsOf(item.id, undefined, quality)
    if (s) Object.assign(s, { has_stems: true, stems_best: quality === 'mejor' })
    if (item.stage !== 'refine') {
      state.separation.queue.push({ id: item.id, title: item.title, stage: 'refine' })
    }
  }
  const next = state.separation.queue.findIndex((q) => q.stage === 'separate')
  const at = next >= 0 ? next : 0
  state.separation.current = state.separation.queue.splice(at, 1)[0] || null
  const seq = (state.separation.events.at(-1)?.seq || 0) + 1
  state.separation.events.push({
    seq,
    id: item.id,
    title: item.title,
    stage: item.stage,
    ok,
    error: how.error || ''
  })
  state.jobs.separacion = {
    ...finishedJob('separacion', { separated: [], failed: [], cancelled: false }),
    active: !!state.separation.current
  }
}

/** Una tarea larga ya terminada, como la cuenta `/api/jobs/{name}` (contrato A). */
export function finishedJob(name, result, extra = {}) {
  return {
    name,
    active: false,
    done: 1,
    total: 1,
    message: '',
    result,
    error: '',
    started: 1,
    ended: 2,
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
      ? {
          id: active.id,
          provider: active.provider,
          name: active.provider_name,
          model: active.model,
          chat_model: active.chat_model,
          base_url: active.base_url || 'https://x/v1',
          local: false
        }
      : null,
    ai_enabled: true,
    fallback: state.aiFallback !== false,
    fallbacks: Object.values(state.aiProfiles)
      .filter((p) => p.id !== state.aiActive)
      .map((p) => ({ id: p.id, name: p.provider_name, chat_model: p.chat_model })),
    ai_ready: !!active,
    ai_reason: active ? '' : 'no hay ningun proveedor de IA elegido',
    catalog_status: {
      source: 'snapshot',
      providers: 1,
      models: 2,
      fetched_at: 0,
      checked_at: 0,
      error: '',
      refreshing: false
    }
  }
}

/** El estado que comparten el doble y la prueba. */
export function createState() {
  return {
    songs: [],
    /** proveedores de IA: un catálogo mínimo, sin perfiles guardados */
    aiCatalog: [
      {
        id: 'llm7',
        name: 'LLM7',
        group: 'free',
        base_url: 'https://api.llm7.io/v1',
        key: 'optional',
        key_url: '',
        docs: '',
        models_dev: null,
        suggest: { fast: 'minimax-m2.7', chat: 'minimax-m2.7' },
        fields: [],
        headers: {},
        note: 'sin cuenta',
        quirks: {}
      },
      {
        id: 'openai',
        name: 'OpenAI',
        group: 'lab',
        base_url: 'https://api.openai.com/v1',
        key: 'required',
        key_url: 'https://platform.openai.com/api-keys',
        docs: '',
        models_dev: 'openai',
        suggest: { fast: 'chico', chat: 'grande' },
        fields: [],
        headers: {},
        note: '',
        quirks: {}
      },
      {
        id: 'ollama',
        name: 'Ollama',
        group: 'local',
        base_url: 'http://localhost:11434/v1',
        key: 'none',
        key_url: '',
        docs: '',
        models_dev: null,
        suggest: {},
        fields: [],
        headers: {},
        note: 'sin clave',
        quirks: {}
      },
      {
        id: 'bedrock',
        name: 'Amazon Bedrock',
        group: 'lab',
        key: 'required',
        key_url: '',
        docs: '',
        base_url: 'https://bedrock-runtime.{region}.amazonaws.com/openai/v1',
        models_dev: null,
        suggest: {},
        fields: [{ name: 'region', label: 'Region', placeholder: 'us-east-1' }],
        headers: {},
        note: '',
        quirks: {}
      },
      {
        id: 'custom',
        name: 'Compatible con OpenAI',
        group: 'custom',
        base_url: '',
        key: 'optional',
        key_url: '',
        docs: '',
        models_dev: null,
        suggest: {},
        fields: [{ name: 'name', label: 'Nombre', placeholder: '' }],
        headers: {},
        note: '',
        quirks: {}
      }
    ],
    aiProfiles: {},
    aiActive: '',
    aiFallback: true,
    /** conversaciones guardadas del asistente */
    chats: [],
    aiModels: [
      {
        id: 'grande',
        name: 'Grande',
        tools: true,
        cost_in: 1,
        cost_out: 5,
        context: 128000,
        released: '2026-09-01',
        deprecated: false,
        known: true
      },
      {
        id: 'chico',
        name: 'Chico',
        tools: true,
        cost_in: 0.1,
        cost_out: 0.4,
        context: 32000,
        released: '2026-08-01',
        deprecated: false,
        known: true
      },
      {
        id: 'viejo',
        name: 'Viejo',
        tools: false,
        cost_in: 1,
        cost_out: 1,
        context: 8000,
        released: '2024-01-01',
        deprecated: true,
        known: true
      }
    ],
    playlists: [],
    /** las canciones de la lista abierta, en su orden (el doble no distingue listas) */
    playlistSongs: [],
    addedFolders: [],
    inbox: 0,
    duplicates: { identical: [], similar: [] },
    /** las tareas largas: la ultima de cada nombre, como en el nucleo */
    jobs: {},
    /**
     * separar en pistas: si se puede, lo que pesa el separador, la cola y
     * lo que acabo (como /api/separate), y las pistas de cada cancion ya
     * separada, por id
     */
    separation: {
      ok: true,
      reason: '',
      bytes: 307000000,
      pending: 0,
      installed: true,
      folder: 'Separadas',
      format: 'flac',
      opus: true,
      current: null,
      /** @type {any[]} */
      queue: [],
      /** @type {any[]} */
      events: []
    },
    /** @type {Record<number, any>} */
    stems: {},
    /** yt-dlp: la version que se usa, la que viaja con la app y el motor de JS */
    ytdlp: {
      version: '2026.09.01',
      bundled_version: '2026.09.01',
      js_runtime: 'deno',
      js_runtime_hint: ''
    },
    /** Lo que aparece al escanear una carpeta recien añadida. */
    scanFinds: [song(1, { title: 'Mi Gozo' }), song(2, { title: 'Shekinah' })],
    status: {
      configured: false,
      folders: 0,
      // carpetas gestionadas que ya no están donde estaban
      missing_folders: [],
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
  /** Arranca una tarea: aqui termina al momento y se queda para `job`. */
  const start = (name, result) => {
    state.jobs[name] = finishedJob(name, result)
    return { job: copy(state.jobs[name]) }
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
      const id =
        d.id || (d.provider === 'custom' ? 'custom-' + (d.name || 'x').toLowerCase() : d.provider)
      state.aiProfiles[id] = {
        id,
        provider: d.provider,
        provider_name: d.name || d.provider,
        has_key: !!(d.key || state.aiProfiles[id]?.has_key),
        key: d.key ? d.key.slice(0, 4) + '…' : '',
        model: d.model || '',
        chat_model: d.chat_model || '',
        base_url: d.base_url || '',
        fields: d.fields || {},
        headers: d.headers || {},
        extra: d.extra || {},
        timeout: d.timeout || 60
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
    aiCheck: async (d) => ({
      ok: true,
      model: d.model,
      chat_model: d.chat_model,
      latency_ms: 120,
      tools_ok: true,
      tools_reason: ''
    }),
    aiFree: async () => {
      state.aiProfiles.llm7 = {
        id: 'llm7',
        provider: 'llm7',
        provider_name: 'LLM7',
        has_key: false,
        key: '',
        model: 'minimax-m2.7',
        chat_model: 'minimax-m2.7',
        base_url: '',
        fields: {},
        headers: {},
        extra: {},
        timeout: 20
      }
      state.aiActive = 'llm7'
      return {
        ...aiOverview(state),
        free: {
          ok: true,
          chosen: 'llm7',
          name: 'LLM7',
          model: 'minimax-m2.7',
          chat_model: 'minimax-m2.7',
          tools_ok: true,
          tried: [{ id: 'llm7', name: 'LLM7', ok: true, reason: '' }]
        }
      }
    },
    aiModels: async () => ({
      ok: true,
      source: 'provider',
      provider: 'Prueba',
      models: state.aiModels,
      catalog: state.aiModels,
      suggest: { chat: 'grande', fast: 'chico' },
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
    relocateFolder: async (from, to) => {
      state.status.missing_folders = state.status.missing_folders.filter((p) => p !== from)
      return { from, to, back: 0, folders: [], exclusions: [], always_excluded: [] }
    },
    addExclusion: async () => ({ folders: [], exclusions: [], always_excluded: [] }),
    removeExclusion: async () => ({ folders: [], exclusions: [], always_excluded: [] }),
    scan: async () => {
      // escanear encuentra musica: si la prueba no dijo cual, la de siempre
      if (!state.songs.length) state.songs = state.scanFinds.map((s) => ({ ...s }))
      state.status.stats.total = state.songs.length
      return start('escaneo', { stats: { ...state.status.stats } })
    },
    job: async (name) => {
      if (!state.jobs[name]) throw new Error(`404: no hay ninguna tarea «${name}»`)
      return copy(state.jobs[name])
    },
    // por paginas, como el nucleo: `count` es cuantas hay en total
    search: async (p = {}) => {
      const from = Number(p.from_key || 0)
      const limit = Number(p.limit || 200)
      const page = state.songs.slice(from, from + limit).map(light)
      return { total: page.length, count: state.songs.length, songs: page }
    },
    quickSearch: async () => ({
      total: state.songs.length,
      songs: state.songs.map((s) => ({ ...s }))
    }),
    facets: async () => ({
      artists: [],
      albums: [],
      genres: [],
      sorts: [],
      filters: [],
      numeric: []
    }),
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
    playlistSongs: async () => ({ songs: state.playlistSongs.map(light) }),
    addToPlaylist: async () => ({ added: 1, songs: [] }),
    removeFromPlaylist: async () => ({ songs: [] }),
    reorderPlaylist: async (id, ids) => {
      const by = new Map(state.playlistSongs.map((c) => [c.id, c]))
      state.playlistSongs = ids.map((i) => by.get(i)).filter(Boolean)
      return { songs: state.playlistSongs.map((c) => ({ ...c })) }
    },
    exportPlaylist: async () => ({ file: '/musica/Listas/x.m3u8' }),
    inbox: async () => ({ files: [], total: state.inbox }),
    youtube: async () => ({
      available: false,
      reason: 'falta yt-dlp',
      active: false,
      ...state.ytdlp
    }),
    youtubeUpdate: async () => {
      const previous = state.ytdlp.version
      state.ytdlp.version = '2026.09.20'
      return start('yt-dlp', {
        previous,
        version: state.ytdlp.version,
        updated: previous !== state.ytdlp.version
      })
    },
    youtubeInfo: async () => ({ ok: false, reason: 'sin red', items: [] }),
    youtubeDownload: async () => ({ ok: true, active: true }),
    youtubeCancel: async () => ({ ok: true }),
    downloadHistory: async () => ({ items: [], total: 0 }),
    clearDownloadHistory: async () => ({ removed: 0 }),
    runImport: async () => start('importacion', { results: [] }),
    convertible: async () => ({ total: 0, files: [], protected: [] }),
    // en prueba contesta al momento; de verdad, es una tarea
    convert: async (d = {}) =>
      d.dry_run
        ? { converted: 0, failures: 0, dry_run: true }
        : start('conversion', { converted: 0, failures: 0 }),
    duplicatesScan: async () => start('duplicados', copy(state.duplicates)),
    resolveDuplicate: async () => ({
      ok: true,
      kept: '/a.mp3',
      renamed: false,
      final_name: 'a.mp3',
      deleted: []
    }),
    chat: async () => ({ text: 'hola', tools: [], actions: [], confirm: null }),
    chatStart: async () => ({ id: 'job1' }),
    chatPoll: async () => ({
      text: 'hola',
      tools: [],
      done: true,
      result: { text: 'hola', tools: [], actions: [], confirm: null }
    }),
    chatCancel: async () => ({ ok: true }),
    chats: async () => ({
      chats: state.chats.map((c) => ({
        id: c.id,
        title: c.title,
        updated: c.updated,
        n: c.messages.length
      }))
    }),
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
      if (!c.title)
        c.title = (messages.find((m) => m.role === 'me' && !m.hidden)?.text || '').slice(0, 60)
      return { n: messages.length }
    },
    chatRename: async (id, title) => {
      const c = state.chats.find((x) => x.id === id)
      if (c) c.title = title
      return { ok: true }
    },
    chatDelete: async (id) => {
      state.chats = state.chats.filter((x) => x.id !== id)
      return { ok: true }
    },
    chatSearch: async (q) => ({
      hits: state.chats.flatMap((c) =>
        c.messages
          .filter((m) => (m.text || '').toLowerCase().includes(q.toLowerCase()))
          .map((m) => ({ chat_id: c.id, title: c.title, role: m.role, snippet: m.text, at: 0 }))
      )
    }),
    aiUsage: async () => ({
      today: { calls: 2, prompt: 3000, completion: 400, cost: 0.0021, unpriced: 0 },
      month: { calls: 20, prompt: 30000, completion: 4000, cost: 0.021, unpriced: 0 },
      total: { calls: 20, prompt: 30000, completion: 4000, cost: 0.021, unpriced: 0 },
      by_provider: [{ provider: 'openai', calls: 20, tokens: 34000, cost: 0.021 }],
      budget: 0,
      over_budget: false
    }),
    aiFallback: async (enabled) => {
      state.aiFallback = enabled
      return aiOverview(state)
    },
    aiBudget: async (dollars) => ({
      today: { calls: 0, prompt: 0, completion: 0, cost: 0, unpriced: 0 },
      month: { calls: 20, prompt: 30000, completion: 4000, cost: 0.021, unpriced: 0 },
      total: { calls: 20, prompt: 30000, completion: 4000, cost: 0.021, unpriced: 0 },
      by_provider: [],
      budget: dollars,
      over_budget: dollars > 0 && 0.021 > dollars
    }),
    chatExport: async (id) => ({ markdown: `# conversacion ${id}\n` }),
    /** una onda de mentira: callada al principio, fuerte al final */
    waveform: async (id, buckets = 800) => {
      const n = Math.max(1, Math.min(buckets, 4000))
      const peaks = Array.from({ length: n }, (_, i) => (i < n / 2 ? 0.1 : 1))
      return { peaks, rms: peaks.map((p) => p * 0.6), buckets: n }
    },
    setStudy: async (id, study) => {
      const s = find(id)
      const raw = study && Object.keys(study).length ? JSON.stringify(study) : ''
      if (s) s.study = raw
      return s ? { ...s } : null
    },
    playlistSheet: async (id) => ({ file: `/musica/Listas/lista-${id}.html` }),
    separation: async () => copy(state.separation),
    // separar tarda: aqui la cancion se queda en marcha hasta que la prueba
    // la acabe (`finishSeparation`)
    separate: async (id) => {
      const s = find(id)
      if (!s) throw new Error('esa canción ya no está en la biblioteca')
      const quick = state.stems[id]?.quality === 'rapida'
      const stage = quick ? 'refine' : 'separate'
      const item = { id, title: `${s.artist} - ${s.title}`, stage, done: 0, total: 0 }
      if (!state.separation.current) state.separation.current = item
      else state.separation.queue.push(item)
      state.jobs.separacion = { ...finishedJob('separacion', null), active: true }
      return { ...copy(state.separation), job: copy(state.jobs.separacion) }
    },
    cancelSeparation: async () => {
      state.separation.current = null
      state.separation.queue = []
      state.jobs.separacion = finishedJob('separacion', null, { error: 'cancelado' })
      return copy(state.separation)
    },
    unqueueSeparation: async (id) => {
      state.separation.queue = state.separation.queue.filter((q) => q.id !== id)
      return copy(state.separation)
    },
    removeSeparator: async () => {
      state.separation.pending = state.separation.bytes
      state.separation.installed = false
      return { removed: 4, ...copy(state.separation) }
    },
    stems: async (id) => {
      const st = state.stems[id]
      if (!st) {
        const e = new Error('esta canción no tiene pistas separadas')
        // @ts-ignore el 404 del nucleo
        e.status = 404
        throw e
      }
      return copy(st)
    },
    deleteStems: async (id) => {
      delete state.stems[id]
      const s = find(id)
      if (s) s.has_stems = false
      return { ok: true, folder: `/musica/Separadas/cancion-${id}` }
    },
    exportMix: async (id, d) =>
      start('mezcla', {
        path: d.path,
        name: d.path.split('/').pop(),
        id: 99,
        title: 'mezcla'
      }),
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
      key === 'coverUrl' || key === 'audioUrl' || key === 'coverUrlAlt'
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
  // `runJob` es el de verdad (arrancar, preguntar hasta que acaba, devolver
  // el resultado), pero preguntando al doble y sin esperar entre preguntas.
  double.runJob = vi.fn((start, name, options = {}) =>
    actual.runJob(start, name, { interval: 0, ...options, poll: (n) => double.job(n) })
  )
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
    revision: 0,
    pitch: 0,
    path: '',
    stems: false,
    metronome: {
      on: false,
      bpm: 100,
      meter: 4,
      shift: 0,
      mult: 0,
      volume: 0.8,
      has_grid: false,
      free: true,
      confidence: 0
    }
  }
  let items = []
  let origin = null
  /** lo que dio el último análisis del compás (lo que Rust guarda por ruta) */
  let grid = { bpm: 120, meter: 4 }

  // Cada emision es una copia entera y nueva, como la que llega de Rust (JSON
  // por el puente): con `{ ...state }` los objetos de dentro (`track`,
  // `origin`, el metronomo) eran siempre los mismos, y las pruebas no veian
  // que la interfaz se repintaba entera en cada tick.
  const push = () => {
    const snapshot = JSON.parse(JSON.stringify(state))
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
    // como Rust: uno solo es el bucle A-B; con `segments`, varios en orden
    setLoop: vi.fn(async (a, b, opts) => {
      const loops = opts?.segments ?? (a != null && b != null && b > a ? [[a, b]] : [])
      emit({
        loop_a: loops[0]?.[0] ?? 0,
        loop_b: loops.at(-1)?.[1] ?? 0,
        loops,
        loop_defer: !!opts?.defer && loops.length > 0
      })
    }),
    setPitch: vi.fn(async (semitones) => emit({ pitch: semitones })),
    // como Rust: las pistas eran para la cancion que suena; si ya es otra, nada
    setStems: vi.fn(async (song, tracks) => {
      if (song !== state.path) return
      emit({ stems: !!tracks?.length })
    }),
    // como Rust: el tempo es el puesto a mano o el de la rejilla, y el doble
    // o la mitad valen para los dos
    setMetronome: vi.fn(async (settings) => {
      const factor = settings.mult === 1 ? 2 : settings.mult === -1 ? 0.5 : 1
      const hasGrid = state.metronome.has_grid
      emit({
        metronome: {
          ...state.metronome,
          ...settings,
          bpm: (settings.bpm ?? (hasGrid ? grid.bpm : 100)) * factor,
          meter: settings.meter ?? (hasGrid ? grid.meter : 4),
          free: settings.bpm != null || !hasGrid
        }
      })
    }),
    /** una rejilla de mentira: 120 bpm en 4/4 desde 0,25 s, con la confianza que diga el estado */
    analyzeBeats: vi.fn(async (path) => {
      grid = { bpm: 120, meter: 4 }
      emit({
        metronome: { ...state.metronome, has_grid: true, bpm: 120, meter: 4, confidence: 0.8 }
      })
      return {
        bpm: 120,
        meter: 4,
        beats: Array.from({ length: 400 }, (_, i) => 0.25 + i * 0.5),
        first_downbeat: 0,
        phase3: 0,
        phase4: 0,
        confidence: 0.8,
        path
      }
    }),
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
