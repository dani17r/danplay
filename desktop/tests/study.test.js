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
import { useContextMenu } from '../src/composables/useContextMenu.js'
import { song } from './support/backend.js'

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

beforeEach(() => {
  vi.clearAllMocks()
  vi.useRealTimers()
  localStorage.clear() // el candado de la onda y el paso del tono, como de fabrica
  resetPlayback()
  held.playback.reset()
  held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
  held.playback.emit({
    track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
    playing: true,
    position: 12,
    ...base
  })
})

// Las barras montadas se desmontan siempre al acabar, aunque la prueba
// falle a medias: una que se quedara pegada al body seguiria escuchando el
// teclado y se llevaria las teclas de la siguiente.
const mounted = []
const montar = async () => {
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
})
const boton = (w, text, nth = 0) => w.findAll('button').filter((b) => b.text().includes(text))[nth]

describe('la barra de estudio', () => {
  // jsdom no maqueta: la caja de la onda dice que va de x=0 a x=400
  function conAnchura(w, left = 0, width = 400) {
    w.find('.tl-box').element.getBoundingClientRect = () => ({
      left,
      width,
      right: left + width,
      top: 0,
      height: 56,
      bottom: 56,
      x: left,
      y: 0
    })
  }
  function puntero(target, tipo, x, extra = {}) {
    target.dispatchEvent(
      new MouseEvent(tipo, { bubbles: true, cancelable: true, clientX: x, clientY: 20, ...extra })
    )
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
    // con el candado (puesto de entrada) y sonando, no salta al tramo: la
    // cancion sigue por 0:12 y el tramo entra cuando llegue
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
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

  it('un clic sin arrastrar lleva la cancion ahi y no toca el bucle (sin candado)', async () => {
    const w = await montar()
    vi.clearAllMocks() // al entrar se aplica lo guardado (sin bucle): eso no cuenta
    conAnchura(w)
    await w.find('.tl-lock').trigger('click')
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 300)
    await puntero(window, 'pointerup', 301)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(150.5)
    expect(held.playback.bridge.setLoop).not.toHaveBeenCalled()
    w.unmount()
  })

  it('con el candado, sonando, un clic no mueve la cancion y el candado lo avisa', async () => {
    vi.useFakeTimers()
    const w = await montar()
    vi.clearAllMocks()
    conAnchura(w)
    const candado = w.find('.tl-lock')
    expect(candado.classes(), 'puesto de entrada').toContain('on')
    expect(candado.attributes('aria-pressed')).toBe('true')
    expect(w.find('.study-hint').text()).toContain('con el candado')
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 300)
    await puntero(window, 'pointerup', 301)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(0)
    expect(w.find('.tl-lock').classes(), 'se sacude para decir por que').toContain('nudged')
    await vi.advanceTimersByTimeAsync(800)
    expect(w.find('.tl-lock').classes()).not.toContain('nudged')
    // en pausa no hay nada que estropear: el clic coloca la cancion
    held.playback.emit({ playing: false })
    await flushPromises()
    await puntero(caja, 'pointerdown', 100)
    await puntero(window, 'pointerup', 100)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(50)
    // quitarlo se recuerda para la proxima vez
    await w.find('.tl-lock').trigger('click')
    expect(w.find('.tl-lock').classes()).not.toContain('on')
    expect(localStorage.getItem('danplay.study.lock')).toBe('false')
    vi.useRealTimers()
    w.unmount()
  })

  it('sin candado, elegir un tramo por fuera de donde va salta a A', async () => {
    localStorage.setItem('danplay.study.lock', 'false')
    const w = await montar()
    vi.clearAllMocks()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    await puntero(caja, 'pointerdown', 100)
    await puntero(window, 'pointermove', 200)
    await puntero(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(50, 100)
    // la cancion iba por 0:12, fuera del tramo: salta a A
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(50)
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
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ markers: [{ t: 60, label: 'coro' }] }) })
    ]
    const w = await montar()
    conAnchura(w)
    expect(w.find('.tl-marker-name').text()).toBe('coro')
    const caja = w.find('.tl-box').element
    // el coro esta en x=120; se suelta en x=124, a 4 px
    await puntero(caja, 'pointerdown', 50)
    await puntero(window, 'pointermove', 124)
    await puntero(window, 'pointerup', 124)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(25, 60)
    // con el candado, sonando, pulsar el marcador en la onda lo elige sin mover la cancion
    held.playback.bridge.seek.mockClear()
    await w.find('.tl-marker').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    expect(w.find('.study-marker').classes()).toContain('on')
    // en pausa, pulsarlo lleva alli
    held.playback.emit({ playing: false })
    await flushPromises()
    await w.find('.tl-marker').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(60)
    w.unmount()
  })

  it('con el candado, un tramo de la onda se pone sin saltar; desde la lista si salta', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] })
      })
    ]
    const w = await montar()
    vi.clearAllMocks()
    await w.find('.tl-marker').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(30, 45)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(30)
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
    // B cierra el tramo y vuelve a A, con candado o sin el: es la repeticion A-B
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(12)
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
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          markers: [
            { t: 30, end: 45, label: 'Coro', notes: 'fuerte' },
            { t: 120, label: 'Solo' }
          ]
        })
      })
    ]
    const w = await montar()
    expect(w.findAll('.study-marker')).toHaveLength(2)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    vi.clearAllMocks()
    // sonando por 0:12: pulsar «Coro» repite 0:30-0:45 y salta a 0:30 (sigue sonando)
    await w.findAll('.study-pick')[0].trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(30, 45)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(30)
    expect(
      held.playback.bridge.toggle,
      'si sonaba, sigue sonando; si no, se queda lista'
    ).not.toHaveBeenCalled()
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
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await montar()
    vi.clearAllMocks()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(30)
    expect(held.playback.bridge.toggle).not.toHaveBeenCalled()
  })

  it('renombrar es otro boton: pulsar el nombre no abre ningun dialogo', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] })
      })
    ]
    const w = await montar()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(document.querySelector('.modal')).toBeNull()
    expect(w.find('.study-marker .field-btn[title^="Renombrar"]').exists()).toBe(true)
  })

  it('con un marcador elegido, mover los bordes del tramo lo cambia a el', async () => {
    vi.useFakeTimers()
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ loop: [50, 100], markers: [{ t: 50, end: 100, label: 'Coro' }] })
      })
    ]
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
    expect(held.api.setStudy.mock.calls.at(-1)[1].markers).toEqual([
      { t: 50, end: 150, label: 'Coro' }
    ])
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
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro', notes: 'x' }] })
      })
    ]
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
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ loop: [30, 40], speed: 0.8, notes: 'cejilla 2' })
      })
    ]
    const w = await montar()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(30, 40)
    expect(held.playback.bridge.setSpeed).toHaveBeenCalledWith(0.8)
    expect(w.find('textarea').element.value).toBe('cejilla 2')
    await boton(w, '').trigger('click') // el primer boton de la cabecera es cerrar
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(held.playback.bridge.setSpeed).toHaveBeenLastCalledWith(1)
    expect(w.emitted('close')).toBeTruthy()
    w.unmount()
  })

  it('un clic en la onda quita el tramo; con el candado, sonando, sin mover la cancion', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100] }) })]
    const w = await montar()
    vi.clearAllMocks()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    // con el candado (puesto de entrada) y sonando: se quita y la cancion
    // sigue donde iba; habia algo que quitar, asi que el candado no se queja
    await puntero(caja, 'pointerdown', 300)
    await puntero(window, 'pointerup', 300)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(w.find('.tl-loop').exists()).toBe(false)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    expect(w.find('.tl-lock').classes()).not.toContain('nudged')
    // sin candado, un clic (tambien dentro del tramo) lo quita y lleva alli
    await w.find('.tl-lock').trigger('click')
    await puntero(caja, 'pointerdown', 100)
    await puntero(window, 'pointermove', 200)
    await puntero(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(50, 100)
    await puntero(caja, 'pointerdown', 150)
    await puntero(window, 'pointerup', 150)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(75)
  })

  it('con «varios», cada arrastre añade un tramo, se repiten seguidos y un clic quita uno', async () => {
    vi.useFakeTimers()
    const w = await montar()
    vi.clearAllMocks()
    conAnchura(w)
    const caja = w.find('.tl-box').element
    expect(w.find('.tl-multi').attributes('aria-pressed')).toBe('false')
    await w.find('.tl-multi').trigger('click')
    expect(w.find('.tl-multi').attributes('aria-pressed')).toBe('true')
    expect(w.find('.study-hint').text()).toContain('Arrastra para añadir tramos')
    // 0:20-0:40 y 1:40-2:00
    await puntero(caja, 'pointerdown', 40)
    await puntero(window, 'pointermove', 80)
    await puntero(window, 'pointerup', 80)
    await puntero(caja, 'pointerdown', 200)
    await puntero(window, 'pointermove', 240)
    await puntero(window, 'pointerup', 240)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(20, 120, {
      segments: [
        [20, 40],
        [100, 120]
      ],
      defer: false
    })
    expect(w.findAll('.tl-loop')).toHaveLength(2)
    expect(w.findAll('.tl-loop-n').map((n) => n.text())).toEqual(['1', '2'])
    expect(w.find('.study-loop').text()).toContain('2 tramos · 0:40')
    expect(held.playback.bridge.seek, 'con el candado, sonando, no salta').not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {
      loops: [
        [20, 40],
        [100, 120]
      ]
    })
    // un clic fuera de los tramos no quita nada (con el candado, lo avisa)...
    await puntero(caja, 'pointerdown', 150)
    await puntero(window, 'pointerup', 150)
    expect(w.findAll('.tl-loop')).toHaveLength(2)
    await vi.advanceTimersByTimeAsync(0)
    expect(w.find('.tl-lock').classes()).toContain('nudged')
    // ...y uno sobre el primero lo quita; el otro sigue
    await puntero(caja, 'pointerdown', 60)
    await puntero(window, 'pointerup', 60)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(100, 120)
    expect(w.findAll('.tl-loop')).toHaveLength(1)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  it('varios tramos se guardan como un marcador, y al elegirlo vuelven todos', async () => {
    vi.useFakeTimers()
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          loops: [
            [20, 40],
            [100, 120]
          ]
        })
      })
    ]
    const w = await montar()
    const ambos = {
      segments: [
        [20, 40],
        [100, 120]
      ],
      defer: false
    }
    expect(held.playback.bridge.setLoop, 'lo guardado se aplica al entrar').toHaveBeenCalledWith(
      20,
      120,
      ambos
    )
    expect(w.find('.tl-multi').attributes('aria-pressed'), '«varios» se pone solo').toBe('true')
    await boton(w, 'Guardar tramo').trigger('click')
    await flushPromises()
    const chip = w.find('.study-marker')
    expect(chip.text()).toContain('2 tramos · 0:20…')
    expect(chip.text()).toContain('Tramo 1')
    expect(w.findAll('.tl-region'), 'en la onda, una banda por tramo').toHaveLength(2)
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {
      loops: ambos.segments,
      markers: [{ t: 20, end: 120, parts: ambos.segments, label: 'Tramo 1' }]
    })
    // se quitan, y el marcador los vuelve a poner todos
    await boton(w, 'Quitar').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(20, 120, ambos)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(20)
    vi.useRealTimers()
  })

  it('las opciones de un tramo: sonar ya, repetir al acabar, a los pulsos y quitar', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50.1, 99.9] }) })
    ]
    held.playback.emit({ playing: false })
    const w = await montar()
    vi.clearAllMocks()
    const { menu } = useContextMenu()
    const opcion = (label) => menu.value.items.find((i) => i.label === label)
    await w.find('.tl-loop-menu').trigger('click')
    expect(menu.value.open).toBe(true)
    expect(menu.value.title).toBe('El tramo')
    expect(menu.value.items.filter((i) => i.label).map((i) => i.label)).toEqual([
      'Reproducir ahora',
      'Repetir cuando acabe la canción',
      'Ir aquí',
      'Ajustar a los pulsos',
      'Guardar como marcador',
      'Añadir más tramos',
      'Quitar la selección'
    ])
    // sonar ya: arranca y va al principio del tramo
    await opcion('Reproducir ahora').action()
    await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(50.1)
    // al acabar: la cancion sigue hasta el final y entonces se repite
    await opcion('Repetir cuando acabe la canción').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(50.1, 99.9, {
      segments: [[50.1, 99.9]],
      defer: true
    })
    expect(w.find('.study-loop').text()).toContain('al acabar')
    await w.find('.tl-loop-menu').trigger('click')
    expect(opcion('Repetir ya, sin esperar al final')).toBeTruthy()
    // a los pulsos de la rejilla (uno cada medio segundo desde 0,25), sin
    // perder el «al acabar»
    await opcion('Ajustar a los pulsos').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(50.25, 99.75, {
      segments: [[50.25, 99.75]],
      defer: true
    })
    await w.find('.tl-loop-menu').trigger('click')
    await opcion('Repetir ya, sin esperar al final').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(50.25, 99.75)
    await w.find('.tl-loop-menu').trigger('click')
    await opcion('Quitar la selección').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(w.find('.tl-loop').exists()).toBe(false)
  })

  it('el clic derecho en la onda ofrece ir ahi y, con varios, quitar solo ese tramo', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          loops: [
            [20, 40],
            [100, 120]
          ]
        })
      })
    ]
    const w = await montar()
    vi.clearAllMocks()
    conAnchura(w)
    const { menu } = useContextMenu()
    const opcion = (label) => menu.value.items.find((i) => i.label === label)
    // fuera de los tramos: lo que se puede hacer en ese punto
    await w.find('.tl-box').trigger('contextmenu', { clientX: 300, clientY: 20 })
    expect(menu.value.title).toBe('La onda')
    expect(opcion('Ir aquí').note).toBe('2:30')
    expect(opcion('Reproducir ahora')).toBeUndefined()
    await opcion('Ir aquí').action()
    expect(held.playback.bridge.seek, 'lo pide uno: vale con el candado').toHaveBeenLastCalledWith(
      150
    )
    // sobre el segundo tramo: sus opciones, y quitar solo ese
    await w.find('.tl-box').trigger('contextmenu', { clientX: 220, clientY: 20 })
    expect(menu.value.title).toBe('Tramo 2')
    expect(opcion('Quitar todos los tramos')).toBeTruthy()
    await opcion('Quitar este tramo').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(20, 40)
    expect(w.findAll('.tl-loop')).toHaveLength(1)
  })
})

