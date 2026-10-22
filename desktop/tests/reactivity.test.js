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

async function mountView() {
  const w = mount(App, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}

describe('startup without folders', () => {
  it('shows the welcome page and does not request songs', async () => {
    const w = await mountView()
    expect(w.text()).toContain('Bienvenido a DanPlay')
    expect(api.search).not.toHaveBeenCalled()
  })

  it('after choosing a folder and scanning, the list appears on its own', async () => {
    const w = await mountView()
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

describe('the core arrives late', () => {
  it('the library fills itself when the core starts', async () => {
    // Abrir DanPlay con una cancion desde el explorador es cuando mas tarda
    // el nucleo: todo pasa a la vez. Si la interfaz carga antes de que
    // conteste, la primera carga FALLA; y antes ese fallo cortaba el
    // arranque antes de escuchar el aviso de «ya estoy», asi que la
    // biblioteca se quedaba vacia para siempre.
    held.coreListener = null
    state.status.configured = true
    api.status.mockRejectedValueOnce(new Error('el nucleo no contesta'))
    const w = await mountView()
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

  it('later notifications do not reload for nothing', async () => {
    state.status.configured = true
    await mountView()
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
describe('the music is no longer where it was', () => {
  it('if the folder moved, it says so and asks where it is now', async () => {
    state.status.configured = true
    state.status.missing_folders = ['/home/ana/Musica']
    const w = await mountView()
    expect(w.text()).toContain('No encuentro tu música')
    expect(w.text()).toContain('/home/ana/Musica')
    expect(w.text()).toContain('¿Dónde está ahora?')
  })

  it('picking it at its new location brings everything back, with no more questions', async () => {
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
    const w = await mountView()
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

  it('if it empties with the app open, the import screen appears on its own', async () => {
    vi.useFakeTimers()
    state.status.configured = true
    state.songs = [song(1)]
    const w = await mountView()
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

  it('with the folders in place but no songs, it invites choosing another', async () => {
    state.status.configured = true
    const w = await mountView()
    expect(w.text()).toContain('Tu biblioteca está vacía')
    expect(w.text()).not.toContain('No encuentro tu música')
  })

  it('in Settings it covers nothing: that is where it gets fixed', async () => {
    state.status.configured = true
    state.status.missing_folders = ['/musica']
    const w = await mountView()
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
describe('something changes in the core behind the scenes', () => {
  it('the list, the playlists and the status refresh on their own', async () => {
    vi.useFakeTimers()
    state.status.configured = true
    state.status.stats.total = 1
    state.songs = [song(1)]
    const w = await mountView()
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

  it('several notifications in a row make a single refresh', async () => {
    vi.useFakeTimers()
    state.status.configured = true
    state.songs = [song(1)]
    await mountView()
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

describe('the open player list', () => {
  it('updates itself when a song arrives from outside', async () => {
    state.status.configured = true
    const w = await mountView()
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

  it('in another view it does not reload for nothing', async () => {
    state.status.configured = true
    await mountView()
    api.externalList.mockClear()
    held.api.fireExternal?.()
    await flushPromises()
    expect(api.externalList).not.toHaveBeenCalled()
  })
})

describe('reindexing from Settings', () => {
  it('on return to the library the view already has the new data', async () => {
    const w = await mountView()
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

describe('table reactivity', () => {
  it('stars update right away', async () => {
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
    const w = await mountView()
    await flushPromises()
    // por su etiqueta, no por el indice: las estrellas van al reves en el DOM
    // para poder pintar «esta y las anteriores» al pasar el raton
    const star = w.findAll('.stars .ico').find((i) => i.attributes('title') === '4 de 5')
    expect(star, 'no hay una estrella con titulo «4 de 5»').toBeTruthy()
    await star.trigger('click')
    await flushPromises()
    expect(api.setStars).toHaveBeenCalledWith(1, 4)
    // solo las de la fila: la ficha de la derecha tambien las enseña
    expect(w.findAll('tbody .stars .ico.on').length).toBe(4)
  })

  it('the favorite updates right away', async () => {
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
    const w = await mountView()
    await w.find('.heart').trigger('click')
    await flushPromises()
    expect(w.find('.heart').classes()).toContain('on')
  })
})

describe('switching view', () => {
  it('the grid shows the same songs as the list', async () => {
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
    const w = await mountView()
    expect(w.find('table').exists()).toBe(true)
    await w.find('.view-switch').findAll('button')[2].trigger('click') // cuadricula
    await flushPromises()
    expect(w.find('.grid').exists()).toBe(true)
    expect(w.text()).toContain('Mi Gozo')
  })

  it('grouping by artist creates group headers', async () => {
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
    const w = await mountView()
    await w
      .findAll('button')
      .find((b) => b.text().includes('Vista'))
      .trigger('click')
    await flushPromises()
    // Se busca por su etiqueta y no por posicion: el menu tiene varios
    // desplegables y basta con añadir uno arriba para que un indice mienta.
    const groupField = w.findAll('.field').find((f) => f.text().includes('Agrupar'))
    expect(groupField, 'no encuentro el desplegable de agrupar').toBeTruthy()
    await groupField.find('.select-box').trigger('click')
    await flushPromises()
    const option = w.findAll('.select-opt').find((o) => o.text().includes('Por artista'))
    expect(option, 'no encuentro la opcion "Por artista"').toBeTruthy()
    await option.trigger('click')
    await flushPromises()
    const headers = w.findAll('.group-head').map((c) => c.text())
    expect(headers.some((t) => t.includes('Barak'))).toBe(true)
    expect(headers.some((t) => t.includes('New Wine'))).toBe(true)
  })
})

describe('switching view from the sidebar', () => {
  const oneSong = () => [
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

  async function withLibrary() {
    state.status.configured = true
    state.songs = oneSong()
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    return w
  }
  const clickButton = async (w, text) => {
    const e = w.findAll('.nav-link').find((x) => x.text().includes(text))
    expect(e, `no encuentro el enlace "${text}"`).toBeTruthy()
    await e.trigger('click')
    await settle()
  }

  it('the assistant receives what was being viewed, the selection and what is playing', async () => {
    // «pon la segunda», «esta», «las seleccionadas»: sin esto el modelo no
    // sabia a que se referia la persona
    state.status.configured = true
    state.songs = [...oneSong(), { ...oneSong()[0], id: 2, title: 'Shekinah', artist: 'New Wine' }]
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    await clickButton(w, 'Asistente')
    // la pregunta del primer arranque (abrir las canciones con DanPlay) es un
    // diálogo modal: con él abierto, lo que se escribe espera en la cola
    dialogCancel()
    await w.find('.chat-foot textarea').setValue('pon la segunda')
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

  it('Favorites changes the title and requests only favorites', async () => {
    const w = await withLibrary()
    await clickButton(w, 'Favoritos')
    expect(w.find('.filters').text()).toContain('Favoritos')
    const last = api.search.mock.calls.at(-1)[0]
    expect(last.only_favorites).toBe(true)
  })

  it('Artists groups by artist', async () => {
    const w = await withLibrary()
    await clickButton(w, 'Artistas')
    expect(w.find('.filters').text()).toContain('Artistas')
    expect(w.findAll('.group-head').length).toBeGreaterThan(0)
  })

  it('the assistant stays as it was after leaving and coming back, and catches up', async () => {
    // Las paginas se cargan al abrirlas; el asistente, ademas, se guarda al
    // salir (<KeepAlive>): lo que tenias abierto en el sigue abierto.
    const w = await withLibrary()
    await clickButton(w, 'Asistente')
    await w.find('.chat-head button[aria-expanded]').trigger('click')
    expect(w.find('.chat-list').exists()).toBe(true)
    const before = api.chats.mock.calls.length
    await clickButton(w, 'Todas las canciones')
    expect(w.find('.chat').exists()).toBe(false)
    expect(w.findAll('[data-song-row]').length).toBeGreaterThan(0)
    await clickButton(w, 'Asistente')
    expect(w.find('.chat-list').exists(), 'la lista de conversaciones se cerro').toBe(true)
    // y al volver se relee lo que haya cambiado mientras tanto
    expect(api.chats.mock.calls.length).toBeGreaterThan(before)
  })

  it('Duplicates shows its own page', async () => {
    const w = await withLibrary()
    await clickButton(w, 'Duplicados')
    expect(w.text()).toContain('Escúchalas y quédate con la que prefieras')
    expect(api.duplicatesScan).toHaveBeenCalled()
  })

  it('Inbox shows its own page', async () => {
    const w = await withLibrary()
    await clickButton(w, 'Entrada')
    // se comprueba lo que la pagina hace, no su redaccion exacta
    expect(w.text()).toContain('~/Musica/Entrada')
    expect(w.text()).toContain('Cómo funciona')
    expect(w.findAll('.steps li').length).toBe(4)
    expect(w.text()).toContain('Probar sin tocar nada')
  })

  it('Settings and back to the library keeps the songs', async () => {
    const w = await withLibrary()
    await clickButton(w, 'Ajustes')
    expect(w.text()).toContain('Apariencia')
    await clickButton(w, 'Todas las canciones')
    expect(w.text()).toContain('Mi Gozo')
  })

  it('never goes back to the welcome page if there are songs', async () => {
    const w = await withLibrary()
    for (const v of ['Favoritos', 'Artistas', 'Duplicados', 'Todas las canciones']) {
      await clickButton(w, v)
      expect(w.text(), `la bienvenida reaparecio en ${v}`).not.toContain('Bienvenido a DanPlay')
    }
  })
})

describe('adding the same folder several times', () => {
  const field = (w) => w.find('.page input[type="text"], .page input:not([type])')
  const analyze = async (w) => {
    await w
      .findAll('.page button')
      .find((b) => b.text() === 'Analizar')
      .trigger('click')
    await flushPromises()
    await flushPromises()
    await flushPromises()
  }

  it('the first time it adds it and loads the library', async () => {
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    await field(w).setValue('/musica')
    await analyze(w)
    expect(api.addFolder).toHaveBeenCalled()
    expect(w.text()).toContain('Mi Gozo')
  })

  it('if the backend says it was already there, it warns but loads anyway', async () => {
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
    await field(w2).setValue('/musica')
    await analyze(w2)
    state.status.configured = true

    // no se reescanea: si ya estaba indexada, releerla entera es trabajo
    // tirado. Lo que si tiene que pasar es que se explique.
    expect(w2.find('.toast').exists(), 'deberia salir un aviso flotante').toBe(true)
    expect(w2.find('.toast').text()).toContain('ya está añadida')
    expect(w2.text()).toContain('Mi Gozo') // y la biblioteca queda cargada
    expect(w2.text()).not.toContain('Bienvenido a DanPlay')
  })

  it('the same path is not added twice', async () => {
    const w = mount(App, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    await field(w).setValue('/musica')
    await analyze(w)
    const paths = api.addFolder.mock.calls.map((c) => c[0])
    const unique = new Set(paths)
    expect(unique.size).toBe(paths.length)
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
const songPair = twoSongs

/** Id de la fila marcada como sonando, o null. */
function playing(w) {
  const rows = w.findAll('tbody tr')
  const i = rows.findIndex((f) => f.classes().includes('playing'))
  return i < 0 ? null : i + 1
}

async function play(w, index) {
  await w.findAll('tbody tr')[index].find('.row-play').trigger('click')
  await flushPromises()
}

describe('the repeat button', () => {
  it('cycles through the four modes and tells whoever owns the queue', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await mountView()
    for (const expected of ['one', 'once', 'queue', 'list']) {
      await w.find('.repeat-btn').trigger('click')
      await flushPromises()
      expect(playback.bridge.setRepeat).toHaveBeenLastCalledWith(expected)
    }
  })

  it('the chosen mode is remembered between sessions', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await mountView()
    await w.find('.repeat-btn').trigger('click')
    await flushPromises()
    expect(localStorage.getItem('danplay.repeat')).toBe('one')
  })
})

describe('playing a song', () => {
  it('sends the whole list as the queue, starting with the clicked one', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await mountView()
    await play(w, 1)
    const [items, start] = playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([1, 2])
    expect(start).toBe(2)
    expect(playing(w)).toBe(2)
  })
})

// El boton de la fila de la cancion que suena es pausa, y pulsado reanuda:
// antes volvia a empezar la cancion y desde la lista no habia forma de pararla.
describe('pausing from the row itself', () => {
  it('on the playing one, the button pauses and resumes instead of restarting', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await mountView()
    await play(w, 1)
    expect(playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    const button = () => w.findAll('tbody tr')[1].find('.row-play')
    expect(button().attributes('title')).toBe('Pausar')
    expect(button().find('svg').exists()).toBe(true)

    await play(w, 1) // otra vez sobre la misma
    await flushPromises()
    await flushPromises()
    expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    expect(button().attributes('title')).toBe('Reanudar')
    // la otra fila sigue ofreciendo reproducir
    expect(w.findAll('tbody tr')[0].find('.row-play').attributes('title')).toBe('Reproducir')
  })

  it('the sidebar dot marks where the playing song comes from', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await mountView()
    expect(w.find('.now-dot').exists()).toBe(false)
    await play(w, 0)
    const all = w.findAll('.nav-link').find((b) => b.text().includes('Todas las canciones'))
    expect(all.find('.now-dot').exists()).toBe(true)
    expect(all.find('.now-dot').classes()).not.toContain('paused')
    await play(w, 0) // pausa
    expect(all.find('.now-dot').classes()).toContain('paused')
  })
})

// El menu de la cancion: «Enviar por Telegram» solo si Telegram esta en el
// equipo (lo dice Rust al arrancar), y «Abrir la carpeta» siempre.
describe('the context menu of a song', () => {
  async function openMenu() {
    state.status.configured = true
    state.songs = [song(1), song(2)] // con su ruta, como las de verdad
    const w = await mountView()
    await w.findAll('tbody tr')[0].trigger('contextmenu')
    await flushPromises()
    return w
  }
  const labels = (w) => w.findAll('.ctx-item .ctx-label').map((b) => b.text())

  it('with Telegram installed it offers to send the song, and sends its path', async () => {
    const w = await openMenu()
    expect(labels(w)).toContain('Enviar por Telegram')
    expect(labels(w)).toContain('Abrir la carpeta')
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

  it('without Telegram, the option does not appear', async () => {
    held.app.shareTargets.mockResolvedValueOnce({ telegram: false })
    const w = await openMenu()
    expect(labels(w)).not.toContain('Enviar por Telegram')
    expect(labels(w)).toContain('Abrir la carpeta')
  })

  it('with the panel visible it does not offer «Ver detalles»', async () => {
    const w = await openMenu()
    expect(w.find('.details').exists()).toBe(true)
    expect(labels(w)).not.toContain('Ver detalles')
  })
})

// Ctrl y Mayus al pulsar seleccionan varias; el menu sobre una de ellas actua
// sobre todas: enviar, añadir a una lista, papelera (con una confirmacion que
// dice cuantas).
describe('multiple selection', () => {
  const three = () => [song(1), song(2), song(3)]
  async function list() {
    state.status.configured = true
    state.songs = three()
    return mountView()
  }
  const rows = (w) => w.findAll('tbody tr')
  const selectedCount = (w) => rows(w).filter((f) => f.classes().includes('selected')).length

  it('Ctrl adds and removes; Shift takes the range; Ctrl+Shift adds it', async () => {
    const w = await list()
    await rows(w)[0].trigger('click')
    await rows(w)[2].trigger('click', { ctrlKey: true })
    await flushPromises()
    expect(selectedCount(w)).toBe(2)
    await rows(w)[2].trigger('click', { ctrlKey: true }) // la quita
    await flushPromises()
    expect(selectedCount(w)).toBe(1)
    await rows(w)[0].trigger('click')
    await rows(w)[2].trigger('click', { shiftKey: true }) // 1..3
    await flushPromises()
    expect(selectedCount(w)).toBe(3)
    await rows(w)[1].trigger('click') // sin teclas: solo esa
    await flushPromises()
    expect(selectedCount(w)).toBe(1)
  })

  it('the menu on the selection acts on all of them', async () => {
    const w = await list()
    await rows(w)[0].trigger('click')
    await rows(w)[2].trigger('click', { shiftKey: true })
    await flushPromises()
    await rows(w)[1].trigger('contextmenu')
    await flushPromises()
    const labels = w.findAll('.ctx-item .ctx-label').map((b) => b.text())
    expect(labels).toContain('Enviar 3 por Telegram')
    expect(labels).toContain('Añadir 3 a una lista')
    expect(labels).toContain('Mandar 3 a la papelera…')
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

  it('trashing several asks for confirmation and says how many', async () => {
    const w = await list()
    await rows(w)[0].trigger('click')
    await rows(w)[1].trigger('click', { ctrlKey: true })
    await flushPromises()
    await rows(w)[0].trigger('contextmenu')
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

  it('the menu on one that is NOT in the selection is for that one alone', async () => {
    const w = await list()
    await rows(w)[0].trigger('click')
    await rows(w)[1].trigger('click', { ctrlKey: true })
    await flushPromises()
    await rows(w)[2].trigger('contextmenu')
    await flushPromises()
    const labels = w.findAll('.ctx-item .ctx-label').map((b) => b.text())
    expect(labels).toContain('Enviar por Telegram')
    expect(labels).not.toContain('Enviar 2 por Telegram')
  })
})

describe('the playlist menu', () => {
  it('offers renaming and sending via Telegram', async () => {
    state.status.configured = true
    state.songs = [song(1)]
    state.playlists = [{ id: 1, name: 'domingo', n: 1 }]
    const w = await mountView()
    await w.find('.nav-playlist').trigger('contextmenu')
    await flushPromises()
    const labels = w.findAll('.ctx-item .ctx-label').map((b) => b.text())
    expect(labels).toContain('Renombrar…')
    expect(labels).toContain('Enviar por Telegram')
  })
})

describe('the details panel, hidden or visible', () => {
  it('when closed (via settings) «Ver detalles» shows up and opens the dialog', async () => {
    const { usePreferences } = await import('../src/composables/usePreferences.js')
    usePreferences().showDetails.value = false
    state.status.configured = true
    state.songs = [song(1)]
    const w = await mountView()
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

describe('the play button uses the selection', () => {
  it('with nothing loaded, it plays the selected song', async () => {
    state.status.configured = true
    state.songs = songPair()
    const w = await mountView()
    await w.findAll('tbody tr')[1].trigger('click') // seleccionar la segunda
    await flushPromises()
    expect(playing(w), 'seleccionar no reproduce').toBe(null)
    await w.find('.pl-play').trigger('click')
    await flushPromises()
    expect(playing(w)).toBe(2)
  })
})

// --------------------------------------------------- arrastrar canciones
// Se coge una cancion de la lista y se suelta en un repertorio del menu, en
// Favoritos o en «Nueva lista». Lo que NO puede pasar es que un clic normal
// acabe moviendo algo sin querer, asi que hay un umbral antes de arrastrar.
describe('dragging a song', () => {
  async function withPlaylist() {
    state.status.configured = true
    state.songs = songPair()
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
    return await mountView()
  }
  const row = (w, i = 0) => w.findAll('tbody tr')[i]
  const playlistLink = (w) => w.find('.nav-playlist')
  const favorites = (w) => w.find('[data-drop="favorites"]')

  // jsdom no trae PointerEvent, y test-utils no deja poner las coordenadas
  // encima de un MouseEvent ya creado: se lanza a mano.
  function pointer(el, type, { pointerType, ...rest } = {}) {
    const ev = new MouseEvent(type, { bubbles: true, cancelable: true, ...rest })
    if (pointerType) Object.defineProperty(ev, 'pointerType', { value: pointerType })
    el.element.dispatchEvent(ev)
    return flushPromises()
  }

  it('a click without moving the pointer drags nothing', async () => {
    const w = await withPlaylist()
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10 })
    expect(w.vm.drag.song).toBe(null)
    await pointer(playlistLink(w), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).not.toHaveBeenCalled()
  })

  it('past the threshold the drag starts and the ghost appears', async () => {
    const w = await withPlaylist()
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(row(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(w.vm.drag.song.id).toBe(1)
    expect(document.querySelector('.drag-ghost').textContent).toContain('Primera')
    expect(row(w).classes()).toContain('dragged')
    await pointer(playlistLink(w), 'pointerup')
  })

  it('dropping it on a playlist adds it there', async () => {
    const w = await withPlaylist()
    await pointer(row(w, 1), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(playlistLink(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(playlistLink(w).classes()).toContain('drop-over')
    await pointer(playlistLink(w), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledWith(7, [2])
    expect(w.vm.drag.song, 'el arrastre no termino').toBe(null)
  })

  it('dropping it on Favorites marks it as a favorite', async () => {
    const w = await withPlaylist()
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(favorites(w), 'pointermove', { clientX: 90, clientY: 120 })
    await pointer(favorites(w), 'pointerup')
    await flushPromises()
    expect(api.toggleFavorite).toHaveBeenCalledWith(1, true)
  })

  it('dropping it where there is no target does nothing', async () => {
    const w = await withPlaylist()
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(row(w, 1), 'pointermove', { clientX: 90, clientY: 200 })
    await pointer(row(w, 1), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).not.toHaveBeenCalled()
    expect(api.toggleFavorite).not.toHaveBeenCalled()
  })

  it('Escape lets go of what is being dragged without dropping it anywhere', async () => {
    const w = await withPlaylist()
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(playlistLink(w), 'pointermove', { clientX: 90, clientY: 200 })
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(w.vm.drag.song).toBe(null)
    expect(document.querySelector('.drag-ghost')).toBe(null)
    expect(api.addToPlaylist).not.toHaveBeenCalled()
  })

  it('a finger does not drag: on touch the list scrolls', async () => {
    const w = await withPlaylist()
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10, pointerType: 'touch' })
    await pointer(playlistLink(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(w.vm.drag.song).toBe(null)
  })

  it('drop targets are only highlighted while something is being dragged', async () => {
    const w = await withPlaylist()
    expect(w.find('.sidebar').classes()).not.toContain('drop-ready')
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(row(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(w.find('.sidebar').classes()).toContain('drop-ready')
    await pointer(row(w), 'pointerup')
    await flushPromises()
    expect(w.find('.sidebar').classes()).not.toContain('drop-ready')
  })

  it('outside a playlist rows are not targets: dropping on another changes nothing', async () => {
    const w = await withPlaylist()
    expect(row(w).attributes('data-drop')).toBeUndefined()
    await pointer(row(w), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(row(w, 1), 'pointermove', { clientX: 90, clientY: 200 })
    await pointer(row(w, 1), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
  })

  it('a leftover from the movable layout does not break the view', async () => {
    // aquella opcion guardaba un objeto en la misma clave que el formato
    localStorage.setItem('danplay.layout', '{"topbar":"bottom","player":"top"}')
    const w = await withPlaylist()
    expect(w.vm.layout).toBe('table')
    expect(w.findAll('tbody tr')).toHaveLength(2)
  })

  it('nothing is left for moving the panels around', async () => {
    const w = await withPlaylist()
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
describe('after clicking a button with the mouse', () => {
  const press = (key) => {
    const target = document.activeElement || window
    target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    return flushPromises()
  }
  async function playing() {
    state.status.configured = true
    state.songs = songPair()
    // sin el dialogo de «¿abrir las canciones con DanPlay?», que bloquea los atajos
    localStorage.setItem('danplay.default-player-asked', '1')
    const w = await mountView()
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

  it('the button releases focus and Space pauses again', async () => {
    const w = await playing()
    const next = w
      .findAll('.player .pl-btn')
      .find((b) => b.attributes('title')?.startsWith('Siguiente'))
    next.element.focus()
    expect(document.activeElement).toBe(next.element)
    next.element.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    await flushPromises()
    expect(document.activeElement).not.toBe(next.element)
    await press(' ')
    expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(playback.bridge.next).not.toHaveBeenCalled()
  })

  it('a playlist in the sidebar too', async () => {
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
    const w = await playing()
    const nav = w.find('.nav-playlist')
    nav.element.focus()
    nav.element.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    await flushPromises()
    expect(document.activeElement).not.toBe(nav.element)
    await press(' ')
    expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
  })

  it('inside a dialog focus is left alone', async () => {
    const w = await playing()
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
describe('reordering a playlist by dragging', () => {
  const three = () => [
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
  async function inPlaylist(others = []) {
    state.status.configured = true
    state.songs = three()
    state.playlistSongs = three()
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }, ...others]
    const w = await mountView()
    await w.find('.nav-playlist').trigger('click')
    await flushPromises()
    await flushPromises()
    return w
  }
  const row = (w, i = 0) => w.findAll('tbody tr')[i]
  const titles = (w) => w.findAll('tbody tr td.title').map((td) => td.text().trim())
  function pointer(el, type, rest = {}) {
    el.element.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, ...rest }))
    return flushPromises()
  }
  // jsdom no maqueta: se le dice a la fila donde esta para poder apuntar a
  // su mitad de arriba o a la de abajo
  function place(el, top = 100, height = 30) {
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

  it('the playlist rows are targets, and it announces it can be sorted', async () => {
    const w = await inPlaylist()
    expect(titles(w)).toEqual(['Primera', 'Segunda', 'Tercera'])
    expect(row(w).attributes('data-drop')).toBe('sort:1')
    expect(w.find('[data-sort-list]').exists()).toBe(true)
    expect(w.find('.sort-hint').text()).toContain('arrastra')
  })

  it('dropping it on the bottom half of another puts it after', async () => {
    const w = await inPlaylist()
    place(row(w, 2))
    await pointer(row(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(row(w, 2), 'pointermove', { clientX: 90, clientY: 125 })
    expect(row(w, 2).classes()).toContain('drop-after')
    expect(row(w, 2).classes()).not.toContain('drop-before')
    await pointer(row(w, 2), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 3, 1])
    expect(titles(w)).toEqual(['Segunda', 'Tercera', 'Primera'])
    expect(w.vm.drag.song).toBe(null)
  })

  it('and on the top half, before', async () => {
    const w = await inPlaylist()
    place(row(w, 0))
    await pointer(row(w, 2), 'pointerdown', { clientX: 10, clientY: 300 })
    await pointer(row(w, 0), 'pointermove', { clientX: 90, clientY: 105 })
    expect(row(w, 0).classes()).toContain('drop-before')
    await pointer(row(w, 0), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [3, 1, 2])
    expect(titles(w)).toEqual(['Tercera', 'Primera', 'Segunda'])
  })

  it('leaving it where it was requests nothing from the core', async () => {
    const w = await inPlaylist()
    place(row(w, 1))
    // la primera, justo encima de la segunda: es su sitio de siempre
    await pointer(row(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(row(w, 1), 'pointermove', { clientX: 90, clientY: 105 })
    await pointer(row(w, 1), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
    expect(titles(w)).toEqual(['Primera', 'Segunda', 'Tercera'])
  })

  it('nothing is highlighted over the dragged row itself', async () => {
    const w = await inPlaylist()
    place(row(w, 0))
    await pointer(row(w, 0), 'pointerdown', { clientX: 10, clientY: 105 })
    await pointer(row(w, 0), 'pointermove', { clientX: 90, clientY: 125 })
    expect(row(w, 0).classes()).toContain('dragged')
    expect(row(w, 0).classes()).not.toContain('drop-after')
    await pointer(row(w, 0), 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
  })

  it('if the core cannot do it, the playlist goes back to how it was', async () => {
    const w = await inPlaylist()
    api.reorderPlaylist.mockRejectedValueOnce(new Error('sin base de datos'))
    place(row(w, 2))
    await pointer(row(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(row(w, 2), 'pointermove', { clientX: 90, clientY: 125 })
    await pointer(row(w, 2), 'pointerup')
    await flushPromises()
    await flushPromises()
    expect(titles(w)).toEqual(['Primera', 'Segunda', 'Tercera'])
    expect(w.find('.toast').text()).toContain('No se pudo cambiar el orden')
  })

  it('headers do not sort a playlist: they say so and do not reload', async () => {
    const w = await inPlaylist()
    expect(w.find('thead th[aria-sort]').exists(), 'no hay columna que mande').toBe(false)
    const before = api.playlistSongs.mock.calls.length
    await w.find('thead th.sortable .th-sort').trigger('click')
    await flushPromises()
    expect(w.find('.toast').text()).toContain('arrastra')
    expect(api.playlistSongs.mock.calls.length).toBe(before)
  })

  it('in the compact list, rows can be sorted too', async () => {
    const w = await inPlaylist()
    w.vm.layout = 'rows'
    await flushPromises()
    const rows = w.findAll('.rows .row')
    expect(rows).toHaveLength(3)
    expect(rows[0].attributes('data-drop')).toBe('sort:1')
    place(rows[2])
    await pointer(rows[0], 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(rows[2], 'pointermove', { clientX: 90, clientY: 125 })
    await pointer(rows[2], 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 3, 1])
  })

  it('in the cards view the left or right half is used', async () => {
    const w = await inPlaylist()
    w.vm.layout = 'cards'
    await flushPromises()
    const cards = w.findAll('.card-song')
    expect(cards[0].attributes('data-drop-axis')).toBe('x')
    cards[2].element.getBoundingClientRect = () => ({
      top: 0,
      height: 60,
      bottom: 60,
      left: 300,
      width: 200,
      right: 500,
      x: 300,
      y: 0
    })
    await pointer(cards[0], 'pointerdown', { clientX: 10, clientY: 10 })
    // a la izquierda de la tercera, aunque sea por su mitad de abajo
    await pointer(cards[2], 'pointermove', { clientX: 320, clientY: 55 })
    expect(cards[2].classes()).toContain('drop-before')
    await pointer(cards[2], 'pointerup')
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 1, 3])
  })

  it('from the list to the sidebar still works: it is added to another playlist', async () => {
    const w = await inPlaylist([{ id: 8, name: 'Lunes', n: 0 }])
    const other = w.findAll('.nav-playlist')[1]
    await pointer(row(w, 0), 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(other, 'pointermove', { clientX: 90, clientY: 200 })
    await pointer(other, 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledWith(8, [1])
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
  })
})

// ----------------------------------------------------- salir de la aplicacion
// Con la bandeja, cerrar la ventana ya no cierra DanPlay: la unica salida
// desde la interfaz es esta. Lo que le cuenta a la bandeja ya no sale de
// aqui: el estado vive en Rust y el icono lo lee de alli.
describe('quitting DanPlay', () => {
  it('the View menu offers to quit', async () => {
    state.status.configured = true
    state.songs = twoSongs()
    const w = await mountView()
    await w
      .findAll('button')
      .find((b) => b.text().includes('Vista'))
      .trigger('click')
    await flushPromises()
    const quit = w.findAll('button').find((b) => b.text().includes('Salir de DanPlay'))
    expect(quit, 'no encuentro como salir de la aplicacion').toBeTruthy()
  })
})

describe('typing in the search box', () => {
  it('does not fire a search on every keystroke', async () => {
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

      const searchBox = w.find('.topbar input')
      for (const t of ['b', 'ba', 'bar', 'bara', 'barak']) {
        await searchBox.setValue(t)
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
describe('late responses', () => {
  const rows = (w) => w.findAll('tbody tr')
  const clickNav = async (w, text) => {
    await w
      .findAll('.nav-link')
      .find((x) => x.text().includes(text))
      .trigger('click')
    await settle()
  }

  it('clicking a row while the list loads does not leave it half done or spinning', async () => {
    // Compartian contador: elegir una fila (o abrir su menu, que tambien la
    // elige) daba la carga por vieja, la tiraba y el indicador no paraba.
    state.status.configured = true
    state.songs = [song(1), song(2), song(3)]
    const w = await mountView()
    expect(rows(w)).toHaveLength(3)
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
    await rows(w)[1].trigger('click')
    await rows(w)[2].trigger('contextmenu')
    await flushPromises()
    release({ total: 4, count: 4, songs: [song(1), song(2), song(3), song(4)] })
    await flushPromises()
    await flushPromises()
    expect(rows(w)).toHaveLength(4)
    expect(w.find('.filters .loading').exists(), 'el indicador sigue girando').toBe(false)
  })

  it('two clicks in a row do not end up showing the details of the first', async () => {
    state.status.configured = true
    state.songs = [song(1, { title: 'Primera' }), song(2, { title: 'Segunda' })]
    const w = await mountView()
    let release
    api.song.mockImplementationOnce(
      (id) =>
        new Promise((r) => {
          release = () => r(song(id, { title: 'Primera' }))
        })
    )
    await rows(w)[0].trigger('click')
    await rows(w)[1].trigger('click')
    await flushPromises()
    expect(w.find('.details-title').text()).toBe('Segunda')
    release()
    await flushPromises()
    expect(w.find('.details-title').text()).toBe('Segunda')
  })

  it('the lyrics of A arriving with B selected do not switch the details panel back to A', async () => {
    state.status.configured = true
    state.songs = [song(1, { title: 'Primera' }), song(2, { title: 'Segunda' })]
    const w = await mountView()
    await rows(w)[0].trigger('click')
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
    await rows(w)[1].trigger('click')
    await flushPromises()
    expect(w.find('.details-title').text()).toBe('Segunda')
    release({ result: {}, song: song(1, { title: 'Primera', lyrics: 'mi gozo' }) })
    await flushPromises()
    await flushPromises()
    expect(w.find('.details-title').text(), 'la ficha volvio a la primera').toBe('Segunda')
  })

  it('in «Artistas», which always groups, Ctrl and Shift also select several', async () => {
    // Al agrupar se perdia el evento del clic por el camino y las teclas no
    // hacian nada; en «Artistas» pasaba siempre.
    state.status.configured = true
    state.songs = [song(1), song(2), song(3)]
    const w = await mountView()
    await clickNav(w, 'Artistas')
    expect(w.findAll('.group-head').length).toBeGreaterThan(0)
    // las cabeceras de los grupos tambien son filas de la tabla
    const rows = (w) => w.findAll('[data-song-row]')
    const picked = () => rows(w).filter((f) => f.classes().includes('selected')).length
    await rows(w)[0].trigger('click')
    await rows(w)[2].trigger('click', { ctrlKey: true })
    await flushPromises()
    expect(picked()).toBe(2)
    await rows(w)[0].trigger('click')
    await rows(w)[2].trigger('click', { shiftKey: true })
    await flushPromises()
    expect(picked()).toBe(3)
  })

  it('when grouped, Shift selects the visible range and the queue follows that order', async () => {
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
    const w = await mountView()
    await clickNav(w, 'Artistas')
    const rows = (w) => w.findAll('[data-song-row]')
    const views = () => rows(w).map((f) => f.find('td.title').text())
    expect(views()).toEqual([
      'Mi Gozo',
      'Sera Llena La Tierra',
      'Que Se Abra El Cielo',
      'A Una Voz',
      'Shekinah'
    ])
    const picked = () =>
      rows(w)
        .filter((f) => f.classes().includes('selected'))
        .map((f) => f.find('td.title').text())
    await rows(w)[2].trigger('click')
    await rows(w)[4].trigger('click', { shiftKey: true })
    await flushPromises()
    expect(picked()).toEqual(['Que Se Abra El Cielo', 'A Una Voz', 'Shekinah'])

    await rows(w)[0].trigger('dblclick')
    await flushPromises()
    const [items, start] = playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([2, 4, 3, 1, 5])
    expect(start).toBe(2)
  })

  it('if you leave the chat while it answers, what it requested still happens', async () => {
    // Vue descarta los `emit` de un componente desmontado: «pon la lista X» y
    // cambiar de pagina dejaba la lista sin sonar.
    state.status.configured = true
    state.songs = [song(1), song(2)]
    state.playlists = [{ id: 7, name: 'Domingo', n: 2 }]
    state.playlistSongs = [song(2), song(1)]
    const w = await mountView()
    await clickNav(w, 'Asistente')
    let release
    api.chatPoll.mockImplementationOnce(
      () =>
        new Promise((r) => {
          release = r
        })
    )
    // la pregunta del primer arranque es un diálogo modal: se contesta antes
    dialogCancel()
    await w.find('.chat-foot textarea').setValue('pon la lista Domingo')
    await w.find('.chat-foot .btn').trigger('click')
    await flushPromises()
    await clickNav(w, 'Todas las canciones')
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
    await clickNav(w, 'Asistente')
    expect(w.text()).toContain('Pongo Domingo')
  })
})

// La lista entera, sin cortar: antes se pedian 1000 y la vista se quedaba en
// las mil primeras sin decir nada. Aqui se mira lo que llega, no como se pinta:
// la tabla es un doble (en jsdom no hay maquetacion y pintaria las mil filas).
describe('the whole list', () => {
  const mountLight = async () => {
    const w = mount(App, { attachTo: document.body, global: { stubs: { SongTable: true } } })
    await flushPromises()
    await flushPromises()
    await flushPromises()
    return w
  }
  const inTable = (w) => w.findComponent({ name: 'SongTable' }).props('songs')

  it('with more than a thousand, all of them arrive: one page right away and the rest in the background', async () => {
    state.status.configured = true
    state.songs = Array.from({ length: 1200 }, (_, i) => song(i + 1))
    const w = await mountLight()
    const requested = api.search.mock.calls.map(([p]) => [p.limit, p.from_key])
    expect(requested[0]).toEqual([400, 0])
    expect(requested.at(-1)).toEqual([5000, 400])
    expect(inTable(w)).toHaveLength(1200)
    expect(inTable(w).at(-1).id).toBe(1200)
    expect(w.find('.filters .chip').text()).toBe('1200')
    // y llegan ligeras: la letra y lo demas se piden con la ficha
    const first = await api.search.mock.results[0].value
    expect(first.count).toBe(1200)
    expect(first.songs[0]).not.toHaveProperty('lyrics')
    expect(first.songs[0]).toHaveProperty('has_lyrics', false)
  })

  it('if the core ignored the offset, songs would still not repeat', async () => {
    state.status.configured = true
    state.songs = Array.from({ length: 900 }, (_, i) => song(i + 1))
    const original = api.search.getMockImplementation()
    api.search.mockImplementation(async (p) => ({
      total: p.limit,
      count: 900,
      songs: state.songs.slice(0, p.limit)
    }))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const w = await mountLight()
      const ids = inTable(w).map((s) => s.id)
      expect(ids).toHaveLength(900)
      expect(new Set(ids).size).toBe(900)
    } finally {
      api.search.mockImplementation(original)
      warn.mockRestore()
    }
  })

  it('views that arrive whole (a playlist) count what they bring', async () => {
    state.status.configured = true
    state.songs = [song(1)]
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
    state.playlistSongs = [song(1), song(2), song(3)]
    const w = await mountView()
    await w.find('.nav-playlist').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.find('.filters .chip').text()).toBe('3')
  })
})
