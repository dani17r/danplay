import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// --- doble del backend (vi.hoisted porque vi.mock se iza) -----------------
const { estadoFalso, status, api, native, pickFolder, tray, bandeja } = vi.hoisted(() => {
const estadoFalso = {
  configured: false, folders: 0, stats: { total: 0, bytes: 0, seconds: 0 },
  model: 'x', ia: false, fingerprint: false, rust: true, ffmpeg: true,
  never_convert: ['Secuencias'], convert: false, quality: 'high', tareas: {}
}
const status = { songs: [], yaAñadidas: [], playlists: [] }

const api = {
  inTauri: false,
  status: vi.fn(async () => JSON.parse(JSON.stringify(estadoFalso))),
  inbox: vi.fn(async () => ({ total: 0, files: [] })),
  playlists: vi.fn(async () => ({ playlists: status.playlists.map(l => ({ ...l })), favoritos: 0 })),
  search: vi.fn(async () => ({ total: status.songs.length, songs: status.songs.map(c => ({ ...c })) })),
  playlistSongs: vi.fn(async () => ({ songs: [] })),
  duplicates: vi.fn(async () => ({ identical: [], similar: [] })),
  song: vi.fn(async (id) => status.songs.find(c => c.id === id) || null),
  // Las acciones son las que devuelve el nucleo de verdad (ver
  // tests/test_api.py): added | already_there | replaced | confirm. Este
  // remedo las tenia en castellano, del esquema anterior, asi que la prueba
  // pasaba contra un contrato que ya no existia y no vio que los avisos de
  // «esa carpeta ya estaba» habian dejado de salir en la app.
  addFolder: vi.fn(async (path, label, force) => {
    if (status.yaAñadidas.includes(path) && !force) {
      return { action: 'already_there', notice: { kind: 'same', message: 'esa carpeta ya esta añadida' }, folders: [] }
    }
    status.yaAñadidas.push(path)
    estadoFalso.configured = true
    return { action: 'added', notice: null, folders: [] }
  }),
  scan: vi.fn(async () => {
    status.songs = [
      { id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200, bitrate: 128000, stars: 0, favorite: 0, feat: '', folder: 'Artistas/Barak', key: '', bpm: 0 },
      { id: 2, title: 'Shekinah', artist: 'Barak', album: '', duration: 300, bitrate: 320000, stars: 0, favorite: 0, feat: '', folder: 'Artistas/Barak', key: '', bpm: 0 }
    ]
    estadoFalso.stats.total = status.songs.length
    return { tarea: {}, stats: estadoFalso.stats }
  }),
  setStars: vi.fn(async (id, n) => ({ ...status.songs.find(c => c.id === id), stars: n })),
  toggleFavorite: vi.fn(async (id, v) => ({ ...status.songs.find(c => c.id === id), favorite: v ? 1 : 0 })),
  coverUrl: () => '/x.jpg',
  audioUrl: () => '/x.mp3',
  path: vi.fn(async () => ({ path: '/x.mp3' })),
  createPlaylist: vi.fn(async () => ({ id: 1 })),
  addToPlaylist: vi.fn(async () => ({ added: 1 })),
  removeFromPlaylist: vi.fn(async () => ({})),
  deletePlaylist: vi.fn(async () => ({})),
  folders: vi.fn(async () => ({ folders: [], exclusions: [], always_excluded: [] })),
  convertible: vi.fn(async () => ({ total: 0, files: [], protected: [] })),
  settings: vi.fn(async () => ({ convert_mp3: false, quality: 'high', keep_original: false,
    write_tags: true, ai_enabled: true, model: 'x', library: '/m',
    ai_key: '', has_ai_key: false, fingerprint_key: false, settings_file: '/a' })),
  saveSettings: vi.fn(async (d) => d),
  runImport: vi.fn(async () => ({ results: [] })),
  quitarCarpeta: vi.fn(async () => ({ folders: [], exclusions: [], always_excluded: [] })),
  agregarExclusion: vi.fn(async () => ({ folders: [], exclusions: [], always_excluded: [] })),
  quitarExclusion: vi.fn(async () => ({ folders: [], exclusions: [], always_excluded: [] })),
  details: vi.fn(async () => ({ details: null })),
  enrich: vi.fn(async () => ({ song: {} })),
  transpose: vi.fn(async () => ({ text: '', latin: '', capo: [] }))
}
const native = { available: false }
const pickFolder = vi.fn(async () => null)
// para poder disparar a mano lo que llega del menu de la bandeja
const bandeja = { orden: null }
const tray = {
  available: false,
  setNowPlaying: vi.fn(async () => {}),
  nowPlaying: vi.fn(async () => null),
  openMini: vi.fn(async () => {}),
  closeMini: vi.fn(async () => {}),
  showApp: vi.fn(async () => {}),
  send: vi.fn(async () => {}),
  onCommand: vi.fn(async (fn) => { bandeja.orden = fn; return () => {} }),
  onChanged: vi.fn(async () => () => {})
}
return { estadoFalso, status, api, native, pickFolder, tray, bandeja }
})

