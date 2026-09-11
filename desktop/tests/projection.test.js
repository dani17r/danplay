import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// La proyeccion: la letra en grande siguiendo lo que suena. Comparte estado
// con la app por el mismo evento que la ventanita de la bandeja.
const held = vi.hoisted(() => ({ playback: null, projection: null, songs: {} }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createPlaybackDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.playback = createPlaybackDouble()
  held.projection = { show: v.fn(async () => {}), hide: v.fn(async () => {}), fullscreen: v.fn(async () => {}), isFullscreen: v.fn(async () => false) }
  return {
    ...actual, inTauri: true,
    api: { ...actual.api, inTauri: true, song: v.fn(async (id) => held.songs[id] || null) },
    playback: held.playback.bridge,
    projection: held.projection
  }
})

import Projection from '../src/Projection.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'

const LRC = '[00:10.00]Mi gozo\n[00:20.00]es el Señor\n[00:30.00]y su gozo'
const base = { index: 0, length: 1, duration: 200, has_previous: false, has_next: false, error: '', has_output: true }
const state = (extra) => ({ track: null, playing: false, position: 0, ...base, ...extra })

beforeEach(() => {
  vi.clearAllMocks()
  resetPlayback()
  held.playback.reset()
  localStorage.clear()
  held.songs = {
    7: { id: 7, title: 'Mi Gozo', artist: 'Barak', lyrics: 'Mi gozo\nes el Señor', lyrics_synced: LRC },
    8: { id: 8, title: 'Sin tiempos', artist: 'Barak', lyrics: 'Primera estrofa\nsigue\n\nSegunda estrofa', lyrics_synced: '' },
    9: { id: 9, title: 'Muda', artist: 'Barak', lyrics: '', lyrics_synced: '' }
  }
  held.playback.emit(state({}))
})

const montar = async () => {
  const w = mount(Projection, { attachTo: document.body })
  await flushPromises(); await flushPromises()
  return w
}

describe('la proyeccion', () => {
  it('sin nada sonando enseña la marca y una pista', async () => {
    const w = await montar()
    expect(w.find('.proj-idle').text()).toContain('DanPlay')
    w.unmount()
  })

  it('con letra con tiempos resalta la linea que suena y las de alrededor', async () => {
    const w = await montar()
    held.playback.emit(state({ track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 }, playing: true, position: 21 }))
    await flushPromises(); await flushPromises()
    const current = w.find('.proj-line.current')
    expect(current.text()).toBe('es el Señor')
    expect(w.find('.proj-line.past').text()).toBe('Mi gozo')
    expect(w.find('.proj-line.next').text()).toBe('y su gozo')
    expect(w.find('.proj-now').text()).toContain('Barak — Mi Gozo')
    // solo la linea: sin contexto
    await w.findAll('.proj-bar .btn').find((b) => b.text() === '1 línea').trigger('click')
    expect(w.findAll('.proj-line')).toHaveLength(1)
    w.unmount()
  })

  it('sin tiempos se pasa por bloques con las flechas', async () => {
    const w = await montar()
    held.playback.emit(state({ track: { id: 8, title: 'Sin tiempos', artist: 'Barak', duration: 200 }, playing: true, position: 5 }))
    await flushPromises(); await flushPromises()
    expect(w.find('.proj-text').text()).toContain('Primera estrofa')
    expect(w.find('.proj-pager').text()).toContain('1 / 2')
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }))
    await flushPromises()
    expect(w.find('.proj-text').text()).toContain('Segunda estrofa')
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }))
    await flushPromises()
    expect(w.find('.proj-pager').text()).toContain('2 / 2')
    w.unmount()
  })

  it('una cancion sin letra lo dice, y F pide pantalla completa', async () => {
    const w = await montar()
    held.playback.emit(state({ track: { id: 9, title: 'Muda', artist: 'Barak', duration: 100 }, playing: true }))
    await flushPromises(); await flushPromises()
    expect(w.text()).toContain('no tiene letra')
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'f' }))
    await flushPromises()
    expect(held.projection.fullscreen).toHaveBeenCalledWith(true)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(held.projection.fullscreen).toHaveBeenLastCalledWith(false)
    w.unmount()
  })

  it('el tamaño de letra se recuerda', async () => {
    const w = await montar()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: '+' }))
    await flushPromises()
    expect(JSON.parse(localStorage.getItem('danplay.projection.fontSize'))).toBe(62)
    expect(w.find('.proj').attributes('style')).toContain('--proj-size: 62px')
    w.unmount()
  })
})