describe('lo que se guarda va a su cancion', () => {
  it('al abrir con algo ya sonando, lo guardado se lee una sola vez', async () => {
    // lo normal: el reproductor ya sabe que suena cuando se abre la barra
    await usePlayback().ready()
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
      playing: true,
      position: 12,
      ...base
    })
    await flushPromises()
    vi.clearAllMocks()
    await montar()
    expect(held.api.song).toHaveBeenCalledTimes(1)
    expect(held.api.song).toHaveBeenCalledWith(7)
  })

  it('si la cancion cambia antes de guardarse, las notas se guardan en la suya', async () => {
    // El guardado espera 600 ms y leia la cancion y las notas al hacerlo:
    // si entre medias cambiaba la cancion, las notas de la primera acababan
    // en la segunda (en el indice y en la etiqueta del archivo).
    vi.useFakeTimers()
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: '' }),
      song(8, { title: 'Otra', study: '' })
    ]
    const w = await montar()
    await w.find('textarea').setValue('cejilla en 2')
    held.playback.emit({
      track: { id: 8, title: 'Otra', artist: 'Barak', duration: 100 },
      path: '/musica/otra.mp3',
      position: 0
    })
    await flushPromises()
    await flushPromises()
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenCalledWith(7, { notes: 'cejilla en 2' })
    expect(
      held.api.setStudy.mock.calls.filter(([id]) => id === 8),
      'se guardo en la otra'
    ).toEqual([])
    // y la nueva empieza con lo suyo, no con lo de la anterior
    expect(w.find('textarea').element.value).toBe('')
    vi.useRealTimers()
  })

  it('al cerrar se guarda lo pendiente sin esperar', async () => {
    vi.useFakeTimers()
    const w = await montar()
    await w.find('textarea').setValue('entrar tras el redoble')
    await boton(w, 'Cerrar').trigger('click')
    await flushPromises()
    expect(held.api.setStudy).toHaveBeenCalledWith(7, { notes: 'entrar tras el redoble' })
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })
})