vi.mock('../src/api.js', () => ({ api, native, pickFolder, tray }))

import App from '../src/App.vue'
import { cancelDrag } from '../src/composables/useDragSong.js'

beforeEach(() => {
  status.songs = []
  status.yaAñadidas = []
  status.playlists = []
  cancelDrag()          // que un arrastre a medias no se cuele en la siguiente
  estadoFalso.configured = false
  estadoFalso.stats.total = 0
  localStorage.clear()
  vi.clearAllMocks()
})

async function montar () {
  const w = mount(App, { attachTo: document.body })
  await flushPromises(); await flushPromises()
  return w
}

describe('arranque sin carpetas', () => {
  it('muestra la bienvenida y no pide canciones', async () => {
    const w = await montar()
    expect(w.text()).toContain('Bienvenido a DanPlay')
    expect(api.search).not.toHaveBeenCalled()
  })

  it('tras elegir carpeta y analizar, la lista aparece sola', async () => {
    const w = await montar()
    await w.find('.page input[type="text"], .page input:not([type])').setValue('/musica')
    await w.findAll('button').find(b => b.text().includes('Analizar')).trigger('click')
    await flushPromises(); await flushPromises(); await flushPromises()

    expect(w.text()).not.toContain('Bienvenido a DanPlay')
    expect(api.search).toHaveBeenCalled()
    expect(w.text()).toContain('Mi Gozo')
    expect(w.text()).toContain('Shekinah')
  })
})

describe('reindexado desde Ajustes', () => {
  it('al volver a la biblioteca la vista ya trae lo nuevo', async () => {
    const w = await montar()
    // simula que ya habia carpeta y se reindexa desde Ajustes
    estadoFalso.configured = true
    await api.scan()
    // navegar a la biblioteca
    await w.findAll('.nav-link').find(e => e.text().includes('Todas las canciones')).trigger('click')
    await flushPromises(); await flushPromises()
    expect(w.text()).toContain('Mi Gozo')
  })
})

describe('reactividad de la tabla', () => {
  it('las estrellas se reflejan al momento', async () => {
    estadoFalso.configured = true
    status.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
                   bitrate: 128000, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }]
    const w = await montar()
    await flushPromises()
    // por su etiqueta, no por el indice: las estrellas van al reves en el DOM
    // para poder pintar «esta y las anteriores» al pasar el raton
    const estrella = w.findAll('.stars .ico').find(i => i.attributes('title') === '4 de 5')
    expect(estrella, 'no hay una estrella con titulo «4 de 5»').toBeTruthy()
    await estrella.trigger('click')
    await flushPromises()
    expect(api.setStars).toHaveBeenCalledWith(1, 4)
    expect(w.findAll('.stars .ico.on').length).toBe(4)
  })

  it('el favorito se refleja al momento', async () => {
    estadoFalso.configured = true
    status.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
                   bitrate: 128000, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }]
    const w = await montar()
    await w.find('.heart').trigger('click')
    await flushPromises()
    expect(w.find('.heart').classes()).toContain('on')
  })
})

