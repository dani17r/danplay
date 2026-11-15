import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

// Ajustar el clic de una canción a mano, de muchos pulsos a la vez: elegirlos
// (un clic, Ctrl, Mayús, o arrastrando por la franja de arriba), llevarlos
// juntos, estirarlos desde un extremo de la banda, el imán a los golpes del
// audio, y lo que se pinta detrás (golpes, sombra, rejilla automática).
const held = vi.hoisted(() => ({ api: null, state: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  return { ...actual, api: held.api }
})

import StudyTimeline from '../src/components/StudyTimeline.vue'
import { useContextMenu } from '../src/composables/useContextMenu.js'
import { gridToOwn } from '../src/utils/beats.js'

const STYLES = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'styles')
/** 40 s en 800 px: 20 px por segundo, y un pulso (cada 0,5 s) cada 10 px. */
const DURATION = 40
const WIDTH = 800
const PX = WIDTH / DURATION
/** El pulso `i` cae en el segundo 0,5 + 0,5 i (el 0 es el primero). */
const T = (i) => 0.5 + 0.5 * i
/** Y su x en pantalla (la caja empieza en 0). */
const X = (i) => T(i) * PX
const N = 79

const makeGrid = () => ({
  bpm: 120,
  meter: 4,
  beats: Array.from({ length: N }, (_, i) => T(i)),
  first_downbeat: 0,
  phase3: 0,
  phase4: 0,
  confidence: 1,
  bars: Array.from({ length: Math.ceil(N / 4) }, (_, k) => k * 4),
  closed: true
})

const mounted = []
beforeEach(() => {
  vi.clearAllMocks()
  held.api.waveform.mockResolvedValue({ peaks: [0.5, 1, 0.5, 1], rms: [0.2, 0.5, 0.2, 0.5] })
  useContextMenu().closeMenu()
})
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
})

async function mountEdit(over = {}) {
  const w = mount(StudyTimeline, {
    props: {
      songId: 7,
      duration: DURATION,
      position: 0,
      editBeats: true,
      grid: makeGrid(),
      ...over
    },
    attachTo: document.body
  })
  mounted.push(w)
  // jsdom no maqueta: la caja de la onda va de x=0 a x=800
  const el = w.find('.tl-box').element
  Object.defineProperty(el, 'clientWidth', { configurable: true, value: WIDTH })
  el.getBoundingClientRect = () => ({
    left: 0,
    top: 0,
    width: WIDTH,
    height: 56,
    right: WIDTH,
    bottom: 56,
    x: 0,
    y: 0
  })
  await flushPromises()
  return w
}

const fire = (target, type, init = {}) =>
  target.dispatchEvent(
    new MouseEvent(type, { bubbles: true, cancelable: true, clientY: 30, ...init })
  )
const move = (x, init) => fire(window, 'pointermove', { clientX: x, ...init })
const up = (x, init) => fire(window, 'pointerup', { clientX: x, ...init })
const beatEl = (w, i) => w.find(`.tl-beat[data-i="${i}"]`).element
/** Pulsa en un sitio, lo lleva y lo suelta: un arrastre entero. */
async function drag(w, target, x0, x1, init = {}) {
  fire(target, 'pointerdown', { clientX: x0, ...init })
  move(x1, init)
  up(x1, init)
  await flushPromises()
}
/** Un clic: pulsar y soltar en el mismo sitio. */
async function click(w, target, x, init = {}) {
  fire(target, 'pointerdown', { clientX: x, ...init })
  up(x, init)
  await flushPromises()
}
const emittedAll = (w, name) => (w.emitted(name) || []).map((a) => a[0])
const lastGrid = (w) => w.emitted('update:grid').at(-1)
const range = (a, b) => Array.from({ length: b - a + 1 }, (_, k) => a + k)
/** Al milisegundo, como salen los pulsos. */
const r3 = (t) => Math.round(t * 1000) / 1000
/** Los pulsos que no se tocaron, tal cual estaban. */
const untouched = (beats, except) =>
  beats.every((t, i) => except.includes(i) || t === makeGrid().beats[i])
