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
  return { ...actual, inTauri: true, api: held.api, playback: held.playback.bridge,
           projection: { show: vi.fn(async () => {}) } }
})

import Player from '../src/components/Player.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'

const base = { index: 0, length: 1, duration: 200, has_previous: false, has_next: false,
  error: '', has_output: true, loop_a: 0, loop_b: 0, pitch_preserved: true, speed: 1, volume: 0.9 }
const TRACK = { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 }

let w = null
beforeEach(() => {
  vi.clearAllMocks()
  resetPlayback()
  held.playback.reset()
  document.body.innerHTML = ''
})
afterEach(() => { w?.unmount(); w = null })

async function montar (patch = {}) {
  w = mount(Player, { attachTo: document.body })
  await flushPromises()
  held.playback.emit({ track: TRACK, playing: true, position: 40, ...base, ...patch })
  await flushPromises()
  return w
}
const bridge = () => held.playback.bridge

// jsdom no maqueta: la barra dice que va de x=0 a x=400
function conAnchura (el, left = 0, width = 400) {
  el.element.getBoundingClientRect = () =>
    ({ left, width, right: left + width, top: 0, height: 4, bottom: 4, x: left, y: 0 })
}
function puntero (target, tipo, x, extra = {}) {
  target.dispatchEvent(new MouseEvent(tipo, { bubbles: true, cancelable: true, clientX: x, clientY: 2, ...extra }))
  return flushPromises()
}
const tecla = (key, extra = {}) => {
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra }))
  return flushPromises()
}
const relleno = () => w.find('.track-fill').attributes('style')
const horaActual = () => w.findAll('.pl-bar .time')[0].text()

describe('la aguja de la barra de tiempo', () => {
  it('se coge y sigue al puntero sin pedir nada hasta soltar', async () => {
    await montar()
    const barra = w.find('.track')
    conAnchura(barra)
    await puntero(barra.element, 'pointerdown', 100)
    expect(barra.classes()).toContain('scrubbing')
    expect(relleno()).toContain('width: 25%')
    expect(horaActual()).toBe('0:50')
    await puntero(window, 'pointermove', 300)
    expect(relleno()).toContain('width: 75%')
    expect(horaActual()).toBe('2:30')
    expect(bridge().seek).not.toHaveBeenCalled()
    await puntero(window, 'pointerup', 300)
    expect(bridge().seek).toHaveBeenCalledTimes(1)
    expect(bridge().seek).toHaveBeenCalledWith(150)
    expect(barra.classes()).not.toContain('scrubbing')
    // y la aguja se queda donde se solto, sin esperar a Rust
    expect(relleno()).toContain('width: 75%')
  })

  it('un clic sin mover salta a ese punto, como antes', async () => {
    await montar()
    const barra = w.find('.track')
    conAnchura(barra)
    await puntero(barra.element, 'pointerdown', 200)
    await puntero(window, 'pointerup', 200)
    expect(bridge().seek).toHaveBeenCalledWith(100)
  })

  it('arrastrar mas alla de los extremos se queda en 0 o en el final', async () => {
    await montar()
    const barra = w.find('.track')
    conAnchura(barra)
    await puntero(barra.element, 'pointerdown', 100)
    await puntero(window, 'pointermove', -50)
    expect(relleno()).toContain('width: 0%')
    await puntero(window, 'pointermove', 900)
    expect(horaActual()).toBe('3:20')
    await puntero(window, 'pointerup', 900)
    expect(bridge().seek).toHaveBeenCalledWith(200)
  })

  it('un tick viejo de Rust no devuelve la aguja atras tras soltar', async () => {
    await montar()
    const barra = w.find('.track')
    conAnchura(barra)
    bridge().seek.mockImplementationOnce(async () => {})
    await puntero(barra.element, 'pointerdown', 300)
    await puntero(window, 'pointerup', 300)
    held.playback.emit({ position: 40.3 })
    await flushPromises()
    expect(horaActual()).toBe('2:30')
  })

  it('sin cancion la barra no hace nada', async () => {
    await montar({ track: null, duration: 0, playing: false })
    const barra = w.find('.track')
    conAnchura(barra)
    await puntero(barra.element, 'pointerdown', 100)
    expect(barra.classes()).not.toContain('scrubbing')
    await puntero(window, 'pointerup', 100)
    expect(bridge().seek).not.toHaveBeenCalled()
  })

  it('con el boton secundario no se coge', async () => {
    await montar()
    const barra = w.find('.track')
    conAnchura(barra)
    await puntero(barra.element, 'pointerdown', 100, { button: 2 })
    expect(barra.classes()).not.toContain('scrubbing')
  })
})

describe('desde el principio', () => {
  it('hay un boton en la barra de tiempo, antes de la hora', async () => {
    await montar()
    const b = w.find('.pl-bar .pl-restart')
    expect(b.exists()).toBe(true)
    expect(b.attributes('title')).toContain('Desde el principio')
    expect(b.attributes('disabled')).toBeUndefined()
    await b.trigger('click')
    await flushPromises()
    expect(bridge().seek).toHaveBeenCalledWith(0)
    // la cola sigue siendo el ultimo boton del reproductor
    expect(w.findAll('.player .pl-btn').at(-1).attributes('title')).toContain('Cola')
  })

  it('sin cancion el boton esta apagado', async () => {
    await montar({ track: null, duration: 0, playing: false })
    expect(w.find('.pl-restart').attributes('disabled')).toBeDefined()
  })
})

describe('atajos de teclado', () => {
  it('espacio pausa y reanuda', async () => {
    await montar()
    await tecla(' ')
    expect(bridge().toggle).toHaveBeenCalledTimes(1)
  })

  it('0 e Inicio vuelven al principio', async () => {
    await montar()
    await tecla('0')
    expect(bridge().seek).toHaveBeenLastCalledWith(0)
    held.playback.emit({ position: 90 })
    await tecla('Home')
    expect(bridge().seek).toHaveBeenLastCalledWith(0)
  })

  it('en pausa, 0 ademas arranca', async () => {
    await montar({ playing: false })
    await tecla('0')
    expect(bridge().toggle).toHaveBeenCalledTimes(1)
    expect(bridge().seek).toHaveBeenLastCalledWith(0)
  })

  it('1-9 van al tanto por ciento', async () => {
    await montar()
    await tecla('5')
    expect(bridge().seek).toHaveBeenLastCalledWith(100)
    await tecla('9')
    expect(bridge().seek).toHaveBeenLastCalledWith(180)
  })

  it('flechas: 5 s; con Ctrl 10 s; con Mayus 30 s', async () => {
    await montar()
    await tecla('ArrowRight')
    expect(bridge().seek).toHaveBeenLastCalledWith(45)
    await tecla('ArrowLeft', { ctrlKey: true })
    expect(bridge().seek).toHaveBeenLastCalledWith(35)
    await tecla('ArrowRight', { shiftKey: true })
    expect(bridge().seek).toHaveBeenLastCalledWith(65)
    await tecla('ArrowLeft')
    expect(bridge().seek).toHaveBeenLastCalledWith(60)
  })

  it('las flechas arriba y abajo mueven el volumen', async () => {
    await montar()
    await tecla('ArrowUp')
    expect(bridge().setVolume).toHaveBeenLastCalledWith(0.95)
    await tecla('ArrowDown')
    expect(bridge().setVolume).toHaveBeenLastCalledWith(0.9)
  })

  it('con el foco en un campo de texto no actuan', async () => {
    await montar()
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    input.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }))
    await flushPromises()
    expect(bridge().toggle).not.toHaveBeenCalled()
  })
})