describe('tono, velocidad fina y metronomo', () => {
  function conAnchura(w, left = 0, width = 400) {
    w.find('.tl-box').element.getBoundingClientRect = () => ({
      left,
      width,
      right: left + width,
      top: 0,
      height: 80,
      bottom: 80,
      x: left,
      y: 0
    })
  }
  const bridge = () => held.playback.bridge

  it('el tono se cuenta en tonos: de medio en medio (un semitono), enseña el tono resultante y se guarda', async () => {
    vi.useFakeTimers()
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, key: 'G' }
    })
    const w = await montar()
    expect(w.text()).toContain('Tono')
    expect(w.find('.study-step.on').text(), 'de entrada, de medio en medio').toBe('½')
    await w.find('.study-pitch-up').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(1)
    expect(w.find('.study-pitch-n').text()).toBe('+½')
    await w.find('.study-pitch-up').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(2)
    expect(w.find('.study-pitch-n').text(), 'dos semitonos son un tono').toBe('+1')
    expect(w.text()).toContain('G → A')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { pitch: 2 })
    await boton(w, 'original').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(0)
    expect(w.find('.study-pitch-n').text()).toBe('0')
    vi.useRealTimers()
  })

  it('el tono va tambien de cuarto en cuarto de tono y de tono en tono', async () => {
    vi.useFakeTimers()
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, key: 'G' }
    })
    const w = await montar()
    const paso = (n) => w.findAll('.study-step').find((b) => b.text() === n)
    await paso('¼').trigger('click')
    await w.find('.study-pitch-up').trigger('click')
    await flushPromises()
    // un cuarto de tono es medio semitono: no tiene nombre de tonalidad
    expect(bridge().setPitch).toHaveBeenLastCalledWith(0.5)
    expect(w.find('.study-pitch-n').text()).toBe('+¼')
    expect(w.text()).toContain('G → G +¼')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { pitch: 0.5 })
    await paso('1').trigger('click')
    await w.find('.study-pitch-down').trigger('click')
    await w.find('.study-pitch-down').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(-3.5)
    expect(w.find('.study-pitch-n').text()).toBe('−1¾')
    // el paso elegido se recuerda
    expect(localStorage.getItem('danplay.study.pitchStep')).toBe('2')
    vi.useRealTimers()
  })

  it('un tono guardado con cuartos vuelve con la cancion', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ pitch: -0.5 }) })]
    const w = await montar()
    expect(bridge().setPitch).toHaveBeenCalledWith(-0.5)
    expect(w.find('.study-pitch-n').text()).toBe('−¼')
  })

  it('el tono guardado se aplica al entrar y se quita al cerrar, como la velocidad', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ pitch: -3, speed: 0.9 }) })
    ]
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
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(
      expect.objectContaining({ on: true, bpm: null, meter: null, shift: 0, mult: 0 })
    )
    expect(w.find('.study-metro-toggle').classes()).toContain('on')
    expect(w.find('.study-bpm-input input').element.value).toBe('120')
    // la cancion sigue: no se ha tocado play ni pausa
    expect(bridge().toggle).not.toHaveBeenCalled()
    await boton(w, 'Parar').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: false }))
    expect(bridge().toggle).not.toHaveBeenCalled()
  })

  it('a otra velocidad el clic enseña el tempo que se oye, con su decimal', async () => {
    const w = await montar()
    held.playback.emit({ speed: 0.5 })
    await flushPromises()
    expect(w.find('.study-bpm-input input').element.value).toBe('60')
    held.playback.emit({ speed: 0.83 })
    await flushPromises()
    expect(w.find('.study-bpm-input input').element.value).toBe('99.6')
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
    expect(w.find('.study-meters .btn.on').text(), 'el detectado').toBe('4/4')
    await boton(w, '3/4').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ meter: 3 }))
    expect(w.find('.study-meters .btn.on').text()).toBe('3/4')
    await boton(w, 'el 1 es el siguiente').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ shift: 1 }))
    await boton(w, '×2').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ mult: 1 }))
    expect(boton(w, '×2').classes(), 'se ve puesto').toContain('on')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {
      metronome: { meter: 3, shift: 1, mult: 1 }
    })
    vi.useRealTimers()
  })

  it('el tempo se escribe con decimales, y las flechas lo mueven de decima en decima', async () => {
    vi.useFakeTimers()
    const w = await montar()
    vi.clearAllMocks()
    const tempo = w.find('.study-bpm-input input')
    await tempo.trigger('focus')
    await tempo.setValue('90,7')
    await tempo.trigger('keydown', { key: 'Enter' })
    await tempo.trigger('blur')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 90.7 }))
    expect(tempo.element.value).toBe('90.7')
    expect(w.text()).toContain('tempo a mano')
    await tempo.trigger('focus')
    await tempo.trigger('keydown', { key: 'ArrowUp' })
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 90.8 }))
    await tempo.trigger('keydown', { key: 'ArrowDown', shiftKey: true })
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 89.8 }))
    // Escape deja el que habia; lo que no es un numero, tambien
    await tempo.setValue('rapido')
    await tempo.trigger('keydown', { key: 'Escape' })
    await tempo.trigger('blur')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 89.8 }))
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { metronome: { bpm: 89.8 } })
    // la «x» del campo, que solo sale con tempo a mano, vuelve al de la cancion
    await w.find('.study-bpm-input .field-btn').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: null }))
    expect(w.text()).toContain('sigue la canción')
    expect(w.find('.study-bpm-input .field-btn').exists()).toBe(false)
    vi.useRealTimers()
  })

  it('×2 dobla tambien el tempo puesto a mano, y la onda pinta el doble de pulsos', async () => {
    const w = await montar()
    const tempo = w.find('.study-bpm-input input')
    await tempo.trigger('focus')
    await tempo.setValue('68')
    await tempo.trigger('blur')
    await flushPromises()
    expect(tempo.element.value).toBe('68')
    const onda = () => w.findComponent({ name: 'StudyTimeline' }).props('grid')
    const antes = onda().beats.length
    await boton(w, '×2').trigger('click')
    await flushPromises()
    // antes el tempo a mano se quedaba y ×2 no hacia nada
    expect(tempo.element.value).toBe('136')
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(
      expect.objectContaining({ bpm: 68, mult: 1 })
    )
    expect(onda().beats.length).toBe(antes * 2)
    // escribir con el doble puesto guarda el tempo sin el: al quitarlo vuelve
    await tempo.trigger('focus')
    await tempo.setValue('140')
    await tempo.trigger('blur')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 70 }))
    await boton(w, '×2').trigger('click')
    await flushPromises()
    expect(tempo.element.value).toBe('70')
    expect(boton(w, '×2').classes()).not.toContain('on')
  })

  it('sin acento: el compas 0 se guarda y la onda no marca ningun «1»', async () => {
    vi.useFakeTimers()
    const w = await montar()
    await boton(w, 'sin acento').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ meter: 0 }))
    expect(w.findComponent({ name: 'StudyTimeline' }).props('grid').meter).toBe(0)
    expect(boton(w, 'el 1 es el siguiente').attributes('disabled')).toBeDefined()
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { metronome: { meter: 0 } })
    // volver al detectado no se guarda como ajuste
    await boton(w, '4/4').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ meter: null }))
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {})
    vi.useRealTimers()
  })

  it('lo ajustado a mano vuelve con la cancion, y al cambiar de cancion se analiza la nueva', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { meter: 3, shift: 2 } }) }),
      song(8, { title: 'Otra' })
    ]
    await montar()
    expect(bridge().setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ meter: 3, shift: 2 })
    )
    vi.clearAllMocks()
    held.playback.emit({
      track: { id: 8, title: 'Otra', artist: 'Barak', duration: 100 },
      path: '/musica/otra.mp3',
      position: 0
    })
    await flushPromises()
    await flushPromises()
    expect(held.playback.bridge.analyzeBeats).toHaveBeenCalledWith('/musica/otra.mp3', null)
    // la nueva no tiene ajustes: se vuelve a lo detectado
    expect(bridge().setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ meter: null, shift: 0 })
    )
  })

  it('si no se puede analizar, el clic va libre y lo dice', async () => {
    held.playback.bridge.analyzeBeats.mockRejectedValueOnce(
      new Error('no se encuentra un pulso estable')
    )
    const w = await montar()
    expect(w.text()).toContain('sin compás detectado: va libre')
    vi.clearAllMocks()
    await boton(w, 'Clic').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
  })

  it('el volumen del clic tiene su deslizador, hasta el doble', async () => {
    const w = await montar()
    const vol = w.find('.study-metro-vol input[type="range"]')
    expect(vol.attributes('max')).toBe('2')
    await vol.setValue('0.35')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(
      expect.objectContaining({ volume: 0.35 })
    )
    await vol.setValue('1.8')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ volume: 1.8 }))
    expect(w.find('.study-metro-vol').text()).toContain('180 %')
  })

  it('la cancion tambien se puede subir, hasta un 50 % mas', async () => {
    const w = await montar()
    const vol = w.find('.study-song-vol input[type="range"]')
    expect(vol.attributes('max')).toBe('1.5')
    await vol.setValue('1.3')
    await flushPromises()
    expect(bridge().setVolume).toHaveBeenLastCalledWith(1.3)
    expect(w.find('.study-song-vol').text()).toContain('130 %')
    // lo que pasa del 100 % se pinta aparte
    expect(w.find('.study-song-vol .slider-over').exists()).toBe(true)
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

  it('en el navegador el bucle vuelve a A al pasar de B, como en Rust', async () => {
    resetPlayback()
    held.api.inTauri = false
    const player = usePlayback()
    await player.ready()
    const audio = player.audioElement()
    const at = async (t) => {
      Object.defineProperty(audio, 'currentTime', { value: t, writable: true, configurable: true })
      audio.dispatchEvent(new Event('timeupdate'))
      await flushPromises()
    }
    await player.setLoop(10, 20)
    // un tramo por delante de la aguja no hace nada hasta que la cancion entra
    await at(5)
    expect(audio.currentTime).toBe(5)
    await at(15)
    await at(21)
    expect(audio.currentTime).toBe(10)
    expect(player.loopA.value).toBe(10)
    // uno elegido por detras (la onda con candado) no la mueve...
    await player.setLoop(1, 3)
    await at(12)
    expect(audio.currentTime).toBe(12)
    // ...y al acabarse la cancion vuelve a el en vez de pasar a la siguiente
    audio.dispatchEvent(new Event('ended'))
    await flushPromises()
    expect(audio.currentTime).toBe(1)
    expect(player.playing.value).toBe(true)
    await player.clearLoop()
    expect(player.loopB.value).toBe(0)
    held.api.inTauri = true
  })

  it('con varios tramos, el reproductor pinta uno por tramo', async () => {
    const w = mount(Player, { props: { study: false }, attachTo: document.body })
    await flushPromises()
    held.playback.emit({
      loop_a: 20,
      loop_b: 120,
      loops: [
        [20, 40],
        [100, 120]
      ]
    })
    await flushPromises()
    const segs = w.findAll('.track-loop')
    expect(segs).toHaveLength(2)
    expect(segs[1].attributes('style')).toContain('left: 50%')
    expect(segs[1].attributes('style')).toContain('width: 10%')
    w.unmount()
  })

  it('en el navegador, varios tramos van seguidos y «al acabar» espera al final', async () => {
    resetPlayback()
    held.api.inTauri = false
    const player = usePlayback()
    await player.ready()
    const audio = player.audioElement()
    const at = async (t) => {
      Object.defineProperty(audio, 'currentTime', { value: t, writable: true, configurable: true })
      audio.dispatchEvent(new Event('timeupdate'))
      await flushPromises()
    }
    await player.setLoops([
      [10, 20],
      [50, 60]
    ])
    await at(15)
    await at(20.1)
    expect(audio.currentTime, 'del final del primero, al segundo').toBe(50)
    await at(60.2)
    expect(audio.currentTime, 'del ultimo, al primero').toBe(10)
    // al acabar: la cancion pasa por los tramos sin quedarse, y al terminar
    // vuelve al primero y ya se repiten
    await player.setLoops(
      [
        [10, 20],
        [50, 60]
      ],
      { defer: true }
    )
    expect(player.loopDefer.value).toBe(true)
    await at(15)
    await at(21)
    expect(audio.currentTime).toBe(21)
    audio.dispatchEvent(new Event('ended'))
    await flushPromises()
    expect(audio.currentTime).toBe(10)
    expect(player.loopDefer.value).toBe(false)
    await at(20.2)
    expect(audio.currentTime).toBe(50)
    // saltar a proposito dentro de un tramo tambien deja de esperar
    await player.setLoops([[10, 20]], { defer: true })
    await player.seek(12)
    expect(player.loopDefer.value).toBe(false)
    await player.clearLoop()
    expect(player.loops.value).toEqual([])
    held.api.inTauri = true
  })

  it('el volumen del reproductor sube hasta el 150 % y las flechas paran en el 100 %', async () => {
    const w = mount(Player, { props: { study: false }, attachTo: document.body })
    await flushPromises()
    const vol = w.find('.pl-volume input[type="range"]')
    expect(vol.attributes('max')).toBe('1.5')
    await vol.setValue('1.2')
    await flushPromises()
    expect(held.playback.bridge.setVolume).toHaveBeenLastCalledWith(1.2)
    expect(w.find('.pl-volume .slider-over').exists(), 'lo que pasa del 100 %, aparte').toBe(true)
    held.playback.emit({ volume: 0.97 })
    await flushPromises()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }))
    await flushPromises()
    expect(held.playback.bridge.setVolume).toHaveBeenLastCalledWith(1)
    held.playback.emit({ volume: 1 })
    await flushPromises()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }))
    await flushPromises()
    expect(held.playback.bridge.setVolume).toHaveBeenLastCalledWith(1.05)
    w.unmount()
  })
})