const menuLabels = () =>
  useContextMenu()
    .menu.value.items.filter((i) => !i.separator)
    .map((i) => i.label)
const menuItem = (label) => useContextMenu().menu.value.items.find((i) => i.label === label)

describe('choosing beats', () => {
  it('shows the beat bar only while adjusting the beats', async () => {
    const off = await mountEdit({ editBeats: false, selection: [3, 4] })
    expect(off.find('.tl-beatbar').exists()).toBe(false)
    expect(off.find('.tl-selband').exists()).toBe(false)
    expect(off.findAll('.tl-beat')).toHaveLength(0)
    const on = await mountEdit()
    expect(on.find('.tl-beatbar').exists()).toBe(true)
    expect(on.findAll('.tl-beat').length).toBeGreaterThan(70)
  })

  it('drag on the beat bar picks the beats between the two ends, either way round', async () => {
    const w = await mountEdit()
    const bar = w.find('.tl-beatbar').element
    // los pulsos 4 a 8 caen en 2,5 a 4,5 s (x 50 a 90): 48 y 91 los abarcan justo
    await drag(w, bar, 48, 91)
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual(range(4, 8))
    await drag(w, bar, 91, 48)
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual(range(4, 8))
    // el trozo se dibuja mientras se arrastra, y se va al soltar
    fire(bar, 'pointerdown', { clientX: 48 })
    move(91)
    await flushPromises()
    expect(w.find('.tl-boxsel').exists()).toBe(true)
    up(91)
    await flushPromises()
    expect(w.find('.tl-boxsel').exists()).toBe(false)
    // un trozo sin pulsos deja la selección vacía
    await drag(w, bar, 52, 58)
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual([])
  })

  it('Ctrl or Shift on the bar adds to what was chosen; a plain drag replaces it', async () => {
    const w = await mountEdit({ selection: [20, 21] })
    const bar = w.find('.tl-beatbar').element
    await drag(w, bar, 48, 91, { ctrlKey: true })
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual([...range(4, 8), 20, 21])
    await drag(w, bar, 48, 91, { shiftKey: true })
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual([...range(4, 8), 20, 21])
    await drag(w, bar, 48, 91)
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual(range(4, 8))
  })

  it('a click on the empty bar clears the selection, and never seeks or picks a loop', async () => {
    const w = await mountEdit({ selection: [5, 6] })
    const bar = w.find('.tl-beatbar').element
    await click(w, bar, 300)
    expect(emittedAll(w, 'update:selection')).toEqual([[]])
    // ni lleva la canción ahí, ni empieza un tramo, ni añade un pulso
    fire(bar, 'dblclick', { clientX: 300 })
    await flushPromises()
    expect(w.emitted('seek')).toBeUndefined()
    expect(w.emitted('update:loops')).toBeUndefined()
    expect(w.emitted('update:grid')).toBeUndefined()
    // con Ctrl, dejarlo como está
    await click(w, bar, 300, { ctrlKey: true })
    expect(emittedAll(w, 'update:selection')).toHaveLength(1)
  })

  it('a click on a beat chooses it; Ctrl adds or removes it; a few pixels is still a click', async () => {
    const w = await mountEdit({ selection: [3, 4] })
    await click(w, beatEl(w, 10), X(10))
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual([10])
    await click(w, beatEl(w, 7), X(7), { ctrlKey: true })
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual([3, 4, 7])
    await click(w, beatEl(w, 4), X(4), { ctrlKey: true })
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual([3])
    // dos píxeles de temblor siguen siendo un clic: nada se mueve
    await drag(w, beatEl(w, 12), X(12), X(12) + 2)
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual([12])
    expect(w.emitted('update:grid')).toBeUndefined()
    expect(w.emitted('seek')).toBeUndefined()
  })

  it('Shift + click chooses from the last chosen beat, or from the first one chosen', async () => {
    const w = await mountEdit()
    await click(w, beatEl(w, 3), X(3))
    await w.setProps({ selection: [3] })
    await click(w, beatEl(w, 7), X(7), { shiftKey: true })
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual(range(3, 7))
    // el ancla sigue siendo el 3 (Mayús no la mueve), también hacia atrás
    await w.setProps({ selection: range(3, 7) })
    await click(w, beatEl(w, 1), X(1), { shiftKey: true })
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual(range(1, 3))
    // sin ancla (una selección que viene de fuera), el primero elegido
    const other = await mountEdit({ selection: [10, 11] })
    await click(other, beatEl(other, 14), X(14), { shiftKey: true })
    expect(emittedAll(other, 'update:selection').at(-1)).toEqual(range(10, 14))
    expect(other.emitted('update:grid')).toBeUndefined()
  })

  it('draws the chosen beats highlighted and the band from the first to the last, with a handle at each end', async () => {
    const w = await mountEdit({ selection: range(10, 14) })
    expect(w.findAll('.tl-beat.sel').map((b) => Number(b.attributes('data-i')))).toEqual(
      range(10, 14)
    )
    const band = w.find('.tl-selband')
    expect(band.exists()).toBe(true)
    // de 5,5 a 7,5 s en 40 s
    expect(parseFloat(band.attributes('style').match(/left: ([\d.]+)%/)[1])).toBeCloseTo(13.75, 3)
    expect(parseFloat(band.attributes('style').match(/width: ([\d.]+)%/)[1])).toBeCloseTo(5, 3)
    expect(band.findAll('.tl-selband-h')).toHaveLength(2)
    expect(band.find('.tl-selband-n').text()).toBe('5')
    // uno solo: una marca sin asas (no hay nada que estirar)
    await w.setProps({ selection: [10] })
    expect(w.find('.tl-selband').exists()).toBe(true)
    expect(w.findAll('.tl-selband-h')).toHaveLength(0)
    await w.setProps({ selection: [] })
    expect(w.find('.tl-selband').exists()).toBe(false)
  })
})

