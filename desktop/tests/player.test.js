import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// La barra de reproduccion: la aguja se arrastra, hay un boton para empezar
// de cero y los atajos de teclado hacen lo que dicen los tooltips.
const held = vi.hoisted(() => ({ playback: null, api: null, state: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: held.playback.bridge,
    projection: { show: vi.fn(async () => {}) }
  }
})

import Player from '../src/components/Player.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'

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
  volume: 0.9
}
const TRACK = { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 }

let w = null
beforeEach(() => {
  vi.clearAllMocks()
  resetPlayback()
  held.playback.reset()
  document.body.innerHTML = ''
})
afterEach(() => {
  w?.unmount()
  w = null
})

async function mountView(patch = {}) {
  w = mount(Player, { attachTo: document.body })
  await flushPromises()
  held.playback.emit({ track: TRACK, playing: true, position: 40, ...base, ...patch })
  await flushPromises()
  return w
}
const bridge = () => held.playback.bridge

// jsdom no maqueta: la barra dice que va de x=0 a x=400
function withWidth(el, left = 0, width = 400) {
  el.element.getBoundingClientRect = () => ({
    left,
    width,
    right: left + width,
    top: 0,
    height: 4,
    bottom: 4,
    x: left,
    y: 0
  })
}
function pointer(target, type, x, extra = {}) {
  target.dispatchEvent(
    new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: 2, ...extra })
  )
  return flushPromises()
}
const press = (key, extra = {}) => {
  window.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })
  )
  return flushPromises()
}
const padding = () => w.find('.track-fill').attributes('style')
const currentTime = () => w.findAll('.pl-bar .time')[0].text()

describe('the seek bar handle', () => {
  it('is grabbed and follows the pointer without requesting anything until released', async () => {
    await mountView()
    const bar = w.find('.track')
    withWidth(bar)
    await pointer(bar.element, 'pointerdown', 100)
    expect(bar.classes()).toContain('scrubbing')
    expect(padding()).toContain('width: 25%')
    expect(currentTime()).toBe('0:50')
    await pointer(window, 'pointermove', 300)
    expect(padding()).toContain('width: 75%')
    expect(currentTime()).toBe('2:30')
    expect(bridge().seek).not.toHaveBeenCalled()
    await pointer(window, 'pointerup', 300)
    expect(bridge().seek).toHaveBeenCalledTimes(1)
    expect(bridge().seek).toHaveBeenCalledWith(150)
    expect(bar.classes()).not.toContain('scrubbing')
    // y la aguja se queda donde se solto, sin esperar a Rust
    expect(padding()).toContain('width: 75%')
  })

  it('a click without moving jumps to that point, as before', async () => {
    await mountView()
    const bar = w.find('.track')
    withWidth(bar)
    await pointer(bar.element, 'pointerdown', 200)
    await pointer(window, 'pointerup', 200)
    expect(bridge().seek).toHaveBeenCalledWith(100)
  })

  it('dragging past the ends stays at 0 or at the end', async () => {
    await mountView()
    const bar = w.find('.track')
    withWidth(bar)
    await pointer(bar.element, 'pointerdown', 100)
    await pointer(window, 'pointermove', -50)
    expect(padding()).toContain('width: 0%')
    await pointer(window, 'pointermove', 900)
    expect(currentTime()).toBe('3:20')
    await pointer(window, 'pointerup', 900)
    expect(bridge().seek).toHaveBeenCalledWith(200)
  })

  it('an old tick from Rust does not move the handle back after release', async () => {
    await mountView()
    const bar = w.find('.track')
    withWidth(bar)
    bridge().seek.mockImplementationOnce(async () => {})
    await pointer(bar.element, 'pointerdown', 300)
    await pointer(window, 'pointerup', 300)
    held.playback.emit({ position: 40.3 })
    await flushPromises()
    expect(currentTime()).toBe('2:30')
  })

  it('without a song the bar does nothing', async () => {
    await mountView({ track: null, duration: 0, playing: false })
    const bar = w.find('.track')
    withWidth(bar)
    await pointer(bar.element, 'pointerdown', 100)
    expect(bar.classes()).not.toContain('scrubbing')
    await pointer(window, 'pointerup', 100)
    expect(bridge().seek).not.toHaveBeenCalled()
  })

  it('the secondary button does not grab it', async () => {
    await mountView()
    const bar = w.find('.track')
    withWidth(bar)
    await pointer(bar.element, 'pointerdown', 100, { button: 2 })
    expect(bar.classes()).not.toContain('scrubbing')
  })
})

