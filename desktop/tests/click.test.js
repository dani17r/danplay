import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El clic: con «Clic automático», cada canción con el suyo, sin sonar
// mientras carga (con una barra en la onda) y con las siguientes de la cola
// preparadas; a mano, un tempo que sigue igual al pasar de canción. Y todo
// con el panel de estudio abierto o cerrado.
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
import SliderField from '../src/components/ui/SliderField.vue'
import { resetPlayback, usePlayback } from '../src/composables/usePlayback.js'
import { resetClick, useClick, POLL_MS } from '../src/composables/useClick.js'
import { song } from './support/backend.js'
import { useContextMenu } from '../src/composables/useContextMenu.js'

const GRID = {
  bpm: 96,
  meter: 4,
  beats: Array.from({ length: 300 }, (_, i) => 0.5 + i * 0.625),
  first_downbeat: 0,
  phase3: 0,
  phase4: 0,
  confidence: 0.9,
  bars: [0]
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
  pitch_preserved: true,
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
const other = { track: { id: 8, title: 'Otra', artist: 'Barak', duration: 100 } }

beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers()
  localStorage.clear()
  resetPlayback()
  resetClick()
  held.playback.reset()
  held.state.beatGrid = { ...GRID }
  held.state.beatGridPending = []
  held.state.songs = [song(7, { title: 'Mi Gozo', study: '' }), song(8, { title: 'Otra' })]
  held.playback.emit({
    track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
    playing: true,
    position: 12,
    ...base
  })
})

const mounted = []
const mountView = async () => {
  const w = mount(StudyBar, { attachTo: document.body })
  mounted.push(w)
  await flushPromises()
  await flushPromises()
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
  vi.useRealTimers()
})
const settle = async () => {
  await flushPromises()
  await flushPromises()
}

describe('automatic click', () => {
  it('while loading: a bar on the waveform, the button does not allow turning it on, and then it does', async () => {
    held.state.beatGridPending = [0.15, 0.6]
    const w = await mountView()
    const toggle = () => w.find('.study-metro-toggle')
    expect(toggle().attributes('disabled')).toBeDefined()
    expect(toggle().text()).toContain('Cargando… 15 %')
    expect(w.text()).toContain('cargando el clic de esta canción… 15 %')
    expect(w.findComponent({ name: 'StudyTimeline' }).props('clickLoading')).toBe(0.15)
    expect(w.find('.tl-click-load').attributes('aria-valuenow')).toBe('15')
    await vi.advanceTimersByTimeAsync(POLL_MS)
    expect(toggle().text()).toContain('60 %')
    await vi.advanceTimersByTimeAsync(POLL_MS)
    await settle()
    expect(bridge().setBeatGrid).toHaveBeenCalledWith(
      '/musica/mi-gozo.mp3',
      expect.objectContaining({ bpm: 96 })
    )
    expect(bridge().analyzeBeats, 'la de Beat This!, no la de Rust').not.toHaveBeenCalled()
    expect(toggle().attributes('disabled')).toBeUndefined()
    expect(w.find('.tl-click-load').exists()).toBe(false)
    await toggle().trigger('click')
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
  })

  it('if the click was on, it goes quiet on the song still loading and comes back on its own when it arrives', async () => {
    const w = await mountView()
    await w.find('.study-metro-toggle').trigger('click')
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    held.state.beatGridPending = [0.3]
    vi.clearAllMocks()
    held.playback.emit({ ...other, path: '/musica/otra.mp3', position: 0 })
    await settle()
    // el de la anterior no sigue sonando sobre esta: iría a otro tempo
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ on: false }))
    expect(bridge().setMetronome).not.toHaveBeenLastCalledWith(
      expect.objectContaining({ on: true })
    )
    expect(w.find('.study-metro-toggle').attributes('disabled')).toBeDefined()
    expect(w.find('.study-metro-toggle').text()).toContain('Cargando… 30 %')
    await vi.advanceTimersByTimeAsync(POLL_MS)
    await settle()
    expect(bridge().setBeatGrid).toHaveBeenCalledWith('/musica/otra.mp3', expect.anything())
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    expect(w.find('.study-metro-toggle').text()).toContain('Parar')
  })

  it('with the panel closed: the new song brings its click and settings, and the next ones are prepared', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: '' }),
      song(8, { title: 'Otra', study: JSON.stringify({ metronome: { meter: 3 } }) }),
      song(9, { title: 'Tres' }),
      song(10, { title: 'Cuatro' }),
      song(11, { title: 'Cinco' })
    ]
    const player = usePlayback()
    await player.ready()
    await player.setQueue(held.state.songs, 7)
    held.playback.emit({ path: '/musica/mi-gozo.mp3' })
    await settle()
    const click = useClick()
    await click.toggle()
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    expect(held.api.beatGridPrepare.mock.calls.map((c) => c[0])).toEqual([8, 9, 10])
    vi.clearAllMocks()
    held.playback.emit({ ...other, path: '/musica/otra.mp3', index: 1 })
    await settle()
    expect(held.api.song, 'sus ajustes, leídos sin el panel').toHaveBeenCalledWith(8)
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ meter: 3 }))
    expect(bridge().setBeatGrid).toHaveBeenCalledWith('/musica/otra.mp3', expect.anything())
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    expect(held.api.beatGridPrepare.mock.calls.map((c) => c[0])).toEqual([9, 10, 11])
  })

  it('on shuffle nothing is prepared: the next one is unknown', async () => {
    const player = usePlayback()
    await player.ready()
    await player.setQueue(held.state.songs, 7)
    held.playback.emit({ path: '/musica/mi-gozo.mp3', shuffle: true })
    await settle()
    await useClick().toggle()
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    expect(held.api.beatGridPrepare).not.toHaveBeenCalled()
  })

  it('without Beat This! here, the Rust grid right away', async () => {
    held.state.beatGrid = null
    const w = await mountView()
    expect(bridge().analyzeBeats).toHaveBeenCalledWith('/musica/mi-gozo.mp3', null)
    expect(w.find('.study-metro-toggle').attributes('disabled')).toBeUndefined()
    expect(w.find('.tl-click-load').exists()).toBe(false)
  })
})

