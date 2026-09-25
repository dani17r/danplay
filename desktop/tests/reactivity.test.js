import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// --- doble del backend --------------------------------------------------
// Se construye a partir del `api` de verdad (tests/support/backend.js), asi
// que si la app llama a algo que no esta programado, la prueba lo dice con su
// nombre en vez de pasar en verde contra un contrato que ya no existe.
const held = vi.hoisted(() => ({
  state: null,
  api: null,
  playback: null,
  pickFolder: null,
  coreListener: null,
  app: null
}))

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
import { dialogCancel } from '../src/composables/useDialog.js'
import { resetChat } from '../src/composables/useChat.js'
import { song } from './support/backend.js'
import { settle } from './support/pages.js'

const api = held.api
const state = held.state
const playback = held.playback

beforeEach(() => {
  state.songs = []
  state.addedFolders = []
  state.playlists = []
  state.playlistSongs = []
  state.status.configured = false
  state.status.stats.total = 0
  state.status.missing_folders = []
  cancelDrag() // que un arrastre a medias no se cuele en la siguiente
  resetPlayback()
  resetPreferences()
  clearNotices()
  // El dialogo es uno para toda la app: si una prueba dejo uno abierto (el de
  // «¿abrir las canciones con DanPlay?» al arrancar), la siguiente lo hereda y
  // los atajos de teclado se quedan bloqueados sin que se vea por que.
  dialogCancel()
  resetChat()
  playback.reset()
  localStorage.clear()
  vi.clearAllMocks()
})

