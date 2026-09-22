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

const base = { index: 0, length: 1, duration: 200, has_previous: false, has_next: false, error: '', has_output: true,
  loop_a: 0, loop_b: 0, pitch_preserved: true, speed: 1, pitch: 0, path: '/musica/mi-gozo.mp3',
  metronome: { on: false, bpm: 100, meter: 4, shift: 0, mult: 0, volume: 0.8, has_grid: false, free: true, confidence: 0 } }

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
const boton = (w, text, nth = 0) => w.findAll('button').filter((b) => b.text().includes(text))[nth]

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
    // el boton lo quita
    await boton(w, 'Quitar').trigger('click')
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

  it('un tramo se guarda como marcador con su inicio y su final, y sus notas', async () => {
    vi.useFakeTimers()
    const w = await montar()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 100)
    await puntero(window, 'pointermove', 200)
    await puntero(window, 'pointerup', 200)
    expect(boton(w, 'Guardar tramo')).toBeTruthy()
    await boton(w, 'Guardar tramo').trigger('click')
    await flushPromises()
    // queda elegido: tramo entero, y sus notas abiertas en su pestaña
    const chip = w.find('.study-marker')
    expect(chip.classes()).toContain('on')
    expect(chip.text()).toContain('0:50 – 1:40')
    expect(chip.text()).toContain('Tramo 1')
    expect(boton(w, 'Guardar tramo'), 'ya guardado: no se ofrece otra vez').toBeUndefined()
    expect(boton(w, 'Guardar como marcador')).toBeUndefined()
    const tabs = w.findAll('.study-tab')
    expect(tabs.map((t) => t.text())).toEqual(['La canción', '«Tramo 1»'])
    expect(tabs[1].classes()).toContain('on')
    expect(w.findAll('textarea'), 'un solo cuadro de notas').toHaveLength(1)
    await w.find('textarea').setValue('entrar tras el redoble')
    await tabs[0].trigger('click')
    await w.find('textarea').setValue('cejilla en 2')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {
      loop: [50, 100],
      markers: [{ t: 50, end: 100, label: 'Tramo 1', notes: 'entrar tras el redoble' }],
      notes: 'cejilla en 2'
    })
    // y en la onda se ve como banda con su banderita
    expect(w.find('.tl-region').attributes('style')).toContain('left: 25%')
    expect(w.find('.tl-region').classes()).toContain('on')
    vi.useRealTimers()
  })

  it('pulsar un marcador pone su bucle y coloca la cancion al principio del tramo', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({
      markers: [{ t: 30, end: 45, label: 'Coro', notes: 'fuerte' }, { t: 120, label: 'Solo' }] }) })]
    const w = await montar()
    expect(w.findAll('.study-marker')).toHaveLength(2)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    vi.clearAllMocks()
    // sonando por 0:12: pulsar «Coro» repite 0:30-0:45 y salta a 0:30 (sigue sonando)
    await w.findAll('.study-pick')[0].trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(30, 45)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(30)
    expect(held.playback.bridge.toggle, 'si sonaba, sigue sonando; si no, se queda lista').not.toHaveBeenCalled()
    expect(w.find('.study-marker').classes()).toContain('on')
    expect(w.findAll('.study-tab').map((t) => t.text())).toContain('«Coro»')
    expect(w.find('.study-tab.on').text()).toBe('«Coro»')
    expect(w.find('textarea').element.value).toBe('fuerte')
    expect(w.text()).toContain('0:30 – 0:45')
    // un instante suelto solo lleva alli, sin tocar el bucle
    vi.clearAllMocks()
    await w.findAll('.study-pick')[1].trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(120)
    expect(held.playback.bridge.setLoop).not.toHaveBeenCalled()
  })

  it('en pausa, elegir un marcador deja la cancion colocada sin arrancarla', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] }) })]
    held.playback.emit({ playing: false })
    const w = await montar()
    vi.clearAllMocks()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(30)
    expect(held.playback.bridge.toggle).not.toHaveBeenCalled()
  })

  it('renombrar es otro boton: pulsar el nombre no abre ningun dialogo', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] }) })]
    const w = await montar()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(document.querySelector('.modal')).toBeNull()
    expect(w.find('.study-marker .field-btn[title^="Renombrar"]').exists()).toBe(true)
  })

  it('con un marcador elegido, mover los bordes del tramo lo cambia a el', async () => {
    vi.useFakeTimers()
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100], markers: [{ t: 50, end: 100, label: 'Coro' }] }) })]
    const w = await montar()
    // el bucle guardado es el del marcador: entra elegido
    expect(w.find('.study-marker').classes()).toContain('on')
    conAnchura(w)
    const caja = w.find('.tl-box').element
    // el borde B (x=200) a x=300 (2:30)
    await puntero(caja, 'pointerdown', 201)
    await puntero(window, 'pointermove', 300)
    await puntero(window, 'pointerup', 300)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(50, 150)
    expect(w.find('.study-marker').text()).toContain('0:50 – 2:30')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy.mock.calls.at(-1)[1].markers).toEqual([{ t: 50, end: 150, label: 'Coro' }])
    // dibujar un tramo nuevo de cero NO toca el marcador: deja de estar elegido
    await puntero(caja, 'pointerdown', 20)
    await puntero(window, 'pointermove', 60)
    await puntero(window, 'pointerup', 60)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(10, 30)
    expect(w.find('.study-marker').classes()).not.toContain('on')
    expect(w.find('.study-marker').text()).toContain('0:50 – 2:30')
    expect(boton(w, 'Guardar tramo')).toBeTruthy()
    vi.useRealTimers()
  })

  it('sin tramo elegido, «aqui» marca el instante por el que va', async () => {
    vi.useFakeTimers()
    const w = await montar()
    await boton(w, 'aquí').trigger('click')
    held.playback.emit({ position: 45.5 })
    await flushPromises()
    await boton(w, 'aquí').trigger('click')
    await flushPromises()
    expect(w.findAll('.study-pick-time').map((j) => j.text())).toEqual(['0:12', '0:45'])
    await vi.advanceTimersByTimeAsync(700)
    const saved = held.api.setStudy.mock.calls.at(-1)[1]
    expect(saved.markers.map((m) => m.t)).toEqual([12, 45.5])
    expect(saved.markers.every((m) => !('end' in m))).toBe(true)
    vi.useRealTimers()
  })

  it('quitar el marcador elegido cierra sus notas', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro', notes: 'x' }] }) })]
    const w = await montar()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(w.find('.study-tab.on').text()).toBe('«Coro»')
    await w.find('.study-marker .field-btn[title="Quitar"]').trigger('click')
    await flushPromises()
    expect(w.findAll('.study-tab').map((t) => t.text())).not.toContain('«Coro»')
    expect(w.find('.study-tab.on').text()).toBe('La canción')
    expect(w.findAll('.study-marker')).toHaveLength(0)
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

describe('tono, velocidad fina y metronomo', () => {
  function conAnchura (w, left = 0, width = 400) {
    w.find('.tl-box').element.getBoundingClientRect = () =>
      ({ left, width, right: left + width, top: 0, height: 80, bottom: 80, x: left, y: 0 })
  }
  const bridge = () => held.playback.bridge

  it('el tono sube y baja por semitonos, enseña el tono resultante y se guarda', async () => {
    vi.useFakeTimers()
    held.playback.emit({ track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, key: 'G' } })
    const w = await montar()
    expect(w.text()).toContain('Tono')
    await boton(w, '+').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(1)
    await boton(w, '+').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(2)
    expect(w.find('.study-pitch-n').text()).toBe('+2')
    expect(w.text()).toContain('G → A')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { pitch: 2 })
    await boton(w, 'original').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(0)
    expect(w.find('.study-pitch-n').text()).toBe('0')
    vi.useRealTimers()
  })

  it('el tono guardado se aplica al entrar y se quita al cerrar, como la velocidad', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ pitch: -3, speed: 0.9 }) })]
    const w = await montar()
    expect(bridge().setPitch).toHaveBeenCalledWith(-3)
    expect(bridge().setSpeed).toHaveBeenCalledWith(0.9)
    await boton(w, 'Cerrar').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(0)
    expect(bridge().setSpeed).toHaveBeenLastCalledWith(1)
    expect(w.emitted('close')).toBeTruthy()
  })

  it('la velocidad tiene deslizador fino ademas de los botones', async () => {
    const w = await montar()
    const sliders = w.findAll('input[type="range"]')
    expect(sliders.length).toBeGreaterThanOrEqual(1)
    await sliders[0].setValue('0.83')
    await flushPromises()
    expect(bridge().setSpeed).toHaveBeenLastCalledWith(0.83)
    expect(w.text()).toContain('83 %')
  })

  it('al entrar se analiza el compas de la cancion que suena y se pinta la rejilla', async () => {
    const w = await montar()
    expect(held.playback.bridge.analyzeBeats).toHaveBeenCalledWith('/musica/mi-gozo.mp3', null)
    // con la rejilla ya en Rust se vuelven a mandar los ajustes para que el clic la coja
    expect(bridge().setMetronome).toHaveBeenCalled()
    expect(w.text()).toContain('sigue la canción')
    expect(w.text()).toContain('el «1» seguro')
    w.unmount()
  })

  it('el clic arranca al compas y se para aparte de la cancion', async () => {
    const w = await montar()
    vi.clearAllMocks()
    await boton(w, 'Clic').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true, bpm: null, meter: null, shift: 0, mult: 0 }))
    expect(w.find('.study-metro-toggle').classes()).toContain('on')
    expect(w.find('.study-bpm b').text()).toBe('120')
    // la cancion sigue: no se ha tocado play ni pausa
    expect(bridge().toggle).not.toHaveBeenCalled()
    await boton(w, 'Parar').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: false }))
    expect(bridge().toggle).not.toHaveBeenCalled()
  })

  it('a otra velocidad el clic enseña el tempo que se oye', async () => {
    const w = await montar()
    held.playback.emit({ speed: 0.5 })
    await flushPromises()
    expect(w.find('.study-bpm b').text()).toBe('60')
  })

  it('subir o bajar los bpm lo pone libre; «el de la cancion» vuelve a seguirla; todo se guarda', async () => {
    vi.useFakeTimers()
    const w = await montar()
    vi.clearAllMocks()
    await boton(w, '+', 1).trigger('click') // el segundo «+» es el del metronomo
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 121 }))
    expect(w.text()).toContain('tempo a mano')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { metronome: { bpm: 121 } })
    await boton(w, 'el de la canción').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: null }))
    expect(w.text()).toContain('sigue la canción')
    // compas, el «1» corrido y el doble de pulsos
    await boton(w, '4/4').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ meter: 3 }))
    await boton(w, 'el 1 es el siguiente').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ shift: 1 }))
    await boton(w, '×2').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ mult: 1 }))
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { metronome: { meter: 3, shift: 1, mult: 1 } })
    vi.useRealTimers()
  })

  it('lo ajustado a mano vuelve con la cancion, y al cambiar de cancion se analiza la nueva', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { meter: 3, shift: 2 } }) }),
                        song(8, { title: 'Otra' })]
    await montar()
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ meter: 3, shift: 2 }))
    vi.clearAllMocks()
    held.playback.emit({ track: { id: 8, title: 'Otra', artist: 'Barak', duration: 100 }, path: '/musica/otra.mp3', position: 0 })
    await flushPromises(); await flushPromises()
    expect(held.playback.bridge.analyzeBeats).toHaveBeenCalledWith('/musica/otra.mp3', null)
    // la nueva no tiene ajustes: se vuelve a lo detectado
    expect(bridge().setMetronome).toHaveBeenCalledWith(expect.objectContaining({ meter: null, shift: 0 }))
  })

  it('si no se puede analizar, el clic va libre y lo dice', async () => {
    held.playback.bridge.analyzeBeats.mockRejectedValueOnce(new Error('no se encuentra un pulso estable'))
    const w = await montar()
    expect(w.text()).toContain('sin compás detectado: va libre')
    vi.clearAllMocks()
    await boton(w, 'Clic').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
  })

  it('el volumen del clic tiene su deslizador', async () => {
    const w = await montar()
    const vol = w.find('.study-metro-vol input[type="range"]')
    await vol.setValue('0.35')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ volume: 0.35 }))
  })

  it('la onda recibe la rejilla y es mas alta', async () => {
    const w = await montar()
    conAnchura(w)
    expect(w.find('.tl').attributes('style')).toContain('--tl-h: 80px')
    expect(w.findComponent({ name: 'StudyTimeline' }).props('grid')?.bpm).toBe(120)
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
