import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El clic: con «Clic automático», cada canción con el suyo, sin sonar
// mientras carga (con una barra en la onda) y con las siguientes de la cola
// preparadas; con tempo fijo, uno que sigue igual al pasar de canción. Y todo
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
import StudyTimeline from '../src/components/StudyTimeline.vue'
import { resetPlayback, usePlayback } from '../src/composables/usePlayback.js'
import {
  resetClick,
  useClick,
  POLL_MS,
  LOAD_LIMIT_MS,
  ANSWER_MS,
  AHEAD
} from '../src/composables/useClick.js'
import { ApiError } from '../src/api.js'
import { song } from './support/backend.js'
import { useContextMenu } from '../src/composables/useContextMenu.js'
import { useNotices } from '../src/composables/useNotices.js'

// lo que contesta el doble del núcleo al re-análisis: las pruebas de antes lo
// cambian a su gusto y no lo devuelven
const answerReanalysis = held.api.reanalyzeBeats

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
    // toda la lista de una vez: la siguiente, y con ella las demás
    expect(held.api.beatGridPrepare.mock.calls).toEqual([[8, [9, 10, 11]]])
    vi.clearAllMocks()
    held.playback.emit({ ...other, path: '/musica/otra.mp3', index: 1 })
    await settle()
    expect(held.api.song, 'sus ajustes, leídos sin el panel').toHaveBeenCalledWith(8)
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ meter: 3 }))
    expect(bridge().setBeatGrid).toHaveBeenCalledWith('/musica/otra.mp3', expect.anything())
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    // ya estaban pedidas: no se pregunta otra vez por ellas
    expect(held.api.beatGridPrepare).not.toHaveBeenCalled()
  })

  it('a whole setlist is left prepared, and what could not be is asked for again', async () => {
    // se preparaban las 3 siguientes: a la cuarta canción del ensayo había
    // que esperar otra vez al clic
    held.state.songs = Array.from({ length: 30 }, (_, i) => song(7 + i, { title: 'Canción ' + i }))
    held.api.beatGridPrepare.mockImplementation(async () => ({ queued: true, left: [20] }))
    const player = usePlayback()
    await player.ready()
    await player.setQueue(held.state.songs, 7)
    held.playback.emit({ path: '/musica/mi-gozo.mp3' })
    await settle()
    await useClick().toggle()
    await settle()
    const rest = Array.from({ length: 28 }, (_, i) => 9 + i)
    expect(held.api.beatGridPrepare.mock.calls).toEqual([[8, rest]])
    // en la siguiente canción, solo la que el núcleo no pudo dejar
    held.api.beatGridPrepare.mockClear()
    held.playback.emit({ ...other, path: '/musica/otra.mp3', index: 1 })
    await settle()
    expect(held.api.beatGridPrepare.mock.calls).toEqual([[20, []]])
  })

  it('of a very long list, the next ones: not the whole library', async () => {
    held.state.songs = Array.from({ length: 200 }, (_, i) => song(7 + i, { title: 'Canción ' + i }))
    const player = usePlayback()
    await player.ready()
    await player.setQueue(held.state.songs, 7)
    held.playback.emit({ path: '/musica/mi-gozo.mp3' })
    await settle()
    await useClick().toggle()
    await settle()
    const [first, others] = held.api.beatGridPrepare.mock.calls[0]
    expect([first, ...others]).toEqual(Array.from({ length: AHEAD }, (_, i) => 8 + i))
  })

  it('on shuffle a short list is prepared whole, and of a long one nothing: the next is unknown', async () => {
    const player = usePlayback()
    await player.ready()
    await player.setQueue(held.state.songs, 7)
    held.playback.emit({ path: '/musica/mi-gozo.mp3', shuffle: true })
    await settle()
    await useClick().toggle()
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
    expect(held.api.beatGridPrepare.mock.calls).toEqual([[8, []]])
    // una lista larga: no se pasa la red por canciones al azar
    resetClick()
    held.api.beatGridPrepare.mockClear()
    held.state.songs = Array.from({ length: 200 }, (_, i) => song(7 + i, { title: 'Canción ' + i }))
    await player.setQueue(held.state.songs, 7)
    held.playback.emit({ path: '/musica/mi-gozo.mp3', shuffle: true })
    await settle()
    await useClick().toggle()
    await settle()
    expect(held.api.beatGridPrepare).not.toHaveBeenCalled()
  })

  it('without Beat This! here, the Rust grid right away', async () => {
    held.state.beatGrid = null
    const w = await mountView()
    expect(bridge().analyzeBeats).toHaveBeenCalledWith('/musica/mi-gozo.mp3', null)
    expect(w.find('.study-metro-toggle').attributes('disabled')).toBeUndefined()
    expect(w.find('.tl-click-load').exists()).toBe(false)
    expect(useClick().fallbackWhy.value, 'no hay detector: no es un fallo').toBe('')
  })

  it('a detector that could not with this song: the approximate click, and why is kept', async () => {
    // antes no se decía: la canción se quedaba con el clic de un solo tempo
    // y parecía cosa del clic
    held.api.beatGridNow.mockRejectedValueOnce(
      new ApiError('el detector se cerró sin terminar (137)', 422)
    )
    await mountView()
    const click = useClick()
    expect(bridge().analyzeBeats).toHaveBeenCalledWith('/musica/mi-gozo.mp3', null)
    expect(click.source.value).toBe('rust')
    expect(click.fallbackWhy.value).toBe('el detector se cerró sin terminar (137)')
    // la siguiente, con su clic de verdad, ya no arrastra el motivo
    held.playback.emit({ ...other, playing: true, position: 0, ...base, path: '/musica/otra.mp3' })
    await settle()
    expect(click.source.value).toBe('beat-this')
    expect(click.fallbackWhy.value).toBe('')
  })

  it('a core that never answers does not leave it loading for ever', async () => {
    const usual = held.api.beatGridNow.getMockImplementation()
    held.api.beatGridNow.mockImplementation(() => new Promise(() => {}))
    try {
      const w = await mountView()
      const click = useClick()
      expect(click.loading.value).toBe(true)
      // una respuesta que no llega se vuelve a pedir, no se espera sin fin
      await vi.advanceTimersByTimeAsync(ANSWER_MS + POLL_MS)
      expect(held.api.beatGridNow.mock.calls.length).toBeGreaterThan(1)
      expect(click.loading.value).toBe(true)
      await vi.advanceTimersByTimeAsync(LOAD_LIMIT_MS + ANSWER_MS)
      await settle()
      expect(click.loading.value).toBe(false)
      expect(click.fallbackWhy.value).toBe('tarda demasiado')
      expect(bridge().analyzeBeats).toHaveBeenCalledWith('/musica/mi-gozo.mp3', null)
      expect(w.find('.study-metro-toggle').attributes('disabled')).toBeUndefined()
    } finally {
      held.api.beatGridNow.mockImplementation(usual)
    }
  })
})

