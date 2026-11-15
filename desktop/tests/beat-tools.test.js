import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Ajustar el clic manual de muchos pulsos a la vez: deshacer y rehacer, la
// selección y sus teclas, las herramientas de la barra y el panel «Tempo y
// onda». La lógica pura está probada aparte (beatedit, beatalign,
// beathistory); aquí, lo que la une con el modo estudio.
const held = vi.hoisted(() => ({ playback: null, api: null, state: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return { ...actual, inTauri: true, api: held.api, playback: held.playback.bridge }
})

import StudyBar from '../src/components/StudyBar.vue'
import StudyTimeline from '../src/components/StudyTimeline.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetClick, useClick } from '../src/composables/useClick.js'
import { useNotices } from '../src/composables/useNotices.js'
import { dialogOk, useDialog } from '../src/composables/useDialog.js'
import { useContextMenu } from '../src/composables/useContextMenu.js'
import {
  planAlign,
  describePlan,
  findTempo,
  timeText,
  bpmText,
  lagText
} from '../src/composables/useBeatEdit.js'
import { song, stemsOf } from './support/backend.js'

const round3 = (t) => Math.round(t * 1000) / 1000
/**
 * (la línea de tiempo declara `update:selection` y avisa si no, pero eso es cosa
 * de otra rama: aquí no estorba)
 */
let warn = null

/** El clic automático de la canción: 96 bpm. */
const GRID = {
  bpm: 96,
  meter: 4,
  beats: Array.from({ length: 300 }, (_, i) => round3(0.5 + i * 0.625)),
  first_downbeat: 0,
  phase3: 0,
  phase4: 0,
  confidence: 0.9,
  bars: Array.from({ length: 75 }, (_, i) => i * 4)
}
/** Su clic manual: 120 bpm, un «1» cada cuatro. */
const HIS = {
  beats: Array.from({ length: 120 }, (_, i) => round3(1 + i * 0.5)),
  bars: Array.from({ length: 30 }, (_, i) => i * 4),
  meter: 4,
  touched: []
}
const base = {
  index: 0,
  length: 1,
  duration: 200,
  has_previous: false,
  has_next: false,
  error: '',
  has_output: true,
  loop_a: 0,
  loop_b: 0,
  speed: 1,
  pitch: 0,
  path: '/musica/mi-gozo.mp3',
  metronome: {
    on: false,
    bpm: 100,
    meter: 4,
    shift: 0,
    mult: 0,
    volume: 0.8,
    has_grid: false,
    free: true,
    confidence: 0
  }
}
const bridge = () => held.playback.bridge
const copy = (x) => JSON.parse(JSON.stringify(x))

beforeEach(() => {
  vi.clearAllMocks()
  const original = console.warn
  warn = vi.spyOn(console, 'warn').mockImplementation((...args) => {
    if (!String(args[0]).includes('update:selection')) original(...args)
  })
  vi.useFakeTimers()
  localStorage.clear()
  resetPlayback()
  resetClick()
  held.playback.reset()
  useNotices().clearNotices()
  held.state.beatGrid = copy(GRID)
  held.state.beatGridPending = []
  held.state.hits = {}
  held.state.stems = {}
  withOwn()
  held.playback.emit({
    track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
    playing: false,
    position: 12,
    ...base
  })
})

/** Su canción 7 con su clic manual guardado (o sin él). */
function withOwn(own = HIS, extra = {}) {
  const metronome = own ? { own: copy(own), ...extra } : { ...extra }
  held.state.songs = [
    song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome }) }),
    song(8, { title: 'Otra' })
  ]
  localStorage.setItem('danplay.click.mode', JSON.stringify('own'))
  resetClick()
}

const mounted = []
const mountView = async () => {
  const w = mount(StudyBar, { attachTo: document.body })
  mounted.push(w)
  await settle()
  return w
}
afterEach(() => {
  for (const w of mounted.splice(0)) {
    try {
      w.unmount()
    } catch {
      /* ya estaba desmontada */
    }
  }
  document.querySelectorAll('.modal-back').forEach((e) => e.remove())
  vi.useRealTimers()
  warn?.mockRestore()
})
const settle = async () => {
  await flushPromises()
  await flushPromises()
}
/** Deja pasar el tiempo (las pruebas usan un reloj falso). */
const wait = async (ms) => {
  await vi.advanceTimersByTimeAsync(ms)
  await settle()
}
const text = (w, sel) => w.find(sel).text()
const buttonOf = (w, label, nth = 0) =>
  w
    .findAll('button')
    .filter((b) => b.text().trim() === label || b.attributes('aria-label') === label)[nth]
/** Un botón de la barra de herramientas (en la línea de tiempo hay otros con el mismo nombre). */
const inTools = (w, label) =>
  w
    .find('.beat-tools')
    .findAll('button')
    .find((b) => b.text().trim() === label)
const stored = () => JSON.parse(held.state.songs.find((s) => s.id === 7).study || '{}')
const savedOwn = () => stored().metronome?.own
/** Lo último que se le dio al motor como rejilla. */
const engine = () => bridge().setBeatGrid.mock.calls.at(-1)?.[1]

function key(k, mods = {}, target = window) {
  const e = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...mods })
  target.dispatchEvent(e)
  return e
}

