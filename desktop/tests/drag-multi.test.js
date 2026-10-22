// Arrastrar varias canciones a la vez (la selección múltiple).
//
// Lo que se promete: si la canción que se agarra es una de las elegidas (y son
// dos o más), el arrastre las lleva TODAS, en el orden en que se ven
// (`drag.songs`, además de `drag.song`, la agarrada); el fantasma lo dice con
// una insignia y una pila. Si se agarra una que no está elegida, va sola y la
// selección no cambia. Al soltar, cada destino hace UNA cosa con todas: un
// repertorio las añade de una vez (un aviso), Favoritos marca las que no lo
// eran, «Nueva lista» la crea con ellas y, dentro de un repertorio, el bloque
// entero cambia de sitio sin perder su orden. Con el dedo, con el botón
// derecho o desde un control no se arrastra nada, como con una sola.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

const held = vi.hoisted(() => ({ state: null, api: null, playback: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble, createAppDouble } =
    await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: held.playback.bridge,
    app: createAppDouble(),
    core: { onStatus: v.fn(async () => () => {}), onChanged: v.fn(async () => () => {}) },
    projection: { show: v.fn(async () => {}), hide: v.fn(async () => {}) }
  }
})

import App from '../src/App.vue'
import SongTable from '../src/components/SongTable.vue'
import SongRows from '../src/components/SongRows.vue'
import SongCards from '../src/components/SongCards.vue'
import SongGrid from '../src/components/SongGrid.vue'
import { song } from './support/backend.js'
import { startDrag, cancelDrag, onDrop, useDragSong } from '../src/composables/useDragSong.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetPreferences } from '../src/composables/usePreferences.js'
import { resetChat } from '../src/composables/useChat.js'
import { dialogCancel, dialogOk, useDialog } from '../src/composables/useDialog.js'
import { clearNotices } from '../src/composables/useNotices.js'
import { allCss } from './support/css.js'

const api = held.api
const state = held.state
const playback = held.playback
const { drag } = useDragSong()

const four = () => [
  song(1, { title: 'Uno' }),
  song(2, { title: 'Dos' }),
  song(3, { title: 'Tres' }),
  song(4, { title: 'Cuatro' })
]

beforeEach(() => {
  state.songs = four()
  state.playlists = [{ id: 7, name: 'Domingo', n: 0 }]
  state.playlistSongs = []
  state.status.configured = true
  cancelDrag()
  resetPlayback()
  resetPreferences()
  resetChat()
  clearNotices()
  dialogCancel()
  playback.reset()
  localStorage.clear()
  localStorage.setItem('danplay.default-player-asked', '1')
  vi.clearAllMocks()
})

afterEach(() => {
  cancelDrag()
  document.body.innerHTML = ''
})

