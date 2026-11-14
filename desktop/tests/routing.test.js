import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El reparto de las salidas es de cada salida de audio, y manda la que
// suena: lo de la interfaz de la iglesia no vale para los cascos de casa. Y
// si a la que suena le faltan canales, suena todo junto (no muda) y se dice.
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
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { useRouting, resetRouting } from '../src/composables/useRouting.js'
import { useNotices, clearNotices } from '../src/composables/useNotices.js'
import { song } from './support/backend.js'

const KEY = 'danplay.outputs'
const CHURCH = 'alsa:hw:CARD=Scarlett,DEV=0'
const HOME = 'alsa:hw:CARD=Generic,DEV=0'
/** La música por 3-4 y el clic y las guías por 1-2: lo que se manda al motor. */
const SPLIT = {
  buses: [
    { left: 2, right: 3 },
    { left: 0, right: 1 }
  ],
  song: 0,
  click: 1,
  cues: 1
}
/** Todo junto, por los dos lados. */
const TOGETHER = { buses: [{ left: 0, right: 1 }], song: 0, click: 0, cues: 0 }

const bridge = () => held.playback.bridge
const sentRouting = () => bridge().setRouting.mock.calls.at(-1)[0]
const sentOutput = () => bridge().setOutput.mock.calls.at(-1)[0]
const notices = () => useNotices().notices.value.map((n) => n.message)
/** La salida abierta, como la cuenta Rust en el estado. */
const opened = (over = {}) => ({
  device: 'alsa:pipewire',
  name: 'PipeWire',
  channels: 2,
  rate: 48000,
  fallback: false,
  error: null,
  xruns: 0,
  ...over
})
/** Pasa lo que tarda en saberse cómo quedó la salida recién pedida. */
async function settled() {
  await vi.advanceTimersByTimeAsync(1600)
  await flushPromises()
}
const church = { id: CHURCH, name: 'Scarlett 4i4', channels: 4, default: false }
const home = { id: HOME, name: 'Tarjeta del equipo', channels: 2, default: false }

beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  localStorage.clear()
  clearNotices()
  resetPlayback()
  resetClick()
  held.playback.reset()
  delete held.playback.state.output
  bridge().outputDevices = vi.fn(async () => [])
  resetRouting()
})
afterEach(() => {
  resetRouting()
  clearNotices()
  vi.useRealTimers()
})

describe('one routing per audio output', () => {
  it('what was set for the interface is not applied to another output, and comes back with it', async () => {
    bridge().outputDevices = vi.fn(async () => [church, home])
    const r = useRouting()
    await r.loadDevices()
    await r.update({ device: CHURCH })
    await r.update({ preset: 'click-12' })
    expect(sentOutput()).toEqual({ device: CHURCH, channels: 4 })
    expect(sentRouting()).toEqual(SPLIT)

    // en casa, otra salida: sin reparto guardado, todo junto
    await r.update({ device: HOME })
    expect(r.prefs.value.preset).toBe('stereo')
    expect(sentOutput()).toEqual({ device: HOME, channels: 2 })
    expect(sentRouting()).toEqual(TOGETHER)
    await r.update({ preset: 'click-left' })

    // y de vuelta en la iglesia, lo suyo; cada una con lo que se le dejó
    await r.update({ device: CHURCH })
    expect(r.prefs.value.preset).toBe('click-12')
    expect(sentRouting()).toEqual(SPLIT)
    await r.update({ device: HOME })
    expect(r.prefs.value.preset).toBe('click-left')

    // y así se queda guardado
    resetRouting()
    const again = useRouting()
    await flushPromises()
    expect(again.prefs.value).toMatchObject({ device: HOME, preset: 'click-left' })
    await again.update({ device: CHURCH })
    expect(again.prefs.value.preset).toBe('click-12')
  })

  it('the single routing saved until 1.41 stays with the output it was made for', async () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({
        device: CHURCH,
        preset: 'custom',
        custom: { song: 'p3', click: 'L', cues: 'R', stems: { drums: 'm4' } },
        mono: { song: true, click: false, cues: false, stems: {} }
      })
    )
    resetRouting()
    const r = useRouting()
    await flushPromises()
    expect(r.prefs.value).toMatchObject({
      device: CHURCH,
      preset: 'custom',
      custom: { song: 'p3', click: 'L', cues: 'R', stems: { drums: 'm4' } },
      mono: { song: true }
    })
    // la del sistema no lo hereda
    await r.update({ device: null })
    expect(r.prefs.value.preset).toBe('stereo')
    expect(sentRouting()).toEqual(TOGETHER)
    await r.update({ device: CHURCH })
    expect(r.prefs.value.custom.song).toBe('p3')
  })

  it('the one saved with no output chosen is the one of the system output', async () => {
    localStorage.setItem(KEY, JSON.stringify({ device: null, preset: 'click-left' }))
    resetRouting()
    const r = useRouting()
    await flushPromises()
    expect(r.prefs.value.preset).toBe('click-left')
    await r.update({ device: HOME })
    expect(r.prefs.value.preset).toBe('stereo')
  })
})