describe('moving a group of beats', () => {
  it('dragging a chosen beat moves every chosen one by the same amount', async () => {
    const w = await mountEdit({ selection: range(10, 13) })
    // 6 px a la izquierda = 0,3 s
    fire(beatEl(w, 11), 'pointerdown', { clientX: X(11) })
    move(X(11) - 6)
    await flushPromises()
    // mientras se lleva, se ve cuántos se llevan y cuánto
    expect(w.findAll('.tl-beat.moving').map((b) => Number(b.attributes('data-i')))).toEqual(
      range(10, 13)
    )
    expect(w.find('.tl-readout').text()).toBe('−300 ms')
    // la banda va con ellos: de 5,2 a 6,7 s
    expect(
      parseFloat(
        w
          .find('.tl-selband')
          .attributes('style')
          .match(/left: ([\d.]+)%/)[1]
      )
    ).toBeCloseTo(13, 3)
    up(X(11) - 6)
    await flushPromises()
    const [grid, label] = lastGrid(w)
    expect(label).toBe('mover 4 pulsos')
    expect(grid.beats.slice(10, 14)).toEqual([5.2, 5.7, 6.2, 6.7])
    expect(untouched(grid.beats, range(10, 13))).toBe(true)
    // ya estaban elegidos: la selección no cambia
    expect(w.emitted('update:selection')).toBeUndefined()
    expect(w.find('.tl-readout').exists()).toBe(false)
  })

  it('keeps the spacing and stops at the beat next to the block', async () => {
    const w = await mountEdit({ selection: range(10, 13) })
    // medio segundo a la derecha chocaría con el pulso 14 (7,5 s): sobran 20 ms
    fire(beatEl(w, 11), 'pointerdown', { clientX: X(11) })
    move(X(11) + 10)
    await flushPromises()
    expect(w.find('.tl-readout').text()).toBe('+480 ms')
    up(X(11) + 10)
    await flushPromises()
    const [grid] = lastGrid(w)
    expect(grid.beats.slice(10, 14)).toEqual([5.98, 6.48, 6.98, 7.48])
    expect(grid.beats[14]).toBe(7.5)
    const gaps = range(11, 13).map((i) => Math.round((grid.beats[i] - grid.beats[i - 1]) * 1000))
    expect(gaps).toEqual([500, 500, 500])
  })

  it('a beat that is not chosen becomes the selection and moves alone', async () => {
    const w = await mountEdit({ selection: [10, 11] })
    await drag(w, beatEl(w, 20), X(20), X(20) + 4)
    expect(emittedAll(w, 'update:selection')).toEqual([[20]])
    const [grid, label] = lastGrid(w)
    expect(label).toBe('mover un pulso')
    expect(grid.beats[20]).toBe(10.7)
    expect(untouched(grid.beats, [20])).toBe(true)
  })

  it('Shift + drag takes this beat and all those after it, without changing the selection', async () => {
    const w = await mountEdit({ selection: [3] })
    await drag(w, beatEl(w, 70), X(70), X(70) + 4, { shiftKey: true })
    const [grid, label] = lastGrid(w)
    expect(label).toBe('mover desde aquí')
    expect(grid.beats.slice(70)).toEqual(range(70, 78).map((i) => r3(T(i) + 0.2)))
    expect(untouched(grid.beats, range(70, 78))).toBe(true)
    expect(w.emitted('update:selection')).toBeUndefined()
    // sin pasarse del final de la canción: la última, a 40 s como mucho
    await drag(w, beatEl(w, 70), X(70), X(70) + 60, { shiftKey: true })
    const far = lastGrid(w)[0]
    expect(far.beats.at(-1)).toBe(40)
    expect(far.beats[70]).toBe(T(70) + 0.5)
  })

  it('dragging the band moves the chosen beats like dragging one of them', async () => {
    const w = await mountEdit({ selection: range(10, 14) })
    await drag(w, w.find('.tl-selband').element, X(12) + 1, X(12) + 5)
    const [grid, label] = lastGrid(w)
    expect(label).toBe('mover 5 pulsos')
    expect(grid.beats.slice(10, 15)).toEqual(range(10, 14).map((i) => r3(T(i) + 0.2)))
    expect(untouched(grid.beats, range(10, 14))).toBe(true)
    // la banda no es la onda: no elige tramo ni lleva la canción
    expect(w.emitted('seek')).toBeUndefined()
    expect(w.emitted('update:loops')).toBeUndefined()
  })

  it('sends a grid with its downbeats, meter and first downbeat in agreement', async () => {
    const w = await mountEdit({ selection: range(10, 13) })
    await drag(w, beatEl(w, 11), X(11), X(11) - 6)
    const [grid] = lastGrid(w)
    expect(grid.meter).toBe(4)
    expect(grid.bars).toEqual(makeGrid().bars)
    expect(grid.first_downbeat).toBe(grid.bars[0])
    // lo que guarda quien la recibe sale igual de ella
    expect(gridToOwn(grid).bars).toEqual(grid.bars)
    expect(gridToOwn(grid).beats).toEqual(grid.beats)
  })

  it('a drag the neighbours leave where it was is not a change', async () => {
    // el pulso 2 pegado al 3: no hay a dónde ir hacia la derecha
    const grid = makeGrid()
    grid.beats[3] = grid.beats[2] + 0.02
    const w = await mountEdit({ grid, selection: [2] })
    await drag(w, beatEl(w, 2), X(2), X(2) + 6)
    expect(w.emitted('update:grid')).toBeUndefined()
  })
})