/** Entra en «Ajustar pulsos» y deja la barra lista. */
async function startEditing(w) {
  await (buttonOf(w, 'Ajustar pulsos') ?? buttonOf(w, 'Empezar el mío')).trigger('click')
  await settle()
  expect(w.find('.beat-tools').exists(), 'las herramientas salen al ajustar').toBe(true)
}
/** Elige estos pulsos como lo haría la línea de tiempo. */
async function choose(w, list) {
  w.findComponent(StudyTimeline).vm.$emit('update:selection', list)
  await settle()
}
const range = (a, b) => Array.from({ length: b - a + 1 }, (_, i) => a + i)
/** Un arrastre que acaba de soltarse en la onda (la rejilla entera). */
async function drop(w, f, label) {
  const g = useClick().grid.value
  const beats = [...g.beats]
  f(beats)
  w.findComponent(StudyTimeline).vm.$emit('update:grid', { ...g, beats, bars: [...g.bars] }, label)
  await settle()
}

describe('the history', () => {
  it('Ctrl+Z puts back the saved click and Ctrl+Shift+Z does it again', async () => {
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[10] += 0.05), 'mover 1 pulso')
    expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.05, 5)
    expect(buttonOf(w, 'Deshacer').attributes('title')).toBe('Deshacer: mover 1 pulso (Ctrl+Z)')
    expect(buttonOf(w, 'Rehacer').attributes('disabled')).toBeDefined()

    const e = key('z', { ctrlKey: true })
    await settle()
    expect(e.defaultPrevented).toBe(true)
    expect(savedOwn().beats).toEqual(HIS.beats)
    // y suena como estaba
    expect(engine().beats).toEqual(HIS.beats)
    expect(useNotices().notices.value.at(-1).message).toBe('Deshecho: mover 1 pulso')
    expect(buttonOf(w, 'Rehacer').attributes('title')).toBe('Rehacer: mover 1 pulso (Ctrl+Mayús+Z)')

    key('z', { ctrlKey: true, shiftKey: true })
    await settle()
    expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.05, 5)
    expect(engine().beats[10]).toBeCloseTo(HIS.beats[10] + 0.05, 5)
    expect(useNotices().notices.value.at(-1).message).toBe('Rehecho: mover 1 pulso')

    // Ctrl+Y es lo mismo que rehacer
    key('z', { ctrlKey: true })
    await settle()
    key('y', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.05, 5)
  })

  it('undoing back to «no manual click yet» goes back to the automatic one', async () => {
    withOwn(null)
    const w = await mountView()
    expect(useClick().source.value, 'sin el suyo, suena el automático').toBe('beat-this')
    await startEditing(w)
    await drop(w, (b) => (b[40] += 0.08), 'mover 1 pulso')
    expect(savedOwn().beats).toHaveLength(300)
    expect(useClick().source.value).toBe('own')

    key('z', { ctrlKey: true })
    await settle()
    // ya no hay clic manual guardado, y vuelve el automático
    expect(stored().metronome?.own).toBeUndefined()
    expect(useClick().source.value).toBe('beat-this')
    expect(engine().beats).toEqual(GRID.beats)

    key('y', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats[40]).toBeCloseTo(GRID.beats[40] + 0.08, 5)
    expect(useClick().source.value).toBe('own')
  })

  it('undoing back to the automatic click brings back its double and its shifted «1»', async () => {
    withOwn(null, { mult: 1, shift: 1 })
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[3] += 0.03))
    // el clic manual ya lleva el doble y el «1» corrido: Rust no los vuelve a aplicar
    expect(bridge().setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ mult: 0, shift: 0 })
    )
    expect(stored().metronome.mult).toBeUndefined()
    bridge().setMetronome.mockClear()

    key('z', { ctrlKey: true })
    await settle()
    expect(stored().metronome.own).toBeUndefined()
    expect(stored().metronome.mult).toBe(1)
    expect(stored().metronome.shift).toBe(1)
    expect(bridge().setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ mult: 1, shift: 1 })
    )
  })

  it('editing the automatic click keeps where its «1» fell, also when it has no list of bars', async () => {
    withOwn(null)
    held.state.beatGrid = { ...copy(GRID), first_downbeat: 2, bars: [] }
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[40] += 0.08), 'mover 1 pulso')
    const bars = savedOwn().bars
    expect(bars.slice(0, 4)).toEqual([2, 6, 10, 14])
    expect(bars.every((i) => (i - 2) % 4 === 0)).toBe(true)
  })

  it('a new change throws away what was undone', async () => {
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[5] += 0.02))
    key('z', { ctrlKey: true })
    await settle()
    expect(buttonOf(w, 'Rehacer').attributes('disabled')).toBeUndefined()
    await drop(w, (b) => (b[6] += 0.02))
    expect(buttonOf(w, 'Rehacer').attributes('disabled')).toBeDefined()
  })

  it('a re-analysis in Manual and «Borrar el manual» are undone too', async () => {
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[20] += 0.04))
    const moved = savedOwn()

    // Borrar el manual: pide confirmación y se puede recuperar
    await buttonOf(w, 'Borrar el manual').trigger('click')
    await settle()
    expect(useDialog().dialog.value.message).toContain('Ctrl+Z lo recupera')
    dialogOk()
    await settle()
    expect(stored().metronome?.own).toBeUndefined()
    expect(buttonOf(w, 'Deshacer').attributes('title')).toContain('borrar el manual')
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(moved.beats)
    expect(useClick().source.value).toBe('own')
  })

  it('a re-analysis in Manual is one step too, and undoing it brings back what he had', async () => {
    const NEW = { ...GRID, bpm: 120, beats: Array.from({ length: 300 }, (_, i) => 0.4 + i * 0.5) }
    held.state.reanalysis = { grid: NEW }
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[20] += 0.04), 'mover 1 pulso')
    const mine = copy(savedOwn())
    const { menu } = useContextMenu()
    await buttonOf(w, 'Re-analizar…').trigger('click')
    await menu.value.items.find((i) => i.label === 'Arreglar huecos y saltos').action()
    await settle()
    expect(savedOwn().beats, 'se re-analizó').not.toEqual(mine.beats)
    expect(buttonOf(w, 'Deshacer').attributes('title')).toContain('Re-analizar')
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(mine.beats)
    expect(savedOwn().touched).toEqual(mine.touched)
    expect(engine().beats).toEqual(mine.beats)
  })

  it('the keys are not his while the study is hidden', async () => {
    const w = mount(StudyBar, { attachTo: document.body, props: { visible: true } })
    mounted.push(w)
    await settle()
    await startEditing(w)
    await drop(w, (b) => (b[5] += 0.02))
    await w.setProps({ visible: false })
    const e = key('z', { ctrlKey: true })
    await settle()
    expect(e.defaultPrevented).toBe(false)
    expect(savedOwn().beats[5]).toBeCloseTo(HIS.beats[5] + 0.02, 5)
    await w.setProps({ visible: true })
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(HIS.beats)
  })

  it('the history is of each song: it starts over on another one', async () => {
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[5] += 0.02))
    expect(buttonOf(w, 'Deshacer').attributes('disabled')).toBeUndefined()
    held.playback.emit({ track: { id: 8, title: 'Otra', artist: 'Barak', duration: 100 } })
    await settle()
    expect(buttonOf(w, 'Deshacer')?.attributes('disabled')).toBeDefined()
    const before = held.api.setStudy.mock.calls.length
    key('z', { ctrlKey: true })
    await settle()
    expect(held.api.setStudy.mock.calls.length, 'Ctrl+Z no toca la otra canción').toBe(before)
  })
})

