import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// --- doble del backend --------------------------------------------------
// Se construye a partir del `api` de verdad (tests/support/backend.js), asi
// que si la app llama a algo que no esta programado, la prueba lo dice con su
// nombre en vez de pasar en verde contra un contrato que ya no existe.
const held = vi.hoisted(() => ({ state: null, api: null, playback: null, pickFolder: null, coreListener: null, app: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble, createAppDouble } =
    await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  // `usePlayback` elige el puente de Rust o el reproductor web mirando esto
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  held.pickFolder = v.fn(async () => null)
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: held.playback.bridge,
    pickFolder: held.pickFolder,
    app: (held.app = createAppDouble()),
    mini: { hide: v.fn(async () => {}), toggle: v.fn(async () => {}) },
    core: {
      onStatus: async (fn) => {
        held.coreListener = fn
        return () => (held.coreListener = null)
      },
      // «algo cambio en el nucleo»: App se refresca entero al oirlo
      onChanged: async (fn) => {
        held.changeListener = fn
        return () => (held.changeListener = null)
      }
    }
  }
})

import App from '../src/App.vue'
import { cancelDrag } from '../src/composables/useDragSong.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetPreferences } from '../src/composables/usePreferences.js'
import { clearNotices } from '../src/composables/useNotices.js'
import { song } from './support/backend.js'

const api = held.api
const state = held.state
const playback = held.playback

