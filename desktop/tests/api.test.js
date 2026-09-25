import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import {
  api,
  core,
  runJob,
  JOBS,
  WATCH_MS,
  ApiError,
  errorMessage,
  fromBridge,
  mediaUrl
} from '../src/api.js'

// La capa de transporte en modo navegador (npm run dev): lo que se manda al
// nucleo por el proxy de Vite, las tareas largas y la vigilancia de cambios
// que en la app hace Rust.
let calls
function respond(fn) {
  globalThis.fetch = vi.fn(async (url, init = {}) => {
    calls.push({ url, ...init })
    const { status = 200, body = {} } = (await fn(url, init)) || {}
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => body,
      text: async () => (typeof body === 'string' ? body : JSON.stringify(body))
    }
  })
}
beforeEach(() => {
  calls = []
  respond(() => ({ body: {} }))
})
afterEach(() => {
  vi.useRealTimers()
  delete globalThis.fetch
})

describe('las peticiones del navegador', () => {
  it('todas llevan X-DanPlay, tambien los GET (los hay que gastan IA)', async () => {
    await api.status()
    await api.details(3)
    await api.setStars(3, 4)
    for (const c of calls) expect(c.headers['X-DanPlay'], c.url).toBe('1')
    expect(calls.map((c) => c.method)).toEqual(['GET', 'GET', 'POST'])
  })

  it('el tipo de contenido solo va cuando hay cuerpo', async () => {
    await api.status()
    await api.setStars(3, 4)
    expect(calls[0].headers['Content-Type']).toBeUndefined()
    expect(calls[0].body).toBeUndefined()
    expect(calls[1].headers['Content-Type']).toBe('application/json')
    expect(JSON.parse(calls[1].body)).toEqual({ stars: 4 })
  })

  it('los medios los pide el propio navegador, sin pasar por aqui', () => {
    expect(api.audioUrl(7)).toBe('/api/song/7/audio')
    expect(mediaUrl('cover', 7, 'size=96')).toBe('/api/song/7/cover?size=96')
  })

  it('un error del nucleo llega como su «detail», con su codigo', async () => {
    respond(() => ({ status: 409, body: { detail: 'ya hay una descarga en marcha' } }))
    const e = await api.youtubeDownload({ query: 'x' }).catch((err) => err)
    expect(e).toBeInstanceOf(ApiError)
    expect(e.status).toBe(409)
    expect(errorMessage(e)).toBe('ya hay una descarga en marcha')
    respond(() => ({ status: 500, body: '' }))
    expect(errorMessage(await api.status().catch((err) => err))).toBe('HTTP 500')
  })

  it('la busqueda manda la consulta, el limite y desde cual', async () => {
    await api.search({ q: 'barak', limit: 400, from_key: 400 })
    const url = new URL(calls[0].url, 'http://x')
    expect(url.pathname).toBe('/api/search')
    expect(Object.fromEntries(url.searchParams)).toEqual({
      q: 'barak',
      limit: '400',
      from_key: '400'
    })
  })

  it('los errores del puente de Rust se leen igual', () => {
    const e = fromBridge('404: {"detail":"no existe esa cancion"}')
    expect(e.status).toBe(404)
    expect(e.message).toBe('no existe esa cancion')
    expect(String(e)).toBe('no existe esa cancion')
  })
})