describe('stretching a block from the ends of the band', () => {
  it('the right handle moves the last beat and keeps the first where it is', async () => {
    const w = await mountEdit({ selection: range(10, 14) })
    const handle = w.find('.tl-selband-h.b').element
    fire(handle, 'pointerdown', { clientX: X(14) })
    move(X(14) - 10)
    await flushPromises()
    // 5,5 a 7,0 s en cuatro intervalos iguales: 160 bpm
    expect(w.find('.tl-readout').text()).toBe('160 bpm')
    up(X(14) - 10)
    await flushPromises()
    const [grid, label] = lastGrid(w)
    expect(label).toBe('estirar 5 pulsos')
    expect(grid.beats.slice(10, 15)).toEqual([5.5, 5.875, 6.25, 6.625, 7])
    expect(untouched(grid.beats, range(10, 14))).toBe(true)
    expect(w.emitted('update:selection')).toBeUndefined()
  })

  it('the left handle moves the first beat and keeps the last where it is', async () => {
    const w = await mountEdit({ selection: range(10, 14) })
    await drag(w, w.find('.tl-selband-h.a').element, X(10), X(10) + 8)
    const [grid, label] = lastGrid(w)
    expect(label).toBe('estirar 5 pulsos')
    expect(grid.beats.slice(10, 15)).toEqual([5.9, 6.3, 6.7, 7.1, 7.5])
    expect(untouched(grid.beats, range(10, 14))).toBe(true)
  })

  it('does not go past the beat next to the block', async () => {
    const w = await mountEdit({ selection: range(10, 14) })
    // dos segundos más allá chocaría con el pulso 15 (8,0 s): se queda 20 ms antes
    await drag(w, w.find('.tl-selband-h.b').element, X(14), X(14) + 40)
    const [grid] = lastGrid(w)
    expect(grid.beats[10]).toBe(5.5)
    expect(grid.beats[14]).toBe(7.98)
    expect(grid.beats[15]).toBe(8)
  })

  it('says the tempo of the stretched span, with a decimal comma when it has one', async () => {
    const w = await mountEdit({ selection: range(10, 18) })
    const handle = w.find('.tl-selband-h.b').element
    fire(handle, 'pointerdown', { clientX: X(18) })
    // 5,5 a 8,5 s en ocho intervalos de 0,375 s: 160 bpm, sin decimales
    move(X(18) - 20)
    await flushPromises()
    expect(w.find('.tl-readout').text()).toBe('160 bpm')
    // 5,5 a 9,0 s: 0,4375 s por intervalo, 137,1 bpm
    move(X(18) - 10)
    await flushPromises()
    expect(w.find('.tl-readout').text()).toBe('137,1 bpm')
    up(X(18) - 10)
    await flushPromises()
    expect(lastGrid(w)[1]).toBe('estirar 9 pulsos')
  })
})