beforeEach(() => {
  state.songs = []
  state.addedFolders = []
  state.playlists = []
  state.status.configured = false
  state.status.stats.total = 0
  cancelDrag()          // que un arrastre a medias no se cuele en la siguiente
  resetPlayback()
  resetPreferences()
  clearNotices()
  playback.reset()
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

describe('el nucleo llega tarde', () => {
  it('la biblioteca se llena sola cuando el nucleo arranca', async () => {
    // Abrir DanPlay con una cancion desde el explorador es cuando mas tarda
    // el nucleo: todo pasa a la vez. Si la interfaz carga antes de que
    // conteste, la biblioteca sale vacia; y antes se quedaba asi para
    // siempre, porque nadie escuchaba el aviso de «ya estoy».
    state.status.configured = true
    state.status.stats.total = 0
    state.songs = []
    const w = await montar()
    expect(w.text()).toContain('Nada por aqui')

    // el nucleo termina de arrancar y avisa
    state.status.stats.total = 3
    state.songs = [song(1), song(2), song(3)]
    held.coreListener?.({ ready: true, message: '' })
    await flushPromises(); await flushPromises()

    expect(w.text()).not.toContain('Nada por aqui')
  })

  it('los avisos siguientes no recargan por nada', async () => {
    state.status.configured = true
    await montar()
    held.coreListener?.({ ready: true, message: '' })
    await flushPromises()
    api.search.mockClear()
    held.coreListener?.({ ready: true, message: 'sigue en pie' })
    await flushPromises()
    expect(api.search).not.toHaveBeenCalled()
  })
})

// Rust avisa (`danplay://changed`) cuando el nucleo cuenta un cambio, lo haya
// hecho quien lo haya hecho: el asistente, una descarga que termina, la linea
// de ordenes. Antes la lista decia «6 temas» con tres hasta salir y volver a
// entrar en la pagina.
describe('algo cambia en el nucleo por detras', () => {
  it('la lista, los repertorios y el estado se refrescan solos', async () => {
    vi.useFakeTimers()
    state.status.configured = true
    state.status.stats.total = 1
    state.songs = [song(1)]
    const w = await montar()
    expect(w.text()).not.toContain('Herlin')

    // el asistente crea una lista y entra una cancion nueva
    state.playlists = [{ id: 2, name: 'Herlin', n: 3 }]
    state.songs = [song(1), song(2)]
    state.status.stats.total = 2
    held.changeListener?.({ revision: 7 })
    await vi.advanceTimersByTimeAsync(400)
    await flushPromises(); await flushPromises()

    expect(w.text()).toContain('Herlin')
    expect(w.findAll('.nav-playlist').length).toBe(1)
    expect(api.search).toHaveBeenCalled()
    expect(w.text()).toContain(song(2).title)
    vi.useRealTimers()
  })

  it('varios avisos seguidos son un solo refresco', async () => {
    vi.useFakeTimers()
    state.status.configured = true
    state.songs = [song(1)]
    await montar()
    api.search.mockClear(); api.playlists.mockClear()
    held.changeListener?.({ revision: 1 })
    held.changeListener?.({ revision: 2 })
    held.changeListener?.({ revision: 3 })
    await vi.advanceTimersByTimeAsync(400)
    await flushPromises()
    expect(api.search).toHaveBeenCalledTimes(1)
    expect(api.playlists).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })
})

describe('la lista del reproductor abierta', () => {
  it('se actualiza sola cuando llega una cancion desde fuera', async () => {
    state.status.configured = true
    const w = await montar()
    // se entra a «Reproductor»
    api.externalList.mockResolvedValue({ songs: [song(1, { title: 'La primera' })] })
    await w.findAll('.nav-link').find((b) => b.text().includes('Reproductor')).trigger('click')
    await flushPromises(); await flushPromises()
    expect(w.text()).toContain('La primera')

    // llega otra por «Abrir con DanPlay», con la lista delante
    api.externalList.mockResolvedValue({
      songs: [song(2, { title: 'Recien llegada' }), song(1, { title: 'La primera' })]
    })
    api.externalList.mockClear()
    held.api.fireExternal?.()
    await flushPromises(); await flushPromises()

    expect(api.externalList).toHaveBeenCalled()
    expect(w.text()).toContain('Recien llegada')
  })

  it('en otra vista no se recarga por nada', async () => {
    state.status.configured = true
    await montar()
    api.externalList.mockClear()
    held.api.fireExternal?.()
    await flushPromises()
    expect(api.externalList).not.toHaveBeenCalled()
  })
})

describe('reindexado desde Ajustes', () => {
  it('al volver a la biblioteca la vista ya trae lo nuevo', async () => {
    const w = await montar()
    // simula que ya habia carpeta y se reindexa desde Ajustes
    state.status.configured = true
    await api.scan()
    // navegar a la biblioteca
    await w.findAll('.nav-link').find(e => e.text().includes('Todas las canciones')).trigger('click')
    await flushPromises(); await flushPromises()
    expect(w.text()).toContain('Mi Gozo')
  })
})

describe('reactividad de la tabla', () => {
  it('las estrellas se reflejan al momento', async () => {
    state.status.configured = true
    state.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
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
    // solo las de la fila: la ficha de la derecha tambien las enseña
    expect(w.findAll('tbody .stars .ico.on').length).toBe(4)
  })

  it('el favorito se refleja al momento', async () => {
    state.status.configured = true
    state.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
                   bitrate: 128000, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }]
    const w = await montar()
    await w.find('.heart').trigger('click')
    await flushPromises()
    expect(w.find('.heart').classes()).toContain('on')
  })
})