describe('the chosen output is not there', () => {
  beforeEach(() => {
    localStorage.setItem(
      KEY,
      JSON.stringify({
        device: CHURCH,
        byDevice: { [CHURCH]: { preset: 'click-12' }, '': { preset: 'click-left' } }
      })
    )
    resetRouting()
  })

  it('it plays through the system one, with the routing of the system one', async () => {
    const r = useRouting()
    await flushPromises()
    // hasta que se abre no se sabe: lo de la elegida
    expect(sentRouting()).toEqual(SPLIT)
    await settled()
    // al darle a play, Rust dice que la elegida no está
    held.playback.emit({ output: opened({ fallback: true }) })
    await flushPromises()
    expect(r.fellBack.value).toBe(true)
    expect(r.prefs.value).toMatchObject({ device: CHURCH, preset: 'click-left' })
    expect(sentRouting()).toMatchObject({ song: 0, click: 1, cues: 1 })
    expect(sentRouting().buses).toEqual([
      { left: 1, right: 1 },
      { left: 0, right: 0 }
    ])
    // lo que se cambie ahora es de la que suena, no de la que falta
    await r.update({ preset: 'stereo' })
    const stored = JSON.parse(localStorage.getItem(KEY))
    expect(stored.byDevice[''].preset).toBe('stereo')
    expect(stored.byDevice[CHURCH].preset).toBe('click-12')
  })

  it('known before the first play when the outputs of the computer say so', async () => {
    bridge().outputDevices = vi.fn(async () => [home])
    const r = useRouting()
    await flushPromises()
    expect(bridge().outputDevices).toHaveBeenCalled()
    expect(r.fellBack.value).toBe(true)
    expect(r.prefs.value.preset).toBe('click-left')
    // se sigue pidiendo la elegida (por si vuelve), con los canales de lo que suena
    expect(sentOutput()).toEqual({ device: CHURCH, channels: 2 })
    expect(sentRouting().buses).toEqual([
      { left: 1, right: 1 },
      { left: 0, right: 0 }
    ])
  })

  it('a long pause (Rust lets go of the output) does not bring back the routing of the missing one', async () => {
    const r = useRouting()
    await flushPromises()
    await settled()
    held.playback.emit({ output: opened({ fallback: true }) })
    await flushPromises()
    await settled()
    bridge().setRouting.mockClear()
    bridge().setOutput.mockClear()
    held.playback.emit({ output: null })
    await flushPromises()
    await settled()
    expect(r.prefs.value.preset).toBe('click-left')
    expect(bridge().setRouting).not.toHaveBeenCalled()
    expect(bridge().setOutput).not.toHaveBeenCalled()
    // pero si mientras tanto se enchufó y se vuelven a mirar las salidas, ya
    // es la suya otra vez, antes de darle a play
    bridge().outputDevices = vi.fn(async () => [church, home])
    await r.loadDevices()
    await flushPromises()
    expect(r.fellBack.value).toBe(false)
    expect(r.prefs.value.preset).toBe('click-12')
    expect(sentOutput()).toEqual({ device: CHURCH, channels: 4 })
    expect(sentRouting()).toEqual(SPLIT)
  })
})