describe('the keys', () => {
  it('Ctrl+Z is left alone in a text field and with a dialog open', async () => {
    const w = await mountView()
    await startEditing(w)
    await drop(w, (b) => (b[5] += 0.02))
    const field = w.find('.bt-ms input').element
    field.focus()
    const e = key('z', { ctrlKey: true }, field)
    await settle()
    expect(e.defaultPrevented, 'en el campo manda su propio deshacer').toBe(false)
    expect(savedOwn().beats[5]).toBeCloseTo(HIS.beats[5] + 0.02, 5)
    // con un diálogo abierto, tampoco
    const dialog = document.createElement('div')
    dialog.className = 'modal-back'
    document.body.appendChild(dialog)
    const e2 = key('z', { ctrlKey: true })
    await settle()
    expect(e2.defaultPrevented).toBe(false)
    expect(savedOwn().beats[5]).toBeCloseTo(HIS.beats[5] + 0.02, 5)
    dialog.remove()
    // y sin nada que deshacer no se queda con la tecla
    key('z', { ctrlKey: true })
    await settle()
    const e3 = key('z', { ctrlKey: true })
    expect(e3.defaultPrevented).toBe(false)
  })

  it('the arrows nudge the chosen beats by 5 ms (1 with Shift, 25 with Ctrl) and the player does not seek', async () => {
    const w = await mountView()
    await startEditing(w)
    const seeks = vi.fn()
    window.addEventListener('keydown', seeks)
    try {
      // sin elegir nada, las flechas son del reproductor
      const free = key('ArrowRight')
      expect(free.defaultPrevented).toBe(false)
      expect(seeks).toHaveBeenCalledTimes(1)
      seeks.mockClear()

      await choose(w, [10, 11, 12])
      const e = key('ArrowRight')
      await settle()
      expect(e.defaultPrevented).toBe(true)
      expect(seeks, 'el reproductor no la recibe').not.toHaveBeenCalled()
      expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.005, 5)
      expect(savedOwn().beats[12]).toBeCloseTo(HIS.beats[12] + 0.005, 5)
      expect(savedOwn().beats[13]).toBe(HIS.beats[13])
      key('ArrowLeft', { shiftKey: true })
      await settle()
      expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.004, 5)
      key('ArrowRight', { ctrlKey: true })
      await settle()
      expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.029, 5)
    } finally {
      window.removeEventListener('keydown', seeks)
    }
  })

  it('holding an arrow is one step to undo, and a pause makes another', async () => {
    const w = await mountView()
    await startEditing(w)
    await choose(w, range(20, 30))
    for (let i = 0; i < 6; i++) {
      key('ArrowRight')
      await wait(80)
    }
    expect(savedOwn().beats[20]).toBeCloseTo(HIS.beats[20] + 0.03, 5)
    // una sola entrada
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(HIS.beats)
    expect(buttonOf(w, 'Deshacer').attributes('disabled')).toBeDefined()
    key('z', { ctrlKey: true, shiftKey: true })
    await settle()
    // con una pausa de por medio, dos
    await wait(2000)
    key('ArrowRight')
    await wait(2000)
    key('ArrowRight')
    await settle()
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats[20]).toBeCloseTo(HIS.beats[20] + 0.03 + 0.005, 5)
  })

  it('Esc closes the panel first and then lets go of the selection; Ctrl+A chooses all; Delete removes', async () => {
    const w = await mountView()
    await startEditing(w)
    await choose(w, range(5, 8))
    await buttonOf(w, 'Tempo y onda…').trigger('click')
    await settle()
    expect(w.find('.beat-align').exists()).toBe(true)
    key('Escape')
    await settle()
    expect(w.find('.beat-align').exists(), 'Esc cierra el panel').toBe(false)
    expect(text(w, '.bt-summary')).toContain('4 pulsos')
    key('Escape')
    await settle()
    expect(text(w, '.bt-summary')).toContain('Ningún pulso elegido')

    const all = key('a', { ctrlKey: true })
    await settle()
    expect(all.defaultPrevented).toBe(true)
    expect(text(w, '.bt-summary')).toContain('120 pulsos')

    await choose(w, [3, 4])
    const del = key('Delete')
    await settle()
    expect(del.defaultPrevented).toBe(true)
    expect(savedOwn().beats).toHaveLength(118)
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(HIS.beats)
  })
})