describe('cambio de vista', () => {
  it('la cuadricula muestra las mismas canciones que la lista', async () => {
    state.status.configured = true
    state.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
                   bitrate: 128000, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }]
    const w = await montar()
    expect(w.find('table').exists()).toBe(true)
    await w.find('.view-switch').findAll('button')[2].trigger('click')   // cuadricula
    await flushPromises()
    expect(w.find('.grid').exists()).toBe(true)
    expect(w.text()).toContain('Mi Gozo')
  })

  it('agrupar por artista crea cabeceras de grupo', async () => {
    state.status.configured = true
    state.songs = [
      { id: 1, title: 'A', artist: 'Barak', album: '', duration: 10, bitrate: 1, stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 },
      { id: 2, title: 'B', artist: 'New Wine', album: '', duration: 10, bitrate: 1, stars: 0, favorite: 0, feat: '', folder: 'y', key: '', bpm: 0 }
    ]
    const w = await montar()
    await w.findAll('button').find(b => b.text().includes('Vista')).trigger('click')
    await flushPromises()
    // Se busca por su etiqueta y no por posicion: el menu tiene varios
    // desplegables y basta con añadir uno arriba para que un indice mienta.
    const agrupar = w.findAll('.field').find(f => f.text().includes('Agrupar'))
    expect(agrupar, 'no encuentro el desplegable de agrupar').toBeTruthy()
    await agrupar.find('.select-box').trigger('click')
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
    state.status.configured = true
    state.songs = unaCancion()
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
    expect(w.text()).toContain('Escúchalas y quédate con la que prefieras')
    expect(api.duplicates).toHaveBeenCalled()
  })

  it('Entrada muestra su propia pagina', async () => {
    const w = await conBiblioteca()
    await pulsar(w, 'Entrada')
    // se comprueba lo que la pagina hace, no su redaccion exacta
    expect(w.text()).toContain('~/Musica/Entrada')
    expect(w.text()).toContain('Cómo funciona')
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
      notice: { kind: 'same', message: 'esa carpeta ya está añadida' },
      folders: []
    })
    // la carpeta ya estaba indexada, asi que su musica ya esta en la base
    state.songs = state.scanFinds.map(s => ({ ...s }))
    state.status.configured = false
    const w2 = mount(App, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    await campo(w2).setValue('/musica')
    await analizar(w2)
    state.status.configured = true

    // no se reescanea: si ya estaba indexada, releerla entera es trabajo
    // tirado. Lo que si tiene que pasar es que se explique.
    expect(w2.find('.toast').exists(), 'deberia salir un aviso flotante').toBe(true)
    expect(w2.find('.toast').text()).toContain('ya está añadida')
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

// ------------------------------------------------------ modos de repeticion
// Que hace cada modo al acabarse una cancion se prueba donde vive la decision:
// en Rust (queue.rs) y, para el modo navegador, en playback/queueLogic.js.
// Aqui solo se comprueba que el boton cicla y se lo pide a quien manda.

const twoSongs = () => [
  { id: 1, title: 'Primera', artist: 'X', album: '', duration: 10, bitrate: 1,
    stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 },
  { id: 2, title: 'Segunda', artist: 'X', album: '', duration: 10, bitrate: 1,
    stars: 0, favorite: 0, feat: '', folder: 'x', key: '', bpm: 0 }
]
const dosCanciones = twoSongs

/** Id de la fila marcada como sonando, o null. */
function sonando (w) {
  const filas = w.findAll('tbody tr')
  const i = filas.findIndex(f => f.classes().includes('playing'))
  return i < 0 ? null : i + 1
}

async function reproducir (w, indice) {
  await w.findAll('tbody tr')[indice].find('.row-play').trigger('click')
  await flushPromises()
}

describe('el boton de repeticion', () => {
  it('cicla por los cuatro modos y se lo dice a quien tiene la cola', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await montar()
    for (const esperado of ['one', 'once', 'queue', 'list']) {
      await w.find('.repeat-btn').trigger('click')
      await flushPromises()
      expect(playback.bridge.setRepeat).toHaveBeenLastCalledWith(esperado)
    }
  })

  it('el modo elegido se recuerda entre sesiones', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await montar()
    await w.find('.repeat-btn').trigger('click')
    await flushPromises()
    expect(localStorage.getItem('danplay.repeat')).toBe('one')
  })
})

describe('poner una cancion', () => {
  it('manda la lista entera como cola, empezando por la pulsada', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await montar()
    await reproducir(w, 1)
    const [items, start] = playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map(t => t.id)).toEqual([1, 2])
    expect(start).toBe(2)
    expect(sonando(w)).toBe(2)
  })
})