describe('the magnet to the hits of the audio', () => {
  const hits = (list, over = {}) => ({
    selection: [],
    snapHits: true,
    hitsRef: 2,
    hits: list,
    ...over
  })
  /** Lleva el pulso 10 (5,5 s) 4 px (0,2 s) a la derecha: iría a 5,7 s. */
  async function dragTen(w, init = {}) {
    await drag(w, beatEl(w, 10), X(10), X(10) + 4, init)
    return lastGrid(w)?.[0].beats[10]
  }

  it('snaps to a strong hit within 12 ms', async () => {
    const w = await mountEdit(
      hits([
        [5.5, 0.1],
        [5.705, 1],
        [5.9, 3]
      ])
    )
    expect(await dragTen(w)).toBe(5.705)
  })

  it('goes to the nearest one when there are several in reach', async () => {
    const w = await mountEdit(
      hits([
        [5.692, 2],
        [5.703, 2]
      ])
    )
    expect(await dragTen(w)).toBe(5.703)
  })

  it('does not snap beyond 12 ms, to a weak hit, with the magnet off, or with Ctrl held', async () => {
    const far = await mountEdit(hits([[5.713, 3]]))
    expect(await dragTen(far)).toBe(5.7)
    // un golpe flojo (menos de un cuarto de la referencia) no es imán
    const weak = await mountEdit(hits([[5.705, 0.4]]))
    expect(await dragTen(weak)).toBe(5.7)
    const off = await mountEdit(hits([[5.705, 3]], { snapHits: false }))
    expect(await dragTen(off)).toBe(5.7)
    const ctrl = await mountEdit(hits([[5.705, 3]]))
    fire(beatEl(ctrl, 10), 'pointerdown', { clientX: X(10) })
    move(X(10) + 4, { ctrlKey: true })
    up(X(10) + 4, { ctrlKey: true })
    await flushPromises()
    expect(lastGrid(ctrl)[0].beats[10]).toBe(5.7)
    // y sin Ctrl, en la misma canción, se pega
    expect(await dragTen(await mountEdit(hits([[5.705, 3]])))).toBe(5.705)
  })

  it('the whole block moves by the distance the dragged beat snapped', async () => {
    const w = await mountEdit(hits([[6.205, 1]], { selection: range(10, 12) }))
    // el pulso 11 (6,0 s) iría a 6,2: se pega a 6,205 y los tres corren +205 ms
    await drag(w, beatEl(w, 11), X(11), X(11) + 4)
    const [grid] = lastGrid(w)
    expect(grid.beats.slice(10, 13)).toEqual([5.705, 6.205, 6.705])
    expect(untouched(grid.beats, range(10, 12))).toBe(true)
  })

  it('the moving end of a stretch snaps too', async () => {
    const w = await mountEdit(hits([[7.005, 2]], { selection: range(10, 14) }))
    await drag(w, w.find('.tl-selband-h.b').element, X(14), X(14) - 10)
    const [grid] = lastGrid(w)
    expect(grid.beats[14]).toBe(7.005)
    expect(grid.beats[10]).toBe(5.5)
  })
})