/** El botón de un modo del clic (Automático, Manual, Tempo fijo). */
const mode = (w, name) => w.findAll('.study-mode').find((b) => b.text() === name)

describe('manual click', () => {
  it('the set tempo stays the same when changing song, without waiting for anything', async () => {
    const w = await mountView()
    await w.find('.study-metro-toggle').trigger('click')
    await settle()
    await mode(w, 'Tempo fijo').trigger('click')
    await settle()
    expect(mode(w, 'Tempo fijo').attributes('aria-checked')).toBe('true')
    expect(mode(w, 'Automático').attributes('aria-checked')).toBe('false')
    expect(w.text()).toContain('tempo fijo: el mismo en todas las canciones')
    // al pasar a tempo fijo se queda el que se oía: el clic no cambia de golpe
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
    await mode(w, 'Tempo fijo').trigger('click')
    await settle()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ shift: 0 }))
    vi.clearAllMocks()
    await mode(w, 'Automático').trigger('click')
    await settle()
    expect(bridge().setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ bpm: null, shift: 2 })
    )
    expect(w.text()).not.toContain('tempo fijo')
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

  it('«Solo con la canción» undoes what the separated drums added', async () => {
    // al separar, DanPlay rellena solo los huecos sin clic con la batería: el
    // menú tiene con qué quitarlo (modo «mix», sin esperar a la red)
    held.api.reanalyzeBeats = vi.fn(async () => ({ ...FIXED, grid: FIXED, report: { fixed: 6 } }))
    const w = await mountView()
    await reanalyze(w, 'Solo con la canción')
    expect(held.api.reanalyzeBeats).toHaveBeenCalledWith(7, { mode: 'mix' })
    expect(lastGrid().bpm).toBe(120)
  })

  it('without the grid in the answer, it is asked for again', async () => {
    held.api.reanalyzeBeats = vi.fn(async () => {
      held.state.beatGrid = { ...FIXED }
      return { report: { fixed: 3 } }
    })
    const w = await mountView()
    held.api.beatGridNow.mockClear()
    await reanalyze(w, 'Tempo constante')
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

  it('with the separated drums it does not wait inside the request: it asks, and shows how far', async () => {
    // la red se pasaba por la batería dentro de la petición: 15-20 s sin
    // avance a la vista
    const answers = [
      { pending: true, progress: 0 },
      { pending: true, progress: 0.5 },
      { ...FIXED, grid: FIXED, report: { fixed: 4 } }
    ]
    const shown = []
    held.api.reanalyzeBeats = vi.fn(async () => {
      shown.push(useClick().reanalyzing.value)
      return answers.shift()
    })
    const w = await mountView()
    vi.useFakeTimers()
    try {
      const { menu } = useContextMenu()
      const button = w.findAll('button').find((b) => b.text().includes('Re-analizar'))
      await button.trigger('click')
      const done = menu.value.items.find((i) => i.label === 'Con la batería separada').action()
      await vi.advanceTimersByTimeAsync(0)
      expect(w.text()).toContain('oyendo la batería separada… 0 %')
      await vi.advanceTimersByTimeAsync(POLL_MS)
      expect(w.text()).toContain('oyendo la batería separada… 50 %')
      expect(w.find('.tl-click-load').exists()).toBe(true)
      await vi.advanceTimersByTimeAsync(POLL_MS)
      await done
    } finally {
      vi.useRealTimers()
    }
    await settle()
    expect(held.api.reanalyzeBeats).toHaveBeenCalledTimes(3)
    expect(held.api.reanalyzeBeats).toHaveBeenCalledWith(7, { mode: 'drums', wait: false })
    expect(shown).toEqual([null, 0, 0.5])
    expect(lastGrid().bpm).toBe(120)
    expect(useClick().reanalyzing.value).toBe(null)
    expect(w.text()).not.toContain('oyendo la batería')
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
    const w = await mountView()
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
    await mountView()
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ volume: 1.7 }))
  })
})