describe('cambio de vista', () => {
  it('la cuadricula muestra las mismas canciones que la lista', async () => {
    estadoFalso.configured = true
    status.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
                   bitrate: 128000, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }]
    const w = await montar()
    expect(w.find('table').exists()).toBe(true)
    await w.find('.view-switch').findAll('button')[2].trigger('click')   // cuadricula
    await flushPromises()
    expect(w.find('.grid').exists()).toBe(true)
    expect(w.text()).toContain('Mi Gozo')
  })

  it('agrupar por artista crea cabeceras de grupo', async () => {
    estadoFalso.configured = true
    status.songs = [
      { id: 1, title: 'A', artist: 'Barak', album: '', duration: 10, bitrate: 1, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 },
      { id: 2, title: 'B', artist: 'New Wine', album: '', duration: 10, bitrate: 1, stars: 0, favorite: 0, feat: '', folder: 'y', key: '', bpm: 0 }
    ]
    const w = await montar()
    await w.findAll('button').find(b => b.text().includes('Vista')).trigger('click')
    await flushPromises()
    // el selector propio: se abre y se elige la opcion
    await w.findAll('.select-box')[0].trigger('click')
    await flushPromises()
    const opcion = w.findAll('.select-opt').find(o => o.text().includes('Por artista'))
    expect(opcion, 'no encuentro la opcion "Por artista"').toBeTruthy()
    await opcion.trigger('click')
    await flushPromises()
    const cabeceras = w.findAll('.group-head').map(c => c.text())
    expect(cabeceras.some(t => t.includes('Barak'))).toBe(true)
    expect(cabeceras.some(t => t.includes('New Wine'))).toBe(true)
  })
})