/** El botón de un modo del clic (Automático, Propio, A mano). */
const mode = (w, name) => w.findAll('.study-mode').find((b) => b.text() === name)

describe('manual click', () => {
  it('the set tempo stays the same when changing song, without waiting for anything', async () => {
    const w = await mountView()
    await w.find('.study-metro-toggle').trigger('click')
    await settle()
    await mode(w, 'A mano').trigger('click')
    await settle()
    expect(mode(w, 'A mano').attributes('aria-checked')).toBe('true')
    expect(mode(w, 'Automático').attributes('aria-checked')).toBe('false')
    expect(w.text()).toContain('a mano: el mismo tempo en todas las canciones')
    // al pasar a mano se queda el que se oía: el clic no cambia de golpe
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(
      expect.objectContaining({ bpm: 96, on: true })
    )
    expect(w.find('.study-mult').exists(), 'sin doble ni mitad: el tempo es el puesto').toBe(false)
    const tempo = w.find('.study-bpm-input input')
    await tempo.trigger('focus')
    await tempo.setValue('140')
    await tempo.trigger('blur')
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 140 }))
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy, 'es de todas las canciones, no de esta').not.toHaveBeenCalled()
    held.state.beatGridPending = [0.2]
    vi.clearAllMocks()
    held.playback.emit({ ...other, path: '/musica/otra.mp3', position: 0 })
    await settle()
    await vi.advanceTimersByTimeAsync(POLL_MS * 3)
    expect(held.api.beatGridNow).not.toHaveBeenCalled()
    for (const [sent] of bridge().setMetronome.mock.calls) {
      expect(sent.on, 'no se calla').not.toBe(false)
      expect(sent.bpm).toBe(140)
    }
    expect(w.find('.study-metro-toggle').attributes('disabled')).toBeUndefined()
    // y se recuerda
    expect(localStorage.getItem('danplay.click.auto')).toBe('false')
    expect(JSON.parse(localStorage.getItem('danplay.click.manual'))).toEqual({ bpm: 140, meter: 4 })
  })

  it('back on automatic, each song with its own again', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { shift: 2 } }) })
    ]
    const w = await mountView()
    await mode(w, 'A mano').trigger('click')
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ shift: 0 }))
    vi.clearAllMocks()
    await mode(w, 'Automático').trigger('click')
    await settle()
    expect(bridge().setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ bpm: null, shift: 2 })
    )
    expect(w.text()).not.toContain('a mano')
  })
})