// Al pasar de canción, Rust anuncia la nueva (`track`, de la cola) un
// instante antes de que su ruta sea la del motor (`path`, del audio). Lo que
// el estudio mandaba en ese instante iba con la ruta de la anterior y el
// motor lo descartaba: la siguiente sonaba sin sus guías.
describe('changing song with the study open', () => {
  it('the cues of the new song are sent again when its path arrives', async () => {
    const cues = [{ t: 12, file: '/guias/coro.wav', text: 'Coro' }]
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: '' }),
      song(8, { title: 'Otra', study: JSON.stringify({ cues }) })
    ]
    await mountView()
    bridge().setCues.mockClear()
    // la cola ya dice la 8, y el audio aún la ruta de la 7
    held.playback.emit({ ...other, path: '/musica/mi-gozo.mp3' })
    await settle()
    const early = bridge().setCues.mock.calls.filter(([, list]) => list.length)
    for (const [path] of early) expect(path).toBe('/musica/mi-gozo.mp3')
    // llega su ruta: ahora sí van a su canción
    held.playback.emit({ ...other, path: '/musica/otra.mp3' })
    await settle()
    const last = bridge().setCues.mock.calls.at(-1)
    expect(last[0]).toBe('/musica/otra.mp3')
    expect(last[1]).toEqual([expect.objectContaining({ at: 12, path: '/guias/coro.wav' })])
  })
})

// Ajustando pulsos, la onda es de los pulsos: el doble clic que añade uno
// son también dos clics, y cada uno quitaba el tramo que hubiera.
describe('adjusting the beats on the waveform', () => {
  it('a click does not remove the segment', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100] }) }),
      song(8, { title: 'Otra' })
    ]
    const w = await mountView()
    const box = w.find('.tl-box').element
    box.getBoundingClientRect = () => ({
      left: 0,
      width: 400,
      right: 400,
      top: 0,
      height: 56,
      bottom: 56,
      x: 0,
      y: 0
    })
    const pointer = (target, type, x) => {
      target.dispatchEvent(
        new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: 20 })
      )
      return settle()
    }
    expect(w.find('.tl-loop').exists()).toBe(true)
    await w.find('.study-edit-beats').trigger('click')
    await settle()
    expect(w.find('.study-edit-beats').text()).toContain('Listo')
    // un clic suelto fuera del tramo, como los de un doble clic
    await pointer(box, 'pointerdown', 300)
    await pointer(window, 'pointerup', 300)
    expect(w.find('.tl-loop').exists(), 'el tramo sigue').toBe(true)
    // y fuera de «Ajustar pulsos», el mismo clic sí lo quita
    await w.find('.study-edit-beats').trigger('click')
    await settle()
    await pointer(box, 'pointerdown', 300)
    await pointer(window, 'pointerup', 300)
    expect(w.find('.tl-loop').exists()).toBe(false)
  })
})

// «Salir del modo estudio» quita el clic. Se callaba solo en Rust y seguía
// «pedido»: en automático volvía él solo en la canción siguiente.
describe('leaving the study', () => {
  it('turns the click off for good: the next song does not bring it back', async () => {
    const w = await mountView()
    await w.find('.study-metro-toggle').trigger('click')
    await settle()
    expect(useClick().wanted.value).toBe(true)
    const { menu } = useContextMenu()
    await w.find('.study-more').trigger('click')
    await menu.value.items.find((i) => i.label === 'Salir del modo estudio').action()
    await settle()
    expect(useClick().wanted.value).toBe(false)
    bridge().setMetronome.mockClear()
    held.playback.emit({ ...other, path: '/musica/otra.mp3' })
    await settle()
    await vi.advanceTimersByTimeAsync(POLL_MS * 2)
    for (const [sent] of bridge().setMetronome.mock.calls) expect(sent.on).not.toBe(true)
  })
})