describe('las pistas separadas en el estudio', () => {
  // las de la canción 7: seis pistas, con la ruta de cada una
  const conPistas = async (study = '') => {
    const { stemsOf } = await import('./support/backend.js')
    held.state.stems[7] = stemsOf(7)
    held.state.songs = [song(7, { title: 'Mi Gozo', study })]
  }
  const mandado = () => held.playback.bridge.setStems.mock.calls.at(-1)
  const carriles = (w) => w.findAll('.tl-lane').map((l) => l.find('.tl-lane-name').text())

  beforeEach(async () => {
    const { resetSeparation } = await import('../src/composables/useSeparation.js')
    resetSeparation()
    held.state.stems = {}
    held.state.separation.current = null
    held.state.separation.queue = []
  })

  it('sin pistas se ofrece separarla, y al acabar suenan solas', async () => {
    const { finishSeparation } = await import('./support/backend.js')
    const { useSeparation } = await import('../src/composables/useSeparation.js')
    const w = await montar()
    await useSeparation().refresh()
    await flushPromises()
    const separar = boton(w, 'Separar en pistas')
    expect(separar).toBeTruthy()
    await separar.trigger('click')
    const { menu } = useContextMenu()
    expect(menu.value.items.map((i) => i.label)).toEqual(['En 6 pistas', 'En 4 pistas'])
    await menu.value.items[0].action()
    await flushPromises()
    expect(held.api.separate).toHaveBeenCalledWith(7, '6')
    // mientras se separa, se ve cómo va
    expect(w.find('.study-stems-progress').text()).toMatch(/preparando|separando/)
    finishSeparation(held.state)
    await useSeparation().refresh()
    await flushPromises()
    await flushPromises()
    // y en cuanto está, suenan sus pistas: un carril por instrumento
    expect(carriles(w)).toEqual(['Batería', 'Voces', 'Bajo', 'Guitarra', 'Piano', 'Otros'])
    const [path, tracks] = mandado()
    expect(path).toBe('/musica/mi-gozo.mp3')
    expect(tracks.map((t) => t.path.split('/').pop())).toEqual([
      'Bateria.flac',
      'Voces.flac',
      'Bajo.flac',
      'Guitarra.flac',
      'Piano.flac',
      'Otros.flac'
    ])
    expect(tracks.every((t) => t.on && t.gain === 1 && t.pan === 0)).toBe(true)
    expect(w.find('.study').classes()).toContain('with-lanes')
  })

  it('con pistas: el botón las pone y las quita, y se guarda', async () => {
    await conPistas()
    const w = await montar()
    expect(w.find('.tl-lanes').exists()).toBe(false)
    await boton(w, 'Pistas').trigger('click')
    await flushPromises()
    expect(carriles(w)).toHaveLength(6)
    expect(mandado()[1]).toHaveLength(6)
    await new Promise((r) => setTimeout(r, 700)) // el guardado va con retraso
    expect(held.api.setStudy).toHaveBeenLastCalledWith(
      7,
      expect.objectContaining({ mixer: { on: true } })
    )
    await boton(w, 'Pistas').trigger('click')
    await flushPromises()
    expect(w.find('.tl-lanes').exists()).toBe(false)
    expect(mandado()[1]).toBe(null)
  })

  it('callar, dejar sola, volumen y panorama llegan a Rust y se guardan', async () => {
    await conPistas(JSON.stringify({ mixer: { on: true } }))
    const w = await montar()
    expect(carriles(w)).toHaveLength(6)
    const bateria = w.find('[data-lane="drums"]')
    await bateria.findAll('.tl-lane-btn')[0].trigger('click') // M
    await flushPromises()
    let tracks = mandado()[1]
    expect(tracks[0].on).toBe(false)
    expect(tracks.slice(1).every((t) => t.on)).toBe(true)
    expect(bateria.classes()).toContain('off')
    // solo la voz y el bajo: el resto calla
    await w.find('[data-lane="vocals"]').findAll('.tl-lane-btn')[1].trigger('click')
    await w.find('[data-lane="bass"]').findAll('.tl-lane-btn')[1].trigger('click')
    await flushPromises()
    tracks = mandado()[1]
    expect(tracks.map((t) => t.on)).toEqual([false, true, true, false, false, false])
    // el volumen de la voz y a la izquierda
    const [gain, pan] = w.find('[data-lane="vocals"]').findAll('input[type="range"]')
    await gain.setValue('1.5')
    await pan.setValue('-0.5')
    await flushPromises()
    tracks = mandado()[1]
    expect(tracks[1]).toMatchObject({ gain: 1.5, pan: -0.5, on: true })
    await new Promise((r) => setTimeout(r, 700))
    expect(held.api.setStudy).toHaveBeenLastCalledWith(
      7,
      expect.objectContaining({
        mixer: {
          on: true,
          tracks: {
            drums: { mute: true },
            vocals: { solo: true, gain: 1.5, pan: -0.5 },
            bass: { solo: true }
          }
        }
      })
    )
  })

  it('lo guardado vuelve: la canción suena con sus pistas como se dejaron', async () => {
    await conPistas(
      JSON.stringify({
        mixer: { on: true, tracks: { drums: { mute: true }, piano: { gain: 0.5 } } }
      })
    )
    const w = await montar()
    expect(carriles(w)).toHaveLength(6)
    const tracks = mandado()[1]
    expect(tracks[0].on).toBe(false)
    expect(tracks[4]).toMatchObject({ gain: 0.5, on: true })
  })

  it('al cerrar vuelve la canción tal cual', async () => {
    await conPistas(JSON.stringify({ mixer: { on: true } }))
    const w = await montar()
    held.playback.emit({ stems: true })
    await flushPromises()
    await boton(w, 'Cerrar').trigger('click')
    await flushPromises()
    expect(mandado()[1]).toBe(null)
  })

  it('las opciones: guardar la mezcla sin batería, abrir la carpeta, borrarlas', async () => {
    const { pickSavePath } = await import('../src/api.js')
    await conPistas(JSON.stringify({ mixer: { on: true, tracks: { drums: { mute: true } } } }))
    const w = await montar()
    await boton(w, '⋯').trigger('click')
    const { menu } = useContextMenu()
    const labels = menu.value.items.filter((i) => !i.separator).map((i) => i.label)
    expect(labels).toContain('Guardar esta mezcla…')
    expect(labels).toContain('Abrir la carpeta de las pistas')
    expect(labels).toContain('Borrar las pistas…')
    // fuera de la app no hay diálogo de guardar: no se guarda nada
    expect(await pickSavePath()).toBe(null)
    expect(labels).not.toContain('Guardarla como suena…')
  })
})
