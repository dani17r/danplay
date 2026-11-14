import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// «Sin acento» estaba puesto a mano en 16 de las 22 canciones que su dueño
// ajustó (I7 del plan). Una preferencia general en Ajustes dice qué hace el
// clic en las canciones que no tienen su compás ajustado: acento en el «1»
// (lo de siempre) o sin acento. Lo ajustado en una canción manda siempre, y
// las que ya estaban ajustadas no cambian.
const held = vi.hoisted(() => ({ playback: null, api: null, state: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble, createAppDouble } =
    await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: held.playback.bridge,
    app: createAppDouble()
  }
})

import StudyBar from '../src/components/StudyBar.vue'
import SettingsPage from '../src/components/SettingsPage.vue'
import { resetPlayback, usePlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { resetRouting } from '../src/composables/useRouting.js'
import { song } from './support/backend.js'

const KEY = 'danplay.click.accent'
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
  path: '/musica/mi-gozo.mp3'
}
const bridge = () => held.playback.bridge
/** Lo último que llegó a Rust como metrónomo. */
const sent = () => bridge().setMetronome.mock.calls.at(-1)?.[0]
const withClick = (metronome) => ({ study: JSON.stringify({ metronome }) })

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  resetPlayback()
  resetClick()
  resetRouting()
  held.playback.reset()
  held.state.beatGrid = { ...GRID }
  held.state.beatGridPending = []
  held.state.status.configured = true
  held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
})

const mounted = []
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
})
/** Abre el modo estudio en la canción `id` (la 7 por defecto). */
async function openStudy(id = 7, title = 'Mi Gozo') {
  held.playback.emit({
    track: { id, title, artist: 'Barak', duration: 200 },
    playing: false,
    position: 0,
    ...base
  })
  const w = mount(StudyBar, { attachTo: document.body })
  mounted.push(w)
  await flushPromises()
  await flushPromises()
  return w
}
const meterButton = (w, label) =>
  w
    .find('.study-meters')
    .findAll('button')
    .find((b) => b.text() === label)

describe('the click of a song with no meter of its own', () => {
  it('accents the «1» as always, unless the preference says otherwise', async () => {
    await openStudy()
    expect(sent().meter).toBeNull()
  })

  it('with «sin acento» by default, it plays with no accent', async () => {
    localStorage.setItem(KEY, 'false')
    resetPlayback()
    const w = await openStudy()
    expect(sent().meter).toBe(0)
    // y la onda no pinta el «1» donde no suena
    expect(w.findComponent({ name: 'StudyTimeline' }).props('grid')?.meter).toBe(0)
    // el botón de «sin acento» es el que está puesto
    await w.find('.study-more-toggle').trigger('click')
    expect(meterButton(w, 'sin acento').attributes('aria-pressed')).toBe('true')
    expect(meterButton(w, '4/4').attributes('aria-pressed')).toBe('false')
  })

  it('the meter of the song itself always wins, with the preference either way', async () => {
    localStorage.setItem(KEY, 'false')
    resetPlayback()
    held.state.songs = [
      song(7, { title: 'Mi Gozo', ...withClick({ meter: 3 }) }),
      song(8, { title: 'Otra', ...withClick({ meter: 0 }) })
    ]
    await openStudy(7)
    expect(sent().meter).toBe(3)
    // y la que ya estaba «sin acento» se queda así si se vuelve a acentuar por defecto
    await usePlayback().setDefaultAccent(true)
    expect(sent().meter).toBe(3)
    held.playback.emit({
      track: { id: 8, title: 'Otra', artist: 'Barak', duration: 200 },
      path: '/musica/otra.mp3'
    })
    await flushPromises()
    await flushPromises()
    expect(sent().meter).toBe(0)
  })
})

describe('changing the preference in Ajustes', () => {
  async function mountSettings() {
    const w = mount(SettingsPage, { attachTo: document.body })
    mounted.push(w)
    await flushPromises()
    await flushPromises()
    return w
  }
  const accentField = (w) =>
    w
      .findAll('.field')
      .find((f) => f.find('.field-label')?.text() === 'Acento del clic por defecto')
  async function choose(w, name) {
    const field = accentField(w)
    await field.find('.select-box').trigger('click')
    await field
      .findAll('.select-opt')
      .find((o) => o.text().startsWith(name))
      .trigger('click')
    await flushPromises()
  }

  it('starts on the accent, is remembered, and the song that is playing follows it at once', async () => {
    await openStudy()
    const settings = await mountSettings()
    expect(accentField(settings).text()).toContain('Acento en el «1»')
    bridge().setMetronome.mockClear()

    await choose(settings, 'Sin acento')
    expect(localStorage.getItem(KEY)).toBe('false')
    expect(sent().meter).toBe(0)
    expect(usePlayback().accentDefault.value).toBe(false)

    await choose(settings, 'Acento')
    expect(localStorage.getItem(KEY)).toBe('true')
    expect(sent().meter).toBeNull()
  })

  it('and it does not touch a song that has its own meter', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', ...withClick({ meter: 3 }) })]
    await openStudy()
    const settings = await mountSettings()
    bridge().setMetronome.mockClear()
    await choose(settings, 'Sin acento')
    expect(localStorage.getItem(KEY)).toBe('false')
    // no hizo falta mandar nada: la canción sigue en 3/4
    expect(bridge().setMetronome).not.toHaveBeenCalled()
    expect(usePlayback().metronome.value.meter).toBe(3)
  })

  it('is read when DanPlay starts', async () => {
    localStorage.setItem(KEY, 'false')
    resetPlayback()
    expect(usePlayback().accentDefault.value).toBe(false)
    expect(usePlayback().defaultMeter.value).toBe(0)
  })
})

describe('with «sin acento» by default, choosing the detected meter in a song is kept', () => {
  it('it saves 4/4 with the song, so it does not go back to «sin acento»', async () => {
    vi.useFakeTimers()
    try {
      localStorage.setItem(KEY, 'false')
      resetPlayback()
      held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
      const w = await openStudy()
      await w.find('.study-more-toggle').trigger('click')
      expect(meterButton(w, 'sin acento').attributes('aria-pressed')).toBe('true')
      await meterButton(w, '4/4').trigger('click')
      await flushPromises()
      expect(sent().meter).toBe(4)
      expect(meterButton(w, '4/4').attributes('aria-pressed')).toBe('true')
      await vi.advanceTimersByTimeAsync(700)
      expect(held.api.setStudy).toHaveBeenLastCalledWith(
        7,
        expect.objectContaining({ metronome: expect.objectContaining({ meter: 4 }) })
      )
    } finally {
      vi.useRealTimers()
    }
  })

  it('and with the accent by default it is still not saved (the analysis decides)', async () => {
    vi.useFakeTimers()
    try {
      const w = await openStudy()
      await w.find('.study-more-toggle').trigger('click')
      await meterButton(w, '3/4').trigger('click')
      await flushPromises()
      await meterButton(w, '4/4').trigger('click')
      await flushPromises()
      await vi.advanceTimersByTimeAsync(700)
      const saved = held.api.setStudy.mock.calls.at(-1)?.[1]
      expect(saved?.metronome?.meter).toBeUndefined()
    } finally {
      vi.useRealTimers()
    }
  })
})