// El tempo que se enseña es el que se oye. El motor lleva los golpes del
// clic en tiempo de canción, también con el tempo a mano: a 0,8×, un 100
// suena a 80. Se enseñaba 100, y «+» guardaba un tempo que no era.
describe('the tempo shown at another speed', () => {
  const metro = (over) => ({ ...base.metronome, has_grid: true, ...over })
  const shown = (w) => w.find('.study-bpm').find('input').element.value

  it('a hand-set tempo is shown as it sounds, and «+» adds to what sounds', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ speed: 0.8, metronome: { bpm: 100 } }) })
    ]
    held.playback.emit({ speed: 0.8, metronome: metro({ bpm: 100, free: true }) })
    const w = await mountView()
    held.playback.emit({ speed: 0.8, metronome: metro({ bpm: 100, free: true }) })
    await settle()
    expect(String(shown(w))).toContain('80')
    bridge().setMetronome.mockClear()
    await w.findAll('.study-bpm-step').at(-1).trigger('click')
    await settle()
    // 81 oídos a 0,8× son 101,25 de canción
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 101.25 }))
  })
})

// Ajustar pulsos a mano con ×2 o «el 1 es el siguiente» puestos: lo ajustado
// ya los lleva, y a Rust hay que quitárselos (los aplicaba otra vez encima).
describe('adjusting beats with ×2 on', () => {
  it('the double and the shifted «1» are taken off in the engine too', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { mult: 1, shift: 1 } }) })
    ]
    const w = await mountView()
    // de entrada, los de la canción
    expect(bridge().setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ mult: 1, shift: 1 })
    )
    bridge().setMetronome.mockClear()
    const timeline = w.findComponent(StudyTimeline)
    timeline.vm.$emit('update:grid', {
      ...GRID,
      beats: GRID.beats.slice(0, 40),
      bars: [0, 4, 8]
    })
    await settle()
    await vi.advanceTimersByTimeAsync(800)
    await settle()
    expect(bridge().setMetronome).toHaveBeenCalled()
    for (const [sent] of bridge().setMetronome.mock.calls) {
      expect([sent.mult, sent.shift], 'ya no van el doble ni el «1» corrido').toEqual([0, 0])
      // y los pulsos ajustados no viajan con los ajustes: van por su rejilla
      expect(sent.own).toBeUndefined()
    }
  })
})