describe('from the start', () => {
  it('there is a button on the time bar, before the time', async () => {
    await mountView()
    const b = w.find('.pl-bar .pl-restart')
    expect(b.exists()).toBe(true)
    expect(b.attributes('title')).toContain('Desde el principio')
    expect(b.attributes('disabled')).toBeUndefined()
    await b.trigger('click')
    await flushPromises()
    expect(bridge().seek).toHaveBeenCalledWith(0)
    // la cola sigue en la barra, con los de la derecha
    expect(w.find('.player .pl-extra button[title="Cola de reproducción"]').exists()).toBe(true)
  })

  it('without a song the button is disabled', async () => {
    await mountView({ track: null, duration: 0, playing: false })
    expect(w.find('.pl-restart').attributes('disabled')).toBeDefined()
  })
})

describe('keyboard shortcuts', () => {
  it('Space pauses and resumes', async () => {
    await mountView()
    await press(' ')
    expect(bridge().toggle).toHaveBeenCalledTimes(1)
  })

  it('0 and Home go back to the start', async () => {
    await mountView()
    await press('0')
    expect(bridge().seek).toHaveBeenLastCalledWith(0)
    held.playback.emit({ position: 90 })
    await press('Home')
    expect(bridge().seek).toHaveBeenLastCalledWith(0)
  })

  it('when paused, 0 also starts playing', async () => {
    await mountView({ playing: false })
    await press('0')
    expect(bridge().toggle).toHaveBeenCalledTimes(1)
    expect(bridge().seek).toHaveBeenLastCalledWith(0)
  })

  it('1-9 jump to the percentage', async () => {
    await mountView()
    await press('5')
    expect(bridge().seek).toHaveBeenLastCalledWith(100)
    await press('9')
    expect(bridge().seek).toHaveBeenLastCalledWith(180)
  })

  it('arrows: 5 s; with Ctrl 10 s; with Shift 30 s', async () => {
    await mountView()
    await press('ArrowRight')
    expect(bridge().seek).toHaveBeenLastCalledWith(45)
    await press('ArrowLeft', { ctrlKey: true })
    expect(bridge().seek).toHaveBeenLastCalledWith(35)
    await press('ArrowRight', { shiftKey: true })
    expect(bridge().seek).toHaveBeenLastCalledWith(65)
    await press('ArrowLeft')
    expect(bridge().seek).toHaveBeenLastCalledWith(60)
  })

  it('up and down arrows change the volume', async () => {
    await mountView()
    await press('ArrowUp')
    expect(bridge().setVolume).toHaveBeenLastCalledWith(0.95)
    await press('ArrowDown')
    expect(bridge().setVolume).toHaveBeenLastCalledWith(0.9)
  })

  it('with focus in a text field they do nothing', async () => {
    await mountView()
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    input.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }))
    await flushPromises()
    expect(bridge().toggle).not.toHaveBeenCalled()
  })
})

describe('the cover of what is playing', () => {
  it('if blurred, here too', async () => {
    await mountView({ track: { ...TRACK, blur: true } })
    expect(w.find('.pl-cover').classes()).toContain('blurred')
  })

  it('the volume has a name for those who cannot see the screen', async () => {
    await mountView()
    const vol = w.find('.pl-volume input[type="range"]')
    expect(vol.attributes('aria-label')).toBe('Volumen')
    expect(vol.attributes('aria-valuetext')).toBe('90 %')
  })
})
