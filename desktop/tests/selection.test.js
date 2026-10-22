// La selección de canciones (useSelection): lo de siempre (Ctrl, Mayús) y el
// modo «Seleccionar» con casillas, «todas», «ninguna» y lo que se deriva
// (las elegidas aunque sea una, cuántas, si están todas o algunas).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ref } from 'vue'
import { flushPromises } from '@vue/test-utils'

const held = vi.hoisted(() => ({ api: null }))
vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { vi: v } = await import('vitest')
  held.api = { song: v.fn(async (id) => ({ id, title: 'Cancion ' + id })) }
  return { ...actual, api: held.api }
})

import { useSelection } from '../src/composables/useSelection.js'

const song = (id, extra = {}) => ({ id, title: 'Cancion ' + id, favorite: 0, ...extra })
/** Cinco canciones en el orden en que se ven. */
function prepare(ids = [1, 2, 3, 4, 5]) {
  const shown = ref(ids.map((id) => song(id)))
  return { shown, s: useSelection({ shown }) }
}
const ids = (s) => s.chosenSongs.value.map((c) => c.id)

beforeEach(() => vi.clearAllMocks())

describe('without the mode, as always', () => {
  it('a click leaves only that one; Ctrl adds and removes; Shift takes the range', async () => {
    const { s } = prepare()
    await s.select(2)
    await s.select(4, { ctrlKey: true })
    expect(ids(s)).toEqual([2, 4])
    await s.select(5) // sin teclas: solo esa
    expect(ids(s)).toEqual([5])
    await s.select(3, { shiftKey: true }) // tramo desde la 5
    expect(ids(s)).toEqual([3, 4, 5])
    await s.select(4, { ctrlKey: true }) // la quita
    expect(ids(s)).toEqual([3, 5])
  })

  it('the multi-selection list (`selectedSongs`) is still only for two or more', async () => {
    const { s } = prepare()
    await s.select(2)
    expect(s.selectedSongs.value).toEqual([])
    expect(s.chosenSongs.value.map((c) => c.id)).toEqual([2])
    await s.select(1, { ctrlKey: true })
    expect(s.selectedSongs.value.map((c) => c.id)).toEqual([1, 2])
  })
})

describe('the «Seleccionar» mode', () => {
  it('a plain click toggles that one without deselecting the rest; Ctrl changes nothing', async () => {
    const { s } = prepare()
    s.toggleMode()
    expect(s.selectMode.value).toBe(true)
    await s.select(2, {})
    await s.select(4, {})
    expect(ids(s)).toEqual([2, 4])
    await s.select(2, { ctrlKey: true }) // igual que sin Ctrl: la quita
    expect(ids(s)).toEqual([4])
    await s.select(4, {})
    expect(ids(s)).toEqual([])
    expect(s.selected.value).toBe(null)
  })

  it('Shift adds the range to what was there', async () => {
    const { s } = prepare()
    s.setMode(true)
    await s.select(1, {})
    await s.select(3, {})
    await s.select(5, { shiftKey: true }) // de la 3 a la 5, y la 1 sigue
    expect(ids(s)).toEqual([1, 3, 4, 5])
  })

  it('selecting without a click (a result details panel, «Ver detalles») adds, it does not leave only that one', async () => {
    const { s } = prepare()
    s.setMode(true)
    await s.select(1, {})
    await s.select(3, {})
    await s.select(5) // sin evento
    expect(ids(s)).toEqual([1, 3, 5])
    expect(s.selected.value).toBe(5)
  })

  it('leaving the mode with the button finishes (deselects all); setMode(false) only leaves the mode', async () => {
    const { s } = prepare()
    s.setMode(true)
    await s.select(1, {})
    await s.select(2, {})
    s.setMode(false)
    expect(ids(s)).toEqual([1, 2])
    s.setMode(true)
    s.toggleMode()
    expect(s.selectMode.value).toBe(false)
    expect(ids(s)).toEqual([])
  })

  it('«Listo» (finish) leaves the mode and leaves nothing selected; the details panel stays', async () => {
    const { s } = prepare()
    await s.select(2)
    await flushPromises()
    s.setMode(true)
    await s.select(3, {})
    s.finish()
    expect(s.selectMode.value).toBe(false)
    expect(ids(s)).toEqual([])
    expect(s.detail.value?.id).toBe(3)
  })
})