// Los tres modos del clic, con lo que cada uno enseña. «A mano» era un tempo
// para todas las canciones y quien lo pulsaba esperaba lo que había movido él
// en la onda: ahora se llaman por lo que son, y cambiar de uno a otro no
// pierde ni mezcla nada.
describe('the three click modes', () => {
  /** Su clic manual de la canción 7: el automático con un pulso movido (el 40) y su zona. */
  const HIS = {
    beats: GRID.beats.slice(0, 120).map((t, i) => (i === 40 ? t + 0.08 : t)),
    bars: [0],
    meter: 4,
    touched: [[GRID.beats[39], GRID.beats[41]]]
  }
  const withOwn = (own = HIS) => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { own } }) }),
      song(8, { title: 'Otra' })
    ]
  }
  const startIn = (m) => {
    localStorage.setItem('danplay.click.mode', JSON.stringify(m))
    resetClick()
  }
  const lastGrid = () => bridge().setBeatGrid.mock.calls.at(-1)?.[1]
  const study7 = () => held.state.songs.find((s) => s.id === 7).study

  it('are named for what each one holds, and what is saved keeps its old names', async () => {
    const w = await mountView()
    expect(w.findAll('.study-mode').map((b) => b.text())).toEqual([
      'Automático',
      'Manual',
      'Tempo fijo'
    ])
    const titles = w.findAll('.study-mode').map((b) => b.attributes('title'))
    expect(titles[0]).toContain('Lo que analizó DanPlay')
    expect(titles[1]).toContain('Lo que mueves tú en la onda')
    expect(titles[2]).toContain('el mismo en todas las canciones')
    await mode(w, 'Manual').trigger('click')
    await settle()
    expect(JSON.parse(localStorage.getItem('danplay.click.mode'))).toBe('own')
    await mode(w, 'Tempo fijo').trigger('click')
    await settle()
    expect(JSON.parse(localStorage.getItem('danplay.click.mode'))).toBe('manual')
    await mode(w, 'Automático').trigger('click')
    await settle()
    expect(JSON.parse(localStorage.getItem('danplay.click.mode'))).toBe('auto')
  })

  it('going back and forth, each one brings back its own and nothing saved changes', async () => {
    withOwn()
    const saved = study7()
    const w = await mountView()
    const auto = lastGrid()
    expect(auto.beats[0]).toBe(0.5)
    expect(auto.beats.length).toBe(300)
    for (let round = 0; round < 3; round++) {
      await mode(w, 'Manual').trigger('click')
      await settle()
      expect(useClick().source.value).toBe('own')
      expect(lastGrid().beats.length, 'los pulsos de él').toBe(120)
      expect(lastGrid().beats[40]).toBeCloseTo(GRID.beats[40] + 0.08, 6)
      await mode(w, 'Automático').trigger('click')
      await settle()
      expect(useClick().source.value).toBe('beat-this')
      expect(lastGrid().beats.length, 'lo que analizó DanPlay').toBe(300)
    }
    await vi.advanceTimersByTimeAsync(1000)
    expect(held.api.setStudy, 'cambiar de modo no escribe nada').not.toHaveBeenCalled()
    expect(study7()).toBe(saved)
  })

  it('Manual on a song without its own says so, and offers to start one', async () => {
    startIn('own')
    const w = await mountView()
    expect(useClick().source.value, 'mientras, suena el automático').toBe('beat-this')
    expect(w.text()).toContain('esta canción aún no tiene el tuyo')
    expect(w.find('.study-edit-beats').text()).toContain('Empezar el mío')
    expect(mode(w, 'Manual').classes()).not.toContain('has')
    expect(mode(w, 'Manual').attributes('title')).toContain('aún no tiene el suyo')
    // y con el suyo hecho, lo dice y lo marca
    withOwn()
    held.playback.emit({ ...other, path: '/musica/otra.mp3' })
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
      path: '/musica/mi-gozo.mp3'
    })
    await settle()
    const w2 = await mountView()
    expect(w2.text()).toContain('los pulsos que moviste tú en esta canción')
    expect(w2.find('.study-edit-beats').text()).toContain('Ajustar pulsos')
    expect(mode(w2, 'Manual').classes()).toContain('has')
  })

  it('a slow automatic grid does not land on top of Manual, which was chosen meanwhile', async () => {
    // la automática tardaba; entre tanto se pasaba a Manual, y al llegar se
    // ponía encima y ahí se quedaba, con «Manual» marcado
    withOwn()
    const usual = held.api.beatGridNow.getMockImplementation()
    let arrive = () => {}
    held.api.beatGridNow.mockImplementation(
      () => new Promise((resolve) => (arrive = () => resolve({ ...GRID })))
    )
    try {
      const w = await mountView()
      await mode(w, 'Manual').trigger('click')
      await settle()
      expect(useClick().source.value).toBe('own')
      bridge().setBeatGrid.mockClear()
      arrive()
      await settle()
      expect(useClick().source.value).toBe('own')
      expect(bridge().setBeatGrid, 'la tardía se descarta').not.toHaveBeenCalled()
      expect(useClick().grid.value.beats.length).toBe(120)
    } finally {
      held.api.beatGridNow.mockImplementation(usual)
    }
  })
})