describe('las tareas largas (runJob)', () => {
  const job = (extra) => ({
    name: 'escaneo',
    active: true,
    done: 0,
    total: 0,
    message: '',
    result: null,
    error: '',
    ...extra
  })

  it('arranca, pregunta hasta que acaba, cuenta el avance y devuelve el resultado', async () => {
    const seen = []
    const polls = [
      job({ done: 5, total: 10 }),
      job({ active: false, done: 10, total: 10, result: { total: 10 } })
    ]
    const poll = vi.fn(async () => polls.shift())
    const r = await runJob(async () => ({ job: job({ done: 1, total: 10 }) }), JOBS.scan, {
      onProgress: (j) => seen.push(j.done),
      interval: 0,
      poll
    })
    expect(r).toEqual({ total: 10 })
    expect(seen).toEqual([1, 5, 10])
    expect(poll).toHaveBeenCalledWith('escaneo')
  })

  it('si ya habia una en marcha, sigue esa', async () => {
    const poll = vi.fn(async () => job({ active: false, result: 'la de antes' }))
    const r = await runJob(
      async () => ({ job: job({ done: 3 }), already_running: true }),
      'escaneo',
      { interval: 0, poll }
    )
    expect(r).toBe('la de antes')
  })

  it('un error de la tarea llega en castellano', async () => {
    const e = await runJob(
      async () => ({ job: job({ active: false, error: 'la carpeta ya no existe' }) }),
      'escaneo',
      { interval: 0 }
    ).catch((err) => err)
    expect(e).toBeInstanceOf(ApiError)
    expect(e.message).toBe('la carpeta ya no existe')
  })

  it('un fallo suelto al preguntar no la tira; tres seguidos, si', async () => {
    let n = 0
    const flaky = vi.fn(async () => {
      if (++n === 1) throw new Error('el nucleo tarda')
      return job({ active: false, result: 'ok' })
    })
    expect(
      await runJob(async () => ({ job: job() }), 'escaneo', { interval: 0, poll: flaky })
    ).toBe('ok')
    const muerto = vi.fn(async () => {
      throw new Error('sin nucleo')
    })
    const e = await runJob(async () => ({ job: job() }), 'escaneo', {
      interval: 0,
      poll: muerto
    }).catch((err) => err)
    expect(e.message).toBe('sin nucleo')
    expect(muerto).toHaveBeenCalledTimes(3)
  })

  it('sin instantanea en la respuesta, pregunta enseguida', async () => {
    const poll = vi.fn(async () => job({ active: false, result: 1 }))
    expect(await runJob(async () => ({}), 'conversion', { interval: 0, poll })).toBe(1)
  })

  it('por defecto pregunta a /api/jobs/{nombre}', async () => {
    respond((url) => ({
      body: url.includes('/jobs/') ? job({ active: false, result: 'hecho' }) : {}
    }))
    expect(await api.runJob(() => api.scan(), JOBS.scan, { interval: 0 })).toBe('hecho')
    expect(calls.map((c) => c.url)).toEqual(['/api/scan', '/api/jobs/escaneo'])
  })
})

describe('en el navegador, la interfaz se entera de los cambios del nucleo', () => {
  it('mira /api/status cada dos segundos y avisa cuando cambia `revision`', async () => {
    vi.useFakeTimers()
    let revision = 3
    respond(() => ({ body: { revision } }))
    const changed = vi.fn()
    const stop = await core.onChanged(changed)
    await vi.advanceTimersByTimeAsync(10) // la primera lectura fija el punto de partida
    expect(changed).not.toHaveBeenCalled()
    revision = 4 // el vigilante de carpetas cambio algo
    await vi.advanceTimersByTimeAsync(WATCH_MS + 10)
    expect(changed).toHaveBeenCalledWith({ revision: 4 })
    await vi.advanceTimersByTimeAsync(WATCH_MS * 3)
    expect(changed).toHaveBeenCalledTimes(1) // sin cambios, sin avisos
    stop()
    const antes = calls.length
    await vi.advanceTimersByTimeAsync(WATCH_MS * 3)
    expect(calls.length).toBe(antes) // sin nadie escuchando, no pregunta
  })

  it('avisa cuando el nucleo pasa de no contestar a contestar', async () => {
    vi.useFakeTimers()
    let vivo = false
    respond(() => (vivo ? { body: { revision: 1 } } : { status: 502, body: 'Bad Gateway' }))
    const status = vi.fn()
    const stop = await core.onStatus(status)
    await vi.advanceTimersByTimeAsync(10)
    expect(status).not.toHaveBeenCalled()
    vivo = true
    await vi.advanceTimersByTimeAsync(WATCH_MS + 10)
    expect(status).toHaveBeenCalledWith({ ready: true, message: '' })
    vivo = false
    await vi.advanceTimersByTimeAsync(WATCH_MS + 10)
    expect(status).toHaveBeenLastCalledWith({ ready: false, message: 'el núcleo no contesta' })
    stop()
  })
})