describe('all and none', () => {
  it('«todas» means the visible ones, without requesting the details of any', async () => {
    const { s } = prepare()
    await s.select(3)
    await flushPromises()
    held.api.song.mockClear()
    s.selectAll()
    expect(ids(s)).toEqual([1, 2, 3, 4, 5])
    expect(s.selected.value, 'la principal se queda si está entre ellas').toBe(3)
    expect(s.allSelected.value).toBe(true)
    expect(s.partial.value).toBe(false)
    await flushPromises()
    expect(held.api.song).not.toHaveBeenCalled()
  })

  it('if the main one is not in the list, it moves to the first (without opening its details)', async () => {
    const { s, shown } = prepare()
    await s.select(5)
    await flushPromises()
    shown.value = [song(1), song(2)] // otra búsqueda
    held.api.song.mockClear()
    s.selectAll()
    expect(s.selected.value).toBe(1)
    expect(s.detail.value?.id, 'la ficha abierta no cambia').toBe(5)
    expect(held.api.song).not.toHaveBeenCalled()
  })

  it('«ninguna» also deselects the main one, which would otherwise stay ticked', async () => {
    const { s } = prepare()
    s.selectAll()
    s.selectNone()
    expect(ids(s)).toEqual([])
    expect(s.selected.value).toBe(null)
    expect(s.count.value).toBe(0)
    expect(s.allSelected.value).toBe(false)
  })

  it('how many, all or some', async () => {
    const { s } = prepare()
    expect([s.count.value, s.allSelected.value, s.partial.value]).toEqual([0, false, false])
    await s.select(2)
    expect([s.count.value, s.allSelected.value, s.partial.value]).toEqual([1, false, true])
    s.selectAll()
    expect([s.count.value, s.allSelected.value, s.partial.value]).toEqual([5, true, false])
  })

  it('with an empty list it is not «todas»', () => {
    const { s } = prepare([])
    s.selectAll()
    expect(s.allSelected.value).toBe(false)
    expect(s.selected.value).toBe(null)
  })
})

describe('the selected ones', () => {
  it('come in display order and only those in the list count', async () => {
    const { s, shown } = prepare()
    s.setMode(true)
    await s.select(4, {})
    await s.select(1, {})
    await s.select(3, {})
    expect(ids(s)).toEqual([1, 3, 4])
    shown.value = [song(4), song(3), song(9)] // otro orden, la 1 ya no está
    expect(ids(s)).toEqual([4, 3])
  })

  it('what is rendered (the group and the main one) is what counts', async () => {
    const { s } = prepare()
    await s.select(1)
    await s.select(2, { ctrlKey: true })
    s.selected.value = 5 // p. ej. la principal cambió por otro camino
    expect(ids(s)).toEqual([1, 2, 5])
  })

  it('those that go away (trash, out of the list) are deselected', async () => {
    const { s } = prepare()
    s.setMode(true)
    await s.select(1, {})
    await s.select(2, {})
    await s.select(3, {})
    s.forget([3, 1])
    expect(ids(s)).toEqual([2])
    expect(s.selected.value).toBe(2)
  })
})

describe('the one that starts playing (follow)', () => {
  it('without a multi-selection it becomes the only selected one', async () => {
    const { s } = prepare()
    await s.select(1)
    s.follow(3)
    expect(ids(s), 'antes quedaban dos pintadas: la de antes y la que suena').toEqual([3])
  })

  it('with several selected it keeps them; the main one only moves if it is one of them', async () => {
    const { s } = prepare()
    await s.select(1)
    await s.select(2, { ctrlKey: true })
    s.follow(5)
    expect(ids(s)).toEqual([1, 2])
    s.follow(1)
    expect(s.selected.value).toBe(1)
    expect(ids(s)).toEqual([1, 2])
  })

  it('in the mode it touches nothing: playing one is not ticking it', async () => {
    const { s } = prepare()
    s.setMode(true)
    await s.select(2, {})
    s.follow(4)
    expect(ids(s)).toEqual([2])
    expect(s.selected.value).toBe(2)
  })
})