// «Re-analizar» sobre un clic que movió él: el núcleo recibe sus pulsos con
// las zonas que tocó, devuelve el clic mezclado (lo suyo, y lo demás con el
// análisis nuevo) y esto lo guarda y lo aplica. Antes no sabía de él: con su
// clic puesto, el resultado quedaba debajo y no se veía.
describe('re-analysing with a hand-made click', () => {
  const HIS = {
    beats: GRID.beats.slice(0, 120).map((t, i) => (i === 40 ? t + 0.08 : t)),
    bars: [0],
    meter: 4,
    touched: [[GRID.beats[39], GRID.beats[41]]]
  }
  const NEW = { ...GRID, bpm: 120, beats: Array.from({ length: 300 }, (_, i) => 0.4 + i * 0.5) }
  const lastGrid = () => bridge().setBeatGrid.mock.calls.at(-1)?.[1]
  const stored = () => JSON.parse(held.state.songs.find((s) => s.id === 7).study || '{}')
  const last = () => useNotices().notices.value.at(-1)?.message ?? ''
  const { menu } = useContextMenu()
  const withOwn = (own = HIS) => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { own } }) }),
      song(8, { title: 'Otra' })
    ]
  }
  const startIn = (m) => {
    localStorage.setItem('danplay.click.mode', JSON.stringify(m))
    resetClick()
  }
  async function reanalyze(w, label = 'Arreglar huecos y saltos') {
    const button = w.findAll('button').find((b) => b.text().includes('Re-analizar'))
    await button.trigger('click')
    await menu.value.items.find((i) => i.label === label).action()
    await settle()
  }
  const modeBtn = (w, name) => w.findAll('.study-mode').find((b) => b.text() === name)
  beforeEach(() => {
    useNotices().clearNotices()
    held.state.reanalysis = { grid: NEW }
    held.api.reanalyzeBeats = vi.fn(answerReanalysis)
  })

  it('in Manual it sends his beats with his windows, and keeps his while the rest follows the new analysis', async () => {
    withOwn()
    startIn('own')
    const w = await mountView()
    expect(useClick().source.value).toBe('own')
    await reanalyze(w)
    expect(held.api.reanalyzeBeats).toHaveBeenCalledWith(7, {
      mode: 'repair',
      own: { beats: HIS.beats, bars: HIS.bars, meter: 4, touched: HIS.touched }
    })
    // se guarda con su canción: su zona tal cual, lo demás del análisis nuevo
    const own = stored().metronome.own
    expect(own.touched).toEqual(HIS.touched)
    expect(own.beats).toContain(GRID.beats[40] + 0.08)
    expect(own.beats, 'fuera de su zona, el análisis nuevo').toContain(NEW.beats[0])
    expect(own.beats, 'y no lo de antes').not.toContain(GRID.beats[10])
    // y suena y se ve ya
    expect(useClick().source.value).toBe('own')
    expect(lastGrid().beats).toEqual(own.beats)
    expect(last()).toMatch(
      /pulsos cambiaron fuera de lo que moviste tú; se respetó 1 zona tuya|se respetaron 1 zona tuya/
    )
  })

  it('Automático then shows the new analysis as it came, and Manual brings back what was saved', async () => {
    withOwn()
    startIn('own')
    const w = await mountView()
    await reanalyze(w)
    const merged = stored().metronome.own
    await modeBtn(w, 'Automático').trigger('click')
    await settle()
    expect(useClick().source.value).toBe('beat-this')
    expect(lastGrid().beats, 'el análisis nuevo, sin mezclar').toEqual(NEW.beats)
    await modeBtn(w, 'Manual').trigger('click')
    await settle()
    expect(useClick().source.value).toBe('own')
    expect(lastGrid().beats, 'lo suyo, como se guardó').toEqual(merged.beats)
    expect(stored().metronome.own).toEqual(merged)
    // una y otra vez, sin cambiar nada
    for (let i = 0; i < 2; i++) {
      await modeBtn(w, 'Automático').trigger('click')
      await settle()
      expect(lastGrid().beats).toEqual(NEW.beats)
      await modeBtn(w, 'Manual').trigger('click')
      await settle()
      expect(lastGrid().beats).toEqual(merged.beats)
    }
    expect(stored().metronome.own).toEqual(merged)
  })

  it('in Automático only the automatic one changes: his stays exactly as it was, and it says where it is', async () => {
    withOwn()
    const before = held.state.songs[0].study
    const w = await mountView()
    expect(useClick().source.value).toBe('beat-this')
    await reanalyze(w)
    expect(held.api.reanalyzeBeats).toHaveBeenCalledWith(7, { mode: 'repair' })
    expect(lastGrid().beats).toEqual(NEW.beats)
    expect(held.state.songs[0].study, 'lo suyo, intacto').toBe(before)
    expect(last()).toContain('Tu clic manual de esta canción sigue como lo dejaste')
    expect(last()).toContain('«Manual»')
    await modeBtn(w, 'Manual').trigger('click')
    await settle()
    expect(lastGrid().beats).toEqual(HIS.beats)
  })

  it('he gets told why nothing changed: his windows are respected and the rest was already so', async () => {
    // su clic es el automático con un pulso movido, y el análisis nuevo da lo
    // mismo que había fuera de esa zona
    withOwn({ ...HIS, beats: GRID.beats.map((t, i) => (i === 40 ? t + 0.08 : t)) })
    startIn('own')
    held.state.reanalysis = { grid: GRID, fixed: 0 }
    const w = await mountView()
    const before = held.state.songs[0].study
    await reanalyze(w)
    expect(last()).toContain('Tu clic manual se queda igual')
    expect(last()).toContain('1 zona tuya')
    expect(held.state.songs[0].study, 'no se reescribe sin motivo').toBe(before)
  })

  it('an own click from before the windows existed goes without them: the core works out which were his, and they are saved', async () => {
    // el automático de entonces, con un pulso que movió él (el 40)
    const old = { beats: HIS.beats, bars: HIS.bars, meter: 4 }
    withOwn(old)
    startIn('own')
    held.state.beatGrid = JSON.parse(JSON.stringify(GRID))
    held.state.reanalysis = { grid: GRID, fixed: 0 }
    const w = await mountView()
    await reanalyze(w)
    // no se inventa «toda la canción»: va sin zonas y las deduce el núcleo
    expect(held.api.reanalyzeBeats.mock.calls.at(-1)[1].own).not.toHaveProperty('touched')
    // y vuelven, ya guardadas, para que la próxima vez se sepa cuál era su zona
    const saved = stored().metronome.own
    expect(saved.touched).toHaveLength(1)
    const [from, to] = saved.touched[0]
    expect(from).toBeLessThan(HIS.beats[40])
    expect(to).toBeGreaterThan(HIS.beats[40])
    expect(to - from, 'no es toda la canción').toBeLessThan(10)
    expect(last()).not.toContain('se queda entero')
  })

  it('a core that does not return his click leaves it as it was, and says so', async () => {
    withOwn()
    startIn('own')
    held.api.reanalyzeBeats.mockImplementationOnce(async () => ({
      ...NEW,
      grid: NEW,
      report: { fixed: 7 }
    }))
    const w = await mountView()
    const before = held.state.songs[0].study
    await reanalyze(w)
    expect(held.state.songs[0].study).toBe(before)
    expect(last()).toContain('Tu clic manual se queda como estaba')
    expect(last()).toContain('«Automático»')
  })

  it('if he moved on to another song meanwhile, his click is saved in the one it belongs to', async () => {
    withOwn()
    startIn('own')
    const w = await mountView()
    let answer = () => {}
    held.api.reanalyzeBeats.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answer = () =>
            resolve({
              ...NEW,
              grid: NEW,
              report: { fixed: 3 },
              own: { beats: [1, 1.5, 2, 2.5], bars: [0], meter: 4, touched: HIS.touched }
            })
        })
    )
    const button = w.findAll('button').find((b) => b.text().includes('Re-analizar'))
    await button.trigger('click')
    const done = menu.value.items.find((i) => i.label === 'Arreglar huecos y saltos').action()
    await settle()
    held.playback.emit({ ...other, path: '/musica/otra.mp3', position: 0 })
    await settle()
    answer()
    await done
    await settle()
    expect(stored().metronome.own.beats).toEqual([1, 1.5, 2, 2.5])
    expect(held.state.songs.find((s) => s.id === 8).study ?? '').not.toContain('own')
  })
})