describe('the tools', () => {
  /** Hace lo que sea con los pulsos elegidos y deja el clic guardado. */
  async function withSelection(list, f) {
    const w = await mountView()
    await startEditing(w)
    await choose(w, list)
    await f(w)
    await settle()
    return w
  }
  /** Una sola entrada en la historia: un Ctrl+Z lo deja todo como estaba. */
  async function oneStep(w, label, last = true) {
    expect(buttonOf(w, 'Deshacer').attributes('title')).toContain(label)
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(HIS.beats)
    expect(savedOwn().bars).toEqual(HIS.bars)
    if (last) {
      expect(buttonOf(w, 'Deshacer').attributes('disabled'), 'no quedó otro paso').toBeDefined()
    }
  }

  it('Desplazar moves them by what is typed or by the buttons, and keeps the distances', async () => {
    const w = await withSelection(range(10, 19), async (w) => {
      await buttonOf(w, '+10').trigger('click')
    })
    expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.01, 5)
    expect(savedOwn().beats[19]).toBeCloseTo(HIS.beats[19] + 0.01, 5)
    expect(savedOwn().beats[9]).toBe(HIS.beats[9])
    expect(savedOwn().beats[20]).toBe(HIS.beats[20])
    // escrito, con signo menos (un rato después: es otro paso)
    await wait(1500)
    const field = w.find('.bt-ms input')
    await field.setValue('-25')
    await field.trigger('keydown', { key: 'Enter' })
    await settle()
    expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] - 0.015, 5)
    expect(field.element.value, 'el campo queda vacío').toBe('')
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats[10], 'el escrito y el botón son pasos aparte').toBeCloseTo(
      HIS.beats[10] + 0.01,
      5
    )
    await oneStep(w, 'desplazar 10 pulsos', false)
  })

  it('pressing a shift button several times in a row is one step', async () => {
    const w = await withSelection(range(10, 19), async (w) => {
      for (let i = 0; i < 4; i++) await buttonOf(w, '+1').trigger('click')
    })
    expect(savedOwn().beats[10]).toBeCloseTo(HIS.beats[10] + 0.004, 5)
    await oneStep(w, 'desplazar 10 pulsos')
  })

  it('Desplazar stops at the neighbour and says so', async () => {
    const w = await withSelection(range(10, 19), async (w) => {
      await w.find('.bt-ms input').setValue('900')
      await w.find('.bt-ms input').trigger('keydown', { key: 'Enter' })
    })
    // el pulso de al lado está a medio segundo: llega a 20 ms de él
    expect(savedOwn().beats[19]).toBeCloseTo(HIS.beats[20] - 0.02, 5)
    expect(w.find('.bt-ms input').exists()).toBe(true)
  })

  it('Repartir parejo spaces them evenly between the first and the last, in one step', async () => {
    const own = copy(HIS)
    own.beats[12] += 0.07
    own.beats[13] -= 0.05
    held.state.songs[0].study = JSON.stringify({ metronome: { own } })
    resetClick()
    const w = await mountView()
    await startEditing(w)
    await choose(w, range(10, 15))
    await buttonOf(w, 'Repartir parejo').trigger('click')
    await settle()
    for (let i = 10; i <= 15; i++) expect(savedOwn().beats[i]).toBeCloseTo(HIS.beats[i], 3)
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(own.beats)
  })

  it('Doble de pulsos and Mitad', async () => {
    const w = await withSelection(range(10, 17), async (w) => {
      await buttonOf(w, 'Doble de pulsos').trigger('click')
    })
    expect(savedOwn().beats).toHaveLength(127)
    expect(savedOwn().beats[11]).toBeCloseTo((HIS.beats[10] + HIS.beats[11]) / 2, 3)
    // lo nuevo queda elegido: se ve qué salió
    expect(text(w, '.bt-summary')).toContain('15 pulsos')
    await oneStep(w, 'doble de pulsos')

    await choose(w, range(10, 17))
    await buttonOf(w, 'Mitad').trigger('click')
    await settle()
    expect(savedOwn().beats).toHaveLength(116)
    await oneStep(w, 'mitad de pulsos')
  })

  it('El «1» cada N puts the downbeats where asked, and the meter with them', async () => {
    const w = await withSelection(range(8, 23), async (w) => {
      await w.find('.bt-bars .select-box').trigger('click')
      await w
        .findAll('.bt-bars .select-opt')
        .find((o) => o.text() === '3')
        .trigger('click')
      await buttonOf(w, 'El «1» cada').trigger('click')
    })
    const inside = savedOwn().bars.filter((i) => i >= 8 && i <= 23)
    expect(inside).toEqual([8, 11, 14, 17, 20, 23])
    expect(savedOwn().meter).toBe(3)
    await oneStep(w, 'el «1» cada 3')
  })

  it('Quitar pulsos removes them (and the «1» on them) in one step, and Supr does the same', async () => {
    const w = await withSelection([4, 5, 6], async (w) => {
      await buttonOf(w, 'Quitar pulsos').trigger('click')
    })
    expect(savedOwn().beats).toHaveLength(117)
    expect(savedOwn().beats.slice(3, 6)).toEqual([HIS.beats[3], HIS.beats[7], HIS.beats[8]])
    // los «1» siguen cayendo en pulsos que existen
    expect(Math.max(...savedOwn().bars)).toBeLessThan(117)
    await oneStep(w, 'quitar 3 pulsos')
  })

  it('Oír el tramo puts them in a loop (a beat of margin each side), turns the click on and plays', async () => {
    await withSelection(range(10, 19), async (w) => {
      await buttonOf(w, 'Oír el tramo').trigger('click')
    })
    const [a, b] = bridge().setLoop.mock.calls.at(-1)
    expect(a).toBeCloseTo(HIS.beats[10] - 0.5, 2)
    expect(b).toBeCloseTo(HIS.beats[19] + 0.5, 2)
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ on: true }))
    expect(bridge().seek).toHaveBeenLastCalledWith(a)
    expect(bridge().toggle).toHaveBeenCalled()
  })

  it('without a selection the tools act on the beats inside the segment, or are off with a hint', async () => {
    const w = await mountView()
    await startEditing(w)
    expect(buttonOf(w, 'Doble de pulsos').attributes('disabled')).toBeDefined()
    expect(buttonOf(w, 'Doble de pulsos').attributes('title')).toContain('Elige pulsos')
    // un tramo A-B de 10 s a 14 s: los pulsos de dentro son el objetivo
    w.findComponent(StudyTimeline).vm.$emit('update:loops', [[10, 14]], { mode: 'select' })
    await settle()
    expect(text(w, '.bt-summary')).toContain('Sin elegir: se usa el tramo')
    expect(text(w, '.bt-summary')).toContain('9 pulsos')
    await buttonOf(w, 'Doble de pulsos').trigger('click')
    await settle()
    expect(savedOwn().beats).toHaveLength(128)
    await buttonOf(w, 'El tramo').trigger('click')
    await settle()
    expect(text(w, '.bt-summary').startsWith('Sin elegir')).toBe(false)
  })

  it('Volver al automático en el tramo puts the automatic beats back there, in one step', async () => {
    await withSelection(range(10, 19), async (w) => {
      await buttonOf(w, 'Volver al automático en el tramo').trigger('click')
    })
    const t0 = HIS.beats[10]
    const t1 = HIS.beats[19]
    const auto = GRID.beats.filter((t) => t >= t0 && t <= t1)
    const now = savedOwn().beats.filter((t) => t >= t0 && t <= t1)
    expect(now).toEqual(auto)
    expect(savedOwn().beats.slice(0, 10)).toEqual(HIS.beats.slice(0, 10))
    expect(held.api.beatGridNow).toHaveBeenCalled()
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(HIS.beats)
  })
})