async function mountView() {
  const w = mount(App, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}

// jsdom no trae PointerEvent: se lanza un MouseEvent con ese nombre, con las
// coordenadas y, si hace falta, el tipo de puntero encima.
function pointer(el, type, { pointerType, ...rest } = {}) {
  const ev = new MouseEvent(type, { bubbles: true, cancelable: true, ...rest })
  if (pointerType) Object.defineProperty(ev, 'pointerType', { value: pointerType })
  ;(el.element || el).dispatchEvent(ev)
  return flushPromises()
}
/** Coge la fila, la lleva hasta `destino` y (si se pide) la suelta ahí. */
async function dragTo(row, dropTarget, { release = true, x = 90, y = 200 } = {}) {
  await pointer(row, 'pointerdown', { clientX: 10, clientY: 10 })
  await pointer(dropTarget, 'pointermove', { clientX: x, clientY: y })
  if (release) await pointer(dropTarget, 'pointerup', { clientX: x, clientY: y })
}

const tableRows = (w) => w.findAll('tbody tr')
const titles = (w) => tableRows(w).map((f) => f.find('td.title').text().trim())
const selectedCount = (w) =>
  tableRows(w)
    .filter((f) => f.classes().includes('selected'))
    .map((f) => f.find('td.title').text().trim())
const button = (w) => w.find('.filters .select-toggle')
const playlistLink = (w, i = 0) => w.findAll('.nav-playlist')[i]
const favorites = (w) => w.find('[data-drop="favorites"]')
const newPlaylist = (w) => w.find('[data-drop="new-playlist"]')
const notices = (w) => w.findAll('.toast').map((t) => t.text())
const ids = (list) => list.map((s) => s.id)

/** El modo «Seleccionar» y un clic en cada una de esas filas (por posición). */
async function pick(w, ...positions) {
  if (button(w).attributes('aria-pressed') !== 'true') await button(w).trigger('click')
  for (const i of positions) await tableRows(w)[i].trigger('click')
  await flushPromises()
}

describe('con varias elegidas, se arrastran todas', () => {
  it('tres elegidas con las casillas van juntas al repertorio, en su orden y con UN aviso', async () => {
    const w = await mountView()
    // marcadas en otro orden que el de la lista: lo que manda es como se ven
    await pick(w, 3, 0, 2)
    expect(selectedCount(w)).toEqual(['Uno', 'Tres', 'Cuatro'])
    api.addToPlaylist.mockResolvedValueOnce({ added: 3, songs: [] })

    await dragTo(tableRows(w)[2], playlistLink(w), { release: false })
    expect(drag.song.id, 'la agarrada').toBe(3)
    expect(ids(drag.songs)).toEqual([1, 3, 4])
    // el fantasma: la agarrada, con la insignia y la pila
    const ghost = document.querySelector('.drag-ghost')
    expect(ghost.classList.contains('many')).toBe(true)
    expect(ghost.querySelector('.ghost-name').textContent).toBe('Tres')
    expect(ghost.querySelector('.ghost-count').textContent.trim()).toBe('3')
    // las tres filas de origen se atenúan; la otra no
    expect(tableRows(w).map((f) => f.classes().includes('dragged'))).toEqual([
      true,
      false,
      true,
      true
    ])
    expect(playlistLink(w).classes()).toContain('drop-over')

    await pointer(playlistLink(w), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledTimes(1)
    expect(api.addToPlaylist).toHaveBeenCalledWith(7, [1, 3, 4])
    expect(notices(w)).toEqual(['3 añadidas a «Domingo»'])
    expect(drag.song, 'el arrastre no terminó').toBe(null)
    expect(drag.songs).toEqual([])
    expect(document.querySelector('.drag-ghost')).toBe(null)
    // soltarlas no suelta la selección: se pueden seguir haciendo cosas con ellas
    expect(selectedCount(w)).toEqual(['Uno', 'Tres', 'Cuatro'])
    expect(w.find('.sel-bar .sel-status').text()).toBe('3 seleccionadas')
  })

  it('también sin el modo: con Ctrl+clic', async () => {
    const w = await mountView()
    await tableRows(w)[1].trigger('click', { ctrlKey: true })
    await tableRows(w)[3].trigger('click', { ctrlKey: true })
    await flushPromises()
    expect(selectedCount(w)).toEqual(['Dos', 'Cuatro'])
    await dragTo(tableRows(w)[3], playlistLink(w))
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledWith(7, [2, 4])
  })

  it('una que NO está elegida se arrastra sola, y la selección no cambia', async () => {
    const w = await mountView()
    await pick(w, 0, 2)
    await dragTo(tableRows(w)[1], playlistLink(w), { release: false })
    expect(ids(drag.songs)).toEqual([2])
    const ghost = document.querySelector('.drag-ghost')
    expect(ghost.classList.contains('many')).toBe(false)
    expect(ghost.querySelector('.ghost-count')).toBe(null)
    expect(tableRows(w).map((f) => f.classes().includes('dragged'))).toEqual([
      false,
      true,
      false,
      false
    ])
    await pointer(playlistLink(w), 'pointerup')
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledWith(7, [2])
    expect(notices(w)).toEqual(['Añadida a «Domingo»'])
    expect(selectedCount(w)).toEqual(['Uno', 'Tres'])
  })

  it('una sola elegida (la de la ficha) también va sola', async () => {
    const w = await mountView()
    await tableRows(w)[2].trigger('click')
    await flushPromises()
    await dragTo(tableRows(w)[2], playlistLink(w))
    await flushPromises()
    expect(api.addToPlaylist).toHaveBeenCalledWith(7, [3])
  })

  it('soltarlas en Favoritos marca solo las que no lo eran, con un aviso', async () => {
    state.songs = [
      song(1, { title: 'Uno', favorite: 1 }),
      song(2, { title: 'Dos' }),
      song(3, { title: 'Tres' }),
      song(4, { title: 'Cuatro' })
    ]
    const w = await mountView()
    await pick(w, 0, 1, 2)
    await dragTo(tableRows(w)[0], favorites(w))
    await flushPromises()
    expect(api.toggleFavorite.mock.calls).toEqual([
      [2, true],
      [3, true]
    ])
    expect(notices(w)).toEqual(['2 marcadas como favoritas'])
  })

  it('y si ya lo eran todas, lo dice y no toca nada', async () => {
    state.songs = four().map((s) => ({ ...s, favorite: 1 }))
    const w = await mountView()
    await pick(w, 1, 2)
    await dragTo(tableRows(w)[1], favorites(w))
    await flushPromises()
    expect(api.toggleFavorite).not.toHaveBeenCalled()
    expect(notices(w)).toEqual(['Ya eran todas favoritas'])
  })

  it('soltarlas en «Nueva lista» crea la lista con todas', async () => {
    const w = await mountView()
    await pick(w, 1, 3)
    await dragTo(tableRows(w)[1], newPlaylist(w))
    const { dialog } = useDialog()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.message).toBe('Se creará la lista y se añadirán 2 canciones.')
    dialogOk('Ensayo')
    await flushPromises()
    await flushPromises()
    expect(api.createPlaylist).toHaveBeenCalledWith('Ensayo')
    expect(api.addToPlaylist).toHaveBeenCalledWith(2, [2, 4])
    expect(notices(w)).toContain('Lista «Ensayo» creada con 2 canciones')
  })

  it('soltar sobre la misma fila de la que salieron no las suelta ni la desmarca', async () => {
    // El navegador lanza un clic al soltar sobre la fila de la que salió el
    // arrastre; en el modo «Seleccionar» ese clic la quitaría de las elegidas
    // y, fuera del modo, dejaría elegida solo esa.
    const w = await mountView()
    await pick(w, 0, 1, 2)
    const row = tableRows(w)[0]
    await pointer(row, 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(tableRows(w)[3], 'pointermove', { clientX: 90, clientY: 200 })
    await pointer(row, 'pointermove', { clientX: 12, clientY: 12 })
    // el pointerup y su clic llegan juntos, sin dar una vuelta al bucle
    row.element.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, cancelable: true }))
    row.element.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await flushPromises()
    expect(selectedCount(w)).toEqual(['Uno', 'Dos', 'Tres'])
    // un clic de verdad, después, sí cuenta
    await new Promise((r) => setTimeout(r, 5))
    await row.trigger('click')
    await flushPromises()
    expect(selectedCount(w)).toEqual(['Dos', 'Tres'])
  })

  it('Escape las deja caer todas sin soltarlas en nada', async () => {
    const w = await mountView()
    await pick(w, 0, 1)
    await dragTo(tableRows(w)[0], playlistLink(w), { release: false })
    expect(drag.songs).toHaveLength(2)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(drag.song).toBe(null)
    expect(drag.songs).toEqual([])
    await pointer(playlistLink(w), 'pointerup')
    expect(api.addToPlaylist).not.toHaveBeenCalled()
    // ese Escape era del arrastre: la selección sigue
    expect(selectedCount(w)).toEqual(['Uno', 'Dos'])
  })
})