// Las zonas que movió él se guardan con su clic: lo que cambió al soltar el
// pulso respecto a lo que se veía.
describe('the windows he moves are saved with his click', () => {
  const saved = () => held.api.setStudy.mock.calls.at(-1)[1].metronome.own
  /** Lo que se ve ahora en la onda, con `f` aplicado a sus pulsos (como lo manda la línea de tiempo). */
  const edited = (f) => {
    const g = useClick().grid.value
    const beats = [...g.beats]
    f(beats)
    return { ...g, beats, bars: [...g.bars] }
  }
  async function drop(w, grid) {
    w.findComponent(StudyTimeline).vm.$emit('update:grid', grid)
    await settle()
    await vi.advanceTimersByTimeAsync(800)
    await settle()
  }
  const startIn = (m) => {
    localStorage.setItem('danplay.click.mode', JSON.stringify(m))
    resetClick()
  }

  it('moving a beat stores its window next to the beats', async () => {
    startIn('own')
    const w = await mountView()
    await drop(
      w,
      edited((b) => (b[40] += 0.08))
    )
    expect(saved().beats.length).toBe(300)
    expect(saved().touched).toEqual([[GRID.beats[39], GRID.beats[41]]])
    expect(useClick().source.value, 'y suena ya').toBe('own')
  })

  it('he keeps editing: the windows add up, and beats added before them do not move them', async () => {
    startIn('own')
    const w = await mountView()
    await drop(
      w,
      edited((b) => (b[40] += 0.08))
    )
    await drop(
      w,
      edited((b) => (b[200] += 0.05))
    )
    expect(saved().touched).toEqual([
      [GRID.beats[39], GRID.beats[41]],
      [GRID.beats[199], GRID.beats[201]]
    ])
    // un pulso nuevo mucho antes (como lo añade la línea de tiempo, con los
    // «1» de detrás corridos un lugar): todos los índices de detrás cambian y
    // sus zonas, no
    const g = useClick().grid.value
    const at = g.beats.findIndex((t) => t > 2.2)
    await drop(w, {
      ...g,
      beats: [...g.beats.slice(0, at), 2.2, ...g.beats.slice(at)],
      bars: g.bars.map((b) => (b >= at ? b + 1 : b))
    })
    const windows = saved().touched
    expect(windows).toContainEqual([GRID.beats[39], GRID.beats[41]])
    expect(windows).toContainEqual([GRID.beats[199], GRID.beats[201]])
    const first = windows[0]
    expect(first[0]).toBeLessThanOrEqual(2.2)
    expect(first[1]).toBeGreaterThanOrEqual(2.2)
    expect(first[1], 'la zona nueva es solo la de ahí').toBeLessThan(5)
    expect(windows.length).toBe(3)
  })

  it('editing a click from before the windows leaves it without windows: the core works them out', async () => {
    const old = { beats: GRID.beats.slice(0, 120), bars: [0], meter: 4 }
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { own: old } }) }),
      song(8, { title: 'Otra' })
    ]
    startIn('own')
    const w = await mountView()
    await drop(
      w,
      edited((b) => (b[40] += 0.08))
    )
    // sigue sin zonas guardadas: el núcleo las deduce (con lo que mueve ahora)
    expect(saved()).not.toHaveProperty('touched')
  })
})

