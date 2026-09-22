import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
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

// Las barras montadas se desmontan siempre al acabar, aunque la prueba
// falle a medias: una que se quedara pegada al body seguiria escuchando el
// teclado y se llevaria las teclas de la siguiente.
const mounted = []
const montar = async () => {
  const w = mount(StudyBar, { attachTo: document.body })
  mounted.push(w)
  await flushPromises(); await flushPromises()
  return w
}
afterEach(() => {
  for (const w of mounted.splice(0)) {
    try { w.unmount() } catch { /* ya estaba desmontada */ }
  }
})
const boton = (w, text) => w.findAll('button').find((b) => b.text().includes(text))

describe('la barra de estudio', () => {
  // jsdom no maqueta: la caja de la onda dice que va de x=0 a x=400
  function conAnchura (w, left = 0, width = 400) {
    w.find('.tl-box').element.getBoundingClientRect = () =>
      ({ left, width, right: left + width, top: 0, height: 56, bottom: 56, x: left, y: 0 })
  }
  function puntero (target, tipo, x, extra = {}) {
    target.dispatchEvent(new MouseEvent(tipo, { bubbles: true, cancelable: true, clientX: x, clientY: 20, ...extra }))
    return flushPromises()
  }
  const tecla = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    return flushPromises()
  }

  it('el tramo se elige arrastrando sobre la onda, y se guarda', async () => {
    vi.useFakeTimers()
    const w = await montar()
    expect(w.text()).toContain('Barak — Mi Gozo')
    expect(held.api.waveform).toHaveBeenCalledWith(7)
    conAnchura(w)
    const caja = w.find('.tl-box').element
    // de 0:50 (x=100) a 1:40 (x=200), sobre una cancion de 200 s
    await puntero(caja, 'pointerdown', 100)
    await puntero(window, 'pointermove', 150)
    expect(w.find('.tl-loop').exists(), 'mientras se arrastra ya se ve el tramo').toBe(true)
    expect(w.findAll('.tl-time').map((t) => t.text())).toEqual(['0:50', '1:15'])
    await puntero(window, 'pointermove', 200)
    await puntero(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(50, 100)
    // la cancion iba por 0:12, fuera del tramo: salta a A
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(50)
    expect(w.text()).toContain('0:50 – 1:40')
    const tramo = w.find('.tl-loop')
    expect(tramo.attributes('style')).toContain('left: 25%')
    expect(tramo.attributes('style')).toContain('width: 25%')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenCalledWith(7, { loop: [50, 100] })
    // el chip lo quita
    await boton(w, 'quitar').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(w.find('.tl-loop').exists()).toBe(false)
    vi.useRealTimers()
    w.unmount()
  })

  it('arrastrar hacia la izquierda tambien vale, y de derecha a izquierda es el mismo tramo', async () => {
    const w = await montar()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 200)
    await puntero(window, 'pointermove', 100)
    await puntero(window, 'pointerup', 100)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(50, 100)
    w.unmount()
  })

  it('un clic sin arrastrar lleva la cancion ahi y no toca el bucle', async () => {
    const w = await montar()
    vi.clearAllMocks() // al entrar se aplica lo guardado (sin bucle): eso no cuenta
    conAnchura(w)
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 300)
    await puntero(window, 'pointerup', 301)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(150.5)
    expect(held.playback.bridge.setLoop).not.toHaveBeenCalled()
    w.unmount()
  })

  it('un tramo de menos de medio segundo no es un bucle', async () => {
    const w = await montar()
    vi.clearAllMocks()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    // 0,5 s son un pixel a esta escala: cinco pixeles son 2,5 s, pero ocho
    // decimas de pixel no llegan
    await puntero(caja, 'pointerdown', 100)
    await puntero(window, 'pointermove', 100.8)
    await puntero(window, 'pointerup', 100.8)
    expect(held.playback.bridge.setLoop).not.toHaveBeenCalled()
    w.unmount()
  })

  it('los bordes del tramo se cogen y se mueven; el tramo entero tambien', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100] }) })]
    const w = await montar()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    expect(w.find('.tl-loop').attributes('style')).toContain('left: 25%')
    // el borde B esta en x=200: se lleva a x=300 (2:30)
    await puntero(caja, 'pointerdown', 201)
    await puntero(window, 'pointermove', 300)
    await puntero(window, 'pointerup', 300)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(50, 150)
    // el borde A (x=100) a x=80 (0:40)
    await puntero(caja, 'pointerdown', 99)
    await puntero(window, 'pointermove', 80)
    await puntero(window, 'pointerup', 80)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(40, 150)
    // por dentro (x=150) se arrastra el tramo entero 20 px = 10 s a la derecha
    await puntero(caja, 'pointerdown', 150)
    await puntero(window, 'pointermove', 170)
    await puntero(window, 'pointerup', 170)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(50, 160)
    // y no se sale de la cancion: 110 s de largo, como mucho hasta 200
    await puntero(caja, 'pointerdown', 250)
    await puntero(window, 'pointermove', 390)
    await puntero(window, 'pointerup', 390)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(90, 200)
    w.unmount()
  })

  it('un borde no puede pasar al otro', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100] }) })]
    const w = await montar()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 200)
    await puntero(window, 'pointermove', 50)
    await puntero(window, 'pointerup', 50)
    const [a, b] = held.playback.bridge.setLoop.mock.calls.at(-1)
    expect(a).toBe(50)
    expect(b).toBe(50.5)
    w.unmount()
  })

  it('cerca de un marcador, el borde se pega a el', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ markers: [{ t: 60, label: 'coro' }] }) })]
    const w = await montar()
    conAnchura(w)
    expect(w.find('.tl-marker-name').text()).toBe('coro')
    const caja = w.find('.tl-box').element
    // el coro esta en x=120; se suelta en x=124, a 4 px
    await puntero(caja, 'pointerdown', 50)
    await puntero(window, 'pointermove', 124)
    await puntero(window, 'pointerup', 124)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(25, 60)
    // y pulsar el marcador lleva alli
    await w.find('.tl-marker').trigger('click')
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(60)
    w.unmount()
  })

  it('las teclas A y B marcan el tramo mientras suena', async () => {
    vi.useFakeTimers()
    const w = await montar()
    await tecla('a')
    expect(w.text()).toContain('A en 0:12')
    held.playback.emit({ position: 20 })
    await flushPromises()
    await tecla('b')
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(12, 20)
    expect(w.text()).toContain('0:12 – 0:20')
    // con el bucle puesto, A lo acorta por delante y B lo alarga por detras
    held.playback.emit({ position: 15 })
    await flushPromises()
    await tecla('a')
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(15, 20)
    held.playback.emit({ position: 30 })
    await flushPromises()
    await tecla('b')
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(15, 30)
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { loop: [15, 30] })
    vi.useRealTimers()
    w.unmount()
  })

  it('la regla lleva los minutos y el final', async () => {
    const w = await montar()
    const labels = w.findAll('.tl-tick-label').map((t) => t.text())
    // 200 s con 800 px de ancho supuesto: una marca con numero cada 30 s
    expect(labels).toEqual(['0:00', '0:30', '1:00', '1:30', '2:00', '2:30', '3:00', '3:20'])
    expect(w.findAll('.tl-tick.minor').length).toBeGreaterThan(0)
    w.unmount()
  })

  it('sin forma de onda (sin ffmpeg) la linea de tiempo sigue sirviendo', async () => {
    held.api.waveform.mockRejectedValueOnce(new Error('501: hace falta ffmpeg'))
    const w = await montar()
    expect(w.text()).toContain('sin forma de onda')
    conAnchura(w)
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 100)
    await puntero(window, 'pointermove', 200)
    await puntero(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(50, 100)
    w.unmount()
  })

  it('ya no hay botones de inicio y fin', async () => {
    const w = await montar()
    expect(boton(w, 'Marcar A')).toBeUndefined()
    expect(boton(w, 'Marcar B')).toBeUndefined()
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