// El boton de la fila de la cancion que suena es pausa, y pulsado reanuda:
// antes volvia a empezar la cancion y desde la lista no habia forma de pararla.
describe('pausar desde la propia fila', () => {
  it('sobre la que suena, el boton pausa y reanuda en vez de reiniciar', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await montar()
    await reproducir(w, 1)
    expect(playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    const boton = () => w.findAll('tbody tr')[1].find('.row-play')
    expect(boton().attributes('title')).toBe('Pausar')
    expect(boton().find('svg').exists()).toBe(true)

    await reproducir(w, 1)                 // otra vez sobre la misma
    await flushPromises(); await flushPromises()
    expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    expect(boton().attributes('title')).toBe('Reanudar')
    // la otra fila sigue ofreciendo reproducir
    expect(w.findAll('tbody tr')[0].find('.row-play').attributes('title')).toBe('Reproducir')
  })

  it('el punto de la barra lateral marca de donde sale lo que suena', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await montar()
    expect(w.find('.now-dot').exists()).toBe(false)
    await reproducir(w, 0)
    const all = w.findAll('.nav-link').find(b => b.text().includes('Todas las canciones'))
    expect(all.find('.now-dot').exists()).toBe(true)
    expect(all.find('.now-dot').classes()).not.toContain('paused')
    await reproducir(w, 0)                 // pausa
    expect(all.find('.now-dot').classes()).toContain('paused')
  })
})

// El menu de la cancion: «Enviar por Telegram» solo si Telegram esta en el
// equipo (lo dice Rust al arrancar), y «Abrir la carpeta» siempre.
describe('el menu contextual de una cancion', () => {
  async function abrirMenu () {
    state.status.configured = true
    state.songs = [song(1), song(2)]          // con su ruta, como las de verdad
    const w = await montar()
    await w.findAll('tbody tr')[0].trigger('contextmenu')
    await flushPromises()
    return w
  }
  const etiquetas = (w) => w.findAll('.ctx-item .ctx-label').map(b => b.text())

  it('con Telegram instalado ofrece enviar la cancion, y manda su ruta', async () => {
    const w = await abrirMenu()
    expect(etiquetas(w)).toContain('Enviar por Telegram')
    expect(etiquetas(w)).toContain('Abrir la carpeta')
    await w.findAll('.ctx-item').find(b => b.text().includes('Enviar por Telegram')).trigger('click')
    await flushPromises(); await flushPromises()
    expect(held.app.sendToTelegram).toHaveBeenCalledTimes(1)
    expect(held.app.sendToTelegram.mock.calls[0][0]).toBe('/musica/cancion-1.mp3')
    expect(w.text()).toContain('Telegram se ha abierto')
  })

  it('sin Telegram, la opcion no aparece', async () => {
    held.app.shareTargets.mockResolvedValueOnce({ telegram: false })
    const w = await abrirMenu()
    expect(etiquetas(w)).not.toContain('Enviar por Telegram')
    expect(etiquetas(w)).toContain('Abrir la carpeta')
  })
})

describe('el boton de play usa la seleccion', () => {
  it('sin nada cargado, reproduce la cancion seleccionada', async () => {
    state.status.configured = true
    state.songs = dosCanciones()
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
    state.status.configured = true
    state.songs = dosCanciones()
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
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
    expect(w.vm.layout).toBe('table')
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

// ----------------------------------------------------- salir de la aplicacion
// Con la bandeja, cerrar la ventana ya no cierra DanPlay: la unica salida
// desde la interfaz es esta. Lo que le cuenta a la bandeja ya no sale de
// aqui: el estado vive en Rust y el icono lo lee de alli.
describe('salir de DanPlay', () => {
  it('el menu de Vista ofrece salir', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await montar()
    await w.findAll('button').find(b => b.text().includes('Vista')).trigger('click')
    await flushPromises()
    const salir = w.findAll('button').find(b => b.text().includes('Salir de DanPlay'))
    expect(salir, 'no encuentro como salir de la aplicacion').toBeTruthy()
  })
})

describe('escribir en el buscador', () => {
  it('no lanza una busqueda por cada tecla', async () => {
    vi.useFakeTimers()
    try {
      state.status.configured = true
      state.songs = [{ id: 1, title: 'Mi Gozo', artist: 'Barak', album: '', duration: 200,
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