describe('adding and removing beats', () => {
  it('a double click adds a beat there, chooses it and shifts the downbeats after it', async () => {
    const w = await mountEdit()
    // 5,25 s: entre el pulso 9 (5,0) y el 10 (5,5)
    fire(w.find('.tl-box').element, 'dblclick', { clientX: 105 })
    await flushPromises()
    const [grid, label] = lastGrid(w)
    expect(label).toBe('añadir un pulso')
    expect(grid.beats).toHaveLength(N + 1)
    expect(grid.beats[10]).toBe(5.25)
    expect(grid.bars).toEqual(makeGrid().bars.map((b) => (b >= 10 ? b + 1 : b)))
    expect(emittedAll(w, 'update:selection')).toEqual([[10]])
    // y a menos de 30 ms de otro pulso, no
    fire(w.find('.tl-box').element, 'dblclick', { clientX: 100.2 })
    await flushPromises()
    expect(w.emitted('update:grid')).toHaveLength(1)
  })

  it('on a beat inside a chosen group the menu removes them all, and offers to choose more', async () => {
    const w = await mountEdit({ selection: [3, 4, 5] })
    fire(beatEl(w, 4), 'contextmenu', { clientX: X(4) })
    expect(menuLabels()).toEqual([
      'Este es el «1»',
      'Seleccionar desde aquí hasta el final',
      'Seleccionar desde el principio hasta aquí',
      'Quitar 3 pulsos'
    ])
    await menuItem('Quitar 3 pulsos').action()
    await flushPromises()
    const [grid, label] = lastGrid(w)
    expect(label).toBe('quitar 3 pulsos')
    expect(grid.beats).toHaveLength(N - 3)
    expect(grid.beats.slice(2, 5)).toEqual([T(2), T(6), T(7)])
    // el «1» del pulso 4 se fue con él; los de después, tres puestos atrás
    expect(grid.bars).toEqual(
      makeGrid()
        .bars.filter((b) => b !== 4)
        .map((b) => (b > 5 ? b - 3 : b))
    )
    expect(emittedAll(w, 'update:selection')).toEqual([[]])
  })

  it('on a single beat the menu is the one of always, and the other chosen beats shift', async () => {
    const w = await mountEdit({ selection: [20] })
    fire(beatEl(w, 5), 'contextmenu', { clientX: X(5) })
    expect(menuLabels()).toEqual(['Este es el «1»', 'Quitar este pulso'])
    await menuItem('Quitar este pulso').action()
    await flushPromises()
    const [grid, label] = lastGrid(w)
    expect(label).toBe('quitar un pulso')
    expect(grid.beats).toHaveLength(N - 1)
    expect(grid.beats[5]).toBe(T(6))
    // el elegido, que estaba más allá, sigue siendo el mismo pulso
    expect(emittedAll(w, 'update:selection')).toEqual([[19]])
    // también con un grupo elegido: un pulso de fuera de él es solo ese pulso
    const group = await mountEdit({ selection: [3, 4, 5] })
    fire(beatEl(group, 30), 'contextmenu', { clientX: X(30) })
    expect(menuLabels()).toEqual(['Este es el «1»', 'Quitar este pulso'])
  })

  it('the menu chooses from a beat to the end or from the start to it', async () => {
    const w = await mountEdit({ selection: [60, 61] })
    fire(beatEl(w, 61), 'contextmenu', { clientX: X(61) })
    await menuItem('Seleccionar desde aquí hasta el final').action()
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual(range(61, N - 1))
    await menuItem('Seleccionar desde el principio hasta aquí').action()
    expect(emittedAll(w, 'update:selection').at(-1)).toEqual(range(0, 61))
  })

  it('«this is the 1» says so in the history', async () => {
    const w = await mountEdit()
    fire(beatEl(w, 5), 'contextmenu', { clientX: X(5) })
    await menuItem('Este es el «1»').action()
    const [grid, label] = lastGrid(w)
    expect(label).toBe('el «1» aquí')
    expect(grid.bars).toContain(5)
  })

  it('does not leave the song with fewer than two beats', async () => {
    const grid = makeGrid()
    grid.beats = [1, 2]
    grid.bars = [0]
    const w = await mountEdit({ grid, selection: [0, 1] })
    fire(beatEl(w, 0), 'contextmenu', { clientX: 20 })
    await menuItem('Quitar 2 pulsos').action()
    expect(w.emitted('update:grid')).toBeUndefined()
  })
})

