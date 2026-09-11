import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El modo estudio: bucle A-B, velocidad sin cambiar el tono, marcadores y
// notas, guardados con la cancion y aplicados al entrar.
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
import Player from '../src/components/Player.vue'
import { resetPlayback, usePlayback } from '../src/composables/usePlayback.js'
import { song } from './support/backend.js'

const base = { index: 0, length: 1, duration: 200, has_previous: false, has_next: false, error: '', has_output: true, loop_a: 0, loop_b: 0, pitch_preserved: true, speed: 1 }

beforeEach(() => {
  vi.clearAllMocks()
  vi.useRealTimers()
  resetPlayback()
  held.playback.reset()
  held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
  held.playback.emit({ track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 }, playing: true, position: 12, ...base })
})

const montar = async () => {
  const w = mount(StudyBar, { attachTo: document.body })
  await flushPromises(); await flushPromises()
  return w
}
const boton = (w, text) => w.findAll('button').find((b) => b.text().includes(text))

describe('la barra de estudio', () => {
  it('marca A y B donde va la cancion y activa el bucle', async () => {
    vi.useFakeTimers()
    const w = await montar()
    expect(w.text()).toContain('Barak — Mi Gozo')
    await boton(w, 'Marcar A').trigger('click')
    expect(w.text()).toContain('marca B')
    held.playback.emit({ position: 20 })
    await flushPromises()
    await boton(w, 'Marcar B').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(12, 20)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(12)
    expect(w.text()).toContain('0:12 – 0:20')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenCalledWith(7, { loop: [12, 20] })
    // el tercer clic lo quita
    await boton(w, 'Quitar bucle').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    vi.useRealTimers()
    w.unmount()
  })

  it('la velocidad se aplica y se guarda; sin ffmpeg avisa de que el tono cambia', async () => {
    vi.useFakeTimers()
    const w = await montar()
    await boton(w, '0.7×').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setSpeed).toHaveBeenCalledWith(0.7)
    expect(w.text()).toContain('mismo tono')
    held.playback.emit({ pitch_preserved: false })
    await flushPromises()
    expect(w.text()).toContain('cambia el tono')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { speed: 0.7 })
    vi.useRealTimers()
    w.unmount()
  })

  it('los marcadores se ponen donde va y saltan al pulsarlos', async () => {
    vi.useFakeTimers()
    const w = await montar()
    await boton(w, 'aquí').trigger('click')
    held.playback.emit({ position: 45.5 })
    await flushPromises()
    await boton(w, 'aquí').trigger('click')
    await flushPromises()
    const jumps = w.findAll('.study-jump')
    expect(jumps.map((j) => j.text())).toEqual(['0:12', '0:45'])
    await jumps[1].trigger('click')
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(45.5)
    await vi.advanceTimersByTimeAsync(700)
    const saved = held.api.setStudy.mock.calls.at(-1)[1]
    expect(saved.markers.map((m) => m.t)).toEqual([12, 45.5])
    vi.useRealTimers()
    w.unmount()
  })

  it('lo guardado con la cancion se aplica al entrar y se quita al cerrar', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [30, 40], speed: 0.8, notes: 'cejilla 2' }) })]
    const w = await montar()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(30, 40)
    expect(held.playback.bridge.setSpeed).toHaveBeenCalledWith(0.8)
    expect(w.find('textarea').element.value).toBe('cejilla 2')
    await boton(w, '').trigger('click')   // el primer boton de la cabecera es cerrar
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(held.playback.bridge.setSpeed).toHaveBeenLastCalledWith(1)
    expect(w.emitted('close')).toBeTruthy()
    w.unmount()
  })
})

describe('el reproductor y el bucle', () => {
  it('pinta el tramo del bucle y el boton de estudio avisa a la app', async () => {
    const w = mount(Player, { props: { study: false }, attachTo: document.body })
    await flushPromises()
    expect(w.find('.track-loop').exists()).toBe(false)
    held.playback.emit({ loop_a: 50, loop_b: 100 })
    await flushPromises()
    const seg = w.find('.track-loop')
    expect(seg.exists()).toBe(true)
    expect(seg.attributes('style')).toContain('left: 25%')
    expect(seg.attributes('style')).toContain('width: 25%')
    await w.find('.pl-study').trigger('click')
    expect(w.emitted('toggleStudy')).toBeTruthy()
    w.unmount()
  })

  it('en el navegador el bucle vuelve a A al pasar de B', async () => {
    resetPlayback()
    held.api.inTauri = false
    const player = usePlayback()
    await player.ready()
    const audio = player.audioElement()
    await player.setLoop(10, 20)
    Object.defineProperty(audio, 'currentTime', { value: 21, writable: true, configurable: true })
    audio.dispatchEvent(new Event('timeupdate'))
    await flushPromises()
    expect(audio.currentTime).toBe(10)
    expect(player.loopA.value).toBe(10)
    await player.clearLoop()
    expect(player.loopB.value).toBe(0)
    held.api.inTauri = true
  })
})