// ---------------------------------------------------------------------------
// Que llama cada metodo del `api`: el verbo, la ruta y el cuerpo. Es el
// contrato con el nucleo visto desde aqui (las rutas estan en
// docs/CONTRATO-INTERNO.md y en danplay/api.py). Si un nombre cambia en un
// lado y no en el otro, esta tabla lo dice; y si se añade un metodo sin
// ponerlo aqui, tambien.
describe('lo que cada metodo pide al nucleo', () => {
  const ai = { provider: 'openai', model: 'chico' }
  /** metodo -> [argumentos, verbo, ruta, cuerpo (undefined si no lleva)] */
  const TABLA = {
    path: [[7], 'GET', '/api/song/7/path'],
    status: [[], 'GET', '/api/status'],
    settings: [[], 'GET', '/api/settings'],
    checkAi: [[], 'POST', '/api/settings/check-ai'],
    saveSettings: [[{ quality: 'high' }], 'POST', '/api/settings', { quality: 'high' }],
    aiProviders: [[true], 'GET', '/api/ai/providers?refresh=1'],
    aiSaveProfile: [[ai], 'POST', '/api/ai/profile', ai],
    aiDeleteProfile: [['custom-la iglesia'], 'DELETE', '/api/ai/profile/custom-la%20iglesia'],
    aiActivate: [['ollama'], 'POST', '/api/ai/activate', { id: 'ollama' }],
    aiUsage: [[], 'GET', '/api/ai/usage'],
    aiFallback: [[false], 'POST', '/api/ai/fallback', { enabled: false }],
    aiBudget: [[5], 'POST', '/api/ai/budget', { dollars: 5 }],
    aiCheck: [[ai], 'POST', '/api/ai/check', ai],
    aiFree: [[], 'POST', '/api/ai/free'],
    aiModels: [[ai], 'POST', '/api/ai/models', ai],
    folders: [[], 'GET', '/api/folders'],
    checkFolder: [['/m'], 'POST', '/api/check-folder', { path: '/m' }],
    addFolder: [
      ['/m', 'Musica', true],
      'POST',
      '/api/folders',
      { path: '/m', label: 'Musica', force: true }
    ],
    removeFolder: [['/m/a b'], 'DELETE', '/api/folders?path=%2Fm%2Fa%20b'],
    relocateFolder: [['/a', '/b'], 'POST', '/api/folders/relocate', { from: '/a', to: '/b' }],
    addExclusion: [
      ['*/Copias/*'],
      'POST',
      '/api/exclusions',
      { pattern: '*/Copias/*', kind: 'glob', note: '' }
    ],
    removeExclusion: [['Secuencias'], 'DELETE', '/api/exclusions?pattern=Secuencias'],
    job: [['yt-dlp'], 'GET', '/api/jobs/yt-dlp'],
    scan: [[], 'POST', '/api/scan'],
    search: [[{ q: 'barak', limit: 10 }], 'GET', '/api/search?q=barak&limit=10'],
    quickSearch: [['mi gozo'], 'GET', '/api/search?q=mi+gozo&limit=40'],
    facets: [[], 'GET', '/api/facets'],
    song: [[7], 'GET', '/api/song/7'],
    edit: [[7, { title: 'X' }], 'PATCH', '/api/song/7', { title: 'X' }],
    setStars: [[7, 4], 'POST', '/api/song/7/stars', { stars: 4 }],
    toggleFavorite: [[7, true], 'POST', '/api/song/7/favorite', { favorite: true }],
    setBlur: [[7, false], 'POST', '/api/song/7/blur', { blur: false }],
    details: [[7], 'GET', '/api/song/7/details'],
    enrich: [[7, { lyrics: true }], 'POST', '/api/song/7/enrich', { lyrics: true }],
    autofill: [[7], 'POST', '/api/song/7/autofill'],
    setCover: [[7, '/img.jpg'], 'POST', '/api/song/7/cover', { path: '/img.jpg' }],
    transpose: [[{ text: 'G', to_key: 'A' }], 'POST', '/api/transpose', { text: 'G', to_key: 'A' }],
    deleteSong: [[7], 'DELETE', '/api/song/7'],
    playlists: [[], 'GET', '/api/playlists'],
    createPlaylist: [['Domingo'], 'POST', '/api/playlists', { name: 'Domingo' }],
    deletePlaylist: [[3], 'DELETE', '/api/playlists/3'],
    editPlaylist: [[3, { name: 'X' }], 'PATCH', '/api/playlists/3', { name: 'X' }],
    playlistSongs: [[3], 'GET', '/api/playlists/3/songs'],
    addToPlaylist: [[3, [1, 2]], 'POST', '/api/playlists/3/songs', { ids: [1, 2] }],
    removeFromPlaylist: [[3, 7], 'DELETE', '/api/playlists/3/songs/7'],
    reorderPlaylist: [[3, [2, 1]], 'POST', '/api/playlists/3/order', { ids: [2, 1] }],
    setStudy: [[7, { notes: 'x' }], 'PUT', '/api/song/7/study', { notes: 'x' }],
    waveform: [[7], 'GET', '/api/song/7/waveform?buckets=800'],
    playlistSheet: [[3, true], 'POST', '/api/playlists/3/sheet', { with_lyrics: true }],
    exportPlaylist: [[3], 'POST', '/api/playlists/3/export'],
    externalList: [[], 'GET', '/api/external'],
    externalPlayed: [['/a.mp3'], 'POST', '/api/external/play', { path: '/a.mp3' }],
    externalClear: [[], 'DELETE', '/api/external'],
    externalForget: [[-2], 'DELETE', '/api/external/-2'],
    externalSave: [['Del reproductor'], 'POST', '/api/external/save', { name: 'Del reproductor' }],
    inbox: [[], 'GET', '/api/inbox'],
    youtube: [[], 'GET', '/api/youtube'],
    youtubeUpdate: [[], 'POST', '/api/youtube/update'],
    youtubeInfo: [['barak', 3], 'POST', '/api/youtube/info', { query: 'barak', results: 3 }],
    youtubeDownload: [[{ query: 'x' }], 'POST', '/api/youtube/download', { query: 'x' }],
    youtubeCancel: [[], 'POST', '/api/youtube/cancel'],
    downloadHistory: [[30, 60], 'GET', '/api/downloads/history?limit=30&offset=60'],
    clearDownloadHistory: [[], 'DELETE', '/api/downloads/history'],
    runImport: [[{ dry_run: true }], 'POST', '/api/import', { dry_run: true }],
    convertible: [[], 'GET', '/api/convertible'],
    convert: [
      [{ dry_run: false, quality: 'high', keep: true }],
      'POST',
      '/api/convert',
      { dry_run: false, quality: 'high', keep: true }
    ],
    duplicatesScan: [[], 'POST', '/api/duplicates/scan'],
    chat: [
      [[{ role: 'me', text: 'hola' }]],
      'POST',
      '/api/chat',
      { messages: [{ role: 'me', text: 'hola' }] }
    ],
    chatStart: [
      [[], { total: 3 }],
      'POST',
      '/api/chat/start',
      { messages: [], context: { total: 3 } }
    ],
    chatPoll: [['a/b'], 'GET', '/api/chat/poll/a%2Fb'],
    chatCancel: [['j1'], 'POST', '/api/chat/cancel/j1'],
    chats: [[], 'GET', '/api/chats'],
    chatCreate: [['Set'], 'POST', '/api/chats', { title: 'Set' }],
    chatGet: [[4], 'GET', '/api/chats/4'],
    chatAppend: [
      [4, [{ role: 'me', text: 'x' }]],
      'POST',
      '/api/chats/4/messages',
      { messages: [{ role: 'me', text: 'x' }] }
    ],
    chatRename: [[4, 'Otro'], 'PATCH', '/api/chats/4', { title: 'Otro' }],
    chatDelete: [[4], 'DELETE', '/api/chats/4'],
    chatSearch: [['mi gozo'], 'GET', '/api/chats/search?q=mi%20gozo'],
    chatExport: [[4], 'GET', '/api/chats/4/export'],
    chatTools: [[], 'GET', '/api/chat/tools'],
    chatConfirm: [
      ['delete_song', { id: 7 }],
      'POST',
      '/api/chat/confirm',
      { tool: 'delete_song', args: { id: 7 } }
    ],
    resolveDuplicate: [
      ['/a.mp3', ['/b.mp3']],
      'POST',
      '/api/duplicates/resolve',
      { keep: '/a.mp3', remove: ['/b.mp3'], dry_run: false }
    ]
  }
  // no hablan con el nucleo por aqui: el escuchador de Rust, las URL de los
  // medios (las pide el navegador) y `runJob`, que ya tiene sus pruebas
  const APARTE = ['inTauri', 'onExternal', 'runJob', 'audioUrl', 'coverUrl', 'coverUrlAlt']

  it('no se queda ningun metodo sin mirar', () => {
    const sinMirar = Object.keys(api).filter((k) => !(k in TABLA) && !APARTE.includes(k))
    expect(sinMirar, 'metodos del api que la tabla no conoce').toEqual([])
  })

  for (const [metodo, [args, verbo, ruta, cuerpo]] of Object.entries(TABLA)) {
    it(`${metodo} → ${verbo} ${ruta}`, async () => {
      await api[metodo](...args)
      expect(calls).toHaveLength(1)
      expect(calls[0].method).toBe(verbo)
      expect(calls[0].url).toBe(ruta)
      expect(calls[0].body === undefined ? undefined : JSON.parse(calls[0].body)).toEqual(cuerpo)
    })
  }

  it('fuera de la app, lo que solo existe dentro dice que no, sin romper nada', async () => {
    const { app, projection, mini, playback } = await import('../src/api.js')
    expect(playback.available).toBe(false)
    await expect(app.revealInFolder('/a.mp3')).rejects.toThrow(
      'Solo en la aplicación de escritorio'
    )
    await expect(app.openHtml('/a.html')).rejects.toThrow('Solo en la aplicación de escritorio')
    await expect(app.sendToTelegram(['/a.mp3'])).rejects.toThrow(
      'Solo en la aplicación de escritorio'
    )
    expect(await app.shareTargets()).toEqual({ telegram: false })
    expect(await app.trayAvailable()).toBe(false)
    expect((await app.defaultPlayer()).supported).toBe(false)
    expect(await app.makeDefaultPlayer()).toBe(null)
    await app.showWindow()
    await app.quit()
    await mini.hide()
    await mini.toggle()
    const stop = await mini.onVisible(() => {})
    stop()
    expect(await projection.isFullscreen()).toBe(false)
    await projection.fullscreen(true)
    // abrir la proyeccion en el navegador es otra pestaña
    const abierta = vi.fn(() => ({}))
    vi.stubGlobal('open', abierta)
    await projection.show()
    expect(abierta).toHaveBeenCalledWith('/?projection=1', 'danplay-projection')
    await app.openInBrowser('https://danplay.local')
    expect(abierta).toHaveBeenLastCalledWith('https://danplay.local', '_blank')
    vi.unstubAllGlobals()
    expect(calls).toEqual([])
  })
})
