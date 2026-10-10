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
function preparar(ids = [1, 2, 3, 4, 5]) {
  const shown = ref(ids.map((id) => song(id)))
  return { shown, s: useSelection({ shown }) }
}
const ids = (s) => s.chosenSongs.value.map((c) => c.id)

beforeEach(() => vi.clearAllMocks())

describe('sin el modo, como siempre', () => {
  it('un clic deja esa sola; Ctrl suma y quita; Mayús coge el tramo', async () => {
    const { s } = preparar()
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

  it('la lista de varias (`selectedSongs`) sigue siendo solo de dos o más', async () => {
    const { s } = preparar()
    await s.select(2)
    expect(s.selectedSongs.value).toEqual([])
    expect(s.chosenSongs.value.map((c) => c.id)).toEqual([2])
    await s.select(1, { ctrlKey: true })
    expect(s.selectedSongs.value.map((c) => c.id)).toEqual([1, 2])
  })
})

describe('el modo «Seleccionar»', () => {
  it('un clic normal pone o quita esa sin soltar las demás; Ctrl no cambia nada', async () => {
    const { s } = preparar()
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

  it('Mayús suma el tramo a lo que había', async () => {
    const { s } = preparar()
    s.setMode(true)
    await s.select(1, {})
    await s.select(3, {})
    await s.select(5, { shiftKey: true }) // de la 3 a la 5, y la 1 sigue
    expect(ids(s)).toEqual([1, 3, 4, 5])
  })

  it('elegir sin un clic (la ficha de un resultado, «Ver detalles») suma, no deja sola', async () => {
    const { s } = preparar()
    s.setMode(true)
    await s.select(1, {})
    await s.select(3, {})
    await s.select(5) // sin evento
    expect(ids(s)).toEqual([1, 3, 5])
    expect(s.selected.value).toBe(5)
  })

  it('quitar el modo con el botón termina (suelta todo); setMode(false) solo quita el modo', async () => {
    const { s } = preparar()
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

  it('«Listo» (finish) sale del modo y no deja nada elegido; la ficha se queda', async () => {
    const { s } = preparar()
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

describe('todas y ninguna', () => {
  it('«todas» son las que se ven, sin pedir la ficha de ninguna', async () => {
    const { s } = preparar()
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

  it('si la principal no está en la lista, pasa a la primera (sin abrir su ficha)', async () => {
    const { s, shown } = preparar()
    await s.select(5)
    await flushPromises()
    shown.value = [song(1), song(2)] // otra búsqueda
    held.api.song.mockClear()
    s.selectAll()
    expect(s.selected.value).toBe(1)
    expect(s.detail.value?.id, 'la ficha abierta no cambia').toBe(5)
    expect(held.api.song).not.toHaveBeenCalled()
  })

  it('«ninguna» suelta también la principal, que si no seguiría marcada', async () => {
    const { s } = preparar()
    s.selectAll()
    s.selectNone()
    expect(ids(s)).toEqual([])
    expect(s.selected.value).toBe(null)
    expect(s.count.value).toBe(0)
    expect(s.allSelected.value).toBe(false)
  })

  it('cuántas, todas o algunas', async () => {
    const { s } = preparar()
    expect([s.count.value, s.allSelected.value, s.partial.value]).toEqual([0, false, false])
    await s.select(2)
    expect([s.count.value, s.allSelected.value, s.partial.value]).toEqual([1, false, true])
    s.selectAll()
    expect([s.count.value, s.allSelected.value, s.partial.value]).toEqual([5, true, false])
  })

  it('con la lista vacía no están «todas»', () => {
    const { s } = preparar([])
    s.selectAll()
    expect(s.allSelected.value).toBe(false)
    expect(s.selected.value).toBe(null)
  })
})

describe('las elegidas', () => {
  it('van en el orden en que se ven y solo cuentan las que están en la lista', async () => {
    const { s, shown } = preparar()
    s.setMode(true)
    await s.select(4, {})
    await s.select(1, {})
    await s.select(3, {})
    expect(ids(s)).toEqual([1, 3, 4])
    shown.value = [song(4), song(3), song(9)] // otro orden, la 1 ya no está
    expect(ids(s)).toEqual([4, 3])
  })

  it('lo que se pinta (el grupo y la principal) es lo que cuenta', async () => {
    const { s } = preparar()
    await s.select(1)
    await s.select(2, { ctrlKey: true })
    s.selected.value = 5 // p. ej. la principal cambió por otro camino
    expect(ids(s)).toEqual([1, 2, 5])
  })

  it('las que se van (papelera, fuera de la lista) se sueltan', async () => {
    const { s } = preparar()
    s.setMode(true)
    await s.select(1, {})
    await s.select(2, {})
    await s.select(3, {})
    s.forget([3, 1])
    expect(ids(s)).toEqual([2])
    expect(s.selected.value).toBe(2)
  })
})

describe('la que se pone a sonar (follow)', () => {
  it('sin selección de varias pasa a ser la única elegida', async () => {
    const { s } = preparar()
    await s.select(1)
    s.follow(3)
    expect(ids(s), 'antes quedaban dos pintadas: la de antes y la que suena').toEqual([3])
  })

  it('con varias elegidas no las suelta; la principal solo se mueve si es una de ellas', async () => {
    const { s } = preparar()
    await s.select(1)
    await s.select(2, { ctrlKey: true })
    s.follow(5)
    expect(ids(s)).toEqual([1, 2])
    s.follow(1)
    expect(s.selected.value).toBe(1)
    expect(ids(s)).toEqual([1, 2])
  })

  it('en el modo no toca nada: poner una a sonar no es marcarla', async () => {
    const { s } = preparar()
    s.setMode(true)
    await s.select(2, {})
    s.follow(4)
    expect(ids(s)).toEqual([2])
    expect(s.selected.value).toBe(2)
  })
})
