import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

const { estadoFalso, status, api, native, pickFolder, tray } = vi.hoisted(() => {
  const estadoFalso = { configured: true, folders: 1, stats: { total: 3, bytes: 0, seconds: 0 },
    model: 'x', ia: false, fingerprint: false, rust: true, ffmpeg: true,
    never_convert: [], convert: false, quality: 'high', tareas: {} }
  const status = { songs: [], playlists: [] }
  const api = {
    inTauri: false,
    status: vi.fn(async () => JSON.parse(JSON.stringify(estadoFalso))),
    inbox: vi.fn(async () => ({ total: 0, files: [] })),
    playlists: vi.fn(async () => ({ playlists: status.playlists, favoritos: 0 })),
    search: vi.fn(async () => ({ total: status.songs.length,
                                 songs: status.songs.map(c => ({ ...c })) })),
    playlistSongs: vi.fn(async () => ({ songs: status.songs.map(c => ({ ...c })) })),
    duplicates: vi.fn(async () => ({ identical: [], similar: [] })),
    song: vi.fn(async (id) => status.songs.find(c => c.id === id) || null),
    coverUrl: () => '/x.jpg', audioUrl: () => '/x.mp3',
    path: vi.fn(async () => ({ path: '/x.mp3' })),
    setStars: vi.fn(async () => ({})), toggleFavorite: vi.fn(async () => ({})),
    createPlaylist: vi.fn(async () => ({ id: 1 })), deletePlaylist: vi.fn(async () => ({})),
    addFolder: vi.fn(async () => ({ action: 'agregada', folders: [] })),
    scan: vi.fn(async () => ({})), runImport: vi.fn(async () => ({ results: [] })),
    settings: vi.fn(async () => ({})), folders: vi.fn(async () => ({ folders: [], exclusions: [], always_excluded: [] })),
    convertible: vi.fn(async () => ({ total: 0, files: [], protected: [] })),
    saveSettings: vi.fn(async (d) => d), details: vi.fn(async () => ({})),
    enrich: vi.fn(async () => ({})), transpose: vi.fn(async () => ({})),
    resolverDuplicado: vi.fn(async () => ({ ok: true })),
    chat: vi.fn(async () => ({ text: '' })), chatTools: vi.fn(async () => ({ model: 'x' }))
  }
  const tray = {
    available: false,
    setNowPlaying: vi.fn(async () => {}), nowPlaying: vi.fn(async () => null),
    openMini: vi.fn(async () => {}), closeMini: vi.fn(async () => {}),
    showApp: vi.fn(async () => {}), send: vi.fn(async () => {}),
    onCommand: vi.fn(async () => () => {}), onChanged: vi.fn(async () => () => {})
  }
  return { estadoFalso, status, api, native: { available: false },
           pickFolder: vi.fn(async () => null), tray }
})
vi.mock('../src/api.js', () => ({ api, native, pickFolder, tray }))
import App from '../src/App.vue'

const theme = (n, i) => ({ id: i + 1, title: 'Tema ' + (i + 1), artist: 'Artista ' + (i + 1),
  album: '', duration: 100 + i, bitrate: 128000, setStars: 0, toggleFavorite: 0, feat: '',
  folder: 'x', key: '', bpm: 0 })

beforeEach(() => {
  status.songs = Array.from({ length: 6 }, (_, i) => theme(6, i))
  status.playlists = []
  estadoFalso.configured = true
  estadoFalso.stats.total = 6
  localStorage.clear()
  vi.clearAllMocks()
})

const montar = async () => {
  const w = mount(App, { attachTo: document.body })
  await flushPromises(); await flushPromises()
  return w
}
const fueraClic = async () => {
  document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  await flushPromises()
}