describe('cambiar de vista desde el lateral', () => {
  const unaCancion = () => ([{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '',
    duration: 200, bitrate: 128000, stars: 0, favorite: 1, feat: '',
    folder: 'Artistas/Barak', key: '', bpm: 0 }])

  async function conBiblioteca () {
    estadoFalso.configured = true
    status.songs = unaCancion()
    const w = mount(App, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    return w
  }
  const pulsar = async (w, text) => {
    const e = w.findAll('.nav-link').find(x => x.text().includes(text))
    expect(e, `no encuentro el enlace "${text}"`).toBeTruthy()
    await e.trigger('click')
    await flushPromises(); await flushPromises()
  }

  it('Favoritos cambia el titulo y pide solo favoritos', async () => {
    const w = await conBiblioteca()
    await pulsar(w, 'Favoritos')
    expect(w.find('.filters').text()).toContain('Favoritos')
    const ultima = api.search.mock.calls.at(-1)[0]
    expect(ultima.only_favorites).toBe(true)
  })

  it('Artistas agrupa por artista', async () => {
    const w = await conBiblioteca()
    await pulsar(w, 'Artistas')
    expect(w.find('.filters').text()).toContain('Artistas')
    expect(w.findAll('.group-head').length).toBeGreaterThan(0)
  })

  it('Duplicados muestra su propia pagina', async () => {
    const w = await conBiblioteca()
    await pulsar(w, 'Duplicados')
    expect(w.text()).toContain('Escuchalas y quedate con la que prefieras')
    expect(api.duplicates).toHaveBeenCalled()
  })

  it('Entrada muestra su propia pagina', async () => {
    const w = await conBiblioteca()
    await pulsar(w, 'Entrada')
    // se comprueba lo que la pagina hace, no su redaccion exacta
    expect(w.text()).toContain('~/Musica/Entrada')
    expect(w.text()).toContain('Como funciona')
    expect(w.findAll('.steps li').length).toBe(4)
    expect(w.text()).toContain('Probar sin tocar nada')
  })

  it('Ajustes y vuelta a la biblioteca conserva las canciones', async () => {
    const w = await conBiblioteca()
    await pulsar(w, 'Ajustes')
    expect(w.text()).toContain('Apariencia')
    await pulsar(w, 'Todas las canciones')
    expect(w.text()).toContain('Mi Gozo')
  })

  it('nunca vuelve a la bienvenida si ya hay canciones', async () => {
    const w = await conBiblioteca()
    for (const v of ['Favoritos', 'Artistas', 'Duplicados', 'Todas las canciones']) {
      await pulsar(w, v)
      expect(w.text(), `la bienvenida reaparecio en ${v}`).not.toContain('Bienvenido a DanPlay')
    }
  })
})


describe('añadir la misma carpeta varias veces', () => {
  const campo = (w) => w.find('.page input[type="text"], .page input:not([type])')
  const analizar = async (w) => {
    await w.findAll('.page button').find(b => b.text() === 'Analizar').trigger('click')
    await flushPromises(); await flushPromises(); await flushPromises()
  }

  it('la primera vez la añade y carga la biblioteca', async () => {
    const w = mount(App, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    await campo(w).setValue('/musica')
    await analizar(w)
    expect(api.addFolder).toHaveBeenCalled()
    expect(w.text()).toContain('Mi Gozo')
  })

  it('si el backend dice que ya estaba, avisa pero carga igual', async () => {
    // el backend responde que la carpeta ya estaba indexada
    api.addFolder.mockResolvedValueOnce({
      action: 'already_there',
      notice: { kind: 'same', message: 'esa carpeta ya esta añadida' },
      folders: []
    })
    estadoFalso.configured = true          // porque ya estaba de antes
    const w = mount(App, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    // ya hay biblioteca: no hay bienvenida, se añade desde cero simulado
    estadoFalso.configured = false
    const w2 = mount(App, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    await campo(w2).setValue('/musica')
    await analizar(w2)

    expect(api.scan).toHaveBeenCalled()      // reindexa igualmente
    expect(w2.find('.toast').exists(), 'deberia salir un aviso flotante').toBe(true)
    expect(w2.find('.toast').text()).toContain('ya esta añadida')
    expect(w2.text()).toContain('Mi Gozo')       // y la biblioteca queda cargada
    expect(w2.text()).not.toContain('Bienvenido a DanPlay')
  })

  it('no se añade dos veces la misma ruta', async () => {
    const w = mount(App, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    await campo(w).setValue('/musica'); await analizar(w)
    const rutas = api.addFolder.mock.calls.map(c => c[0])
    const unicas = new Set(rutas)
    expect(unicas.size).toBe(rutas.length)
  })
})

// --------------------------------------------------------- modos de repeticion
// El fin de pista lo decide App, no el reproductor: aqui estan la cola y el
// modo. Estas pruebas fijan que hace cada uno al acabarse una cancion.
import Player from '../src/components/Player.vue'

const dosCanciones = () => [
  { id: 1, title: 'Primera', artist: 'X', album: '', duration: 10, bitrate: 1,
    stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 },
  { id: 2, title: 'Segunda', artist: 'X', album: '', duration: 10, bitrate: 1,
    stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }
]

/** Id de la fila marcada como sonando, o null. */
function sonando (w) {
  const filas = w.findAll('tbody tr')
  const i = filas.findIndex(f => f.classes().includes('playing'))
  return i < 0 ? null : i + 1
}

async function ponerModo (w, veces) {
  for (let i = 0; i < veces; i++) await w.find('.repeat-btn').trigger('click')
  await flushPromises()
}

async function reproducir (w, indice) {
  await w.findAll('tbody tr')[indice].find('.row-play').trigger('click')
  await flushPromises()
}

async function terminar (w) {
  w.findComponent(Player).vm.$emit('trackEnded')
  await flushPromises()
}

describe('repeticion al acabarse una cancion', () => {
  it('por defecto repite la lista sin fin', async () => {
    estadoFalso.configured = true
    status.songs = dosCanciones()
    const w = await montar()
    await reproducir(w, 0)
    expect(sonando(w)).toBe(1)
    await terminar(w)
    expect(sonando(w)).toBe(2)
    await terminar(w)
    expect(sonando(w), 'al final debe volver a la primera').toBe(1)
  })

  it('«repetir esta cancion» no avanza: la vuelve a poner', async () => {
    estadoFalso.configured = true
    status.songs = dosCanciones()
    const w = await montar()
    await ponerModo(w, 1)                 // list -> one
    await reproducir(w, 0)
    await terminar(w)
    expect(sonando(w)).toBe(1)
  })

  it('«solo esta cancion» se para al acabar', async () => {
    estadoFalso.configured = true
    status.songs = dosCanciones()
    const w = await montar()
    await ponerModo(w, 2)                 // list -> one -> once
    await reproducir(w, 0)
    await terminar(w)
    expect(sonando(w), 'no debe pasar a la siguiente').toBe(1)
  })

  it('«la lista una vez» avanza pero no da la vuelta', async () => {
    estadoFalso.configured = true
    status.songs = dosCanciones()
    const w = await montar()
    await ponerModo(w, 3)                 // list -> one -> once -> queue
    await reproducir(w, 0)
    await terminar(w)
    expect(sonando(w), 'avanza a la segunda').toBe(2)
    await terminar(w)
    expect(sonando(w), 'y ahi se queda').toBe(2)
  })

  it('el modo elegido se recuerda entre sesiones', async () => {
    estadoFalso.configured = true
    status.songs = dosCanciones()
    const w = await montar()
    await ponerModo(w, 1)
    expect(localStorage.getItem('danplay.repeat')).toBe('one')
  })
})

describe('el boton de play usa la seleccion', () => {
  it('sin nada cargado, reproduce la cancion seleccionada', async () => {
    estadoFalso.configured = true
    status.songs = dosCanciones()
    const w = await montar()
    await w.findAll('tbody tr')[1].trigger('click')     // seleccionar la segunda
    await flushPromises()
    expect(sonando(w), 'seleccionar no reproduce').toBe(null)
    await w.find('.pl-play').trigger('click')
    await flushPromises()
    expect(sonando(w)).toBe(2)
  })
})

// --------------------------------------------------- arrastrar canciones
// Se coge una cancion de la lista y se suelta en un repertorio del menu, en
// Favoritos o en «Nueva lista». Lo que NO puede pasar es que un clic normal
// acabe moviendo algo sin querer, asi que hay un umbral antes de arrastrar.
describe('arrastrar una cancion', () => {
  async function conLista () {
    estadoFalso.configured = true
    status.songs = dosCanciones()
    status.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
    return await montar()
  }
  const fila = (w, i = 0) => w.findAll('tbody tr')[i]
  const repertorio = (w) => w.find('.nav-playlist')
  const favoritos = (w) => w.find('[data-drop="favorites"]')

  // jsdom no trae PointerEvent, y test-utils no deja poner las coordenadas
  // encima de un MouseEvent ya creado: se lanza a mano.
  function puntero (el, tipo, { pointerType, ...resto } = {}) {
    const ev = new MouseEvent(tipo, { bubbles: true, cancelable: true, ...resto })
    if (pointerType) Object.defineProperty(ev, 'pointerType', { value: pointerType })
    el.element.dispatchEvent(ev)
    return flushPromises()
  }

  it('un clic sin mover el puntero no arrastra nada', async () => {
    const w = await conLista()
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10 })
    expect(w.vm.drag.song).toBe(null)
    await puntero(repertorio(w), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).not.toHaveBeenCalled()
  })

  it('al pasar del umbral empieza el arrastre y aparece el fantasma', async () => {
    const w = await conLista()
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(fila(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(w.vm.drag.song.id).toBe(1)
    expect(document.querySelector('.drag-ghost').textContent).toContain('Primera')
    expect(fila(w).classes()).toContain('dragged')
    await puntero(repertorio(w), 'pointerup')
  })

  it('soltarla en un repertorio la añade ahi', async () => {
    const w = await conLista()
    await puntero(fila(w, 1), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(repertorio(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(repertorio(w).classes()).toContain('drop-over')
    await puntero(repertorio(w), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledWith(7, [2])
    expect(w.vm.drag.song, 'el arrastre no termino').toBe(null)
  })

  it('soltarla en Favoritos la marca como favorita', async () => {
    const w = await conLista()
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(favoritos(w), 'pointermove', { clientX: 90, clientY: 120 })
    await puntero(favoritos(w), 'pointerup')
    await flushPromises()
    expect(api.toggleFavorite).toHaveBeenCalledWith(1, true)
  })

  it('soltarla donde no hay destino no hace nada', async () => {
    const w = await conLista()
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(fila(w, 1), 'pointermove', { clientX: 90, clientY: 200 })
    await puntero(fila(w, 1), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).not.toHaveBeenCalled()
    expect(api.toggleFavorite).not.toHaveBeenCalled()
  })

  it('Escape deja caer lo que se lleva sin soltarlo en nada', async () => {
    const w = await conLista()
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(repertorio(w), 'pointermove', { clientX: 90, clientY: 200 })
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(w.vm.drag.song).toBe(null)
    expect(document.querySelector('.drag-ghost')).toBe(null)
    expect(api.addToPlaylist).not.toHaveBeenCalled()
  })

  it('con el dedo no se arrastra: en tactil la lista se desplaza', async () => {
    const w = await conLista()
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10, pointerType: 'touch' })
    await puntero(repertorio(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(w.vm.drag.song).toBe(null)
  })

  it('los sitios donde soltar solo se marcan mientras hay algo en la mano', async () => {
    const w = await conLista()
    expect(w.find('.sidebar').classes()).not.toContain('drop-ready')
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(fila(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(w.find('.sidebar').classes()).toContain('drop-ready')
    await puntero(fila(w), 'pointerup')
    await flushPromises()
    expect(w.find('.sidebar').classes()).not.toContain('drop-ready')
  })

  it('un resto de la disposicion movible no rompe la vista', async () => {
    // aquella opcion guardaba un objeto en la misma clave que el formato
    localStorage.setItem('danplay.layout', '{"topbar":"bottom","player":"top"}')
    const w = await conLista()
    expect(w.vm.layout).toBe('list')
    expect(w.findAll('tbody tr')).toHaveLength(2)
  })

  it('ya no queda nada para mover los paneles de sitio', async () => {
    const w = await conLista()
    expect(w.find('.bar-grip').exists()).toBe(false)
    expect(w.find('.resizer').exists()).toBe(false)
    expect(w.find('.arrange-bar').exists()).toBe(false)
    expect(w.find('.app').classes()).toEqual(['app'])
  })
})

// --------------------------------------------------- bandeja del sistema
// El menu de la bandeja y la ventanita no ven nada de lo que pasa en la app,
// asi que la app tiene que contarselo. Lo importante es que puedan poner en
// gris lo que no se puede hacer.
describe('lo que la app le cuenta a la bandeja', () => {
  async function conLista (canciones = dosCanciones()) {
    estadoFalso.configured = true
    status.songs = canciones
    return await montar()
  }
  const ultimo = () => tray.setNowPlaying.mock.calls.at(-1)[0]

  it('sin nada sonando avisa de que no hay nada que pulsar', async () => {
    await conLista()
    expect(ultimo().id).toBe(null)
    expect(ultimo().has_previous).toBe(false)
    expect(ultimo().has_next).toBe(false)
  })

  it('al poner una cancion cuenta cual es y que se puede cambiar', async () => {
    const w = await conLista()
    await reproducir(w, 0)
    expect(ultimo().id).toBe(1)
    expect(ultimo().title).toBe('Primera')
    expect(ultimo().has_previous).toBe(true)
    expect(ultimo().has_next).toBe(true)
  })

  it('con una sola cancion no hay a donde ir', async () => {
    const w = await conLista([dosCanciones()[0]])
    await reproducir(w, 0)
    expect(ultimo().id).toBe(1)
    expect(ultimo().has_previous, 'no hay otra cancion').toBe(false)
    expect(ultimo().has_next).toBe(false)
  })

  it('«siguiente» desde la bandeja cambia de cancion', async () => {
    const w = await conLista()
    await reproducir(w, 0)
    bandeja.orden({ action: 'next' })
    await flushPromises()
    expect(sonando(w)).toBe(2)
  })

  it('«anterior» desde la bandeja tambien', async () => {
    const w = await conLista()
    await reproducir(w, 1)
    bandeja.orden({ action: 'previous' })
    await flushPromises()
    expect(sonando(w)).toBe(1)
  })

  it('una orden que no conoce no la lia', async () => {
    const w = await conLista()
    await reproducir(w, 0)
    bandeja.orden({ action: 'lo-que-sea' })
    await flushPromises()
    expect(sonando(w)).toBe(1)
  })
})

describe('escribir en el buscador', () => {
  it('no lanza una busqueda por cada tecla', async () => {
    vi.useFakeTimers()
    try {
      estadoFalso.configured = true
      status.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
                     bitrate: 128000, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }]
      const w = mount(App, { attachTo: document.body })
      await vi.advanceTimersByTimeAsync(0); await flushPromises()
      api.search.mockClear()

      const buscador = w.find('.topbar input')
      for (const t of ['b', 'ba', 'bar', 'bara', 'barak']) {
        await buscador.setValue(t)
        await vi.advanceTimersByTimeAsync(40)      // se escribe seguido
      }
      expect(api.search, 'no deberia buscar mientras aun escribes').not.toHaveBeenCalled()

      await vi.advanceTimersByTimeAsync(300)       // se para de escribir
      await flushPromises()
      expect(api.search).toHaveBeenCalledTimes(1)
      expect(api.search.mock.calls[0][0].q).toBe('barak')
      w.unmount()
    } finally {
      vi.useRealTimers()
    }
  })
})