describe('the Tempo y onda panel', () => {
  const P76 = 60 / 76
  /** Un pasaje a 76 bpm: golpes fuertes (con uno más fuerte cada cuatro), `shift` segundos corridos. */
  const comb76 = (from, until, shift = 0) => {
    const out = []
    for (let t = from + shift, k = 0; t <= until; t += P76, k++) {
      out.push([round3(t), k % 4 === 0 ? 4 : 3])
    }
    return out
  }
  /** Unos golpes que no tienen que ver con ningún peine de 76 bpm (cada 0,37 s). */
  const unrelated = (from, until) => {
    const out = []
    for (let t = from + 0.011; t <= until; t += 0.37) out.push([round3(t), 3])
    return out
  }
  const SEL = range(10, 39) // de 6,0 s a 20,5 s: 30 pulsos a 120 bpm
  const spacing = (beats, a, b) => {
    const gaps = []
    for (let i = a + 1; i <= b; i++) gaps.push(beats[i] - beats[i - 1])
    return gaps
  }
  async function openPanel(w, sel = SEL) {
    if (sel) await choose(w, sel)
    await buttonOf(w, 'Tempo y onda…').trigger('click')
    await settle()
    expect(w.find('.beat-align').exists()).toBe(true)
  }
  async function setTempo(w, v) {
    await buttonOf(w, 'Poner un tempo').trigger('click')
    await w.find('.ba-tempo .study-bpm-input input').setValue(String(v))
    await settle()
  }
  const HOW = {
    none: 'No ajustar',
    phase: 'Solo la fase',
    follow: 'Sigue la onda',
    line: 'Tempo fijo'
  }
  async function fitHow(w, v) {
    await buttonOf(w, HOW[v]).trigger('click')
    await settle()
  }
  const result = (w) => text(w, '.ba-text').replace(/\s+/g, ' ')
  const hitsOf = (list) => {
    held.state.hits = { mix: { hits: list, ref: 3 } }
  }

  it('with nothing chosen it says how to pick a segment, and there is nothing to apply', async () => {
    const w = await mountView()
    await startEditing(w)
    await openPanel(w, null)
    expect(text(w, '.ba-span')).toContain('Elige pulsos')
    expect(buttonOf(w, 'Aplicar').attributes('disabled')).toBeDefined()
    // con un tramo A-B, se trabaja sobre él
    w.findComponent(StudyTimeline).vm.$emit('update:loops', [[10, 20]], { mode: 'select' })
    await settle()
    expect(text(w, '.ba-span')).toMatch(/0:10,0 – 0:20,0 · 21 pulsos/)
  })

  it('a typed tempo shows its beats as a preview and sounds in the click without being saved', async () => {
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    expect(text(w, '.ba-span')).toBe('0:06,0 – 0:20,5 · 30 pulsos')
    expect(text(w, '.ba-now')).toBe('ahora 120')
    const writes = held.api.setStudy.mock.calls.length
    await setTempo(w, 76)
    // se ve ya: las líneas fantasma son el peine a 76 bpm
    const ghost = w.vm.edit.preview
    expect(ghost.beats.length).toBeLessThan(HIS.beats.length)
    for (const g of spacing(ghost.beats, 10, 20)) expect(g).toBeCloseTo(P76, 2)
    // y suena a los ~200 ms, sin guardarse
    expect(engine()?.beats).not.toEqual(ghost.beats)
    await wait(250)
    expect(engine().beats).toEqual(ghost.beats)
    expect(held.api.setStudy.mock.calls.length, 'no se guarda nada').toBe(writes)
    expect(savedOwn().beats).toEqual(HIS.beats)
    expect(result(w)).toContain('Sin golpes que comparar')

    await buttonOf(w, 'Aplicar').trigger('click')
    await settle()
    expect(w.find('.beat-align').exists(), 'se cierra al aplicar').toBe(false)
    const own = savedOwn()
    expect(own.beats.length).toBe(ghost.beats.length)
    expect(own.beats[10]).toBe(6)
    for (const g of spacing(own.beats, 10, 25)) expect(g).toBeCloseTo(P76, 2)
    // lo de fuera, intacto
    expect(own.beats.slice(0, 10)).toEqual(HIS.beats.slice(0, 10))
    expect(buttonOf(w, 'Deshacer').attributes('title')).toContain('tempo 76 bpm')
    // sigue elegido lo que salió
    expect(text(w, '.bt-summary')).toMatch(/^19 pulsos · 0:06,0 – 0:20,\d · 76 bpm/)
    key('z', { ctrlKey: true })
    await settle()
    expect(savedOwn().beats).toEqual(HIS.beats)
    expect(buttonOf(w, 'Deshacer').attributes('disabled')).toBeDefined()
  })

  it('«Buscar en la onda» finds the tempo and its phase in the hits of the segment', async () => {
    hitsOf([...comb76(5.5, 22, 0.03), ...unrelated(5, 22).filter((_, i) => i % 3 === 0)])
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await buttonOf(w, 'Buscar en la onda').trigger('click')
    await settle()
    const found = parseFloat(
      w.find('.ba-tempo .study-bpm-input input').element.value.replace(',', '.')
    )
    expect(found).toBeGreaterThan(75.8)
    expect(found).toBeLessThan(76.2)
    expect(text(w, '.ba-found')).toMatch(/pegan \d+ de \d+/)
    expect(held.api.beatHits).toHaveBeenCalledWith(7, 'mix')
    // el peine ya cae sobre los golpes: pega, sin pedir ajuste
    expect(result(w)).toMatch(/^Pega: \d+ de \d+ pulsos/)
    await buttonOf(w, 'Aplicar').trigger('click')
    await settle()
    const own = savedOwn()
    // los pulsos nuevos caen a menos de 4 ms de un golpe
    const hits = held.state.hits.mix.hits.map((h) => h[0])
    const mid = own.beats.filter((t) => t > 7 && t < 19)
    const off = mid.map((t) => Math.min(...hits.map((h) => Math.abs(h - t))))
    expect(Math.max(...off)).toBeLessThan(0.02)
  })

  it('a typed tempo and «Solo la fase» slide the whole comb onto the hits, and say how it went', async () => {
    // los golpes caen 30 ms después de donde caería el peine que sale del primer pulso
    hitsOf(comb76(6, 22, 0.03))
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await setTempo(w, 76)
    await fitHow(w, 'phase')
    expect(result(w)).toMatch(
      /^Pega: 19 de 19 pulsos \(100 %\) · el clic iba 30 ms antes → (a tiempo|1 ms (tarde|antes)) · dispersión/
    )
    await buttonOf(w, 'Aplicar').trigger('click')
    await settle()
    const own = savedOwn()
    expect(own.beats[10]).toBeCloseTo(6.03, 2)
    // el tempo sigue exacto: el peine entero se movió, no se torció
    for (const g of spacing(own.beats, 10, 25)) expect(g).toBeCloseTo(P76, 2)
    expect(buttonOf(w, 'Deshacer').attributes('title')).toContain('tempo y onda')
  })

  it('when the audio does not match it keeps the exact tempo and says so; «Forzar el ajuste» does it anyway', async () => {
    hitsOf(unrelated(5, 22))
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await setTempo(w, 76)
    await fitHow(w, 'phase')
    expect(result(w)).toMatch(
      /^No pega con la onda en este tramo \(\d+ de 19 pulsos\): se queda con el tempo exacto, sin ajustar/
    )
    await buttonOf(w, 'Aplicar').trigger('click')
    await settle()
    const own = savedOwn()
    // exactamente el peine de 76 bpm desde el primer pulso: nada se movió
    expect(own.beats[10]).toBe(6)
    own.beats.slice(10, 26).forEach((t, k) => expect(t).toBeCloseTo(6 + k * P76, 3))
    expect(buttonOf(w, 'Deshacer').attributes('title')).toContain('tempo 76 bpm')
  })

  it('without hits in the segment it says so', async () => {
    hitsOf([[100, 3]])
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await fitHow(w, 'phase')
    expect(result(w)).toBe('Sin golpes claros en este tramo')
    expect(buttonOf(w, 'Aplicar').attributes('disabled')).toBeDefined()
  })

  it('says it is listening while the hits arrive, and why if they do not', async () => {
    let arrive
    held.api.beatHits.mockImplementationOnce(() => new Promise((resolve) => (arrive = resolve)))
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await fitHow(w, 'follow')
    expect(result(w)).toBe('Oyendo la canción…')
    expect(w.find('.ba-spin').exists()).toBe(true)
    arrive({ source: 'mix', ref: 3, hits: comb76(5.5, 22) })
    await settle()
    expect(w.find('.ba-spin').exists()).toBe(false)
    expect(result(w)).not.toContain('Oyendo')
    // la batería separada: sin ella no se puede elegir
    expect(buttonOf(w, 'Batería separada').attributes('disabled')).toBeDefined()
    expect(buttonOf(w, 'Batería separada').attributes('title')).toContain('no tiene la batería')
  })

  it('the drums source is asked for when the song has them, and a failure says why', async () => {
    held.state.stems[7] = stemsOf(7)
    held.state.songs.find((s) => s.id === 7).has_stems = true
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    const drums = buttonOf(w, 'Batería separada')
    expect(drums.attributes('disabled')).toBeUndefined()
    await drums.trigger('click')
    await settle()
    expect(drums.attributes('aria-checked')).toBe('true')
    expect(held.api.beatHits).toHaveBeenCalledWith(7, 'drums')
    // (el doble del núcleo no tiene golpes de la batería: falla como el de verdad)
    await fitHow(w, 'phase')
    expect(result(w)).toContain('No se pudo oír la canción')
    expect(result(w)).toContain('no tiene la batería separada')
  })

  describe('every way out leaves the click as it was saved', () => {
    /** Con la prueba sonando en el motor, por una de las salidas. */
    async function trialThen(leave) {
      const w = await mountView()
      await startEditing(w)
      await openPanel(w)
      await setTempo(w, 76)
      await wait(300)
      const trial = engine().beats
      expect(trial, 'la prueba suena').not.toEqual(HIS.beats)
      expect(w.vm.edit.preview, 'y se ve').toBeTruthy()
      await leave(w)
      await settle()
      return { w, trial }
    }
    const restored = (trial) => {
      expect(engine().beats, 'el motor vuelve a lo guardado').toEqual(HIS.beats)
      expect(engine().beats).not.toEqual(trial)
      expect(savedOwn().beats).toEqual(HIS.beats)
      expect(held.api.setStudy.mock.calls.length).toBeGreaterThanOrEqual(0)
    }

    it('Cancelar', async () => {
      const { w, trial } = await trialThen((w) => buttonOf(w, 'Cancelar').trigger('click'))
      restored(trial)
      expect(w.find('.beat-align').exists()).toBe(false)
      expect(w.vm.edit.preview).toBeNull()
    })
    it('the cross', async () => {
      const { trial } = await trialThen((w) => buttonOf(w, 'Cerrar').trigger('click'))
      restored(trial)
    })
    it('Esc', async () => {
      const { w, trial } = await trialThen(() => key('Escape'))
      restored(trial)
      expect(w.find('.beat-align').exists()).toBe(false)
    })
    it('closing the panel with its button', async () => {
      const { trial } = await trialThen((w) => buttonOf(w, 'Tempo y onda…').trigger('click'))
      restored(trial)
    })
    it('leaving «Ajustar pulsos»', async () => {
      const { w, trial } = await trialThen((w) => buttonOf(w, 'Listo').trigger('click'))
      restored(trial)
      expect(w.find('.beat-tools').exists()).toBe(false)
    })
    it('another song', async () => {
      const { w, trial } = await trialThen(async () => {
        held.playback.emit({ track: { id: 8, title: 'Otra', artist: 'Barak', duration: 100 } })
      })
      expect(w.find('.beat-align').exists()).toBe(false)
      expect(w.vm.edit.preview).toBeNull()
      // lo guardado de la 7 no se tocó, y la prueba no se quedó sonando
      expect(JSON.parse(held.state.songs[0].study).metronome.own.beats).toEqual(HIS.beats)
      expect(engine().beats).not.toEqual(trial)
    })
    it('hiding the study (everything keeps playing)', async () => {
      const w = mount(StudyBar, { attachTo: document.body, props: { visible: true } })
      mounted.push(w)
      await settle()
      await startEditing(w)
      await openPanel(w)
      await setTempo(w, 76)
      await wait(300)
      const trial = engine().beats
      expect(trial).not.toEqual(HIS.beats)
      await w.setProps({ visible: false })
      await settle()
      restored(trial)
      expect(w.find('.beat-align').exists()).toBe(false)
    })
    it('the study bar going away', async () => {
      const { w, trial } = await trialThen((w) => w.unmount())
      mounted.splice(mounted.indexOf(w), 1)
      restored(trial)
    })
    it('undoing', async () => {
      const w = await mountView()
      await startEditing(w)
      await drop(w, (b) => (b[3] += 0.03))
      await openPanel(w)
      await setTempo(w, 76)
      await wait(300)
      key('z', { ctrlKey: true })
      await settle()
      expect(engine().beats, 'deshacer manda lo suyo').toEqual(HIS.beats)
    })
  })

  it('«Oír la prueba» off: the preview is drawn but nothing reaches the click', async () => {
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    const before = bridge().setBeatGrid.mock.calls.length
    await w.find('.ba-result input[type="checkbox"]').setValue(false)
    await setTempo(w, 76)
    await wait(400)
    expect(w.vm.edit.preview).toBeTruthy()
    expect(bridge().setBeatGrid.mock.calls.length).toBe(before)
    expect(JSON.parse(localStorage.getItem('danplay.click.hearTrial'))).toBe(false)
  })

  it('the toggles for hits, magnet and the automatic ghost are remembered', async () => {
    const w = await mountView()
    await startEditing(w)
    expect(w.vm.edit.hits).toEqual([])
    held.state.hits = { mix: { hits: comb76(5, 9), ref: 3 } }
    await inTools(w, 'Golpes').trigger('click')
    await settle()
    expect(JSON.parse(localStorage.getItem('danplay.click.showHits'))).toBe(true)
    expect(w.vm.edit.hits.length).toBeGreaterThan(3)
    expect(w.vm.edit.hitsRef).toBe(3)
    await inTools(w, 'Imán').trigger('click')
    await settle()
    expect(JSON.parse(localStorage.getItem('danplay.click.snapHits'))).toBe(true)
    expect(w.vm.edit.ghostBeats).toEqual([])
    await inTools(w, 'Ver el automático').trigger('click')
    await settle()
    expect(JSON.parse(localStorage.getItem('danplay.click.ghostAuto'))).toBe(true)
    expect(w.vm.edit.ghostBeats).toEqual(GRID.beats)
    // pedidos una vez
    expect(held.api.beatHits).toHaveBeenCalledTimes(1)
  })
})

