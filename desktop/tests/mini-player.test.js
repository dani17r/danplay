import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// La ventanita de la bandeja comparte estado con la app a través de Rust: los
// dos escuchan el mismo evento. Estas pruebas fijan lo que se pidió: que los
// botones estén en gris cuando no se pueden pulsar, según el estado real del
// reproductor, y que pulsar aquí mande la orden de verdad.
// El doble se construye dentro de la fábrica del mock (que se iza) y se deja
// en este cajón para poder mirarlo desde las pruebas.
const held = vi.hoisted(() => ({ playback: null, app: null, mini: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createPlaybackDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.playback = createPlaybackDouble()
  held.app = { showWindow: v.fn(async () => {}), quit: v.fn(async () => {}) }
  held.mini = { hide: v.fn(async () => {}), toggle: v.fn(async () => {}) }
  return {
    ...actual,
    inTauri: true,
    api: { ...actual.api, inTauri: true },
    playback: held.playback.bridge,
    app: held.app,
    mini: held.mini
  }
})

import MiniPlayer from '../src/MiniPlayer.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'

const PLAYING = {
  track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, blur: false },
  index: 0,
  length: 3,
  playing: true,
  position: 0,
  duration: 200,
  has_previous: true,
  has_next: true
}

let w = null
beforeEach(() => {
  vi.clearAllMocks()
  resetPlayback()
  held.playback.reset()
  held.playback.emit({
    track: null,
    index: -1,
    length: 0,
    playing: false,
    position: 0,
    duration: 0,
    has_previous: false,
    has_next: false,
    error: '',
    has_output: true
  })
})
afterEach(() => {
  w?.unmount()
  w = null
})

async function mountMini(state = null) {
  w = mount(MiniPlayer, { attachTo: document.body })
  await flushPromises()
  if (state) {
    held.playback.emit(state)
    await flushPromises()
  }
  return w
}

const buttons = () => w.findAll('.mini-btn')
const previous = () => buttons()[0]
const play = () => buttons()[1]
const next = () => buttons()[2]

describe('el mini reproductor', () => {
  it('sin nada cargado, todos los mandos estan en gris', async () => {
    await mountMini()
    expect(w.text()).toContain('Nada sonando')
    for (const b of buttons()) expect(b.attributes('disabled')).toBeDefined()
  })

  it('con una cancion cargada se habilitan los mandos', async () => {
    await mountMini(PLAYING)
    expect(w.text()).toContain('Mi Gozo')
    expect(w.text()).toContain('Barak')
    for (const b of buttons()) expect(b.attributes('disabled')).toBeUndefined()
  })

  it('sola en la cola, anterior y siguiente siguen en gris', async () => {
    await mountMini({ ...PLAYING, length: 1, has_previous: false, has_next: false })
    expect(previous().attributes('disabled')).toBeDefined()
    expect(next().attributes('disabled')).toBeDefined()
    expect(play().attributes('disabled')).toBeUndefined()
  })

  it('pausar es una orden directa al audio, sin pasar por la app', async () => {
    await mountMini(PLAYING)
    await play().trigger('click')
    await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalled()
  })

  it('cambiar de cancion se le pide a quien tiene la cola', async () => {
    await mountMini(PLAYING)
    await next().trigger('click')
    expect(held.playback.bridge.next).toHaveBeenCalled()
    await previous().trigger('click')
    expect(held.playback.bridge.previous).toHaveBeenCalled()
  })

  it('el boton ofrece pausar o reproducir segun este el audio de verdad', async () => {
    await mountMini(PLAYING)
    expect(play().attributes('title')).toBe('Pausar')
    held.playback.emit({ playing: false })
    await flushPromises()
    expect(play().attributes('title')).toBe('Reproducir')
  })

  it('se entera de que ha cambiado la cancion sin preguntar', async () => {
    await mountMini(PLAYING)
    held.playback.emit({
      track: { id: 9, title: 'Shekinah', artist: 'New Wine', duration: 180, blur: false }
    })
    await flushPromises()
    expect(w.text()).toContain('Shekinah')
    expect(w.text()).toContain('New Wine')
  })

  it('espacio pausa y Escape la quita de en medio', async () => {
    await mountMini(PLAYING)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: ' ' }))
    await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalled()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(held.mini.hide).toHaveBeenCalled()
  })

  it('la barra avanza con lo que dice el audio, no con lo que creemos', async () => {
    await mountMini({ ...PLAYING, position: 50, duration: 200 })
    expect(w.find('.mini-seek-fill').attributes('style')).toContain('width: 25%')
  })

  it('no se puede arrastrar: la coloca la bandeja cada vez', async () => {
    await mountMini(PLAYING)
    expect(w.find('[data-tauri-drag-region]').exists()).toBe(false)
  })
})