// Dónde el clic es una suposición: el análisis dice qué zonas no tienen
// pulso con apoyo en la música (`report.weak`), y se ven en la onda y en el
// aviso de después de re-analizar. Sin el dato, nada.
describe('where the click is a guess', () => {
  const NEW = { ...GRID, bpm: 120, beats: Array.from({ length: 300 }, (_, i) => 0.4 + i * 0.5) }
  const WEAK = [
    [10, 20],
    [60, 75],
    [120, 140],
    [150, 160]
  ]
  const last = () => useNotices().notices.value.at(-1)?.message ?? ''
  const { menu } = useContextMenu()
  const bands = (w) => w.findAll('.tl-weak')
  async function reanalyze(w) {
    const button = w.findAll('button').find((b) => b.text().includes('Re-analizar'))
    await button.trigger('click')
    await menu.value.items.find((i) => i.label === 'Arreglar huecos y saltos').action()
    await settle()
  }
  beforeEach(() => {
    useNotices().clearNotices()
    held.api.reanalyzeBeats = vi.fn(answerReanalysis)
  })

  it('after re-analysing they are drawn on the waveform, with what they mean', async () => {
    held.state.reanalysis = { grid: NEW, weak: WEAK }
    const w = await mountView()
    expect(bands(w), 'antes de saberlo, nada').toHaveLength(0)
    await reanalyze(w)
    expect(bands(w)).toHaveLength(4)
    expect(bands(w)[0].attributes('title')).toContain('Poco claro: aquí el clic es una suposición')
    expect(bands(w)[0].attributes('title')).toContain('0:10 – 0:20')
    // sobre la onda de 200 s: de 10 a 20 es del 5 % al 10 %
    expect(bands(w)[0].attributes('style')).toMatch(/left: 5%;.*width: 5%/)
    // y el aviso las cuenta (hasta tres)
    expect(last()).toContain(
      'Poco claro (aquí el clic es una suposición): 0:10–0:20, 1:00–1:15, 2:00–2:20 y 1 más'
    )
  })

  it('without the data there is nothing to draw, and the notice does not mention them', async () => {
    held.state.reanalysis = { grid: NEW }
    const w = await mountView()
    await reanalyze(w)
    expect(bands(w)).toHaveLength(0)
    expect(last()).not.toContain('Poco claro')
  })

  it('a new analysis without them takes away the ones from before', async () => {
    held.state.reanalysis = { grid: NEW, weak: WEAK }
    const w = await mountView()
    await reanalyze(w)
    expect(bands(w)).toHaveLength(4)
    held.state.reanalysis = { grid: NEW }
    await reanalyze(w)
    expect(bands(w)).toHaveLength(0)
  })

  it('the ones the saved grid brings are drawn from the start', async () => {
    held.state.beatGrid = { ...GRID, report: { weak: [[30, 45]] } }
    const w = await mountView()
    expect(bands(w)).toHaveLength(1)
    expect(bands(w)[0].attributes('title')).toContain('0:30 – 0:45')
  })

  it('a guess of a few seconds is not shown: on soft songs almost everything is one and says nothing', async () => {
    held.state.reanalysis = {
      grid: NEW,
      weak: [
        [10, 12],
        [60, 75],
        [90, 93.5]
      ]
    }
    const w = await mountView()
    await reanalyze(w)
    expect(bands(w)).toHaveLength(1)
    expect(bands(w)[0].attributes('title')).toContain('1:00 – 1:15')
    expect(last()).toContain('1:00–1:15')
    expect(last()).not.toContain('0:10')
  })

  it('how well the click fits the hits, before and after, is in a tooltip if the core measured it', async () => {
    const fit = {
      before: { offset_ms: -14.2, spread_ms: 48, supported: 0.7 },
      after: { offset_ms: -3.4, spread_ms: 12, supported: 0.7 }
    }
    held.state.reanalysis = { grid: NEW, fit }
    const w = await mountView()
    const status = () => w.find('.study-metro-status')
    expect(status().attributes('title')).toBeUndefined()
    await reanalyze(w)
    expect(status().attributes('title')).toContain('14 ms tarde → 3 ms tarde')
    expect(status().attributes('title')).toContain('dispersión 48 ms → 12 ms')
    held.state.reanalysis = { grid: NEW }
    await reanalyze(w)
    expect(status().attributes('title'), 'sin el dato, nada').toBeUndefined()
  })

  it('they belong to their song, and with a fixed tempo the click does not follow it', async () => {
    held.state.reanalysis = { grid: NEW, weak: WEAK }
    const w = await mountView()
    await reanalyze(w)
    expect(bands(w)).toHaveLength(4)
    await mode(w, 'Tempo fijo').trigger('click')
    await settle()
    expect(bands(w)).toHaveLength(0)
    await mode(w, 'Automático').trigger('click')
    await settle()
    expect(bands(w)).toHaveLength(4)
    held.playback.emit({ ...other, path: '/musica/otra.mp3', position: 0 })
    await settle()
    expect(bands(w)).toHaveLength(0)
  })
})