describe('the output lacks the channels of its routing', () => {
  beforeEach(() => {
    localStorage.setItem(KEY, JSON.stringify({ device: null, preset: 'click-12' }))
    resetRouting()
  })

  it('everything plays together instead of the music going mute, and it says so', async () => {
    const r = useRouting()
    await flushPromises()
    // sin saber aún los canales de la salida, lo guardado
    expect(sentOutput()).toEqual({ device: null, channels: 4 })
    expect(sentRouting()).toEqual(SPLIT)
    expect(r.short.value).toBe(false)
    await settled()
    expect(notices()).toEqual([])

    // se abre al darle a play: tiene dos canales, y la música iba por 3-4
    held.playback.emit({ output: opened({ channels: 2 }) })
    await flushPromises()
    expect(r.short.value).toBe(true)
    expect(sentRouting()).toEqual(TOGETHER)
    expect(notices()).toHaveLength(1)
    expect(notices()[0]).toContain('La salida tiene 2 canales')
    expect(notices()[0]).toContain('pide 4')
    expect(notices()[0]).toContain('suena todo junto')
    // el reparto guardado no se toca, y se siguen pidiendo sus canales (la
    // salida no se reabre, y si los tiene se sabrá)
    expect(r.prefs.value.preset).toBe('click-12')
    expect(sentOutput()).toEqual({ device: null, channels: 4 })
    // y al volver a abrir DanPlay sigue siendo el suyo
    resetRouting()
    expect(useRouting().prefs.value.preset).toBe('click-12')
  })

  it('with the interface plugged in (four channels), its routing, and nothing to say', async () => {
    const r = useRouting()
    await flushPromises()
    await settled()
    held.playback.emit({ output: opened({ channels: 4 }) })
    await flushPromises()
    expect(r.short.value).toBe(false)
    expect(sentRouting()).toEqual(SPLIT)
    expect(notices()).toEqual([])
  })

  it('it is said once: neither every tick nor after a long pause, and the routing does not come and go', async () => {
    useRouting()
    await flushPromises()
    await settled()
    held.playback.emit({ output: opened({ channels: 2 }) })
    await flushPromises()
    expect(notices()).toHaveLength(1)
    clearNotices()
    bridge().setRouting.mockClear()
    // el estado llega entero cuatro veces por segundo
    for (let i = 0; i < 8; i++) held.playback.emit({ position: i, output: opened({ xruns: i }) })
    // en pausa un rato Rust suelta la salida, y la vuelve a abrir al sonar
    held.playback.emit({ output: null })
    await flushPromises()
    await settled()
    held.playback.emit({ output: opened({ channels: 2 }) })
    await flushPromises()
    expect(notices()).toEqual([])
    expect(bridge().setRouting).not.toHaveBeenCalled()
  })

  it('choosing a routing that fits brings it back at once', async () => {
    const r = useRouting()
    await flushPromises()
    await settled()
    held.playback.emit({ output: opened({ channels: 2 }) })
    await flushPromises()
    expect(r.short.value).toBe(true)
    await r.update({ preset: 'click-left' })
    expect(r.short.value).toBe(false)
    expect(sentRouting().buses).toEqual([
      { left: 1, right: 1 },
      { left: 0, right: 0 }
    ])
    // otra vez a 3-4: ya se sabe que esta salida se queda en dos
    await r.update({ preset: 'click-12' })
    expect(r.short.value).toBe(true)
    expect(sentRouting()).toEqual(TOGETHER)
    // hasta que se enchufa la interfaz y Rust la abre con sus cuatro
    held.playback.emit({ output: opened({ channels: 4 }) })
    await settled()
    expect(r.short.value).toBe(false)
    expect(sentRouting()).toEqual(SPLIT)
  })

  it('asking for more channels than were open waits for Rust to reopen before calling it short', async () => {
    localStorage.setItem(KEY, JSON.stringify({ device: null, preset: 'click-left' }))
    resetRouting()
    const r = useRouting()
    await flushPromises()
    await settled()
    // abierta con los dos canales que hacían falta: no dice si tiene más
    held.playback.emit({ output: opened({ channels: 2 }) })
    await flushPromises()
    await r.update({ preset: 'click-12' })
    // Rust la reabre con cuatro, y lo que dice el estado es aún de la de antes
    expect(r.short.value).toBe(false)
    expect(sentRouting()).toEqual(SPLIT)
    expect(notices()).toEqual([])
    // reabierta, sigue con dos: ahora sí
    await settled()
    expect(r.short.value).toBe(true)
    expect(sentRouting()).toEqual(TOGETHER)
    expect(notices()).toHaveLength(1)
  })

  it('a recently reopened output with no channels yet says nothing', async () => {
    const r = useRouting()
    await flushPromises()
    await settled()
    // recién abierta, Rust aún no sabe con qué: canales 0
    held.playback.emit({ output: opened({ channels: 0, name: '', device: '' }) })
    await flushPromises()
    expect(r.short.value).toBe(false)
    expect(sentRouting()).toEqual(SPLIT)
    expect(notices()).toEqual([])
  })
})

describe('where the one playing is', () => {
  beforeEach(() => {
    localStorage.setItem(KEY, JSON.stringify({ device: null, preset: 'click-12' }))
    resetRouting()
    held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
      index: 0,
      length: 1,
      duration: 200,
      playing: false,
      position: 12,
      path: '/musica/mi-gozo.mp3'
    })
  })

  it('the «Salidas» button of the study and its drawer say that channels are missing', async () => {
    const w = mount(StudyBar, { attachTo: document.body })
    await flushPromises()
    await settled()
    const outputs = () => w.find('.study-outputs')
    expect(outputs().classes()).not.toContain('warn')
    held.playback.emit({ output: opened({ channels: 2 }) })
    await flushPromises()
    expect(outputs().classes()).toContain('warn')
    expect(outputs().attributes('title')).toContain('suena todo junto')
    await outputs().trigger('click')
    await flushPromises()
    const warning = document.querySelector('.outs-now.warn')
    expect(warning.textContent.replace(/\s+/g, ' ')).toContain(
      'Esta salida tiene 2 canales y este reparto pide 4: mientras tanto suena todo junto'
    )
    // con «todo junto» elegido ya no hay nada que avisar
    document.querySelectorAll('.outs-preset')[0].click()
    await flushPromises()
    expect(document.querySelector('.outs-now.warn')).toBeNull()
    expect(outputs().classes()).not.toContain('warn')
    w.unmount()
  })
})