// «Re-analizar…»: el núcleo recalcula y guarda la rejilla, y tiene que llegar
// al motor y a la onda sin cambiar de canción. Antes se pedía recargar con
// `load()`, que con la misma canción y su rejilla ya puesta no hace nada: el
// aviso decía «arreglado» y seguía sonando el clic de antes.
describe('re-analysing the click', () => {
  const FIXED = { ...GRID, bpm: 120, beats: Array.from({ length: 300 }, (_, i) => 0.4 + i * 0.5) }
  const lastGrid = () => bridge().setBeatGrid.mock.calls.at(-1)?.[1]
  /** «Re-analizar…» y su opción. */
  async function reanalyze(w, label) {
    const { menu } = useContextMenu()
    const button = w.findAll('button').find((b) => b.text().includes('Re-analizar'))
    await button.trigger('click')
    await menu.value.items.find((i) => i.label === label).action()
    await settle()
  }

  it('the new grid reaches the engine and the waveform while the same song plays', async () => {
    held.api.reanalyzeBeats = vi.fn(async () => ({ ...FIXED, grid: FIXED, report: { fixed: 12 } }))
    const w = await mountView()
    expect(lastGrid().bpm).toBe(96)
    await reanalyze(w, 'Arreglar huecos y saltos')
    expect(held.api.reanalyzeBeats).toHaveBeenCalledWith(7, { mode: 'repair' })
    expect(lastGrid().bpm).toBe(120)
    expect(lastGrid().beats[0]).toBe(0.4)
    expect(useClick().grid.value.bpm).toBe(120)
    expect(useClick().loading.value).toBe(false)
  })

  it('without the grid in the answer, it is asked for again', async () => {
    held.api.reanalyzeBeats = vi.fn(async () => {
      held.state.beatGrid = { ...FIXED }
      return { report: { fixed: 3 } }
    })
    const w = await mountView()
    held.api.beatGridNow.mockClear()
    await reanalyze(w, 'Tempo fijo')
    expect(held.api.beatGridNow).toHaveBeenCalledWith(7)
    expect(lastGrid().bpm).toBe(120)
  })

  it('with the click on, it stays on with the new grid', async () => {
    held.api.reanalyzeBeats = vi.fn(async () => ({ ...FIXED, grid: FIXED, report: { fixed: 1 } }))
    const w = await mountView()
    await w.find('.study-metro-toggle').trigger('click')
    await settle()
    bridge().setMetronome.mockClear()
    await reanalyze(w, 'Arreglar huecos y saltos')
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    for (const [sent] of bridge().setMetronome.mock.calls) expect(sent.on).not.toBe(false)
  })

  it('with a hand-adjusted click, that one keeps playing', async () => {
    const own = { beats: Array.from({ length: 40 }, (_, i) => 1 + i * 0.75), bars: [0], meter: 4 }
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { own } }) }),
      song(8, { title: 'Otra' })
    ]
    localStorage.setItem('danplay.click.mode', JSON.stringify('own'))
    resetClick()
    held.api.reanalyzeBeats = vi.fn(async () => ({ ...FIXED, grid: FIXED, report: { fixed: 5 } }))
    const w = await mountView()
    expect(useClick().source.value).toBe('own')
    bridge().setBeatGrid.mockClear()
    await reanalyze(w, 'Arreglar huecos y saltos')
    expect(held.api.reanalyzeBeats).toHaveBeenCalled()
    expect(bridge().setBeatGrid).not.toHaveBeenCalled()
    expect(useClick().source.value).toBe('own')
  })
})

// El volumen del clic es de quien toca, como la cuenta y el sonido: se
// recuerda. Antes volvía al 80 % cada vez que se abría DanPlay.
describe('the click volume', () => {
  it('is remembered, and put back when the study opens', async () => {
    let w = await mountView()
    const slider = w.find('.study-metro-vol').findComponent(SliderField)
    slider.vm.$emit('update:modelValue', 1.7)
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ volume: 1.7 }))
    expect(JSON.parse(localStorage.getItem('danplay.click.volume'))).toBe(1.7)
    w.unmount()
    resetPlayback()
    resetClick()
    held.playback.reset()
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
      playing: true,
      position: 12,
      ...base
    })
    bridge().setMetronome.mockClear()
    w = await mountView()
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ volume: 1.7 }))
  })
})