describe('lo que no arrastra, tampoco con varias elegidas', () => {
  it('con el dedo no: en táctil la lista se desplaza', async () => {
    const w = await mountView()
    await pick(w, 0, 1)
    await pointer(tableRows(w)[0], 'pointerdown', {
      clientX: 10,
      clientY: 10,
      pointerType: 'touch'
    })
    await pointer(playlistLink(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(drag.song).toBe(null)
    await pointer(playlistLink(w), 'pointerup')
    expect(api.addToPlaylist).not.toHaveBeenCalled()
  })

  it('con el botón derecho no: ese es el del menú', async () => {
    const w = await mountView()
    await pick(w, 0, 1)
    await pointer(tableRows(w)[0], 'pointerdown', { clientX: 10, clientY: 10, button: 2 })
    await pointer(playlistLink(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(drag.song).toBe(null)
  })

  it('desde un control de la fila (reproducir) no', async () => {
    const w = await mountView()
    await pick(w, 0, 1)
    const play = tableRows(w)[0].find('.row-play')
    expect(play.exists()).toBe(true)
    await pointer(play, 'pointerdown', { clientX: 10, clientY: 10 })
    await pointer(playlistLink(w), 'pointermove', { clientX: 90, clientY: 200 })
    expect(drag.song).toBe(null)
  })
})

describe('reordenar un bloque dentro de un repertorio', () => {
  const five = () => [
    song(1, { title: 'Uno' }),
    song(2, { title: 'Dos' }),
    song(3, { title: 'Tres' }),
    song(4, { title: 'Cuatro' }),
    song(5, { title: 'Cinco' })
  ]
  async function inPlaylist() {
    state.songs = five()
    state.playlistSongs = five()
    state.playlists = [{ id: 7, name: 'Domingo', n: 5 }]
    const w = await mountView()
    await playlistLink(w).trigger('click')
    await flushPromises()
    await flushPromises()
    expect(titles(w)).toEqual(['Uno', 'Dos', 'Tres', 'Cuatro', 'Cinco'])
    return w
  }
  // jsdom no maqueta: la fila dice dónde está para apuntar a una de sus mitades
  function place(row, top = 100, height = 30) {
    row.element.getBoundingClientRect = () => ({
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
  const ABOVE = 105
  const BELOW = 125

  it('dos salteadas, debajo de la última: van juntas y en su orden', async () => {
    const w = await inPlaylist()
    await pick(w, 0, 2)
    place(tableRows(w)[4])
    await dragTo(tableRows(w)[2], tableRows(w)[4], { release: false, y: BELOW })
    expect(ids(drag.songs)).toEqual([1, 3])
    expect(tableRows(w)[4].classes()).toContain('drop-after')
    // ninguna raya sobre las que van en la mano
    for (const i of [0, 2]) {
      expect(tableRows(w)[i].classes()).not.toContain('drop-before')
      expect(tableRows(w)[i].classes()).not.toContain('drop-after')
    }
    await pointer(tableRows(w)[4], 'pointerup', { clientX: 90, clientY: BELOW })
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledTimes(1)
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 4, 5, 1, 3])
    expect(titles(w)).toEqual(['Dos', 'Cuatro', 'Cinco', 'Uno', 'Tres'])
    // siguen elegidas, ahora en su sitio nuevo
    expect(selectedCount(w)).toEqual(['Uno', 'Tres'])
  })

  it('encima de la primera: el bloque pasa delante', async () => {
    const w = await inPlaylist()
    await pick(w, 3, 4)
    place(tableRows(w)[0])
    await dragTo(tableRows(w)[4], tableRows(w)[0], { y: ABOVE })
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [4, 5, 1, 2, 3])
    expect(titles(w)).toEqual(['Cuatro', 'Cinco', 'Uno', 'Dos', 'Tres'])
  })

  it('en medio del bloque: el bloque se cierra alrededor de la de destino', async () => {
    // elegidas la 1 y la 4, soltadas debajo de la 2: quedan 2, 1, 4
    const w = await inPlaylist()
    await pick(w, 0, 3)
    place(tableRows(w)[1])
    await dragTo(tableRows(w)[0], tableRows(w)[1], { y: BELOW })
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledWith(7, [2, 1, 4, 3, 5])
  })

  it('soltarlo sobre una de las suyas no hace nada', async () => {
    const w = await inPlaylist()
    await pick(w, 0, 2)
    place(tableRows(w)[2])
    await dragTo(tableRows(w)[0], tableRows(w)[2], { y: BELOW })
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
    expect(titles(w)).toEqual(['Uno', 'Dos', 'Tres', 'Cuatro', 'Cinco'])
  })

  it('si el bloque ya está ahí, no se pide nada al núcleo', async () => {
    const w = await inPlaylist()
    await pick(w, 1, 2)
    place(tableRows(w)[3])
    // la 2 y la 3, justo encima de la 4: es su sitio
    await dragTo(tableRows(w)[1], tableRows(w)[3], { y: ABOVE })
    await flushPromises()
    expect(api.reorderPlaylist).not.toHaveBeenCalled()
  })

  it('si el núcleo no puede, la lista vuelve como estaba', async () => {
    const w = await inPlaylist()
    api.reorderPlaylist.mockRejectedValueOnce(new Error('sin base de datos'))
    await pick(w, 0, 1)
    place(tableRows(w)[4])
    await dragTo(tableRows(w)[0], tableRows(w)[4], { y: BELOW })
    await flushPromises()
    await flushPromises()
    expect(titles(w)).toEqual(['Uno', 'Dos', 'Tres', 'Cuatro', 'Cinco'])
    expect(notices(w).join(' ')).toContain('No se pudo cambiar el orden')
  })

  it('dos arrastres seguidos no se cruzan: van en fila y manda el último', async () => {
    const w = await inPlaylist()
    let first
    api.reorderPlaylist.mockImplementationOnce(() => new Promise((resolve) => (first = resolve)))
    // la 1 debajo de la 3 (el núcleo tarda en contestar)...
    place(tableRows(w)[2])
    await dragTo(tableRows(w)[0], tableRows(w)[2], { y: BELOW })
    expect(titles(w)).toEqual(['Dos', 'Tres', 'Uno', 'Cuatro', 'Cinco'])
    // ...y, sin esperar, la 5 encima de la primera que se ve ahora (la 2)
    place(tableRows(w)[0])
    await dragTo(tableRows(w)[4], tableRows(w)[0], { y: ABOVE })
    expect(titles(w)).toEqual(['Cinco', 'Dos', 'Tres', 'Uno', 'Cuatro'])
    expect(api.reorderPlaylist, 'la segunda espera a la primera').toHaveBeenCalledTimes(1)
    // llega la primera: no pisa la lista con su orden, ya viejo
    first({ songs: [2, 3, 1, 4, 5].map((id) => five().find((s) => s.id === id)) })
    await flushPromises()
    await flushPromises()
    expect(api.reorderPlaylist).toHaveBeenCalledTimes(2)
    expect(api.reorderPlaylist.mock.calls[1]).toEqual([7, [5, 2, 3, 1, 4]])
    expect(titles(w)).toEqual(['Cinco', 'Dos', 'Tres', 'Uno', 'Cuatro'])
  })
})

describe('las cuatro vistas pasan las elegidas al arrastre', () => {
  const VIEWS = { table: SongTable, rows: SongRows, cards: SongCards, grid: SongGrid }
  for (const [name, View] of Object.entries(VIEWS)) {
    it(`${name}: agarrar una elegida se lleva todas; otra, solo ella`, async () => {
      const w = mount(View, {
        props: { songs: four(), selected: 4, selectedIds: [2, 4], selectMode: true },
        attachTo: document.body
      })
      const rows = w.findAll('[data-song-row]')
      expect(rows).toHaveLength(4)
      await pointer(rows[3], 'pointerdown', { clientX: 10, clientY: 10 })
      await pointer(rows[0], 'pointermove', { clientX: 90, clientY: 200 })
      expect(drag.song.id).toBe(4)
      expect(ids(drag.songs)).toEqual([2, 4])
      // las dos se atenúan
      await flushPromises()
      expect(rows.map((r) => r.classes().includes('dragged'))).toEqual([false, true, false, true])
      cancelDrag()
      await pointer(rows[2], 'pointerdown', { clientX: 10, clientY: 10 })
      await pointer(rows[0], 'pointermove', { clientX: 90, clientY: 200 })
      expect(ids(drag.songs)).toEqual([3])
      cancelDrag()
      w.unmount()
    })
  }
})

describe('useDragSong con un grupo', () => {
  const row = () => {
    const el = document.createElement('div')
    document.body.appendChild(el)
    return el
  }
  const ev = (target, extra = {}) => ({ clientX: 0, clientY: 0, button: 0, target, ...extra })
  const move = (el) =>
    el.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 60, clientY: 60 }))

  it('el grupo va en el orden en que llega, con la agarrada dentro', () => {
    const el = row()
    const group = [{ id: 5 }, { id: 2 }, { id: 9 }]
    startDrag(group[1], ev(el), group)
    expect(drag.song, 'antes del umbral no hay arrastre').toBe(null)
    move(el)
    expect(drag.song.id).toBe(2)
    expect(ids(drag.songs)).toEqual([5, 2, 9])
    // es una copia: lo que haga quien llamó con su lista no la cambia
    group.pop()
    expect(drag.songs).toHaveLength(3)
    const { isDragged } = useDragSong()
    expect([5, 2, 9, 7].map(isDragged)).toEqual([true, true, true, false])
  })

  it('si la agarrada no está en el grupo, o el grupo es de una, va sola', () => {
    const el = row()
    startDrag({ id: 1 }, ev(el), [{ id: 2 }, { id: 3 }])
    move(el)
    expect(ids(drag.songs)).toEqual([1])
    startDrag({ id: 2 }, ev(el), [{ id: 2 }])
    move(el)
    expect(ids(drag.songs)).toEqual([2])
    startDrag({ id: 3 }, ev(el))
    move(el)
    expect(ids(drag.songs)).toEqual([3])
  })

  it('al soltar llegan la agarrada y todas, y sin grupo `songs` es [song]', () => {
    const received = []
    onDrop((target, s, extra) => received.push([target, s.id, ids(extra.songs)]))
    const el = row()
    const dropTarget = row()
    dropTarget.setAttribute('data-drop', 'playlist:7')
    startDrag({ id: 3 }, ev(el), [{ id: 1 }, { id: 3 }])
    move(dropTarget)
    dropTarget.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    startDrag({ id: 8 }, ev(el))
    move(dropTarget)
    dropTarget.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    expect(received).toEqual([
      ['playlist:7', 3, [1, 3]],
      ['playlist:7', 8, [8]]
    ])
    expect(drag.songs).toEqual([])
    onDrop(() => {})
  })

  it('un clic sin moverse sigue siendo un clic: no se traga nada', () => {
    const el = row()
    let clicks = 0
    el.addEventListener('click', () => clicks++)
    startDrag({ id: 1 }, ev(el), [{ id: 1 }, { id: 2 }])
    el.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
    el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(clicks).toBe(1)
  })

  it('táctil, botón derecho y controles no arman nada aunque haya grupo', () => {
    const el = row()
    const group = [{ id: 1 }, { id: 2 }]
    startDrag(group[0], ev(el, { pointerType: 'touch' }), group)
    move(el)
    expect(drag.song).toBe(null)
    startDrag(group[0], ev(el, { button: 2 }), group)
    move(el)
    expect(drag.song).toBe(null)
    const heart = document.createElement('button')
    el.appendChild(heart)
    startDrag(group[0], ev(heart), group)
    move(el)
    expect(drag.song).toBe(null)
  })
})

describe('el fantasma con varias (css)', () => {
  const CSS = allCss()
  // `allCss` la compacta: `.sel{prop:valor;...}`
  const rule = (sel) => {
    const i = CSS.indexOf('\n' + sel + '{')
    expect(i, `falta ${sel}`).toBeGreaterThanOrEqual(0)
    return CSS.slice(i, CSS.indexOf('}', i))
  }

  it('la pila y la insignia salen de las variables del tema, sin colores fijos', () => {
    const stack = rule('.drag-ghost.many')
    const insignia = rule('.drag-ghost .ghost-count')
    for (const r of [stack, insignia]) {
      expect(r).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(/i)
      expect(r).toMatch(/var\(--/)
    }
    expect(insignia).toMatch(/background:var\(--accent\)/)
    // la insignia se coloca sobre el fantasma, que es `position: fixed`
    expect(insignia).toMatch(/position:absolute/)
  })

  it('con movimiento reducido la pila no entra animada', () => {
    const media = CSS.match(/@media \(prefers-reduced-motion:reduce\)\{\.drag-ghost\.many\{[^}]*\}/)
    expect(media?.[0], 'falta la regla').toMatch(/animation:none/)
  })

  it('la raya de ordenar va en las celdas de la tabla: WebKit no pinta la sombra de un <tr>', () => {
    // Comprobado en WebKitGTK: con la sombra en el `tr` la raya no salía.
    expect(CSS).toMatch(
      /tr\.drop-before\s*>\s*td,\.row\.drop-before\{box-shadow:inset 0 2px 0 var\(--accent\)/
    )
    expect(CSS).toMatch(
      /tr\.drop-after\s*>\s*td,\.row\.drop-after\{box-shadow:inset 0 -2px 0 var\(--accent\)/
    )
    expect(CSS).not.toMatch(/(^|[\n,])tr\.drop-(before|after)[,{]/)
  })
})