async function montar() {
  const w = mount(App, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
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
    await w
      .findAll('button')
      .find((b) => b.text().includes('Analizar'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    await flushPromises()

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
    // conteste, la primera carga FALLA; y antes ese fallo cortaba el
    // arranque antes de escuchar el aviso de «ya estoy», asi que la
    // biblioteca se quedaba vacia para siempre.
    held.coreListener = null
    state.status.configured = true
    api.status.mockRejectedValueOnce(new Error('el nucleo no contesta'))
    const w = await montar()
    expect(api.search).not.toHaveBeenCalled()
    expect(w.text()).not.toContain('Bienvenido a DanPlay')
    expect(held.coreListener, 'no llego a escuchar el aviso del nucleo').toBeTypeOf('function')

    // el nucleo termina de arrancar y avisa
    state.songs = [song(1), song(2), song(3)]
    held.coreListener({ ready: true, message: '' })
    await flushPromises()
    await flushPromises()

    expect(w.text()).toContain(song(3).title)
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

// La musica se movio o se borro por fuera (el gestor de archivos, un disco que
// no esta): el nucleo aparta lo que ya no esta, y si no queda nada, la app no
// enseña una lista vacia sin mas, sino la pantalla para volver a importar.
describe('la musica ya no esta donde estaba', () => {
  it('si la carpeta se movio, lo dice y pregunta donde esta ahora', async () => {
    state.status.configured = true
    state.status.missing_folders = ['/home/ana/Musica']
    const w = await montar()
    expect(w.text()).toContain('No encuentro tu música')
    expect(w.text()).toContain('/home/ana/Musica')
    expect(w.text()).toContain('¿Dónde está ahora?')
  })

  it('al elegirla en su sitio nuevo vuelve todo, sin mas preguntas', async () => {
    state.status.configured = true
    state.status.missing_folders = ['/home/ana/Musica']
    api.addFolder.mockImplementationOnce(async () => {
      state.status.missing_folders = []
      return {
        action: 'relocated',
        notice: { kind: 'relocated', other: '/home/ana/Musica' },
        folders: []
      }
    })
    const w = await montar()
    await w.find('.page input[type="text"], .page input:not([type])').setValue('/disco/Musica')
    await w
      .findAll('button')
      .find((b) => b.text().includes('Analizar'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    await flushPromises()

    expect(api.addFolder).toHaveBeenCalledWith('/disco/Musica', '', false)
    expect(w.find('.toast').text()).toContain('tu carpeta de antes')
    expect(w.text()).not.toContain('No encuentro tu música')
    expect(w.text()).toContain('Mi Gozo')
  })

  it('si se vacia con la app abierta, aparece sola la pantalla de importar', async () => {
    vi.useFakeTimers()
    state.status.configured = true
    state.songs = [song(1)]
    const w = await montar()
    expect(w.text()).toContain(song(1).title)

    // la carpeta se movio por fuera: el nucleo aparta sus canciones y avisa
    state.songs = []
    state.status.missing_folders = ['/musica']
    held.changeListener?.({ revision: 9 })
    await vi.advanceTimersByTimeAsync(400)
    await flushPromises()
    await flushPromises()

    expect(w.text()).toContain('No encuentro tu música')
    expect(w.text()).not.toContain(song(1).title)
    vi.useRealTimers()
  })

  it('con las carpetas en su sitio pero sin canciones, invita a elegir otra', async () => {
    state.status.configured = true
    const w = await montar()
    expect(w.text()).toContain('Tu biblioteca está vacía')
    expect(w.text()).not.toContain('No encuentro tu música')
  })

  it('en Ajustes no tapa nada: es donde se arregla', async () => {
    state.status.configured = true
    state.status.missing_folders = ['/musica']
    const w = await montar()
    await w
      .findAll('.nav-link')
      .find((b) => b.text().includes('Ajustes'))
      .trigger('click')
    await settle()
    expect(w.text()).not.toContain('No encuentro tu música')
    expect(w.text()).toContain('Carpetas gestionadas')
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
    await flushPromises()
    await flushPromises()

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
    api.search.mockClear()
    api.playlists.mockClear()
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
    await w
      .findAll('.nav-link')
      .find((b) => b.text().includes('Reproductor'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.text()).toContain('La primera')

    // llega otra por «Abrir con DanPlay», con la lista delante
    api.externalList.mockResolvedValue({
      songs: [song(2, { title: 'Recien llegada' }), song(1, { title: 'La primera' })]
    })
    api.externalList.mockClear()
    held.api.fireExternal?.()
    await flushPromises()
    await flushPromises()

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
    await w
      .findAll('.nav-link')
      .find((e) => e.text().includes('Todas las canciones'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.text()).toContain('Mi Gozo')
  })
})

describe('reactividad de la tabla', () => {
  it('las estrellas se reflejan al momento', async () => {
    state.status.configured = true
    state.songs = [
      {
        id: 1,
        title: 'Mi Gozo',
        artist: 'Barak',
        album: '',
        duration: 200,
        bitrate: 128000,
        stars: 0,
        favorite: 0,
        feat: '',
        folder: 'x',
        key: '',
        bpm: 0
      }
    ]
    const w = await montar()
    await flushPromises()
    // por su etiqueta, no por el indice: las estrellas van al reves en el DOM
    // para poder pintar «esta y las anteriores» al pasar el raton
    const estrella = w.findAll('.stars .ico').find((i) => i.attributes('title') === '4 de 5')
    expect(estrella, 'no hay una estrella con titulo «4 de 5»').toBeTruthy()
    await estrella.trigger('click')
    await flushPromises()
    expect(api.setStars).toHaveBeenCalledWith(1, 4)
    // solo las de la fila: la ficha de la derecha tambien las enseña
    expect(w.findAll('tbody .stars .ico.on').length).toBe(4)
  })

  it('el favorito se refleja al momento', async () => {
    state.status.configured = true
    state.songs = [
      {
        id: 1,
        title: 'Mi Gozo',
        artist: 'Barak',
        album: '',
        duration: 200,
        bitrate: 128000,
        stars: 0,
        favorite: 0,
        feat: '',
        folder: 'x',
        key: '',
        bpm: 0
      }
    ]
    const w = await montar()
    await w.find('.heart').trigger('click')
    await flushPromises()
    expect(w.find('.heart').classes()).toContain('on')
  })
})

describe('cambio de vista', () => {
  it('la cuadricula muestra las mismas canciones que la lista', async () => {
    state.status.configured = true
    state.songs = [
      {
        id: 1,
        title: 'Mi Gozo',
        artist: 'Barak',
        album: '',
        duration: 200,
        bitrate: 128000,
        stars: 0,
        favorite: 0,
        feat: '',
        folder: 'x',
        key: '',
        bpm: 0
      }
    ]
    const w = await montar()
    expect(w.find('table').exists()).toBe(true)
    await w.find('.view-switch').findAll('button')[2].trigger('click') // cuadricula
    await flushPromises()
    expect(w.find('.grid').exists()).toBe(true)
    expect(w.text()).toContain('Mi Gozo')
  })

  it('agrupar por artista crea cabeceras de grupo', async () => {
    state.status.configured = true
    state.songs = [
      {
        id: 1,
        title: 'A',
        artist: 'Barak',
        album: '',
        duration: 10,
        bitrate: 1,
        stars: 0,
        favorite: 0,
        feat: '',
        folder: 'x',
        key: '',
        bpm: 0
      },
      {
        id: 2,
        title: 'B',
        artist: 'New Wine',
        album: '',
        duration: 10,
        bitrate: 1,
        stars: 0,
        favorite: 0,
        feat: '',
        folder: 'y',
        key: '',
        bpm: 0
      }
    ]
    const w = await montar()
    await w
      .findAll('button')
      .find((b) => b.text().includes('Vista'))
      .trigger('click')
    await flushPromises()
    // Se busca por su etiqueta y no por posicion: el menu tiene varios
    // desplegables y basta con añadir uno arriba para que un indice mienta.
    const agrupar = w.findAll('.field').find((f) => f.text().includes('Agrupar'))
    expect(agrupar, 'no encuentro el desplegable de agrupar').toBeTruthy()
    await agrupar.find('.select-box').trigger('click')
    await flushPromises()
    const opcion = w.findAll('.select-opt').find((o) => o.text().includes('Por artista'))
    expect(opcion, 'no encuentro la opcion "Por artista"').toBeTruthy()
    await opcion.trigger('click')
    await flushPromises()
    const cabeceras = w.findAll('.group-head').map((c) => c.text())
    expect(cabeceras.some((t) => t.includes('Barak'))).toBe(true)
    expect(cabeceras.some((t) => t.includes('New Wine'))).toBe(true)
  })
})

describe('cambiar de vista desde el lateral', () => {
  const unaCancion = () => [
    {
      id: 1,
      title: 'Mi Gozo',
      artist: 'Barak',
      album: '',
      duration: 200,
      bitrate: 128000,
      stars: 0,
      favorite: 1,
      feat: '',
      folder: 'Artistas/Barak',
      key: '',
      bpm: 0
    }
  ]

  async function conBiblioteca() {
    state.status.configured = true
    state.songs = unaCancion()
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    return w
  }
  const pulsar = async (w, text) => {
    const e = w.findAll('.nav-link').find((x) => x.text().includes(text))
    expect(e, `no encuentro el enlace "${text}"`).toBeTruthy()
    await e.trigger('click')
    await settle()
  }

  it('el asistente recibe lo que se estaba viendo, la seleccion y lo que suena', async () => {
    // «pon la segunda», «esta», «las seleccionadas»: sin esto el modelo no
    // sabia a que se referia la persona
    state.status.configured = true
    state.songs = [
      ...unaCancion(),
      { ...unaCancion()[0], id: 2, title: 'Shekinah', artist: 'New Wine' }
    ]
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    await pulsar(w, 'Asistente')
    await w.find('.chat-foot input').setValue('pon la segunda')
    await w.find('.chat-foot .btn').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(api.chatStart).toHaveBeenCalled()
    const ctx = api.chatStart.mock.calls.at(-1)[1]
    expect(ctx.view.name).toBe('Todas las canciones')
    expect(ctx.total).toBe(2)
    expect(ctx.songs.map((s) => s.id)).toEqual([1, 2])
    expect(ctx.songs[1]).toEqual({ id: 2, artist: 'New Wine', title: 'Shekinah' })
    expect(ctx.playing).toBeNull()
  })

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

  it('el asistente se queda como estaba al salir y volver, y se pone al dia', async () => {
    // Las paginas se cargan al abrirlas; el asistente, ademas, se guarda al
    // salir (<KeepAlive>): lo que tenias abierto en el sigue abierto.
    const w = await conBiblioteca()
    await pulsar(w, 'Asistente')
    await w.find('.chat-head button[aria-expanded]').trigger('click')
    expect(w.find('.chat-list').exists()).toBe(true)
    const antes = api.chats.mock.calls.length
    await pulsar(w, 'Todas las canciones')
    expect(w.find('.chat').exists()).toBe(false)
    expect(w.findAll('[data-song-row]').length).toBeGreaterThan(0)
    await pulsar(w, 'Asistente')
    expect(w.find('.chat-list').exists(), 'la lista de conversaciones se cerro').toBe(true)
    // y al volver se relee lo que haya cambiado mientras tanto
    expect(api.chats.mock.calls.length).toBeGreaterThan(antes)
  })

  it('Duplicados muestra su propia pagina', async () => {
    const w = await conBiblioteca()
    await pulsar(w, 'Duplicados')
    expect(w.text()).toContain('Escúchalas y quédate con la que prefieras')
    expect(api.duplicatesScan).toHaveBeenCalled()
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
    await w
      .findAll('.page button')
      .find((b) => b.text() === 'Analizar')
      .trigger('click')
    await flushPromises()
    await flushPromises()
    await flushPromises()
  }

  it('la primera vez la añade y carga la biblioteca', async () => {
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
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
    state.songs = state.scanFinds.map((s) => ({ ...s }))
    state.status.configured = false
    const w2 = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    await campo(w2).setValue('/musica')
    await analizar(w2)
    state.status.configured = true

    // no se reescanea: si ya estaba indexada, releerla entera es trabajo
    // tirado. Lo que si tiene que pasar es que se explique.
    expect(w2.find('.toast').exists(), 'deberia salir un aviso flotante').toBe(true)
    expect(w2.find('.toast').text()).toContain('ya está añadida')
    expect(w2.text()).toContain('Mi Gozo') // y la biblioteca queda cargada
    expect(w2.text()).not.toContain('Bienvenido a DanPlay')
  })

  it('no se añade dos veces la misma ruta', async () => {
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    await campo(w).setValue('/musica')
    await analizar(w)
    const rutas = api.addFolder.mock.calls.map((c) => c[0])
    const unicas = new Set(rutas)
    expect(unicas.size).toBe(rutas.length)
  })
})

// ------------------------------------------------------ modos de repeticion
// Que hace cada modo al acabarse una cancion se prueba donde vive la decision:
// en Rust (queue.rs) y, para el modo navegador, en playback/queueLogic.js.
// Aqui solo se comprueba que el boton cicla y se lo pide a quien manda.

const twoSongs = () => [
  {
    id: 1,
    title: 'Primera',
    artist: 'X',
    album: '',
    duration: 10,
    bitrate: 1,
    stars: 0,
    favorite: 0,
    feat: '',
    folder: 'x',
    key: '',
    bpm: 0
  },
  {
    id: 2,
    title: 'Segunda',
    artist: 'X',
    album: '',
    duration: 10,
    bitrate: 1,
    stars: 0,
    favorite: 0,
    feat: '',
    folder: 'x',
    key: '',
    bpm: 0
  }
]
const dosCanciones = twoSongs

/** Id de la fila marcada como sonando, o null. */
function sonando(w) {
  const filas = w.findAll('tbody tr')
  const i = filas.findIndex((f) => f.classes().includes('playing'))
  return i < 0 ? null : i + 1
}

async function reproducir(w, indice) {
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
    expect(items.map((t) => t.id)).toEqual([1, 2])
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

    await reproducir(w, 1) // otra vez sobre la misma
    await flushPromises()
    await flushPromises()
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
    const all = w.findAll('.nav-link').find((b) => b.text().includes('Todas las canciones'))
    expect(all.find('.now-dot').exists()).toBe(true)
    expect(all.find('.now-dot').classes()).not.toContain('paused')
    await reproducir(w, 0) // pausa
    expect(all.find('.now-dot').classes()).toContain('paused')
  })
})

// El menu de la cancion: «Enviar por Telegram» solo si Telegram esta en el
// equipo (lo dice Rust al arrancar), y «Abrir la carpeta» siempre.
describe('el menu contextual de una cancion', () => {
  async function abrirMenu() {
    state.status.configured = true
    state.songs = [song(1), song(2)] // con su ruta, como las de verdad
    const w = await montar()
    await w.findAll('tbody tr')[0].trigger('contextmenu')
    await flushPromises()
    return w
  }
  const etiquetas = (w) => w.findAll('.ctx-item .ctx-label').map((b) => b.text())

  it('con Telegram instalado ofrece enviar la cancion, y manda su ruta', async () => {
    const w = await abrirMenu()
    expect(etiquetas(w)).toContain('Enviar por Telegram')
    expect(etiquetas(w)).toContain('Abrir la carpeta')
    await w
      .findAll('.ctx-item')
      .find((b) => b.text().includes('Enviar por Telegram'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.app.sendToTelegram).toHaveBeenCalledTimes(1)
    expect(held.app.sendToTelegram.mock.calls[0][0]).toEqual(['/musica/cancion-1.mp3'])
    expect(w.text()).toContain('Telegram se ha abierto')
  })

  it('sin Telegram, la opcion no aparece', async () => {
    held.app.shareTargets.mockResolvedValueOnce({ telegram: false })
    const w = await abrirMenu()
    expect(etiquetas(w)).not.toContain('Enviar por Telegram')
    expect(etiquetas(w)).toContain('Abrir la carpeta')
  })

  it('con el panel a la vista no ofrece «Ver detalles»', async () => {
    const w = await abrirMenu()
    expect(w.find('.details').exists()).toBe(true)
    expect(etiquetas(w)).not.toContain('Ver detalles')
  })
})

// Ctrl y Mayus al pulsar seleccionan varias; el menu sobre una de ellas actua
// sobre todas: enviar, añadir a una lista, papelera (con una confirmacion que
// dice cuantas).
describe('seleccion multiple', () => {
  const tres = () => [song(1), song(2), song(3)]
  async function lista() {
    state.status.configured = true
    state.songs = tres()
    return montar()
  }
  const filas = (w) => w.findAll('tbody tr')
  const seleccionadas = (w) => filas(w).filter((f) => f.classes().includes('selected')).length

  it('Ctrl añade y quita; Mayus coge el tramo; Ctrl+Mayus lo suma', async () => {
    const w = await lista()
    await filas(w)[0].trigger('click')
    await filas(w)[2].trigger('click', { ctrlKey: true })
    await flushPromises()
    expect(seleccionadas(w)).toBe(2)
    await filas(w)[2].trigger('click', { ctrlKey: true }) // la quita
    await flushPromises()
    expect(seleccionadas(w)).toBe(1)
    await filas(w)[0].trigger('click')
    await filas(w)[2].trigger('click', { shiftKey: true }) // 1..3
    await flushPromises()
    expect(seleccionadas(w)).toBe(3)
    await filas(w)[1].trigger('click') // sin teclas: solo esa
    await flushPromises()
    expect(seleccionadas(w)).toBe(1)
  })

  it('el menu sobre la seleccion actua sobre todas', async () => {
    const w = await lista()
    await filas(w)[0].trigger('click')
    await filas(w)[2].trigger('click', { shiftKey: true })
    await flushPromises()
    await filas(w)[1].trigger('contextmenu')
    await flushPromises()
    const etiquetas = w.findAll('.ctx-item .ctx-label').map((b) => b.text())
    expect(etiquetas).toContain('Enviar 3 por Telegram')
    expect(etiquetas).toContain('Añadir 3 a una lista')
    expect(etiquetas).toContain('Mandar 3 a la papelera…')
    await w
      .findAll('.ctx-item')
      .find((b) => b.text().includes('Enviar 3 por Telegram'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.app.sendToTelegram.mock.calls[0][0]).toEqual([
      '/musica/cancion-1.mp3',
      '/musica/cancion-2.mp3',
      '/musica/cancion-3.mp3'
    ])
  })

  it('la papelera de varias pide confirmacion y dice cuantas', async () => {
    const w = await lista()
    await filas(w)[0].trigger('click')
    await filas(w)[1].trigger('click', { ctrlKey: true })
    await flushPromises()
    await filas(w)[0].trigger('contextmenu')
    await flushPromises()
    await w
      .findAll('.ctx-item')
      .find((b) => b.text().includes('Mandar 2 a la papelera'))
      .trigger('click')
    await flushPromises()
    const { dialog, dialogCancel } = await import('../src/composables/useDialog.js').then((m) =>
      m.useDialog()
    )
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.title).toContain('2 canciones')
    dialogCancel()
    await flushPromises()
    expect(api.deleteSong).not.toHaveBeenCalled()
  })

  it('el menu sobre una que NO esta en la seleccion es el de esa sola', async () => {
    const w = await lista()
    await filas(w)[0].trigger('click')
    await filas(w)[1].trigger('click', { ctrlKey: true })
    await flushPromises()
    await filas(w)[2].trigger('contextmenu')
    await flushPromises()
    const etiquetas = w.findAll('.ctx-item .ctx-label').map((b) => b.text())
    expect(etiquetas).toContain('Enviar por Telegram')
    expect(etiquetas).not.toContain('Enviar 2 por Telegram')
  })
})

describe('el menu de un repertorio', () => {
  it('ofrece renombrar y enviar por Telegram', async () => {
    state.status.configured = true
    state.songs = [song(1)]
    state.playlists = [{ id: 1, name: 'domingo', n: 1 }]
    const w = await montar()
    await w.find('.nav-playlist').trigger('contextmenu')
    await flushPromises()
    const etiquetas = w.findAll('.ctx-item .ctx-label').map((b) => b.text())
    expect(etiquetas).toContain('Renombrar…')
    expect(etiquetas).toContain('Enviar por Telegram')
  })
})

describe('el panel de detalles, oculto o a la vista', () => {
  it('cerrado (por ajustes) sale «Ver detalles» y abre la ventana', async () => {
    const { usePreferences } = await import('../src/composables/usePreferences.js')
    usePreferences().showDetails.value = false
    state.status.configured = true
    state.songs = [song(1)]
    const w = await montar()
    expect(w.find('.details').exists()).toBe(false)
    await w.find('tbody tr').trigger('contextmenu')
    await flushPromises()
    await w
      .findAll('.ctx-item')
      .find((b) => b.text().includes('Ver detalles'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    expect(document.body.querySelector('.details-modal')).toBeTruthy()
    expect(document.body.querySelector('.details-modal .details-title')?.textContent).toContain(
      'Cancion 1'
    )
    usePreferences().showDetails.value = true
  })
})

describe('el boton de play usa la seleccion', () => {
  it('sin nada cargado, reproduce la cancion seleccionada', async () => {
    state.status.configured = true
    state.songs = dosCanciones()
    const w = await montar()
    await w.findAll('tbody tr')[1].trigger('click') // seleccionar la segunda
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
  async function conLista() {
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
  function puntero(el, tipo, { pointerType, ...resto } = {}) {
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

  it('fuera de un repertorio las filas no son destino: soltar sobre otra no cambia nada', async () => {
    const w = await conLista()
    expect(fila(w).attributes('data-drop')).toBeUndefined()
    await puntero(fila(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(fila(w, 1), 'pointermove', { clientX: 90, clientY: 200 })
    await puntero(fila(w, 1), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
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

// ------------------------------------------------- el foco tras un clic
// Los atajos se apartan con el foco en un boton, para que espacio no pulse
// el boton Y pause. Pero un clic de raton dejaba el foco en el boton, y el
// espacio de despues volvia a pulsar «Siguiente» en vez de pausar la musica.
describe('tras pulsar un boton con el raton', () => {
  const tecla = (key) => {
    const target = document.activeElement || window
    target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    return flushPromises()
  }
  async function sonando() {
    state.status.configured = true
    state.songs = dosCanciones()
    // sin el dialogo de «¿abrir las canciones con DanPlay?», que bloquea los atajos
    localStorage.setItem('danplay.default-player-asked', '1')
    const w = await montar()
    playback.emit({
      track: { id: 1, title: 'Primera', artist: 'X', duration: 100 },
      playing: true,
      position: 10,
      duration: 100,
      index: 0,
      length: 2
    })
    await flushPromises()
    return w
  }

  it('el boton suelta el foco y el espacio vuelve a pausar', async () => {
    const w = await sonando()
    const siguiente = w
      .findAll('.player .pl-btn')
      .find((b) => b.attributes('title')?.startsWith('Siguiente'))
    siguiente.element.focus()
    expect(document.activeElement).toBe(siguiente.element)
    siguiente.element.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    await flushPromises()
    expect(document.activeElement).not.toBe(siguiente.element)
    await tecla(' ')
    expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(playback.bridge.next).not.toHaveBeenCalled()
  })

  it('tambien un repertorio del menu lateral', async () => {
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
    const w = await sonando()
    const nav = w.find('.nav-playlist')
    nav.element.focus()
    nav.element.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    await flushPromises()
    expect(document.activeElement).not.toBe(nav.element)
    await tecla(' ')
    expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
  })

  it('dentro de un dialogo el foco no se toca', async () => {
    const w = await sonando()
    await w.find('.nav-playlist, .nav-new').trigger('click') // «Nueva lista» abre un dialogo
    await flushPromises()
    const ok = document.querySelector('.modal button')
    expect(ok, 'no se abrio el dialogo').toBeTruthy()
    ok.focus()
    ok.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    await flushPromises()
    expect(document.activeElement).toBe(ok)
  })
})

// --------------------------------------------- ordenar un repertorio a mano
// Dentro de una lista que uno ha creado, las filas son destino ellas mismas:
// se coge una cancion y se deja encima o debajo de otra. Solo ahi: en
// «Todas» o en Favoritos el orden lo dan las columnas.
describe('ordenar un repertorio arrastrando', () => {
  const tres = () => [
    {
      id: 1,
      title: 'Primera',
      artist: 'X',
      album: '',
      duration: 10,
      bitrate: 1,
      stars: 0,
      favorite: 0,
      feat: '',
      folder: 'x',
      key: '',
      bpm: 0
    },
    {
      id: 2,
      title: 'Segunda',
      artist: 'X',
      album: '',
      duration: 10,
      bitrate: 1,
      stars: 0,
      favorite: 0,
      feat: '',
      folder: 'x',
      key: '',
      bpm: 0
    },
    {
      id: 3,
      title: 'Tercera',
      artist: 'X',
      album: '',
      duration: 10,
      bitrate: 1,
      stars: 0,
      favorite: 0,
      feat: '',
      folder: 'x',
      key: '',
      bpm: 0
    }
  ]
  async function enRepertorio(otras = []) {
    state.status.configured = true
    state.songs = tres()
    state.playlistSongs = tres()
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }, ...otras]
    const w = await montar()
    await w.find('.nav-playlist').trigger('click')
    await flushPromises()
    await flushPromises()
    return w
  }
  const fila = (w, i = 0) => w.findAll('tbody tr')[i]
  const titulos = (w) => w.findAll('tbody tr td.title').map((td) => td.text().trim())
  function puntero(el, tipo, resto = {}) {
    el.element.dispatchEvent(new MouseEvent(tipo, { bubbles: true, cancelable: true, ...resto }))
    return flushPromises()
  }
  // jsdom no maqueta: se le dice a la fila donde esta para poder apuntar a
  // su mitad de arriba o a la de abajo
  function colocar(el, top = 100, height = 30) {
    el.element.getBoundingClientRect = () => ({
      top,
      height,
      bottom: top + height,
      left: 0,
      width: 600,
      right: 600,
      x: 0,
      y: top
    })
  }

  it('las filas de la lista son destino, y se avisa de que se puede ordenar', async () => {
    const w = await enRepertorio()
    expect(titulos(w)).toEqual(['Primera', 'Segunda', 'Tercera'])
    expect(fila(w).attributes('data-drop')).toBe('sort:1')
    expect(w.find('[data-sort-list]').exists()).toBe(true)
    expect(w.find('.sort-hint').text()).toContain('arrastra')
  })

  it('soltarla en la mitad de abajo de otra la deja despues', async () => {
    const w = await enRepertorio()
    colocar(fila(w, 2))
    await puntero(fila(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(fila(w, 2), 'pointermove', { clientX: 90, clientY: 125 })
    expect(fila(w, 2).classes()).toContain('drop-after')
    expect(fila(w, 2).classes()).not.toContain('drop-before')
    await puntero(fila(w, 2), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 3, 1])
    expect(titulos(w)).toEqual(['Segunda', 'Tercera', 'Primera'])
    expect(w.vm.drag.song).toBe(null)
  })

  it('y en la mitad de arriba, antes', async () => {
    const w = await enRepertorio()
    colocar(fila(w, 0))
    await puntero(fila(w, 2), 'pointerdown', { clientX: 10, clientY: 300 })
    await puntero(fila(w, 0), 'pointermove', { clientX: 90, clientY: 105 })
    expect(fila(w, 0).classes()).toContain('drop-before')
    await puntero(fila(w, 0), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [3, 1, 2])
    expect(titulos(w)).toEqual(['Tercera', 'Primera', 'Segunda'])
  })

  it('dejarla donde ya estaba no pide nada al nucleo', async () => {
    const w = await enRepertorio()
    colocar(fila(w, 1))
    // la primera, justo encima de la segunda: es su sitio de siempre
    await puntero(fila(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(fila(w, 1), 'pointermove', { clientX: 90, clientY: 105 })
    await puntero(fila(w, 1), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
    expect(titulos(w)).toEqual(['Primera', 'Segunda', 'Tercera'])
  })

  it('sobre la propia fila que se lleva no se marca nada', async () => {
    const w = await enRepertorio()
    colocar(fila(w, 0))
    await puntero(fila(w, 0), 'pointerdown', { clientX: 10, clientY: 105 })
    await puntero(fila(w, 0), 'pointermove', { clientX: 90, clientY: 125 })
    expect(fila(w, 0).classes()).toContain('dragged')
    expect(fila(w, 0).classes()).not.toContain('drop-after')
    await puntero(fila(w, 0), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
  })

  it('si el nucleo no puede, la lista vuelve como estaba', async () => {
    const w = await enRepertorio()
    api.reorderPlaylist.mockRejectedValueOnce(new Error('sin base de datos'))
    colocar(fila(w, 2))
    await puntero(fila(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(fila(w, 2), 'pointermove', { clientX: 90, clientY: 125 })
    await puntero(fila(w, 2), 'pointerup')
    await flushPromises()
    await flushPromises()
    expect(titulos(w)).toEqual(['Primera', 'Segunda', 'Tercera'])
    expect(w.find('.toast').text()).toContain('No se pudo cambiar el orden')
  })

  it('las cabeceras no ordenan un repertorio: lo dicen y no recargan', async () => {
    const w = await enRepertorio()
    expect(w.find('thead th[aria-sort]').exists(), 'no hay columna que mande').toBe(false)
    const antes = api.playlistSongs.mock.calls.length
    await w.find('thead th.sortable .th-sort').trigger('click')
    await flushPromises()
    expect(w.find('.toast').text()).toContain('arrastra')
    expect(api.playlistSongs.mock.calls.length).toBe(antes)
  })

  it('en la lista fina las filas tambien se ordenan', async () => {
    const w = await enRepertorio()
    w.vm.layout = 'rows'
    await flushPromises()
    const filas = w.findAll('.rows .row')
    expect(filas).toHaveLength(3)
    expect(filas[0].attributes('data-drop')).toBe('sort:1')
    colocar(filas[2])
    await puntero(filas[0], 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(filas[2], 'pointermove', { clientX: 90, clientY: 125 })
    await puntero(filas[2], 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 3, 1])
  })

  it('en las fichas se mira la mitad izquierda o derecha', async () => {
    const w = await enRepertorio()
    w.vm.layout = 'cards'
    await flushPromises()
    const fichas = w.findAll('.card-song')
    expect(fichas[0].attributes('data-drop-axis')).toBe('x')
    fichas[2].element.getBoundingClientRect = () => ({
      top: 0,
      height: 60,
      bottom: 60,
      left: 300,
      width: 200,
      right: 500,
      x: 300,
      y: 0
    })
    await puntero(fichas[0], 'pointerdown', { clientX: 10, clientY: 10 })
    // a la izquierda de la tercera, aunque sea por su mitad de abajo
    await puntero(fichas[2], 'pointermove', { clientX: 320, clientY: 55 })
    expect(fichas[2].classes()).toContain('drop-before')
    await puntero(fichas[2], 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 1, 3])
  })

  it('de la lista al menu lateral sigue valiendo: se añade a otro repertorio', async () => {
    const w = await enRepertorio([{ id: 8, name: 'Lunes', n: 0 }])
    const otro = w.findAll('.nav-playlist')[1]
    await puntero(fila(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await puntero(otro, 'pointermove', { clientX: 90, clientY: 200 })
    await puntero(otro, 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledWith(8, [1])
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
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
    await w
      .findAll('button')
      .find((b) => b.text().includes('Vista'))
      .trigger('click')
    await flushPromises()
    const salir = w.findAll('button').find((b) => b.text().includes('Salir de DanPlay'))
    expect(salir, 'no encuentro como salir de la aplicacion').toBeTruthy()
  })
})

describe('escribir en el buscador', () => {
  it('no lanza una busqueda por cada tecla', async () => {
    vi.useFakeTimers()
    try {
      state.status.configured = true
      state.songs = [
        {
          id: 1,
          title: 'Mi Gozo',
          artist: 'Barak',
          album: '',
          duration: 200,
          bitrate: 128000,
          stars: 0,
          favorite: 0,
          feat: '',
          folder: 'x',
          key: '',
          bpm: 0
        }
      ]
      const w = mount(App, { attachTo: document.body })
      await vi.advanceTimersByTimeAsync(0)
      await flushPromises()
      api.search.mockClear()

      const buscador = w.find('.topbar input')
      for (const t of ['b', 'ba', 'bar', 'bara', 'barak']) {
        await buscador.setValue(t)
        await vi.advanceTimersByTimeAsync(40) // se escribe seguido
      }
      expect(api.search, 'no deberia buscar mientras aun escribes').not.toHaveBeenCalled()

      await vi.advanceTimersByTimeAsync(300) // se para de escribir
      await flushPromises()
      expect(api.search).toHaveBeenCalledTimes(1)
      expect(api.search.mock.calls[0][0].q).toBe('barak')
      w.unmount()
    } finally {
      vi.useRealTimers()
    }
  })
})

// ----------------------------------------------------- lo que llega tarde
// Las respuestas del nucleo llegan cuando llegan. Estas pruebas fijan que
// una respuesta vieja no pisa lo que la persona tiene delante.
describe('respuestas que llegan tarde', () => {
  const filas = (w) => w.findAll('tbody tr')
  const pulsarNav = async (w, text) => {
    await w
      .findAll('.nav-link')
      .find((x) => x.text().includes(text))
      .trigger('click')
    await settle()
  }

  it('pulsar una fila mientras carga la lista no la deja a medias ni girando', async () => {
    // Compartian contador: elegir una fila (o abrir su menu, que tambien la
    // elige) daba la carga por vieja, la tiraba y el indicador no paraba.
    state.status.configured = true
    state.songs = [song(1), song(2), song(3)]
    const w = await montar()
    expect(filas(w)).toHaveLength(3)
    let release
    api.search.mockImplementationOnce(
      () =>
        new Promise((r) => {
          release = r
        })
    )
    await w.find('thead th.sortable .th-sort').trigger('click') // ordenar: vuelve a pedir la lista
    await flushPromises()
    expect(w.find('.filters .loading').exists(), 'no se ve que carga').toBe(true)
    await filas(w)[1].trigger('click')
    await filas(w)[2].trigger('contextmenu')
    await flushPromises()
    release({ total: 4, count: 4, songs: [song(1), song(2), song(3), song(4)] })
    await flushPromises()
    await flushPromises()
    expect(filas(w)).toHaveLength(4)
    expect(w.find('.filters .loading').exists(), 'el indicador sigue girando').toBe(false)
  })

  it('dos clics seguidos no acaban enseñando la ficha del primero', async () => {
    state.status.configured = true
    state.songs = [song(1, { title: 'Primera' }), song(2, { title: 'Segunda' })]
    const w = await montar()
    let release
    api.song.mockImplementationOnce(
      (id) =>
        new Promise((r) => {
          release = () => r(song(id, { title: 'Primera' }))
        })
    )
    await filas(w)[0].trigger('click')
    await filas(w)[1].trigger('click')
    await flushPromises()
    expect(w.find('.details-title').text()).toBe('Segunda')
    release()
    await flushPromises()
    expect(w.find('.details-title').text()).toBe('Segunda')
  })

  it('la letra de A que llega con B elegida no devuelve la ficha a A', async () => {
    state.status.configured = true
    state.songs = [song(1, { title: 'Primera' }), song(2, { title: 'Segunda' })]
    const w = await montar()
    await filas(w)[0].trigger('click')
    await flushPromises()
    let release
    api.enrich.mockImplementationOnce(
      () =>
        new Promise((r) => {
          release = r
        })
    )
    await w
      .findAll('.details button')
      .find((b) => b.text().includes('Buscar letra'))
      .trigger('click')
    await filas(w)[1].trigger('click')
    await flushPromises()
    expect(w.find('.details-title').text()).toBe('Segunda')
    release({ result: {}, song: song(1, { title: 'Primera', lyrics: 'mi gozo' }) })
    await flushPromises()
    await flushPromises()
    expect(w.find('.details-title').text(), 'la ficha volvio a la primera').toBe('Segunda')
  })

  it('en «Artistas», que siempre agrupa, Ctrl y Mayus tambien eligen varias', async () => {
    // Al agrupar se perdia el evento del clic por el camino y las teclas no
    // hacian nada; en «Artistas» pasaba siempre.
    state.status.configured = true
    state.songs = [song(1), song(2), song(3)]
    const w = await montar()
    await pulsarNav(w, 'Artistas')
    expect(w.findAll('.group-head').length).toBeGreaterThan(0)
    // las cabeceras de los grupos tambien son filas de la tabla
    const filas = (w) => w.findAll('[data-song-row]')
    const elegidas = () => filas(w).filter((f) => f.classes().includes('selected')).length
    await filas(w)[0].trigger('click')
    await filas(w)[2].trigger('click', { ctrlKey: true })
    await flushPromises()
    expect(elegidas()).toBe(2)
    await filas(w)[0].trigger('click')
    await filas(w)[2].trigger('click', { shiftKey: true })
    await flushPromises()
    expect(elegidas()).toBe(3)
  })

  it('agrupada, Mayus elige el tramo que se ve y la cola va en ese orden', async () => {
    // La lista llega por titulo y se ve por artista. Mayus sacaba el tramo
    // del orden de la lista (no del que se ve) y se llevaba canciones de
    // otro grupo; la cola saltaba de un artista a otro.
    state.status.configured = true
    state.songs = [
      song(1, { title: 'A Una Voz', artist: 'New Wine' }),
      song(2, { title: 'Mi Gozo', artist: 'Barak' }),
      song(3, { title: 'Que Se Abra El Cielo', artist: 'Miel San Marcos' }),
      song(4, { title: 'Sera Llena La Tierra', artist: 'Barak' }),
      song(5, { title: 'Shekinah', artist: 'New Wine' })
    ]
    const w = await montar()
    await pulsarNav(w, 'Artistas')
    const filas = (w) => w.findAll('[data-song-row]')
    const vistas = () => filas(w).map((f) => f.find('td.title').text())
    expect(vistas()).toEqual([
      'Mi Gozo',
      'Sera Llena La Tierra',
      'Que Se Abra El Cielo',
      'A Una Voz',
      'Shekinah'
    ])
    const elegidas = () =>
      filas(w)
        .filter((f) => f.classes().includes('selected'))
        .map((f) => f.find('td.title').text())
    await filas(w)[2].trigger('click')
    await filas(w)[4].trigger('click', { shiftKey: true })
    await flushPromises()
    expect(elegidas()).toEqual(['Que Se Abra El Cielo', 'A Una Voz', 'Shekinah'])

    await filas(w)[0].trigger('dblclick')
    await flushPromises()
    const [items, start] = playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([2, 4, 3, 1, 5])
    expect(start).toBe(2)
  })

  it('si sales del chat mientras responde, lo que pidio se hace igual', async () => {
    // Vue descarta los `emit` de un componente desmontado: «pon la lista X» y
    // cambiar de pagina dejaba la lista sin sonar.
    state.status.configured = true
    state.songs = [song(1), song(2)]
    state.playlists = [{ id: 7, name: 'Domingo', n: 2 }]
    state.playlistSongs = [song(2), song(1)]
    const w = await montar()
    await pulsarNav(w, 'Asistente')
    let release
    api.chatPoll.mockImplementationOnce(
      () =>
        new Promise((r) => {
          release = r
        })
    )
    await w.find('.chat-foot input').setValue('pon la lista Domingo')
    await w.find('.chat-foot .btn').trigger('click')
    await flushPromises()
    await pulsarNav(w, 'Todas las canciones')
    expect(w.find('.chat').exists()).toBe(false)
    release({
      text: 'Pongo Domingo',
      tools: [],
      done: true,
      result: {
        text: 'Pongo Domingo',
        tools: [{ name: 'play', summary: 'Domingo' }],
        actions: [{ kind: 'play_playlist', playlist_id: 7 }],
        confirm: null
      }
    })
    await flushPromises()
    await flushPromises()
    await flushPromises()
    expect(playback.bridge.setQueue).toHaveBeenCalled()
    const [items, , origin] = playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([2, 1])
    expect(origin).toMatchObject({ kind: 'playlist', id: 7 })
    // y la respuesta quedo en la conversacion, para cuando se vuelva
    await pulsarNav(w, 'Asistente')
    expect(w.text()).toContain('Pongo Domingo')
  })
})

// La lista entera, sin cortar: antes se pedian 1000 y la vista se quedaba en
// las mil primeras sin decir nada. Aqui se mira lo que llega, no como se pinta:
// la tabla es un doble (en jsdom no hay maquetacion y pintaria las mil filas).
describe('la lista entera', () => {
  const montarLigera = async () => {
    const w = mount(App, { attachTo: document.body, global: { stubs: { SongTable: true } } })
    await flushPromises()
    await flushPromises()
    await flushPromises()
    return w
  }
  const enLaTabla = (w) => w.findComponent({ name: 'SongTable' }).props('songs')

  it('con mas de mil llegan todas: una pagina enseguida y el resto por detras', async () => {
    state.status.configured = true
    state.songs = Array.from({ length: 1200 }, (_, i) => song(i + 1))
    const w = await montarLigera()
    const pedidas = api.search.mock.calls.map(([p]) => [p.limit, p.from_key])
    expect(pedidas[0]).toEqual([400, 0])
    expect(pedidas.at(-1)).toEqual([5000, 400])
    expect(enLaTabla(w)).toHaveLength(1200)
    expect(enLaTabla(w).at(-1).id).toBe(1200)
    expect(w.find('.filters .chip').text()).toBe('1200')
    // y llegan ligeras: la letra y lo demas se piden con la ficha
    const primera = await api.search.mock.results[0].value
    expect(primera.count).toBe(1200)
    expect(primera.songs[0]).not.toHaveProperty('lyrics')
    expect(primera.songs[0]).toHaveProperty('has_lyrics', false)
  })

  it('si el nucleo no respetara el desplazamiento, no se repiten canciones', async () => {
    state.status.configured = true
    state.songs = Array.from({ length: 900 }, (_, i) => song(i + 1))
    const original = api.search.getMockImplementation()
    api.search.mockImplementation(async (p) => ({
      total: p.limit,
      count: 900,
      songs: state.songs.slice(0, p.limit)
    }))
    const aviso = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const w = await montarLigera()
      const ids = enLaTabla(w).map((s) => s.id)
      expect(ids).toHaveLength(900)
      expect(new Set(ids).size).toBe(900)
    } finally {
      api.search.mockImplementation(original)
      aviso.mockRestore()
    }
  })

  it('las vistas que llegan enteras (un repertorio) cuentan lo que traen', async () => {
    state.status.configured = true
    state.songs = [song(1)]
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
    state.playlistSongs = [song(1), song(2), song(3)]
    const w = await montar()
    await w.find('.nav-playlist').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.find('.filters .chip').text()).toBe('3')
  })
})
