import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { effectScope, ref } from 'vue'

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
  useBeatEdit,
  HITS_KEEP,
  planAlign,
  describePlan,
  findTempo,
  NOTHING_EXCESS,
  tempoChoices,
  ratioLabel,
  timeText,
  bpmText,
  lagText
} from '../src/composables/useBeatEdit.js'
import { song, stemsOf } from './support/backend.js'
import sal22 from './fixtures/sal22-hits.json'

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

  it('«Buscar en la onda» searches near the tempo he typed and finds its exact tempo and phase', async () => {
    hitsOf([...comb76(5.5, 22, 0.03), ...unrelated(5, 22).filter((_, i) => i % 3 === 0)])
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    // el clic va a 120 y él escribe ~76: se busca cerca de 76, no cerca de 120
    await setTempo(w, 75)
    await buttonOf(w, 'Buscar en la onda').trigger('click')
    await settle()
    const field = () =>
      parseFloat(w.find('.ba-tempo .study-bpm-input input').element.value.replace(',', '.'))
    expect(field()).toBeGreaterThan(75.8)
    expect(field()).toBeLessThan(76.2)
    expect(text(w, '.ba-found')).toMatch(/pegan \d+ de \d+/)
    expect(held.api.beatHits).toHaveBeenCalledWith(7, 'mix')
    // el doble solo apoya la mitad de sus puntos: no se ofrece
    expect(w.find('.ba-chips').exists()).toBe(false)
    expect(w.find('.ba-suggest').exists()).toBe(false)
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

  it('without a typed tempo it searches near what the segment has: no hits there offers a blind search, and does not touch the field', async () => {
    hitsOf(comb76(5.5, 22, 0.03))
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    const field = w.find('.ba-tempo .study-bpm-input input')
    await buttonOf(w, 'Buscar en la onda').trigger('click')
    await settle()
    // cerca de 120 no hay nada y NO salta al 76 por su cuenta: lo ofrece
    expect(text(w, '.ba-suggest')).toMatch(
      /^Cerca de 120 bpm no hay golpes claros\. Buscando sin pista: 7[56](,\d)? bpm \(pegan \d+ %\)/
    )
    expect(field.element.value, 'el campo sigue como estaba').toBe('')
    expect(buttonOf(w, 'Poner un tempo').attributes('aria-checked')).toBe('false')
    await buttonOf(w, 'Usar este').trigger('click')
    await settle()
    expect(buttonOf(w, 'Poner un tempo').attributes('aria-checked')).toBe('true')
    const typed = parseFloat(field.element.value.replace(',', '.'))
    expect(typed).toBeGreaterThan(75.8)
    expect(typed).toBeLessThan(76.2)
    expect(w.find('.ba-suggest').exists()).toBe(false)
    expect(text(w, '.ba-found')).toMatch(/pegan \d+ de \d+/)
    // y con su fase: el peine cae sobre los golpes
    await buttonOf(w, 'Aplicar').trigger('click')
    await settle()
    // el primer pulso nuevo cae sobre un golpe (el primero desde el 6,0 está en 6,319)
    const first = savedOwn().beats.find((t) => t >= 6)
    expect(Math.abs(first - 6.319)).toBeLessThan(0.01)
  })

  it('when the blind search finds nothing either it says there are no clear hits', async () => {
    hitsOf([
      [6.2, 3],
      [9.9, 3]
    ])
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await setTempo(w, 100)
    await buttonOf(w, 'Buscar en la onda').trigger('click')
    await settle()
    expect(text(w, '.ba-found')).toBe('Sin golpes claros en este tramo')
    expect(w.find('.ba-suggest').exists()).toBe(false)
  })

  it('dense hits with no tempo in them are «nothing there», however much a comb of them fits by chance', async () => {
    // 12 golpes por segundo al azar: cualquier peine cae sobre la mitad de los pulsos
    // (la semilla es una de las que, además, no dan nada buscando sin pista: en un tramo de
    // 14 s, el mejor de cientos de tempos sale a veces por encima del azar sin ser nada)
    let seed = 11
    const rand = () => (seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32
    const dense = Array.from({ length: 12 * 20 }, () => [round3(4 + rand() * 20), 3 + rand() * 2])
    dense.sort((a, b) => a[0] - b[0])
    hitsOf(dense)
    const near = findTempo(held.state.hits.mix.hits, [6, 20.5], 100)
    expect(near.support, 'a comb at any tempo fits about half').toBeGreaterThan(0.4)
    expect(near.excess).toBeLessThan(NOTHING_EXCESS)
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await setTempo(w, 100)
    await buttonOf(w, 'Buscar en la onda').trigger('click')
    await settle()
    expect(text(w, '.ba-found')).toBe('Sin golpes claros en este tramo')
    expect(w.find('.ba-suggest').exists(), 'no blind search is offered either').toBe(false)
    expect(w.find('.ba-chips').exists()).toBe(false)
    // el campo sigue como lo escribió
    expect(w.find('.ba-tempo .study-bpm-input input').element.value).toBe('100')
  })

  it('a hand-typed tempo with no hits near it (but some far away) is not hidden: the blind search is offered with its support', async () => {
    // golpes a 120 bpm; escribe 100
    const at120 = []
    for (let t = 6.03, k = 0; t < 21; t += 0.5, k++) at120.push([round3(t), k % 4 === 0 ? 4 : 3])
    hitsOf(at120)
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await setTempo(w, 100)
    await buttonOf(w, 'Buscar en la onda').trigger('click')
    await settle()
    expect(text(w, '.ba-suggest')).toContain('Cerca de 100 bpm no hay golpes claros')
    expect(text(w, '.ba-suggest')).toContain('Buscando sin pista: 120 bpm (pegan 100 %)')
    await buttonOf(w, 'Usar este').trigger('click')
    await settle()
    // el 60 pega tanto como el 120 (es un subconjunto): se ofrece, sin elegirlo
    const chip = w.find('.ba-chip')
    expect(chip.text()).toMatch(/^÷2 · 60 bpm · pegan 100 %$/)
  })

  describe('on the real hits of a live song (Sal 22 Te Amo, 120 bpm with a half-time section)', () => {
    const unflat = (flat) =>
      Array.from({ length: flat.length / 2 }, (_, i) => [flat[2 * i], flat[2 * i + 1]])
    const realOwn = () => ({
      beats: sal22.beats,
      bars: sal22.beats.map((_, i) => i).filter((i) => i % 4 === 0),
      meter: 4,
      touched: []
    })
    /** Los pulsos de 100 a 160 s. */
    const inSection = sal22.beats.map((t, i) => [t, i]).filter(([t]) => t >= 100 && t <= 160)
    const SECTION = inSection.map(([, i]) => i)
    async function realPanel() {
      held.state.hits = { mix: { hits: unflat(sal22.mix), ref: 3 } }
      withOwn(realOwn())
      const w = await mountView()
      await startEditing(w)
      await openPanel(w, SECTION)
      return w
    }

    it('searching near 120 finds ~120 and offers the half (÷2) as a chip, without picking it', async () => {
      const w = await realPanel()
      await buttonOf(w, 'Poner un tempo').trigger('click')
      await buttonOf(w, 'Buscar en la onda').trigger('click')
      await settle()
      const field = () =>
        parseFloat(w.find('.ba-tempo .study-bpm-input input').element.value.replace(',', '.'))
      expect(field()).toBeGreaterThan(119)
      expect(field()).toBeLessThan(121)
      const chips = w.findAll('.ba-chip').map((c) => c.text())
      expect(chips).toHaveLength(1)
      expect(chips[0]).toMatch(/^÷2 · 60 bpm · pegan \d+ %$/)
      // el 240 no apoya casi nada: no se ofrece
      expect(chips.join()).not.toContain('×2')
      expect(w.find('.ba-suggest').exists()).toBe(false)
    })

    it('clicking the chip puts that tempo and its phase in the field; the ×2 button keeps multiplying the field', async () => {
      const w = await realPanel()
      await buttonOf(w, 'Poner un tempo').trigger('click')
      await buttonOf(w, 'Buscar en la onda').trigger('click')
      await settle()
      const span = [sal22.beats[SECTION[0]], sal22.beats[SECTION.at(-1)]]
      const expected = findTempo(unflat(sal22.mix), span, 120).alternatives.find(
        (a) => a.ratio === 0.5
      )
      await w.find('.ba-chip').trigger('click')
      await settle()
      const field = () =>
        parseFloat(w.find('.ba-tempo .study-bpm-input input').element.value.replace(',', '.'))
      expect(field()).toBeCloseTo(Math.round(expected.bpm * 10) / 10, 0)
      // lo puesto es lo que salió: sus puntos del peine, y ahora el 120 se ofrece (×2)
      expect(text(w, '.ba-found')).toMatch(/^60 bpm · pegan \d+ de \d+/)
      expect(w.find('.ba-chip').text()).toMatch(/^×2 · 120(,\d)? bpm · pegan \d+ %$/)
      // y con la fase del chip: el primer pulso nuevo cae sobre ella
      await buttonOf(w, 'Aplicar').trigger('click')
      await settle()
      const first = savedOwn().beats.find((t) => t >= span[0] - 0.001)
      const k = Math.round((first - expected.phase) / (60 / expected.bpm))
      expect(Math.abs(first - (expected.phase + k * (60 / expected.bpm)))).toBeLessThan(0.006)
    })

    it('«×2» still multiplies what is in the field', async () => {
      const w = await realPanel()
      await buttonOf(w, 'Poner un tempo').trigger('click')
      await buttonOf(w, 'Buscar en la onda').trigger('click')
      await settle()
      await w.find('.ba-chip').trigger('click')
      await settle()
      await buttonOf(w, '×2').trigger('click')
      await settle()
      const v = parseFloat(
        w.find('.ba-tempo .study-bpm-input input').element.value.replace(',', '.')
      )
      expect(v).toBeGreaterThan(119)
      expect(v).toBeLessThan(121)
    })

    it('«Tempo fijo» over a click that already fits is refused as «Empeoraría», with the numbers, and can be forced', async () => {
      const w = await realPanel()
      await buttonOf(w, 'Tempo fijo').trigger('click')
      await settle()
      expect(result(w)).toMatch(
        /^Empeoraría: pegan \d+ de \d+ pulsos y pasarían a \d+, y al azar pegarían unos \d+; se queda como está \(puedes forzar el ajuste\)$/
      )
      // en música de verdad el azar es la mitad de los pulsos: se dice
      const [, chanceCount] = result(w).match(/al azar pegarían unos (\d+)/)
      expect(Number(chanceCount)).toBeGreaterThan(40)
      expect(buttonOf(w, 'Aplicar').attributes('disabled')).toBeDefined()
      // con «Forzar el ajuste» se hace igual
      await w.find('.ba-fit .check-field input').setValue(true)
      await settle()
      expect(result(w)).toMatch(/^Pega: /)
      expect(buttonOf(w, 'Aplicar').attributes('disabled')).toBeUndefined()
    })
  })

  it('«Sigue la onda» does not show a spread (every beat sits on its hit): «Solo la fase» does', async () => {
    // un clic que cae 30 ms antes de cada golpe, y un poco desigual
    hitsOf(comb76(6, 22, 0.03))
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await setTempo(w, 76)
    await fitHow(w, 'phase')
    expect(result(w)).toContain('dispersión')
    await fitHow(w, 'follow')
    expect(result(w)).toMatch(/^Pega: /)
    expect(result(w)).not.toContain('dispersión')
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
      /^Pega: 19 de 19 pulsos \(al azar pegarían unos \d+\) · el clic iba 30 ms antes → (a tiempo|1 ms (tarde|antes)) · dispersión/
    )
    await buttonOf(w, 'Aplicar').trigger('click')
    await settle()
    const own = savedOwn()
    expect(own.beats[10]).toBeCloseTo(6.03, 2)
    // el tempo sigue exacto: el peine entero se movió, no se torció
    for (const g of spacing(own.beats, 10, 25)) expect(g).toBeCloseTo(P76, 2)
    expect(buttonOf(w, 'Deshacer').attributes('title')).toContain('tempo y onda')
  })

  it('when the audio does not match it keeps the exact tempo and says so', async () => {
    // golpes al mismo tempo pero a contrapié (media vuelta de fase): ningún corrimiento de ±50 ms los alcanza
    hitsOf(comb76(6, 22, P76 / 2))
    const w = await mountView()
    await startEditing(w)
    await openPanel(w)
    await setTempo(w, 76)
    await fitHow(w, 'phase')
    expect(result(w)).toMatch(
      /^No pega con la onda en este tramo: 0 de 19 pulsos, y al azar pegarían unos \d+; se queda con el tempo exacto, sin ajustar/
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

describe('the hits already asked for', () => {
  it('are kept for the last few songs, not for every song edited since the app opened', async () => {
    const songId = ref(1)
    const scope = effectScope()
    const edit = scope.run(() =>
      useBeatEdit({
        study: ref({ metronome: {} }),
        editing: ref(false),
        click: {},
        player: {},
        songId,
        duration: 200,
        grid: null
      })
    )
    try {
      for (let id = 1; id <= HITS_KEEP + 1; id++) {
        songId.value = id
        await edit.loadHits('mix')
      }
      held.api.beatHits.mockClear()
      // la última, guardada: no se vuelve a pedir
      await edit.loadHits('mix')
      expect(held.api.beatHits).not.toHaveBeenCalled()
      // la primera ya se tiró
      songId.value = 1
      await edit.loadHits('mix')
      expect(held.api.beatHits).toHaveBeenCalledTimes(1)
      expect(held.api.beatHits).toHaveBeenCalledWith(1, 'mix')
    } finally {
      scope.stop()
    }
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
    expect(said.text).toMatch(
      /^No pega con la onda en este tramo: \d+ de \d+ pulsos, y al azar pegarían unos \d+; se queda con el tempo exacto, sin ajustar$/
    )
    // con «Forzar», se ajusta aunque no pegue
    const forced = planAlign(
      own,
      [6, 20.5],
      { ...base, fit: { how: 'phase', force: true } },
      unrelated
    )
    expect(forced.refused).toBe('')
  })

  it('«Buscar» (findTempo) near the tempo gives the tempo, the phase and how many comb points hit', () => {
    const f = findTempo(hits(0.011), [6, 20.5], 76)
    expect(f.bpm).toBeGreaterThan(75.8)
    expect(f.bpm).toBeLessThan(76.2)
    expect(f.pegan).toBeLessThanOrEqual(f.points)
    expect(f.pegan / f.points).toBeGreaterThan(0.9)
    expect(f.phase).toBeCloseTo(6.011, 1)
    expect(findTempo([], [6, 20.5])).toBeNull()
  })

  it('«Buscar» never jumps octave: near 120 it does not answer 76', () => {
    const near = findTempo(hits(0.011), [6, 20.5], 120)
    expect(Math.abs(near.bpm - 76)).toBeGreaterThan(5)
    expect(near.support).toBeLessThan(0.5)
    // lo que pega sobre el azar, que es lo que decide si hay algo: nada
    expect(near.excess).toBeLessThan(NOTHING_EXCESS)
    // sin pista sí lo encuentra, y su doble (que solo apoya la mitad) no se elige
    const blind = findTempo(hits(0.011), [6, 20.5])
    expect(blind.bpm).toBeGreaterThan(75.8)
    expect(blind.bpm).toBeLessThan(76.2)
  })

  it('the alternatives carry how many comb points hit, and only the ones that hit almost as much are offered', () => {
    const at120 = []
    for (let t = 6.03, k = 0; t < 21; t += 0.5, k++) at120.push([round3(t), k % 4 === 0 ? 4 : 3])
    const f = findTempo(at120, [6, 20.5], 120)
    const half = f.alternatives.find((a) => a.ratio === 0.5)
    expect(half.bpm).toBeGreaterThan(59.5)
    expect(half.bpm).toBeLessThan(60.5)
    expect(half.pegan).toBeLessThanOrEqual(half.points)
    expect(half.points).toBeLessThan(f.points)
    // el 60 es un subconjunto del 120: pega igual y se ofrece; el 240 solo apoya la mitad y no
    const offered = tempoChoices(f)
    expect(offered[0]).toBe(f)
    expect(offered.map((c) => Math.round(c.bpm))).toEqual([120, 60])
    expect(ratioLabel(f.bpm, half.bpm)).toBe('÷2')
    expect(ratioLabel(half.bpm, f.bpm)).toBe('×2')
    // desde una alternativa, la otra no es «×2» a secas
    expect(ratioLabel(60, 240)).toBe('×4')
    expect(ratioLabel(240, 60)).toBe('÷4')
    expect(ratioLabel(100, 150)).toBe('×1.5')
    expect(tempoChoices(null)).toEqual([])
  })

  it('«empeora», «no cabe» and the spread have their own words, and say what chance would give', () => {
    const fit = (supported, spreadMs = 0, offsetMs = 0, chance = 0) => ({
      supported,
      spreadMs,
      offsetMs,
      chance
    })
    const base = {
      how: 'line',
      setTempo: false,
      n: 40,
      spreadReliable: true,
      before: fit(0.93, 0, 0, 0.45),
      after: fit(0.93, 0, 0, 0.45),
      candidate: fit(0.72, 0, 0, 0.58)
    }
    expect(describePlan({ ...base, refused: 'empeora' }).text).toBe(
      'Empeoraría: pegan 37 de 40 pulsos y pasarían a 29, y al azar pegarían unos 23; se queda como está (puedes forzar el ajuste)'
    )
    expect(describePlan({ ...base, setTempo: true, refused: 'empeora' }).text).toBe(
      'Empeoraría: pegan 37 de 40 pulsos y pasarían a 29, y al azar pegarían unos 23; se queda con el tempo exacto, sin ajustar (puedes forzar el ajuste)'
    )
    expect(
      describePlan({ ...base, refused: 'no pega', candidate: fit(0.275, 0, 0, 0.225) }).text
    ).toBe(
      'No pega con la onda en este tramo: 11 de 40 pulsos, y al azar pegarían unos 9; se queda como está, sin ajustar'
    )
    expect(
      describePlan({
        ...base,
        setTempo: true,
        refused: 'no pega',
        candidate: fit(0.275, 0, 0, 0.225)
      }).text
    ).toBe(
      'No pega con la onda en este tramo: 11 de 40 pulsos, y al azar pegarían unos 9; se queda con el tempo exacto, sin ajustar'
    )
    expect(describePlan({ ...base, refused: 'no cabe' }).text).toBe(
      'No cabe entre los pulsos de los lados'
    )
    const ok = { ...base, refused: '', before: fit(0.5, 9, -14), after: fit(0.775, 3, -1, 0.525) }
    expect(describePlan(ok).text).toContain('Pega: 31 de 40 pulsos (al azar pegarían unos 21) · ')
    expect(describePlan(ok).text).toContain('dispersión 9 → 3 ms')
    expect(describePlan({ ...ok, spreadReliable: false }).text).not.toContain('dispersión')
  })

  it('the chips only offer what fits above chance as much as the main one, not what has more raw support', () => {
    const found = {
      bpm: 120,
      support: 0.9,
      excess: 0.9,
      alternatives: [
        // el 240 de las corcheas: apoya casi todo, pero sobre el azar es mucho menos
        { bpm: 240, ratio: 2, support: 0.97, excess: 0.5 },
        { bpm: 60, ratio: 0.5, support: 0.7, excess: 0.8 }
      ]
    }
    expect(tempoChoices(found).map((c) => c.bpm)).toEqual([120, 60])
    // justo en el 85 % entra; por debajo no
    const edge = { ...found, alternatives: [{ bpm: 60, ratio: 0.5, excess: 0.765 }] }
    expect(tempoChoices(edge)).toHaveLength(2)
    expect(
      tempoChoices({ ...found, alternatives: [{ bpm: 60, ratio: 0.5, excess: 0.76 }] })
    ).toHaveLength(1)
  })

  it('planAlign carries `spreadReliable` from the alignment: false with follow, true otherwise', () => {
    const truth = []
    for (let t = 6, k = 0; k < 29; t += 60 / 76, k++) truth.push([round3(t + 0.02), 4])
    const wrong = { beats: HIS.beats, bars: HIS.bars, meter: 4 }
    const follow = planAlign(
      wrong,
      [6, 20.5],
      { tempo: { mode: 'set', bpm: 76 }, fit: { how: 'follow' } },
      truth
    )
    expect(follow.spreadReliable).toBe(false)
    const phase = planAlign(
      wrong,
      [6, 20.5],
      { tempo: { mode: 'set', bpm: 76 }, fit: { how: 'phase' } },
      truth
    )
    expect(phase.spreadReliable).toBe(true)
    expect(
      planAlign(wrong, [6, 20.5], { tempo: { mode: 'set', bpm: 76 } }, truth).spreadReliable
    ).toBe(true)
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