describe('la lista completa sigue visible al reproducir', () => {
  it('poner una cancion no reduce la lista', async () => {
    const w = await montar()
    expect(w.findAll('tbody tr')).toHaveLength(6)
    await w.findAll('tbody tr')[2].trigger('dblclick')
    await flushPromises()
    expect(w.findAll('tbody tr'), 'la lista se encogio al reproducir').toHaveLength(6)
  })

  it('"Ver todo" desde la cola devuelve la lista entera', async () => {
    const w = await montar()
    await w.findAll('tbody tr')[1].trigger('dblclick')
    await flushPromises()
    // abrir la cola
    const queueButton = w.findAll('.player .pl-btn').at(-1)
    await queueButton.trigger('click')
    await flushPromises()
    expect(w.findAll('.queue-row')).toHaveLength(3)      // antes, ahora, luego
    await w.find('.queue-more').trigger('click')
    await flushPromises(); await flushPromises()
    expect(w.findAll('tbody tr'), 'no volvio la lista completa').toHaveLength(6)
  })

  it('en un repertorio tambien se ven todas', async () => {
    status.playlists = [{ id: 7, name: 'Domingo', n: 6, seconds: 600 }]
    const w = await montar()
    await w.findAll('.nav-link').find(e => e.text().includes('Domingo')).trigger('click')
    await flushPromises(); await flushPromises()
    expect(w.findAll('tbody tr')).toHaveLength(6)
    await w.findAll('tbody tr')[0].trigger('dblclick')
    await flushPromises()
    expect(w.findAll('tbody tr')).toHaveLength(6)
  })
})

// Estaba todo revuelto: el tema entre «agrupar» y «densidad». Ahora va en dos
// grupos, lo que cambia la lista que ves y lo que cambia la app entera.
describe('el menu de Vista', () => {
  const abrir = async (w) => {
    await w.findAll('button').find(b => b.text().includes('Vista')).trigger('click')
    await flushPromises()
  }

  it('separa lo de la lista de lo de la app', async () => {
    const w = await montar()
    await abrir(w)
    const grupos = w.findAll('.view-menu .menu-block')
    expect(grupos).toHaveLength(2)
    expect(grupos[0].text()).toContain('Agrupar')
    expect(grupos[0].text()).toContain('Densidad')
    expect(grupos[1].text()).toContain('Tema')
    expect(grupos[1].text()).toContain('Tamaño de la app')
    expect(grupos[0].text(), 'el tema no pinta nada entre lo de la lista')
      .not.toContain('Tema')
  })

  it('el mini reproductor solo se ofrece dentro de la app de escritorio', async () => {
    const w = await montar()          // native.available es false en las pruebas
    await abrir(w)
    expect(w.find('.mini-open').exists()).toBe(false)
  })
})

describe('los menus se cierran al pulsar fuera', () => {
  it('el menu de Vista', async () => {
    const w = await montar()
    await w.findAll('button').find(b => b.text().includes('Vista')).trigger('click')
    await flushPromises()
    expect(w.findAll('.view-menu')).toHaveLength(1)
    await fueraClic()
    expect(w.findAll('.view-menu'), 'el menu de Vista sigue abierto').toHaveLength(0)
  })

  it('la cola del reproductor', async () => {
    const w = await montar()
    await w.findAll('tbody tr')[0].trigger('dblclick')
    await flushPromises()
    await w.findAll('.player .pl-btn').at(-1).trigger('click')
    await flushPromises()
    expect(w.findAll('.queue')).toHaveLength(1)
    await fueraClic()
    expect(w.findAll('.queue'), 'la cola sigue abierta').toHaveLength(0)
  })

  it('el desplegable de un selector', async () => {
    const w = await montar()
    await w.findAll('button').find(b => b.text().includes('Vista')).trigger('click')
    await flushPromises()
    await w.findAll('.select-box')[0].trigger('click')
    await flushPromises()
    expect(w.findAll('.select-menu')).toHaveLength(1)
    await fueraClic()
    expect(w.findAll('.select-menu')).toHaveLength(0)
  })
})