describe('what is painted behind the beats', () => {
  /** Cuántas rayas lleva un trazo (cada una empieza con M). */
  const strokes = (d) => (d.match(/M/g) || []).length
  const hitsPath = (w) => w.find('.tl-hits path').attributes('d')

  it('draws the hits as one path, as tall as they are strong, up to the reference', async () => {
    const w = await mountEdit({
      showHits: true,
      hitsRef: 2,
      hits: [
        [10, 1],
        [20, 5]
      ]
    })
    expect(w.findAll('.tl-hits')).toHaveLength(1)
    expect(w.findAll('.tl-hits path')).toHaveLength(1)
    // la mitad de la referencia, media altura; de más, tope
    expect(hitsPath(w)).toBe('M25.000,100V50.0M50.000,100V0.0')
    await w.setProps({ showHits: false })
    expect(w.find('.tl-hits').exists()).toBe(false)
  })

  it('draws only the hits in view', async () => {
    const list = Array.from({ length: 160 }, (_, i) => [i * 0.25, 1])
    const w = await mountEdit({ showHits: true, hitsRef: 1, hits: list })
    expect(strokes(hitsPath(w))).toBe(160)
    w.vm.zoomToSpan(10, 12)
    await flushPromises()
    const { from, to } = w.vm.view
    const inside = list.filter(([t]) => t >= from && t <= to).length
    expect(inside).toBeGreaterThan(0)
    expect(inside).toBeLessThan(40)
    expect(strokes(hitsPath(w))).toBe(inside)
  })

  it('keeps the 2,500 strongest when more are in view', async () => {
    const total = 3000
    // cada golpe más fuerte que el anterior: los que sobran son los primeros
    const list = Array.from({ length: total }, (_, i) => [(i * DURATION) / total, 0.5 + i / 1000])
    const w = await mountEdit({ showHits: true, hitsRef: 4, hits: list })
    const d = hitsPath(w)
    expect(strokes(d)).toBe(2500)
    const x = (t) => `M${((t / DURATION) * 100).toFixed(3)},`
    expect(d.includes(x(list[0][0]))).toBe(false)
    expect(d.includes(x(list[499][0]))).toBe(false)
    expect(d.includes(x(list[500][0]))).toBe(true)
    expect(d.includes(x(list[total - 1][0]))).toBe(true)
  })

  it('sorts hits that come out of order and copes with having no reference', async () => {
    const w = await mountEdit({
      showHits: true,
      hits: [
        [20, 4],
        [10, 2]
      ]
    })
    // sin referencia, el más fuerte es el tope
    expect(hitsPath(w)).toBe('M25.000,100V50.0M50.000,100V0.0')
  })

  it('paints the preview and the automatic grid behind the beats, and nothing in them is interactive', async () => {
    const auto = Array.from({ length: 20 }, (_, i) => 1 + i * 0.6)
    const w = await mountEdit({
      ghostBeats: auto,
      preview: { beats: [2, 2.4, 2.8, 3.2, 3.6], bars: [0, 4] }
    })
    expect(strokes(w.find('.tl-ghost').attributes('d'))).toBe(20)
    expect(strokes(w.find('.tl-preview:not(.one)').attributes('d'))).toBe(5)
    expect(strokes(w.find('.tl-preview.one').attributes('d'))).toBe(2)
    // por detrás de los pulsos, que son lo que se coge
    const marks = w.find('.tl-marks').element
    expect(
      marks.compareDocumentPosition(beatEl(w, 0)) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
    expect(w.find('.tl-marks').findAll('.tl-beat')).toHaveLength(0)
    expect(marks.getAttribute('aria-hidden')).toBe('true')
    // las dos capas no reciben el puntero: lo de debajo se sigue pudiendo coger
    const css = readFileSync(join(STYLES, 'study.css'), 'utf8')
    expect(css).toMatch(/\.tl-marks,\s*\.tl-hits\s*\{[^}]*pointer-events:\s*none/)
    // sin ellas, nada
    const plain = await mountEdit()
    expect(plain.find('.tl-marks').exists()).toBe(false)
    expect(plain.find('.tl-hits').exists()).toBe(false)
  })

  it('only paints the lines in view', async () => {
    const w = await mountEdit({ ghostBeats: Array.from({ length: 80 }, (_, i) => i * 0.5) })
    expect(strokes(w.find('.tl-ghost').attributes('d'))).toBe(80)
    w.vm.zoomToSpan(10, 12)
    await flushPromises()
    const { from, to } = w.vm.view
    const inside = Array.from({ length: 80 }, (_, i) => i * 0.5).filter(
      (t) => t >= from && t <= to
    ).length
    expect(strokes(w.find('.tl-ghost').attributes('d'))).toBe(inside)
  })
})

describe('zooming to a span', () => {
  it('fills the view with the span and a little margin', async () => {
    const w = await mountEdit()
    w.vm.zoomToSpan(10, 20)
    await flushPromises()
    const { from, to } = w.vm.view
    expect(from).toBeLessThan(10)
    expect(to).toBeGreaterThan(20)
    expect(to - from).toBeLessThan(12)
    // al revés, igual
    w.vm.zoomToSpan(20, 10)
    await flushPromises()
    expect(w.vm.view.from).toBeCloseTo(from, 6)
    expect(w.vm.view.to).toBeCloseTo(to, 6)
    // un trozo diminuto no acerca más que lo que deja el zoom
    w.vm.zoomToSpan(30, 30.1)
    await flushPromises()
    const { from: a, to: b } = w.vm.view
    expect(a).toBeLessThan(30)
    expect(b).toBeGreaterThan(30.1)
    expect(b - a).toBeGreaterThan(1.49)
    // y a un extremo de la canción, no se sale
    w.vm.zoomToSpan(0, 2)
    await flushPromises()
    expect(w.vm.view.from).toBe(0)
  })
})