describe('planAlign', () => {
  const own = { beats: HIS.beats, bars: HIS.bars, meter: 4 }
  const hits = (shift) => {
    const out = []
    const p = 60 / 76
    for (let t = 6 + shift, k = 0; t < 21; t += p, k++) out.push([round3(t), 3 + (k % 2)])
    return out
  }
  const base = { tempo: { mode: 'set', bpm: 76 } }

  it('leaves everything alone when asked for nothing', () => {
    const r = planAlign(own, [6, 20.5], {}, hits(0))
    expect(r.changed).toBe(false)
    expect(r.own).toBeNull()
  })

  it('a tempo alone rebuilds the segment and nothing else', () => {
    const r = planAlign(own, [6, 20.5], base, [])
    expect(r.changed).toBe(true)
    expect(r.own.beats.slice(0, 10)).toEqual(HIS.beats.slice(0, 10))
    expect(r.own.beats[10]).toBe(6)
    expect(r.label).toBe('tempo 76 bpm')
    expect(r.setTempo).toBe(true)
    expect(r.refused).toBe('')
  })

  it('«phase» finds the shift and reports the offset before and after', () => {
    const r = planAlign(own, [6, 20.5], { ...base, fit: { how: 'phase' } }, hits(0.025))
    expect(r.refused).toBe('')
    expect(r.label).toBe('tempo y onda')
    expect(r.own.beats[10]).toBeCloseTo(6.025, 2)
    expect(r.after.supported).toBeGreaterThan(0.95)
    expect(Math.abs(r.after.offsetMs)).toBeLessThan(2)
    expect(r.before.supported).toBeLessThan(0.5)
  })

  it('aligning without a tempo only moves beats and is labelled so', () => {
    const exact = {
      beats: HIS.beats.map((t, i) => (i >= 10 && i < 40 ? round3(t + 0.02) : t)),
      bars: HIS.bars,
      meter: 4
    }
    const onBeat = []
    for (let i = 0; i < 120; i++) onBeat.push([HIS.beats[i], 3])
    const r = planAlign(exact, [6, 20.5], { fit: { how: 'phase' } }, onBeat)
    expect(r.label).toBe('alinear a la onda')
    expect(r.own.beats[10]).toBeCloseTo(HIS.beats[10], 2)
    expect(r.setTempo).toBe(false)
  })

  it('refuses when the hits do not fit, keeps the exact tempo, and explains', () => {
    const unrelated = []
    for (let t = 5.9; t < 21; t += 0.37) unrelated.push([round3(t), 3])
    const r = planAlign(own, [6, 20.5], { ...base, fit: { how: 'phase' } }, unrelated)
    expect(r.refused).toBe('no pega')
    expect(r.own.beats[10]).toBe(6)
    r.own.beats.slice(10, 25).forEach((t, k) => expect(t).toBeCloseTo(6 + (k * 60) / 76, 3))
    const said = describePlan(r)
    expect(said.kind).toBe('warn')
    expect(said.text).toMatch(/^No pega con la onda en este tramo \(\d+ de \d+ pulsos\)/)
    // con «Forzar», se ajusta aunque no pegue
    const forced = planAlign(
      own,
      [6, 20.5],
      { ...base, fit: { how: 'phase', force: true } },
      unrelated
    )
    expect(forced.refused).toBe('')
  })

  it('«Buscar» (findTempo) gives the tempo, the phase and how many comb points hit', () => {
    const f = findTempo(hits(0.011), [6, 20.5], 120)
    expect(f.bpm).toBeGreaterThan(75.8)
    expect(f.bpm).toBeLessThan(76.2)
    expect(f.pegan).toBeLessThanOrEqual(f.points)
    expect(f.pegan / f.points).toBeGreaterThan(0.9)
    expect(f.phase).toBeCloseTo(6.011, 1)
    expect(findTempo([], [6, 20.5])).toBeNull()
  })
})

describe('the helpers', () => {
  it('say times and tempos as people read them', () => {
    expect(timeText(65.2)).toBe('1:05,2')
    expect(timeText(0)).toBe('0:00,0')
    expect(bpmText(76.3)).toBe('76,3')
    expect(bpmText(120)).toBe('120')
    expect(lagText(-14)).toBe('14 ms tarde')
    expect(lagText(9)).toBe('9 ms antes')
    expect(lagText(0.4)).toBe('a tiempo')
  })
})
