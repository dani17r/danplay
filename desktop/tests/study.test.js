import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El modo estudio: bucle A-B, velocidad sin cambiar el tono, marcadores y
// notas, guardados con la cancion y aplicados al entrar.
const held = vi.hoisted(() => ({
  playback: null,
  api: null,
  state: null,
  savePath: null,
  audioPath: null
}))

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
    // el diálogo de guardar del sistema: lo que diga la prueba (null: cancelado)
    pickSavePath: async () => held.savePath,
    // el selector de audios del sistema (para importar una guía)
    pickAudio: async () => held.audioPath
  }
})

import StudyBar from '../src/components/StudyBar.vue'
import Player from '../src/components/Player.vue'
import { resetPlayback, usePlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
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
  resetClick()
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
const mountView = async () => {
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
/** Un botón por lo que dice o, si es solo un icono, por su nombre (aria-label). */
const button = (w, text, nth = 0) =>
  w.findAll('button').filter((b) => b.text().includes(text) || b.attributes('aria-label') === text)[
    nth
  ]
/** Despliega «Compás, cuenta y sonido» del clic. */
async function openClickMore(w) {
  const t = w.find('.study-more-toggle')
  if (t.attributes('aria-expanded') !== 'true') await t.trigger('click')
  await flushPromises()
}
/** «Salir del modo estudio», desde su ⋯. */
async function quit(w) {
  const { menu } = useContextMenu()
  await w.find('.study-more').trigger('click')
  await menu.value.items.find((i) => i.label === 'Salir del modo estudio').action()
  await flushPromises()
}
/** Lo que llega a Rust con un tramo: los dos bordes y qué pasa al acabar (de fábrica, parar). */
const loopArgs = (a, b, end = 'stop') => [a, b, { segments: [[a, b]], end }]

describe('the study bar', () => {
  // jsdom no maqueta: la caja de la onda dice que va de x=0 a x=400
  function withWidth(w, left = 0, width = 400) {
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
  function pointer(target, type, x, extra = {}) {
    target.dispatchEvent(
      new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: 20, ...extra })
    )
    return flushPromises()
  }
  const press = (key) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    return flushPromises()
  }

  it('the segment is chosen by dragging over the waveform, and saved', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    expect(w.text()).toContain('Barak — Mi Gozo')
    expect(held.api.waveform).toHaveBeenCalledWith(7)
    withWidth(w)
    const box = w.find('.tl-box').element
    // de 0:50 (x=100) a 1:40 (x=200), sobre una cancion de 200 s
    await pointer(box, 'pointerdown', 100)
    await pointer(window, 'pointermove', 150)
    expect(w.find('.tl-loop').exists(), 'mientras se arrastra ya se ve el tramo').toBe(true)
    expect(w.findAll('.tl-time').map((t) => t.text())).toEqual(['0:50', '1:15'])
    await pointer(window, 'pointermove', 200)
    await pointer(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(50, 100))
    // con el candado (puesto de entrada) y sonando, no salta al tramo: la
    // cancion sigue por 0:12 y el tramo entra cuando llegue
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    expect(w.text()).toContain('0:50 – 1:40')
    const loopBand = w.find('.tl-loop')
    expect(loopBand.attributes('style')).toContain('left: 25%')
    expect(loopBand.attributes('style')).toContain('width: 25%')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenCalledWith(7, { loop: [50, 100] })
    // el boton lo quita
    await button(w, 'Quitar').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(w.find('.tl-loop').exists()).toBe(false)
    vi.useRealTimers()
    w.unmount()
  })

  it('dragging to the left works too, and right to left gives the same segment', async () => {
    const w = await mountView()
    withWidth(w)
    const box = w.find('.tl-box').element
    await pointer(box, 'pointerdown', 200)
    await pointer(window, 'pointermove', 100)
    await pointer(window, 'pointerup', 100)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(50, 100))
    w.unmount()
  })

  it('a click without dragging moves the song there and leaves the loop alone (unlocked)', async () => {
    const w = await mountView()
    vi.clearAllMocks() // al entrar se aplica lo guardado (sin bucle): eso no cuenta
    withWidth(w)
    await w.find('.tl-lock').trigger('click')
    const box = w.find('.tl-box').element
    await pointer(box, 'pointerdown', 300)
    await pointer(window, 'pointerup', 301)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(150.5)
    expect(held.playback.bridge.setLoop).not.toHaveBeenCalled()
    w.unmount()
  })

  it('when locked and playing, a click does not move the song and the lock says so', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    vi.clearAllMocks()
    withWidth(w)
    const lock = w.find('.tl-lock')
    expect(lock.classes(), 'puesto de entrada').toContain('on')
    expect(lock.attributes('aria-pressed')).toBe('true')
    // lo que se hace con el ratón sobre la onda, en el botón de ayuda (no a la vista)
    expect(w.find('.study-help').attributes('title')).toContain('con el candado')
    const box = w.find('.tl-box').element
    await pointer(box, 'pointerdown', 300)
    await pointer(window, 'pointerup', 301)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(0)
    expect(w.find('.tl-lock').classes(), 'se sacude para decir por que').toContain('nudged')
    await vi.advanceTimersByTimeAsync(800)
    expect(w.find('.tl-lock').classes()).not.toContain('nudged')
    // en pausa no hay nada que estropear: el clic coloca la cancion
    held.playback.emit({ playing: false })
    await flushPromises()
    await pointer(box, 'pointerdown', 100)
    await pointer(window, 'pointerup', 100)
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(50)
    // quitarlo se recuerda para la proxima vez
    await w.find('.tl-lock').trigger('click')
    expect(w.find('.tl-lock').classes()).not.toContain('on')
    expect(localStorage.getItem('danplay.study.lock')).toBe('false')
    vi.useRealTimers()
    w.unmount()
  })

  it('unlocked, choosing a segment outside the current position jumps to A', async () => {
    localStorage.setItem('danplay.study.lock', 'false')
    const w = await mountView()
    vi.clearAllMocks()
    withWidth(w)
    const box = w.find('.tl-box').element
    await pointer(box, 'pointerdown', 100)
    await pointer(window, 'pointermove', 200)
    await pointer(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(50, 100))
    // la cancion iba por 0:12, fuera del tramo: salta a A
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(50)
    w.unmount()
  })

  it('a segment under half a second is not a loop', async () => {
    const w = await mountView()
    vi.clearAllMocks()
    withWidth(w)
    const box = w.find('.tl-box').element
    // 0,5 s son un pixel a esta escala: cinco pixeles son 2,5 s, pero ocho
    // decimas de pixel no llegan
    await pointer(box, 'pointerdown', 100)
    await pointer(window, 'pointermove', 100.8)
    await pointer(window, 'pointerup', 100.8)
    expect(held.playback.bridge.setLoop).not.toHaveBeenCalled()
    w.unmount()
  })

  it('the segment edges can be grabbed and moved; the whole segment too', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100] }) })]
    const w = await mountView()
    withWidth(w)
    const box = w.find('.tl-box').element
    expect(w.find('.tl-loop').attributes('style')).toContain('left: 25%')
    // el borde B esta en x=200: se lleva a x=300 (2:30)
    await pointer(box, 'pointerdown', 201)
    await pointer(window, 'pointermove', 300)
    await pointer(window, 'pointerup', 300)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(50, 150))
    // el borde A (x=100) a x=80 (0:40)
    await pointer(box, 'pointerdown', 99)
    await pointer(window, 'pointermove', 80)
    await pointer(window, 'pointerup', 80)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(40, 150))
    // por dentro (x=150) se arrastra el tramo entero 20 px = 10 s a la derecha
    await pointer(box, 'pointerdown', 150)
    await pointer(window, 'pointermove', 170)
    await pointer(window, 'pointerup', 170)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(50, 160))
    // y no se sale de la cancion: 110 s de largo, como mucho hasta 200
    await pointer(box, 'pointerdown', 250)
    await pointer(window, 'pointermove', 390)
    await pointer(window, 'pointerup', 390)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(90, 200))
    w.unmount()
  })

  it('one edge cannot cross the other', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100] }) })]
    const w = await mountView()
    withWidth(w)
    const box = w.find('.tl-box').element
    await pointer(box, 'pointerdown', 200)
    await pointer(window, 'pointermove', 50)
    await pointer(window, 'pointerup', 50)
    const [a, b] = held.playback.bridge.setLoop.mock.calls.at(-1)
    expect(a).toBe(50)
    expect(b).toBe(50.5)
    w.unmount()
  })

  it('near a marker, the edge snaps to it', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ markers: [{ t: 60, label: 'coro' }] }) })
    ]
    const w = await mountView()
    withWidth(w)
    expect(w.find('.tl-marker-name').text()).toBe('coro')
    const box = w.find('.tl-box').element
    // el coro esta en x=120; se suelta en x=124, a 4 px
    await pointer(box, 'pointerdown', 50)
    await pointer(window, 'pointermove', 124)
    await pointer(window, 'pointerup', 124)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(25, 60))
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

  it('when locked, a segment from the waveform is set without jumping; from the list it does jump', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] })
      })
    ]
    const w = await mountView()
    vi.clearAllMocks()
    await w.find('.tl-marker').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(30, 45))
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(30)
    w.unmount()
  })

  it('the A and B keys mark the segment while playing', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    await press('a')
    expect(w.text()).toContain('A en 0:12')
    held.playback.emit({ position: 20 })
    await flushPromises()
    await press('b')
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(12, 20))
    // B cierra el tramo y vuelve a A, con candado o sin el: es la repeticion A-B
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(12)
    expect(w.text()).toContain('0:12 – 0:20')
    // con el bucle puesto, A lo acorta por delante y B lo alarga por detras
    held.playback.emit({ position: 15 })
    await flushPromises()
    await press('a')
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(15, 20))
    held.playback.emit({ position: 30 })
    await flushPromises()
    await press('b')
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(15, 30))
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { loop: [15, 30] })
    vi.useRealTimers()
    w.unmount()
  })

  it('the ruler shows the minutes and the end', async () => {
    const w = await mountView()
    const labels = w.findAll('.tl-tick-label').map((t) => t.text())
    // 200 s con 800 px de ancho supuesto: una marca con numero cada 30 s
    expect(labels).toEqual(['0:00', '0:30', '1:00', '1:30', '2:00', '2:30', '3:00', '3:20'])
    expect(w.findAll('.tl-tick.minor').length).toBeGreaterThan(0)
    w.unmount()
  })

  it('without a waveform (no ffmpeg) the timeline still works', async () => {
    held.api.waveform.mockRejectedValueOnce(new Error('501: hace falta ffmpeg'))
    const w = await mountView()
    expect(w.text()).toContain('sin forma de onda')
    withWidth(w)
    const box = w.find('.tl-box').element
    await pointer(box, 'pointerdown', 100)
    await pointer(window, 'pointermove', 200)
    await pointer(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(50, 100))
    w.unmount()
  })

  it('there are no start and end buttons anymore', async () => {
    const w = await mountView()
    expect(button(w, 'Marcar A')).toBeUndefined()
    expect(button(w, 'Marcar B')).toBeUndefined()
    w.unmount()
  })

  it('speed is applied and saved; without ffmpeg it warns that the pitch changes', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    await button(w, '0.7×').trigger('click')
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

  it('a segment is saved as a marker with its start, its end and its notes', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    withWidth(w)
    const box = w.find('.tl-box').element
    await pointer(box, 'pointerdown', 100)
    await pointer(window, 'pointermove', 200)
    await pointer(window, 'pointerup', 200)
    expect(button(w, 'Guardar tramo')).toBeTruthy()
    await button(w, 'Guardar tramo').trigger('click')
    await flushPromises()
    // queda elegido: tramo entero, y sus notas abiertas en su pestaña
    const chip = w.find('.study-marker')
    expect(chip.classes()).toContain('on')
    expect(chip.text()).toContain('0:50 – 1:40')
    expect(chip.text()).toContain('Tramo 1')
    expect(button(w, 'Guardar tramo'), 'ya guardado: no se ofrece otra vez').toBeUndefined()
    expect(button(w, 'Guardar como marcador')).toBeUndefined()
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

  it('clicking a marker sets its loop and puts the song at the segment start', async () => {
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
    const w = await mountView()
    expect(w.findAll('.study-marker')).toHaveLength(2)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    vi.clearAllMocks()
    // sonando por 0:12: pulsar «Coro» repite 0:30-0:45 y salta a 0:30 (sigue sonando)
    await w.findAll('.study-pick')[0].trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(30, 45))
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

  it('when paused, choosing a marker positions the song without starting it', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    vi.clearAllMocks()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(30)
    expect(held.playback.bridge.toggle).not.toHaveBeenCalled()
  })

  it('renaming is a separate button: clicking the name opens no dialog', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro' }] })
      })
    ]
    const w = await mountView()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(document.querySelector('.modal')).toBeNull()
    expect(w.find('.study-marker .field-btn[title^="Renombrar"]').exists()).toBe(true)
  })

  it('with a marker selected, moving the segment edges changes the marker', async () => {
    vi.useFakeTimers()
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ loop: [50, 100], markers: [{ t: 50, end: 100, label: 'Coro' }] })
      })
    ]
    const w = await mountView()
    // el bucle guardado es el del marcador: entra elegido
    expect(w.find('.study-marker').classes()).toContain('on')
    withWidth(w)
    const box = w.find('.tl-box').element
    // el borde B (x=200) a x=300 (2:30)
    await pointer(box, 'pointerdown', 201)
    await pointer(window, 'pointermove', 300)
    await pointer(window, 'pointerup', 300)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(50, 150))
    expect(w.find('.study-marker').text()).toContain('0:50 – 2:30')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy.mock.calls.at(-1)[1].markers).toEqual([
      { t: 50, end: 150, label: 'Coro' }
    ])
    // dibujar un tramo nuevo de cero NO toca el marcador: deja de estar elegido
    await pointer(box, 'pointerdown', 20)
    await pointer(window, 'pointermove', 60)
    await pointer(window, 'pointerup', 60)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(10, 30))
    expect(w.find('.study-marker').classes()).not.toContain('on')
    expect(w.find('.study-marker').text()).toContain('0:50 – 2:30')
    expect(button(w, 'Guardar tramo')).toBeTruthy()
    vi.useRealTimers()
  })

  it('with no segment chosen, «aqui» marks the current instant', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    await button(w, 'aquí').trigger('click')
    held.playback.emit({ position: 45.5 })
    await flushPromises()
    await button(w, 'aquí').trigger('click')
    await flushPromises()
    expect(w.findAll('.study-pick-time').map((j) => j.text())).toEqual(['0:12', '0:45'])
    await vi.advanceTimersByTimeAsync(700)
    const saved = held.api.setStudy.mock.calls.at(-1)[1]
    expect(saved.markers.map((m) => m.t)).toEqual([12, 45.5])
    expect(saved.markers.every((m) => !('end' in m))).toBe(true)
    vi.useRealTimers()
  })

  it('removing the selected marker closes its notes', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ markers: [{ t: 30, end: 45, label: 'Coro', notes: 'x' }] })
      })
    ]
    const w = await mountView()
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(w.find('.study-tab.on').text()).toBe('«Coro»')
    await w.find('.study-marker .field-btn[title="Quitar"]').trigger('click')
    await flushPromises()
    expect(w.findAll('.study-tab').map((t) => t.text())).not.toContain('«Coro»')
    expect(w.find('.study-tab.on').text()).toBe('La canción')
    expect(w.findAll('.study-marker')).toHaveLength(0)
  })

  it('what is saved with the song is applied on entry and removed on close', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ loop: [30, 40], speed: 0.8, notes: 'cejilla 2' })
      })
    ]
    const w = await mountView()
    expect(held.playback.bridge.setLoop).toHaveBeenCalledWith(...loopArgs(30, 40))
    expect(held.playback.bridge.setSpeed).toHaveBeenCalledWith(0.8)
    expect(w.find('textarea').element.value).toBe('cejilla 2')
    // ocultar no toca nada de lo que suena: ni el tramo, ni la velocidad, ni el clic
    vi.clearAllMocks()
    await button(w, 'Ocultar').trigger('click')
    await flushPromises()
    expect(w.emitted('hide')).toBeTruthy()
    expect(held.playback.bridge.setLoop).not.toHaveBeenCalled()
    expect(held.playback.bridge.setSpeed).not.toHaveBeenCalled()
    expect(held.playback.bridge.setMetronome).not.toHaveBeenCalled()
    // salir del modo estudio, sí: la canción vuelve a como viene
    await quit(w)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(held.playback.bridge.setSpeed).toHaveBeenLastCalledWith(1)
    expect(w.emitted('exit')).toBeTruthy()
    w.unmount()
  })

  it('a click on the waveform clears the segment; when locked and playing, without moving the song', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 100] }) })]
    const w = await mountView()
    vi.clearAllMocks()
    withWidth(w)
    const box = w.find('.tl-box').element
    // con el candado (puesto de entrada) y sonando: se quita y la cancion
    // sigue donde iba; habia algo que quitar, asi que el candado no se queja
    await pointer(box, 'pointerdown', 300)
    await pointer(window, 'pointerup', 300)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(w.find('.tl-loop').exists()).toBe(false)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    expect(w.find('.tl-lock').classes()).not.toContain('nudged')
    // sin candado, un clic (tambien dentro del tramo) lo quita y lleva alli
    await w.find('.tl-lock').trigger('click')
    await pointer(box, 'pointerdown', 100)
    await pointer(window, 'pointermove', 200)
    await pointer(window, 'pointerup', 200)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(50, 100))
    await pointer(box, 'pointerdown', 150)
    await pointer(window, 'pointerup', 150)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(75)
  })

  it('with «varios», each drag adds a segment, they repeat in sequence and a click removes one', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    vi.clearAllMocks()
    withWidth(w)
    const box = w.find('.tl-box').element
    expect(w.find('.tl-multi').attributes('aria-pressed')).toBe('false')
    await w.find('.tl-multi').trigger('click')
    expect(w.find('.tl-multi').attributes('aria-pressed')).toBe('true')
    expect(w.find('.study-help').attributes('title')).toContain('Arrastra para añadir tramos')
    // 0:20-0:40 y 1:40-2:00
    await pointer(box, 'pointerdown', 40)
    await pointer(window, 'pointermove', 80)
    await pointer(window, 'pointerup', 80)
    await pointer(box, 'pointerdown', 200)
    await pointer(window, 'pointermove', 240)
    await pointer(window, 'pointerup', 240)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(20, 120, {
      segments: [
        [20, 40],
        [100, 120]
      ],
      end: 'stop'
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
    await pointer(box, 'pointerdown', 150)
    await pointer(window, 'pointerup', 150)
    expect(w.findAll('.tl-loop')).toHaveLength(2)
    await vi.advanceTimersByTimeAsync(0)
    expect(w.find('.tl-lock').classes()).toContain('nudged')
    // ...y uno sobre el primero lo quita; el otro sigue
    await pointer(box, 'pointerdown', 60)
    await pointer(window, 'pointerup', 60)
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(100, 120))
    expect(w.findAll('.tl-loop')).toHaveLength(1)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  it('several segments are saved as one marker, and choosing it brings them all back', async () => {
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
    const w = await mountView()
    const both = {
      segments: [
        [20, 40],
        [100, 120]
      ],
      end: 'stop'
    }
    expect(held.playback.bridge.setLoop, 'lo guardado se aplica al entrar').toHaveBeenCalledWith(
      20,
      120,
      both
    )
    expect(w.find('.tl-multi').attributes('aria-pressed'), '«varios» se pone solo').toBe('true')
    await button(w, 'Guardar tramo').trigger('click')
    await flushPromises()
    const chip = w.find('.study-marker')
    expect(chip.text()).toContain('2 tramos · 0:20…')
    expect(chip.text()).toContain('Tramo 1')
    expect(w.findAll('.tl-region'), 'en la onda, una banda por tramo').toHaveLength(2)
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {
      loops: both.segments,
      markers: [{ t: 20, end: 120, parts: both.segments, label: 'Tramo 1' }]
    })
    // se quitan, y el marcador los vuelve a poner todos
    await button(w, 'Quitar').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    await w.find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(20, 120, both)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(20)
    vi.useRealTimers()
  })

  it('segment options: play now, repeat at the end, snap to beats and remove', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50.1, 99.9] }) })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    vi.clearAllMocks()
    const { menu } = useContextMenu()
    const option = (label) => menu.value.items.find((i) => i.label === label)
    await w.find('.tl-loop-menu').trigger('click')
    expect(menu.value.open).toBe(true)
    expect(menu.value.title).toBe('El tramo')
    expect(menu.value.items.filter((i) => i.label).map((i) => i.label)).toEqual([
      'Reproducir ahora',
      'Al acabar: repetir',
      'Al acabar: parar',
      'Al acabar: seguir',
      'Ir aquí',
      'Guía aquí…',
      'Ajustar a los pulsos',
      'Guardar como marcador',
      'Añadir más tramos',
      'Quitar la selección',
      'Estilo de la onda'
    ])
    // de fábrica, al acabar se para (una vez, y quieto al principio)
    // la puesta, marcada (en el color de la app), no dicha con una nota
    expect(option('Al acabar: parar').active).toBe(true)
    expect(option('Al acabar: repetir').active).toBe(false)
    // sonar ya: arranca y va al principio del tramo
    await option('Reproducir ahora').action()
    await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(50.1)
    // en bucle: se repite hasta que se quite
    await option('Al acabar: repetir').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(50.1, 99.9, 'repeat'))
    expect(w.find('.study-end.on').attributes('aria-label')).toBe('Repetir')
    // a los pulsos de la rejilla (uno cada medio segundo desde 0,25), sin
    // perder el «al acabar»
    await w.find('.tl-loop-menu').trigger('click')
    await option('Ajustar a los pulsos').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(
      ...loopArgs(50.25, 99.75, 'repeat')
    )
    // y se recuerda para la próxima vez
    expect(JSON.parse(localStorage.getItem('danplay.study.atEnd'))).toBe('repeat')
    await w.find('.tl-loop-menu').trigger('click')
    await option('Al acabar: seguir').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(
      ...loopArgs(50.25, 99.75, 'continue')
    )
    await w.find('.tl-loop-menu').trigger('click')
    await option('Quitar la selección').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(null, null)
    expect(w.find('.tl-loop').exists()).toBe(false)
  })

  it('right-clicking the waveform offers going there and, with several, removing only that segment', async () => {
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
    const w = await mountView()
    vi.clearAllMocks()
    withWidth(w)
    const { menu } = useContextMenu()
    const option = (label) => menu.value.items.find((i) => i.label === label)
    // fuera de los tramos: lo que se puede hacer en ese punto
    await w.find('.tl-box').trigger('contextmenu', { clientX: 300, clientY: 20 })
    expect(menu.value.title).toBe('La onda')
    expect(option('Ir aquí').note).toBe('2:30')
    expect(option('Reproducir ahora')).toBeUndefined()
    await option('Ir aquí').action()
    expect(held.playback.bridge.seek, 'lo pide uno: vale con el candado').toHaveBeenLastCalledWith(
      150
    )
    // sobre el segundo tramo: sus opciones, y quitar solo ese
    await w.find('.tl-box').trigger('contextmenu', { clientX: 220, clientY: 20 })
    expect(menu.value.title).toBe('Tramo 2')
    expect(option('Quitar todos los tramos')).toBeTruthy()
    await option('Quitar este tramo').action()
    await flushPromises()
    expect(held.playback.bridge.setLoop).toHaveBeenLastCalledWith(...loopArgs(20, 40))
    expect(w.findAll('.tl-loop')).toHaveLength(1)
  })
})

describe('what is saved goes to its song', () => {
  it('opening with something already playing reads the saved data only once', async () => {
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
    await mountView()
    expect(held.api.song).toHaveBeenCalledTimes(1)
    expect(held.api.song).toHaveBeenCalledWith(7)
  })

  it('if the song changes before saving, the notes are saved to their own song', async () => {
    // El guardado espera 600 ms y leia la cancion y las notas al hacerlo:
    // si entre medias cambiaba la cancion, las notas de la primera acababan
    // en la segunda (en el indice y en la etiqueta del archivo).
    vi.useFakeTimers()
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: '' }),
      song(8, { title: 'Otra', study: '' })
    ]
    const w = await mountView()
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

  it('hiding it saves pending changes without waiting', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    await w.find('textarea').setValue('entrar tras el redoble')
    await button(w, 'Ocultar').trigger('click')
    await flushPromises()
    expect(held.api.setStudy).toHaveBeenCalledWith(7, { notes: 'entrar tras el redoble' })
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })
})

describe('the large study view', () => {
  it('«Grande» expands it to full height and, closed like that, it reopens large', async () => {
    const w = await mountView()
    const big = () => w.find('.study-size')
    expect(w.find('.study').classes()).not.toContain('big')
    expect(big().attributes('aria-pressed')).toBe('false')
    await big().trigger('click')
    expect(w.find('.study').classes()).toContain('big')
    expect(big().attributes('aria-pressed')).toBe('true')
    // la onda aprovecha el sitio
    expect(w.findComponent({ name: 'StudyTimeline' }).props('height')).toBe(150)
    expect(localStorage.getItem('danplay.study.large')).toBe('true')
    w.unmount()
    const other = await mountView()
    expect(other.find('.study').classes(), 'se recuerda').toContain('big')
    await other.find('.study-size').trigger('click')
    expect(other.find('.study').classes()).not.toContain('big')
    expect(other.findComponent({ name: 'StudyTimeline' }).props('height')).toBe(80)
    expect(localStorage.getItem('danplay.study.large')).toBe('false')
  })
})

describe('pitch, fine speed and metronome', () => {
  function withWidth(w, left = 0, width = 400) {
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

  it('pitch is counted in tones: in half steps (a semitone), shows the resulting key and is saved', async () => {
    vi.useFakeTimers()
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, key: 'G' }
    })
    const w = await mountView()
    expect(w.text()).toContain('Tono')
    expect(w.find('.study-step.on').text(), 'de entrada, de medio en medio').toBe('½ tono')
    expect(w.find('.study-pitch-n').text()).toBe('Como está grabada')
    await w.find('.study-pitch-up').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(1)
    // dicho como lo dice un músico, y con sus semitonos debajo
    expect(w.find('.study-pitch-n').text()).toBe('medio tono arriba')
    expect(w.find('.study-pitch-words small').text()).toBe('1 semitono')
    await w.find('.study-pitch-up').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(2)
    expect(w.find('.study-pitch-n').text(), 'dos semitonos son un tono').toBe('1 tono arriba')
    expect(w.text()).toContain('G → A')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { pitch: 2 })
    await button(w, 'original').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(0)
    expect(w.find('.study-pitch-n').text()).toBe('Como está grabada')
    vi.useRealTimers()
  })

  it('pitch also moves in quarter tones and in whole tones', async () => {
    vi.useFakeTimers()
    held.playback.emit({
      track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, key: 'G' }
    })
    const w = await mountView()
    const step = (n) => w.findAll('.study-step').find((b) => b.text() === n)
    await step('¼ de tono').trigger('click')
    await w.find('.study-pitch-up').trigger('click')
    await flushPromises()
    // un cuarto de tono es medio semitono: no tiene nombre de tonalidad
    expect(bridge().setPitch).toHaveBeenLastCalledWith(0.5)
    expect(w.find('.study-pitch-n').text()).toBe('un cuarto de tono arriba')
    expect(w.text()).toContain('G → G +¼')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { pitch: 0.5 })
    await step('1 tono').trigger('click')
    await w.find('.study-pitch-down').trigger('click')
    await w.find('.study-pitch-down').trigger('click')
    await flushPromises()
    expect(bridge().setPitch).toHaveBeenLastCalledWith(-3.5)
    expect(w.find('.study-pitch-n').text()).toBe('1 tono y tres cuartos abajo')
    // el paso elegido se recuerda
    expect(localStorage.getItem('danplay.study.pitchStep')).toBe('2')
    vi.useRealTimers()
  })

  it('a pitch saved with quarter tones comes back with the song', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ pitch: -0.5 }) })]
    const w = await mountView()
    expect(bridge().setPitch).toHaveBeenCalledWith(-0.5)
    expect(w.find('.study-pitch-n').text()).toBe('un cuarto de tono abajo')
  })

  it('the saved pitch is applied on entry and removed on leaving study mode, like the speed', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ pitch: -3, speed: 0.9 }) })
    ]
    const w = await mountView()
    expect(bridge().setPitch).toHaveBeenCalledWith(-3)
    expect(bridge().setSpeed).toHaveBeenCalledWith(0.9)
    await quit(w)
    expect(bridge().setPitch).toHaveBeenLastCalledWith(0)
    expect(bridge().setSpeed).toHaveBeenLastCalledWith(1)
    expect(w.emitted('exit')).toBeTruthy()
  })

  it('speed has a fine slider besides the buttons', async () => {
    const w = await mountView()
    await w.find('input[aria-label="Velocidad"]').setValue('0.83')
    await flushPromises()
    expect(bridge().setSpeed).toHaveBeenLastCalledWith(0.83)
    expect(w.text()).toContain('83 %')
  })

  it('on entry the beat of the playing song is analysed and the grid is drawn', async () => {
    const w = await mountView()
    expect(held.playback.bridge.analyzeBeats).toHaveBeenCalledWith('/musica/mi-gozo.mp3', null)
    // con la rejilla ya en Rust se vuelven a mandar los ajustes para que el clic la coja
    expect(bridge().setMetronome).toHaveBeenCalled()
    expect(w.text()).toContain('sigue la canción')
    expect(w.text()).toContain('el «1» seguro')
    w.unmount()
  })

  it('the click starts on the beat and stops separately from the song', async () => {
    const w = await mountView()
    vi.clearAllMocks()
    await w.find('.study-metro-toggle').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(
      expect.objectContaining({ on: true, bpm: null, meter: null, shift: 0, mult: 0 })
    )
    expect(w.find('.study-metro-toggle').classes()).toContain('on')
    expect(w.find('.study-bpm-input input').element.value).toBe('120')
    // la cancion sigue: no se ha tocado play ni pausa
    expect(bridge().toggle).not.toHaveBeenCalled()
    await button(w, 'Parar').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: false }))
    expect(bridge().toggle).not.toHaveBeenCalled()
  })

  it('the count-in and the click sound: chosen, remembered, and the count-in is removed on close', async () => {
    localStorage.removeItem('danplay.click.countIn')
    localStorage.removeItem('danplay.click.sound')
    try {
      const w = await mountView()
      // al abrir, lo recordado: nada todavía (sin cuenta, el clásico)
      expect(bridge().setMetronome).toHaveBeenCalledWith(
        expect.objectContaining({ count_in: 0, sound: 'clasico' })
      )
      // plegado de entrada (son muchos botones): se despliega
      expect(button(w, '1 compás')).toBeUndefined()
      await openClickMore(w)
      // los sonidos, con los dos nuevos
      expect(w.findAll('[aria-label="Sonido del clic"] button').map((b) => b.text())).toEqual([
        'clásico',
        'madera',
        'baqueta',
        'cencerro',
        'claves',
        'charles'
      ])
      await button(w, '1 compás').trigger('click')
      await button(w, 'cencerro').trigger('click')
      await flushPromises()
      expect(bridge().setMetronome).toHaveBeenLastCalledWith(
        expect.objectContaining({ count_in: 1, sound: 'cencerro' })
      )
      expect(button(w, '1 compás').classes()).toContain('on')
      expect(button(w, 'cencerro').classes()).toContain('on')
      // mientras cuenta, se dice
      held.playback.emit({ counting_in: true })
      await flushPromises()
      expect(w.text()).toContain('contando…')
      held.playback.emit({ counting_in: false })
      w.unmount()
      // la vez siguiente vuelve lo elegido
      vi.clearAllMocks()
      const other = await mountView()
      expect(bridge().setMetronome).toHaveBeenCalledWith(
        expect.objectContaining({ count_in: 1, sound: 'cencerro' })
      )
      // y al salir del estudio, play vuelve a ser play: sin cuenta
      await quit(other)
      expect(bridge().setMetronome).toHaveBeenLastCalledWith(
        expect.objectContaining({ on: false, count_in: 0 })
      )
    } finally {
      localStorage.removeItem('danplay.click.countIn')
      localStorage.removeItem('danplay.click.sound')
    }
  })

  it('at another speed the click shows the tempo as heard, with its decimal', async () => {
    const w = await mountView()
    held.playback.emit({ speed: 0.5 })
    await flushPromises()
    expect(w.find('.study-bpm-input input').element.value).toBe('60')
    held.playback.emit({ speed: 0.83 })
    await flushPromises()
    expect(w.find('.study-bpm-input input').element.value).toBe('99.6')
  })

  it('raising or lowering the bpm makes it free; «el de la cancion» follows the song again; everything is saved', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    vi.clearAllMocks()
    await button(w, '+', 1).trigger('click') // el segundo «+» es el del metronomo
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 121 }))
    expect(w.text()).toContain('tempo a mano')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { metronome: { bpm: 121 } })
    await button(w, 'el de la canción').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: null }))
    expect(w.text()).toContain('sigue la canción')
    // compas, el «1» corrido y el doble de pulsos
    await openClickMore(w)
    expect(w.find('.study-meters .btn.on').text(), 'el detectado').toBe('4/4')
    await button(w, '3/4').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ meter: 3 }))
    expect(w.find('.study-meters .btn.on').text()).toBe('3/4')
    await button(w, 'el 1 es el siguiente').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ shift: 1 }))
    await button(w, '×2').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ mult: 1 }))
    expect(button(w, '×2').classes(), 'se ve puesto').toContain('on')
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {
      metronome: { meter: 3, shift: 1, mult: 1 }
    })
    vi.useRealTimers()
  })

  it('tempo is typed with decimals, and arrow keys move it by tenths', async () => {
    vi.useFakeTimers()
    const w = await mountView()
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

  it('×2 also doubles a manually set tempo, and the waveform draws twice the beats', async () => {
    const w = await mountView()
    const tempo = w.find('.study-bpm-input input')
    await tempo.trigger('focus')
    await tempo.setValue('68')
    await tempo.trigger('blur')
    await flushPromises()
    expect(tempo.element.value).toBe('68')
    const wave = () => w.findComponent({ name: 'StudyTimeline' }).props('grid')
    const previous = wave().beats.length
    await button(w, '×2').trigger('click')
    await flushPromises()
    // antes el tempo a mano se quedaba y ×2 no hacia nada
    expect(tempo.element.value).toBe('136')
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(
      expect.objectContaining({ bpm: 68, mult: 1 })
    )
    expect(wave().beats.length).toBe(previous * 2)
    // escribir con el doble puesto guarda el tempo sin el: al quitarlo vuelve
    await tempo.trigger('focus')
    await tempo.setValue('140')
    await tempo.trigger('blur')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ bpm: 70 }))
    await button(w, '×2').trigger('click')
    await flushPromises()
    expect(tempo.element.value).toBe('70')
    expect(button(w, '×2').classes()).not.toContain('on')
  })

  it('no accent: meter 0 is saved and the waveform marks no «1»', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    await openClickMore(w)
    await button(w, 'sin acento').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ meter: 0 }))
    expect(w.findComponent({ name: 'StudyTimeline' }).props('grid').meter).toBe(0)
    expect(button(w, 'el 1 es el siguiente').attributes('disabled')).toBeDefined()
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, { metronome: { meter: 0 } })
    // volver al detectado no se guarda como ajuste
    await button(w, '4/4').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ meter: null }))
    await vi.advanceTimersByTimeAsync(700)
    expect(held.api.setStudy).toHaveBeenLastCalledWith(7, {})
    vi.useRealTimers()
  })

  it('manual adjustments come back with the song, and changing song analyses the new one', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ metronome: { meter: 3, shift: 2 } }) }),
      song(8, { title: 'Otra' })
    ]
    await mountView()
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

  it('if it cannot be analysed, the click runs free and says so', async () => {
    held.playback.bridge.analyzeBeats.mockRejectedValueOnce(
      new Error('no se encuentra un pulso estable')
    )
    const w = await mountView()
    expect(w.text()).toContain('sin compás detectado: va libre')
    vi.clearAllMocks()
    await w.find('.study-metro-toggle').trigger('click')
    await flushPromises()
    expect(bridge().setMetronome).toHaveBeenLastCalledWith(expect.objectContaining({ on: true }))
  })

  it('the click volume has its own slider, up to triple', async () => {
    const w = await mountView()
    const vol = w.find('.study-metro-vol input[type="range"]')
    expect(vol.attributes('max')).toBe('3')
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

  it('the song can also be turned up, up to 50 % more', async () => {
    const w = await mountView()
    const vol = w.find('.study-song-vol input[type="range"]')
    expect(vol.attributes('max')).toBe('1.5')
    await vol.setValue('1.3')
    await flushPromises()
    expect(bridge().setVolume).toHaveBeenLastCalledWith(1.3)
    expect(w.find('.study-song-vol').text()).toContain('130 %')
    // lo que pasa del 100 % se pinta aparte
    expect(w.find('.study-song-vol .slider-over').exists()).toBe(true)
  })

  it('the waveform receives the grid and is taller', async () => {
    const w = await mountView()
    withWidth(w)
    expect(w.find('.tl').attributes('style')).toContain('--tl-h: 80px')
    expect(w.findComponent({ name: 'StudyTimeline' }).props('grid')?.bpm).toBe(120)
  })
})

describe('the player and the loop', () => {
  it('draws the loop segment and the study button notifies the app', async () => {
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

  it('in the browser the loop goes back to A after passing B, as in Rust', async () => {
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

  it('with several segments, the player draws one per segment', async () => {
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

  it('in the browser, segments play in sequence and at the end they repeat, stop or continue', async () => {
    resetPlayback()
    held.api.inTauri = false
    try {
      const player = usePlayback()
      await player.ready()
      const audio = player.audioElement()
      const at = async (t) => {
        Object.defineProperty(audio, 'currentTime', {
          value: t,
          writable: true,
          configurable: true
        })
        audio.dispatchEvent(new Event('timeupdate'))
        await flushPromises()
      }
      const two = [
        [10, 20],
        [50, 60]
      ]
      // repitiendo: del final del primero al segundo, y del último al primero
      await player.setLoops(two, { end: 'repeat' })
      expect(player.loopEnd.value).toBe('repeat')
      await at(15)
      await at(20.1)
      expect(audio.currentTime, 'del final del primero, al segundo').toBe(50)
      await at(60.2)
      expect(audio.currentTime, 'del ultimo, al primero').toBe(10)
      // parando: suenan una vez y se queda al principio del primero
      await player.setLoops(two, { end: 'stop' })
      await at(15)
      await at(20.1)
      expect(audio.currentTime).toBe(50)
      await at(60.2)
      expect(audio.currentTime, 'quieto al principio del primero').toBe(10)
      expect(player.playing.value).toBe(false)
      // siguiendo: suenan una vez y la canción sigue de largo
      await player.setLoops(two, { end: 'continue' })
      await at(15)
      await at(20.1)
      expect(audio.currentTime).toBe(50)
      await at(60.2)
      expect(audio.currentTime, 'sigue').toBe(60.2)
      await at(70)
      expect(audio.currentTime).toBe(70)
      await player.clearLoop()
      expect(player.loops.value).toEqual([])
    } finally {
      held.api.inTauri = true
    }
  })

  it('the player volume goes up to 150 % and arrow keys stop at 100 %', async () => {
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

describe('the separated stems in study mode', () => {
  // las de la canción 7: seis pistas, con la ruta de cada una
  const withStems = async (study = '') => {
    const { stemsOf } = await import('./support/backend.js')
    held.state.stems[7] = stemsOf(7)
    held.state.songs = [song(7, { title: 'Mi Gozo', study })]
  }
  const sentStems = () => held.playback.bridge.setStems.mock.calls.at(-1)
  const lanes = (w) => w.findAll('.tl-lanes .tl-lane').map((l) => l.find('.tl-lane-name').text())

  beforeEach(async () => {
    const { resetSeparation } = await import('../src/composables/useSeparation.js')
    resetSeparation()
    held.state.stems = {}
    held.state.separation.current = null
    held.state.separation.queue = []
    held.state.separation.events = []
  })

  it('without stems it offers to split it, and when done they play on their own', async () => {
    const { finishSeparation } = await import('./support/backend.js')
    const { useSeparation } = await import('../src/composables/useSeparation.js')
    const w = await mountView()
    await useSeparation().refresh()
    await flushPromises()
    // un solo botón: sin elegir modelo ni calidad
    const separate = button(w, 'Separar en pistas')
    expect(separate).toBeTruthy()
    await separate.trigger('click')
    await flushPromises()
    expect(held.api.separate).toHaveBeenCalledWith(7)
    // mientras se separa, se ve cómo va
    // en la cabecera, la barra y el %; lo que hace, en su aviso
    expect(w.find('.study-stems-busy').attributes('title')).toMatch(/preparando|separando/)
    finishSeparation(held.state)
    await useSeparation().refresh()
    await flushPromises()
    await flushPromises()
    // en cuanto están las rápidas, suenan sus pistas: un carril por instrumento
    expect(lanes(w)).toEqual(['Batería', 'Voces', 'Bajo', 'Guitarra', 'Piano', 'Otros'])
    const [path, tracks] = sentStems()
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
    // y se siguen mejorando sin cortar nada: se ve junto a los carriles
    expect(w.find('.study-stems-progress').attributes('title')).toMatch(/mejorando/)
    const sent = held.playback.bridge.setStems.mock.calls.length
    finishSeparation(held.state)
    await useSeparation().refresh()
    await flushPromises()
    await flushPromises()
    // las buenas se mandan otra vez (el reproductor ve que cambiaron)
    expect(held.playback.bridge.setStems.mock.calls.length).toBeGreaterThan(sent)
    expect(lanes(w)).toHaveLength(6)
    expect(w.find('.study-stems-progress').exists()).toBe(false)
  })

  it('the quick ones left half done are offered an upgrade', async () => {
    const { stemsOf } = await import('./support/backend.js')
    await withStems(JSON.stringify({ mixer: { on: true } }))
    held.state.stems[7] = stemsOf(7, undefined, 'rapida')
    const w = await mountView()
    await w.find('.study-stems-more').trigger('click')
    const { menu } = useContextMenu()
    const again = menu.value.items.find((i) => i.label?.startsWith('Separar otra vez'))
    expect(again?.label).toBe('Separar otra vez')
    expect(again?.hint).toBe('Con la mejor calidad: las de ahora son las rápidas')
    await again?.action()
    await flushPromises()
    expect(held.api.separate).toHaveBeenCalledWith(7)
    expect(held.state.separation.current).toMatchObject({ id: 7, stage: 'refine' })
  })

  it('with stems: the button turns them on and off, and it is saved', async () => {
    await withStems()
    const w = await mountView()
    expect(w.find('.tl-lanes').exists()).toBe(false)
    await button(w, 'Pistas').trigger('click')
    await flushPromises()
    expect(lanes(w)).toHaveLength(6)
    expect(sentStems()[1]).toHaveLength(6)
    await new Promise((r) => setTimeout(r, 700)) // el guardado va con retraso
    expect(held.api.setStudy).toHaveBeenLastCalledWith(
      7,
      expect.objectContaining({ mixer: { on: true } })
    )
    await button(w, 'Pistas').trigger('click')
    await flushPromises()
    expect(w.find('.tl-lanes').exists()).toBe(false)
    expect(sentStems()[1]).toBe(null)
  })

  it('mute, solo, volume and pan reach Rust and are saved', async () => {
    await withStems(JSON.stringify({ mixer: { on: true } }))
    const w = await mountView()
    expect(lanes(w)).toHaveLength(6)
    const drums = w.find('[data-lane="drums"]')
    await drums.findAll('.tl-lane-btn')[0].trigger('click') // M
    await flushPromises()
    let tracks = sentStems()[1]
    expect(tracks[0].on).toBe(false)
    expect(tracks.slice(1).every((t) => t.on)).toBe(true)
    expect(drums.classes()).toContain('off')
    // solo la voz y el bajo: el resto calla
    await w.find('[data-lane="vocals"]').findAll('.tl-lane-btn')[1].trigger('click')
    await w.find('[data-lane="bass"]').findAll('.tl-lane-btn')[1].trigger('click')
    await flushPromises()
    tracks = sentStems()[1]
    expect(tracks.map((t) => t.on)).toEqual([false, true, true, false, false, false])
    // el volumen de la voz y a la izquierda
    const [gain, pan] = w.find('[data-lane="vocals"]').findAll('input[type="range"]')
    await gain.setValue('1.5')
    await pan.setValue('-0.5')
    await flushPromises()
    tracks = sentStems()[1]
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

  it('saved settings come back: the song plays with its stems as they were left', async () => {
    await withStems(
      JSON.stringify({
        mixer: { on: true, tracks: { drums: { mute: true }, piano: { gain: 0.5 } } }
      })
    )
    const w = await mountView()
    expect(lanes(w)).toHaveLength(6)
    const tracks = sentStems()[1]
    expect(tracks[0].on).toBe(false)
    expect(tracks[4]).toMatchObject({ gain: 0.5, on: true })
  })

  it('leaving study mode brings back the song as is', async () => {
    await withStems(JSON.stringify({ mixer: { on: true } }))
    const w = await mountView()
    held.playback.emit({ stems: true })
    await flushPromises()
    await quit(w)
    await flushPromises()
    expect(sentStems()[1]).toBe(null)
  })

  it('the options: save the mix without drums, open the folder, delete them', async () => {
    const { pickSavePath } = await import('../src/api.js')
    await withStems(JSON.stringify({ mixer: { on: true, tracks: { drums: { mute: true } } } }))
    const w = await mountView()
    await w.find('.study-stems-more').trigger('click')
    const { menu } = useContextMenu()
    const labels = menu.value.items.filter((i) => !i.separator).map((i) => i.label)
    // un solo «Exportar…»: las formas de exportar, en su ventana
    expect(labels).toContain('Exportar…')
    expect(labels).toContain('Abrir la carpeta')
    expect(labels).toContain('Borrar las pistas…')
    expect(labels.filter((l) => /export|guardar/i.test(l))).toEqual(['Exportar…'])
    // fuera de la app no hay diálogo de guardar: no se guarda nada
    expect(await pickSavePath()).toBe(null)
  })

  it('with the click playing, export offers to include the click', async () => {
    await withStems(JSON.stringify({ mixer: { on: true } }))
    const w = await mountView()
    await w.find('.study-metro-toggle').trigger('click')
    await flushPromises()
    await w.find('.study-stems-more').trigger('click')
    const { menu } = useContextMenu()
    menu.value.items.find((i) => i.label === 'Exportar…').action()
    await flushPromises()
    // se abre con la mezcla de las pistas elegida, y el clic entre lo que puede llevar
    const kinds = [...document.querySelectorAll('.exp-kind')]
    expect(kinds.find((k) => k.classList.contains('on')).textContent).toContain('La mezcla')
    const extras = [...document.querySelectorAll('.exp-extra-name')].map((e) => e.textContent)
    expect(extras).toContain('El clic')
  })
})

describe('the 1.28 study mode', () => {
  function withWidth(w, left = 0, width = 400) {
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

  it('leaving «varios» keeps a single segment: the last one touched', async () => {
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
    const w = await mountView()
    withWidth(w)
    // se mueve el borde del primero: es el último que se tocó
    const box = w.find('.tl-box').element
    const at = (x, type, target = box) =>
      target.dispatchEvent(
        new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: 20 })
      )
    at(80, 'pointerdown') // el borde derecho del primero (40 s = x 80)
    at(90, 'pointermove', window)
    at(90, 'pointerup', window)
    await flushPromises()
    expect(bridge().setLoop).toHaveBeenLastCalledWith(20, 120, {
      segments: [
        [20, 45],
        [100, 120]
      ],
      end: 'stop'
    })
    vi.clearAllMocks()
    await w.find('.tl-multi').trigger('click')
    await flushPromises()
    expect(bridge().setLoop).toHaveBeenLastCalledWith(...loopArgs(20, 45))
    expect(w.findAll('.tl-loop')).toHaveLength(1)
  })

  it('markers with their checkbox ticked play in sequence, once or looped', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          markers: [
            { t: 10, end: 20, label: 'Intro' },
            { t: 50, end: 70, label: 'Coro' },
            { t: 90, end: 99, label: 'Puente' }
          ]
        })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    vi.clearAllMocks()
    const checkboxes = w.findAll('.study-check')
    expect(checkboxes).toHaveLength(3)
    await checkboxes[0].trigger('click')
    await checkboxes[2].trigger('click')
    expect(checkboxes[0].attributes('aria-checked')).toBe('true')
    const playChecked = w.find('.study-play-checked')
    expect(playChecked.text()).toContain('Tocar 2 elegidos')
    await playChecked.trigger('click')
    await flushPromises()
    // seguidos, saltándose lo de en medio, desde el primero, y a sonar
    expect(bridge().setLoop).toHaveBeenLastCalledWith(10, 99, {
      segments: [
        [10, 20],
        [90, 99]
      ],
      end: 'stop'
    })
    expect(bridge().seek).toHaveBeenLastCalledWith(10)
    expect(bridge().toggle).toHaveBeenCalledTimes(1)
    // y en bucle, con su botón de al lado
    const repeat = w.findAll('.study-col-foot .study-end')[0]
    await repeat.trigger('click')
    await flushPromises()
    expect(bridge().setLoop).toHaveBeenLastCalledWith(10, 99, {
      segments: [
        [10, 20],
        [90, 99]
      ],
      end: 'repeat'
    })
  })

  it('while another song is being split, this one is queued and it says so', async () => {
    const { useSeparation } = await import('../src/composables/useSeparation.js')
    // sin pistas (las pruebas de antes separaron esta)
    delete held.state.stems[7]
    held.state.separation.queue = []
    held.state.separation.current = {
      id: 99,
      title: 'Otra canción',
      stage: 'separate',
      done: 3,
      total: 10
    }
    try {
      const w = await mountView()
      await useSeparation().refresh()
      await flushPromises()
      const enqueue = button(w, 'Poner en la cola')
      expect(enqueue, 'no dice «Separar» como si empezara ya').toBeTruthy()
      expect(enqueue.attributes('title')).toContain('Ahora se separa «Otra canción» (30 %)')
      await enqueue.trigger('click')
      await flushPromises()
      expect(held.api.separate).toHaveBeenCalledWith(7)
      expect(w.find('.study-stems-busy').attributes('title')).toContain('en la cola')
    } finally {
      held.state.separation.current = null
      held.state.separation.queue = []
    }
  })

  it('Ctrl + wheel zooms the waveform at the mouse position, and the ruler switches to tenths', async () => {
    const w = await mountView()
    withWidth(w)
    const tl = w.findComponent({ name: 'StudyTimeline' })
    expect(tl.vm.view.to - tl.vm.view.from).toBe(200)
    const wheel = (opts) => {
      const e = new WheelEvent('wheel', { clientX: 100, bubbles: true, cancelable: true, ...opts })
      w.find('.tl-box').element.dispatchEvent(e)
      return e
    }
    // la rueda sola no acerca: sube y baja el panel, como en cualquier sitio
    expect(wheel({ deltaY: -400 }).defaultPrevented).toBe(false)
    expect(tl.vm.view.to - tl.vm.view.from).toBe(200)
    // con Ctrl, acerca sobre x=100 (0:50)
    for (let i = 0; i < 8; i++) wheel({ deltaY: -400, ctrlKey: true })
    await flushPromises()
    const span = tl.vm.view.to - tl.vm.view.from
    expect(span).toBeLessThan(10)
    // el punto bajo el ratón no se mueve: sigue a un cuarto de la vista
    expect((50 - tl.vm.view.from) / span).toBeCloseTo(0.25, 1)
    // con Mayús, recorre la canción sin acercar ni alejar
    const from = tl.vm.view.from
    expect(wheel({ deltaY: 200, shiftKey: true }).defaultPrevented).toBe(true)
    await flushPromises()
    expect(tl.vm.view.from).toBeGreaterThan(from)
    expect(tl.vm.view.to - tl.vm.view.from).toBeCloseTo(span, 5)
    expect(w.find('.tl-overview').exists(), 'la tira de toda la canción').toBe(true)
    expect(w.findAll('.tl-tick-label').some((l) => l.text().includes('.'))).toBe(true)
    // ver toda la canción
    await w.find('[aria-label="Ver toda la canción"]').trigger('click')
    await flushPromises()
    expect(tl.vm.view.to - tl.vm.view.from).toBe(200)
  })

  it('adjusting a beat manually saves it as a custom click for the song', async () => {
    vi.useFakeTimers()
    const w = await mountView()
    withWidth(w)
    await button(w, 'Ajustar pulsos').trigger('click')
    await flushPromises()
    expect(button(w, 'Listo'), 'se está ajustando').toBeTruthy()
    expect(w.find('.study-mode.on').text()).toBe('Propio')
    const beats = w.findAll('.tl-beat')
    expect(beats.length).toBeGreaterThan(4)
    // arrastrar el tercero que se ve 2 px a la derecha
    const tl = w.findComponent({ name: 'StudyTimeline' })
    const span = tl.vm.view.to - tl.vm.view.from
    const x0 = Number(beats[2].attributes('style').match(/left: ([\d.]+)%/)[1]) * 4
    const at = (target, type, x) =>
      target.dispatchEvent(
        new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: 20 })
      )
    at(beats[2].element, 'pointerdown', x0)
    at(window, 'pointermove', x0 + 4)
    at(window, 'pointerup', x0 + 4)
    await flushPromises()
    await vi.advanceTimersByTimeAsync(700)
    const saved = held.api.setStudy.mock.calls.at(-1)[1]
    expect(saved.metronome.own.beats.length).toBe(400)
    const moved = saved.metronome.own.beats.find((t, i) => Math.abs(t - (0.25 + i * 0.5)) > 1e-6)
    expect(moved, 'un pulso movido').toBeDefined()
    expect(moved - Math.round((moved - 0.25) / 0.5) * 0.5 - 0.25).toBeCloseTo((4 / 400) * span, 2)
    // y suena ya: Rust tiene la rejilla propia
    expect(bridge().setBeatGrid).toHaveBeenCalled()
    vi.useRealTimers()
  })
})

describe('the outputs', () => {
  it('click and cues on the left, music on the right', async () => {
    const { resetRouting } = await import('../src/composables/useRouting.js')
    resetRouting()
    const w = await mountView()
    await button(w, 'Salidas').trigger('click')
    await flushPromises()
    const presets = document.querySelectorAll('.outs-preset')
    expect([...presets].map((p) => p.querySelector('.outs-preset-name').textContent)).toEqual([
      'Todo junto',
      'Clic y guías a la izquierda, música a la derecha',
      'Clic y guías por 1-2, música por 3-4',
      'A medida'
    ])
    presets[1].click()
    await flushPromises()
    expect(held.playback.bridge.setRouting).toHaveBeenLastCalledWith({
      buses: [
        { left: 1, right: 1 },
        { left: 0, right: 0 }
      ],
      song: 0,
      click: 1,
      cues: 1
    })
    expect(held.playback.bridge.setOutput).toHaveBeenLastCalledWith({ device: null, channels: 2 })
    // y se recuerda
    expect(JSON.parse(localStorage.getItem('danplay.outputs')).preset).toBe('click-left')
    presets[2].click()
    await flushPromises()
    expect(held.playback.bridge.setOutput).toHaveBeenLastCalledWith({ device: null, channels: 4 })
  })

  it('each thing, on its side and in stereo or mono, in its lane', async () => {
    const { resetRouting } = await import('../src/composables/useRouting.js')
    localStorage.removeItem('danplay.outputs')
    resetRouting()
    const w = await mountView()
    const songHead = w.find('.tl-song-head .chans')
    const button = (name) => songHead.find(`[aria-label="${name}"]`)
    expect(button('Izquierda').attributes('aria-pressed')).toBe('true')
    expect(button('Derecha').attributes('aria-pressed')).toBe('true')
    expect(songHead.find('.chan-mode').text()).toBe('Est')

    // la música en mono (una mesa en mono); el clic y las guías siguen en estéreo
    await songHead.find('.chan-mode').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setRouting.mock.calls.at(-1)[0].buses).toEqual([
      { left: 0, right: 1, mono: true },
      { left: 0, right: 1 }
    ])
    expect(songHead.find('.chan-mode').text()).toBe('Mono')
    expect(JSON.parse(localStorage.getItem('danplay.outputs')).mono.song).toBe(true)

    // la canción solo por la derecha: por un canal va en mono, sin elegir
    await button('Izquierda').trigger('click')
    await flushPromises()
    const spec = held.playback.bridge.setRouting.mock.calls.at(-1)[0]
    expect(spec.buses[spec.song]).toEqual({ left: 1, right: 1 })
    expect(songHead.find('.chan-mode').attributes('disabled')).toBeDefined()
    // y el clic, en su carril, a la izquierda: lo demás no se mueve
    const click = w.find('.tl-click-lane .chans')
    await click.find('[aria-label="Derecha"]').trigger('click')
    await flushPromises()
    const after = held.playback.bridge.setRouting.mock.calls.at(-1)[0]
    expect(after.buses[after.click]).toEqual({ left: 0, right: 0 })
    expect(after.buses[after.song]).toEqual({ left: 1, right: 1 })
    localStorage.removeItem('danplay.outputs')
    resetRouting()
  })

  it('the click and the cue of a sequence go with the click, unless told otherwise', async () => {
    const { buildRouting, destOf } = await import('../src/composables/useRouting.js')
    const left = {
      device: null,
      preset: 'click-left',
      custom: { song: 'main', click: 'main', cues: 'main', stems: {} }
    }
    const stems = [
      { key: 'click_track', role: 'click' },
      { key: 'guide', role: 'guide' },
      { key: 'bass', role: '' },
      'drums'
    ]
    const r = buildRouting(left, stems)
    // la musica a la derecha (bus 0), el clic y la guia a la izquierda (bus 1)
    expect(r.spec.buses).toEqual([
      { left: 1, right: 1 },
      { left: 0, right: 0 }
    ])
    expect(r.stems).toEqual({ click_track: 1, guide: 1, bass: 0, drums: 0 })
    // a medida, lo dicho manda
    const custom = {
      ...left,
      preset: 'custom',
      custom: { song: 'main', click: 'L', cues: 'R', stems: { guide: 'main' } }
    }
    expect(destOf(custom, 'stem', 'click_track', 'click')).toBe('L')
    expect(destOf(custom, 'stem', 'guide', 'guide')).toBe('main')
    expect(destOf(custom, 'stem', 'bass', '')).toBe('main')
    // en mono, la guía de la secuencia va como las guías salvo que se diga
    const { monoOf } = await import('../src/composables/useRouting.js')
    const mono = { ...custom, mono: { song: false, click: false, cues: true, stems: {} } }
    expect(monoOf(mono, 'stem', 'guide', 'guide')).toBe(true)
    expect(monoOf(mono, 'stem', 'bass', '')).toBe(false)
    expect(
      monoOf({ ...mono, mono: { ...mono.mono, stems: { guide: false } } }, 'stem', 'guide', 'guide')
    ).toBe(false)
  })
})

describe('opening a sequence', () => {
  it('shows what it contains, opens with its progress and plays in study mode', async () => {
    const { useSequence } = await import('../src/composables/useSequence.js')
    const dialog = await import('../src/composables/useDialog.js')
    const { api } = await import('../src/api.js')
    api.sequenceInfo = vi.fn(async () => ({
      folder: '/d/Mi Cancion-Grupo-G-120.00bpm',
      title: 'Mi Cancion',
      artist: 'Grupo',
      key: 'G',
      bpm: 120,
      project: true,
      cover: true,
      sections: ['Intro', 'Coro'],
      tracks: [
        { name: 'Click Track', role: 'click' },
        { name: 'Guide', role: 'guide' },
        { name: 'Bass', role: '' }
      ]
    }))
    api.importSequence = vi.fn(async () => ({
      job: {
        name: 'sequence',
        active: false,
        done: 5,
        total: 5,
        message: '',
        error: '',
        result: { id: 9, title: 'Mi Cancion', artist: 'Grupo', tracks: 3 }
      }
    }))
    const seq = useSequence()
    const opening = seq.open('/d/Mi Cancion-Grupo-G-120.00bpm/MultiTracks/Bass.wav')
    await flushPromises()
    const asked = dialog.useDialog().dialog.value
    expect(asked.message).toContain('3 pistas (Click Track y Guide van a tus cascos)')
    expect(asked.message).toContain('120 bpm')
    expect(asked.detail).toContain('Intro, Coro')
    dialog.useDialog().dialogOk()
    const made = await opening
    expect(api.importSequence).toHaveBeenCalledWith('/d/Mi Cancion-Grupo-G-120.00bpm')
    expect(made).toMatchObject({ id: 9, tracks: 3 })
    expect(seq.state.busy).toBe(false)
  })
})

describe('the cue lanes', () => {
  const guide = (t, lane, text = 'Coro') => ({
    t,
    file: `/guias/${text}.wav`,
    text,
    voice: 'st-F1',
    ...(lane ? { lane } : {})
  })
  /** jsdom no maqueta: los carriles de guías van de x=0 a x=400, como la onda. */
  function guideWidth(w, width = 400) {
    w.find('.tl-guide-tracks').element.getBoundingClientRect = () => ({
      left: 0,
      width,
      right: width,
      top: 0,
      bottom: 28,
      height: 28,
      x: 0,
      y: 0
    })
  }
  const guideButton = (w) => w.findAll('button').find((b) => b.text().trim() === 'Guía')
  /** Lo último que llegó a Rust de las guías. */
  const sentCues = () => held.playback.bridge.setCues.mock.calls.at(-1)[1]

  it('only with the song stopped: a double click on the lane puts a cue there', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo' })]
    held.playback.emit({ playing: true })
    const w = await mountView()
    // de fábrica, un carril
    expect(w.findAll('.tl-guide-track')).toHaveLength(1)
    expect(w.find('.tl-guide-name').text()).toBe('Guías')
    // sonando, ni el botón ni el doble clic ni el menú de la onda
    expect(guideButton(w).attributes('disabled')).toBeDefined()
    guideWidth(w)
    await w.find('.tl-guide-track').trigger('dblclick', { clientX: 100 })
    await flushPromises()
    expect(w.findComponent({ name: 'StudyCue' }).exists()).toBe(false)
    expect(w.find('.tl-guide-tracks').classes()).not.toContain('editable')

    held.playback.emit({ playing: false })
    await flushPromises()
    expect(guideButton(w).attributes('disabled')).toBeUndefined()
    await w.find('.tl-guide-track').trigger('dblclick', { clientX: 100 })
    await flushPromises()
    const draft = w.findComponent({ name: 'StudyCue' }).props('draft')
    expect(draft).toMatchObject({ t: 50, lane: 'g1', cue: null })
  })

  it('each lane with its name, mute, solo and volume; up to six', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          cues: [guide(10, 'g1', 'Coro'), guide(20, 'g2', 'Puente'), guide(30, '', 'Final')],
          cueLanes: [
            { key: 'g1', name: 'Banda' },
            { key: 'g2', name: 'Baterista', gain: 1.5 }
          ]
        })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    expect(w.findAll('.tl-guide-name').map((b) => b.text())).toEqual(['Banda', 'Baterista'])
    // cada guía en su carril (la que no dice, en el primero)
    const stems = w.findAll('.tl-guide-track')
    expect(stems[0].findAll('.tl-guide-clip').map((c) => c.text())).toEqual(['Coro', 'Final'])
    expect(stems[1].findAll('.tl-guide-clip').map((c) => c.text())).toEqual(['Puente'])
    // a Rust, con el volumen de su carril
    expect(sentCues().map((c) => [c.at, c.gain])).toEqual([
      [10, 1],
      [20, 1.5],
      [30, 1]
    ])

    // callar la banda
    await w.findAll('.tl-guide-lane')[0].findAll('.tl-lane-btn')[0].trigger('click')
    await flushPromises()
    expect(sentCues().map((c) => c.at)).toEqual([20])
    // dejar solo al baterista (y la banda vuelve a sonar: el solo manda)
    await w.findAll('.tl-guide-lane')[0].findAll('.tl-lane-btn')[0].trigger('click')
    await w.findAll('.tl-guide-lane')[1].findAll('.tl-lane-btn')[1].trigger('click')
    await flushPromises()
    expect(sentCues().map((c) => c.at)).toEqual([20])

    // otro carril, hasta seis
    for (let i = 0; i < 5; i++) await button(w, 'Otro carril de guías').trigger('click')
    await flushPromises()
    expect(w.findAll('.tl-guide-track')).toHaveLength(6)
    expect(button(w, 'Otro carril de guías').attributes('disabled')).toBeDefined()

    // y se guarda con la canción (al momento de dejar de tocar)
    await new Promise((r) => setTimeout(r, 700))
    await flushPromises()
    const body = held.api.setStudy.mock.calls.at(-1)[1]
    expect(body.cueLanes).toHaveLength(6)
    expect(body.cueLanes[1]).toMatchObject({ key: 'g2', name: 'Baterista', gain: 1.5, solo: true })
    expect(body.cues.find((c) => c.t === 20).lane).toBe('g2')
  })

  it('dragging a cue moves it, with the song stopped', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ cues: [guide(10, '', 'Coro')] }) })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    guideWidth(w)
    const block = w.find('.tl-guide-clip').element
    block.dispatchEvent(new PointerEvent('pointerdown', { clientX: 20, button: 0, bubbles: true }))
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 60 }))
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 60 }))
    await flushPromises()
    // 40 px de 400 son 20 segundos
    expect(sentCues().map((c) => c.at)).toEqual([30])
    expect(w.findComponent({ name: 'StudyCue' }).exists(), 'arrastrar no la abre').toBe(false)
    // un toque sin arrastrar, sí la abre para cambiarla
    w.find('.tl-guide-clip').element.dispatchEvent(
      new PointerEvent('pointerdown', { clientX: 60, button: 0, bubbles: true })
    )
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 60 }))
    await flushPromises()
    expect(w.findComponent({ name: 'StudyCue' }).props('draft')).toMatchObject({
      t: 30,
      lane: 'g1'
    })
  })

  it('each cue lane on its own side: the drummer one on the left', async () => {
    const { resetRouting } = await import('../src/composables/useRouting.js')
    localStorage.removeItem('danplay.outputs')
    resetRouting()
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          cues: [guide(10, 'g1', 'Coro'), guide(20, 'g2', 'Puente')],
          cueLanes: [
            { key: 'g1', name: 'Banda' },
            { key: 'g2', name: 'Baterista' }
          ]
        })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    const lanes = w.findAll('.tl-guide-lane')
    expect(lanes.map((c) => c.find('.chans').exists())).toEqual([true, true])
    // el baterista, solo por la izquierda (a sus cascos)
    await lanes[1].find('.chans [aria-label="Derecha"]').trigger('click')
    await flushPromises()
    const [loopBand, drummer] = sentCues()
    expect(drummer.bus).not.toBe(loopBand.bus)
    const spec = held.playback.bridge.setRouting.mock.calls.at(-1)[0]
    expect(spec.buses[drummer.bus]).toEqual({ left: 0, right: 0 })
    expect(spec.buses[loopBand.bus]).toEqual({ left: 0, right: 1 })
    localStorage.removeItem('danplay.outputs')
    resetRouting()
  })

  it('«Salidas» is left for the audio output and the usual routings', async () => {
    const w = await mountView()
    await button(w, 'Salidas').trigger('click')
    await flushPromises()
    expect(document.querySelectorAll('.outs-preset').length).toBeGreaterThan(0)
    expect(document.querySelectorAll('.outs-row')).toHaveLength(0)
    expect(document.querySelector('.outs-where').textContent).toContain('L')
  })
})

describe('the waveform style', () => {
  it('is chosen in the study menu and remembered', async () => {
    const { resetPreferences, usePreferences } =
      await import('../src/composables/usePreferences.js')
    localStorage.removeItem('danplay.waveStyle')
    resetPreferences()
    const w = await mountView()
    const { menu } = useContextMenu()
    await w.find('.study-more').trigger('click')
    const style = menu.value.items.find((i) => i.label === 'Estilo de la onda')
    expect(style.note).toBe('barras')
    expect(style.children.map((c) => c.label)).toEqual([
      'Barras',
      'Bloques',
      'Rayitas',
      'Suave',
      'Contorno',
      'Con reflejo'
    ])
    style.children.find((c) => c.label === 'Suave').action()
    await flushPromises()
    expect(usePreferences().waveStyle.value).toBe('smooth')
    expect(localStorage.getItem('danplay.waveStyle')).toBe('smooth')
    // y al volver a abrir la app sigue
    resetPreferences()
    expect(usePreferences().waveStyle.value).toBe('smooth')
    localStorage.removeItem('danplay.waveStyle')
    resetPreferences()
  })
})

describe('the 1.28.2 study mode', () => {
  const guide = (t, lane, text = 'Coro', file = `/guias/${text}.wav`) => ({
    t,
    file,
    text,
    voice: text ? 'st-F1' : '',
    ...(lane ? { lane } : {})
  })
  function guideWidth(w, width = 400) {
    w.find('.tl-guide-tracks').element.getBoundingClientRect = () => ({
      left: 0,
      width,
      right: width,
      top: 0,
      bottom: 28,
      height: 28,
      x: 0,
      y: 0
    })
  }
  const sentCues = () => held.playback.bridge.setCues.mock.calls.at(-1)[1]
  const { menu } = useContextMenu()
  const menuLabels = () => menu.value.items.filter((i) => i.label).map((i) => i.label)
  const option = (label) => menu.value.items.find((i) => i.label === label)

  it('the song controls, above the waveform', async () => {
    const w = await mountView()
    const bar = w.find('.study-transport')
    expect(bar.find('.study-time').text()).toBe('0:12 / 3:20')
    await bar.find('[aria-label="Pausar"]').trigger('click')
    expect(held.playback.bridge.toggle).toHaveBeenCalledTimes(1)
    await flushPromises()
    expect(bar.find('[aria-label="Reproducir"]').exists(), 'el icono cambia').toBe(true)
    await bar.find('[aria-label="5 segundos adelante"]').trigger('click')
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(17)
    await flushPromises()
    await bar.find('[aria-label="5 segundos atrás"]').trigger('click')
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(12)
    await bar.find('[aria-label="Al principio"]').trigger('click')
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(0)
    // detener: se para (si sonaba) y vuelve al principio
    held.playback.emit({ playing: true, position: 40 })
    await flushPromises()
    vi.clearAllMocks()
    await bar.find('[aria-label="Detener"]').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(0)
  })

  it('when it stops by itself at the end of the segment («Al acabar: parar») it says so', async () => {
    const { useNotices } = await import('../src/composables/useNotices.js')
    const { notices } = useNotices()
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 60] }) })]
    held.playback.emit({ playing: true, position: 51 })
    await mountView()
    const before = notices.value.length
    // una pausa a mano, a mitad del tramo: nada que decir (de lo que monta
    // la vista puede llegar otro aviso a la vez: se mira solo este)
    const stopped = () =>
      notices.value.slice(before).filter((n) => n.message.includes('Se paró al acabar el tramo'))
    held.playback.emit({ playing: false, position: 55 })
    await flushPromises()
    expect(stopped()).toEqual([])
    // al acabar el tramo el motor la para y la deja en su principio
    held.playback.emit({ playing: true, position: 59.6 })
    await flushPromises()
    held.playback.emit({ playing: false, position: 50 })
    await flushPromises()
    expect(notices.value.at(-1)?.message).toContain('Se paró al acabar el tramo')
    expect(notices.value.at(-1)?.message).toContain('«Repetir»')
  })

  it('each icon button says what it is for', async () => {
    const w = await mountView()
    const unnamed = w
      .findAll('.study-head button, .study-bar button')
      .filter((b) => !b.text().trim() && !b.attributes('aria-label'))
    expect(unnamed.map((b) => b.html().slice(0, 80))).toEqual([])
    const noNotice = w
      .findAll('.study-head .icon-btn, .study-bar .icon-btn')
      .filter((b) => !b.attributes('title'))
    expect(noNotice.map((b) => b.attributes('aria-label'))).toEqual([])
  })

  it('snapping sticks cues to the click beats when placing and moving them', async () => {
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ cues: [guide(10, '', 'Coro')] }) })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    await flushPromises()
    guideWidth(w)
    const magnet = w.find('.study-magnet')
    expect(magnet.attributes('aria-pressed')).toBe('false')
    expect(w.find('.tl-guide-beat').exists()).toBe(false)
    await magnet.trigger('click')
    expect(magnet.attributes('aria-pressed')).toBe('true')
    expect(JSON.parse(localStorage.getItem('danplay.cues.magnet'))).toBe(true)
    // con el imán se ven los pulsos en los carriles
    expect(w.find('.tl-guide-beat').exists()).toBe(true)

    // doble clic en 50,1 s: la guía cae en el pulso (uno cada medio segundo desde 0,25)
    await w.find('.tl-guide-track').trigger('dblclick', { clientX: 100.2 })
    await flushPromises()
    expect(w.findComponent({ name: 'StudyCue' }).props('draft').t).toBe(50.25)
    expect(w.findComponent({ name: 'StudyCue' }).props('beats').length).toBeGreaterThan(0)
    w.findComponent({ name: 'StudyCue' }).vm.$emit('close')
    await flushPromises()

    // arrastrada a 30,65 s: al pulso de 30,75
    const block = w.find('.tl-guide-clip').element
    block.dispatchEvent(new PointerEvent('pointerdown', { clientX: 20, button: 0, bubbles: true }))
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 61.3 }))
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 61.3 }))
    await flushPromises()
    expect(sentCues().map((c) => c.at)).toEqual([30.75])
  })

  it('lane options: a cue here, import a short audio file, rename…', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo' })]
    held.playback.emit({ playing: false })
    held.audioPath = '/home/yo/cuenta.wav'
    try {
      const w = await mountView()
      guideWidth(w)
      await w.find('.tl-lane-more').trigger('click')
      expect(menu.value.title).toBe('Guías')
      expect(menuLabels()).toEqual([
        'Guía aquí…',
        'Importar un audio…',
        'Renombrar…',
        'Callarlo',
        'Otro carril de guías'
      ])
      // clic derecho en el carril: en ese punto
      await w.find('.tl-guide-track').trigger('contextmenu', { clientX: 100 })
      expect(option('Guía aquí…').note).toBe('0:50')
      await option('Importar un audio…').action()
      await flushPromises()
      expect(held.api.importCue).toHaveBeenCalledWith(7, '/home/yo/cuenta.wav', 200)
      // se abre en su editor, para recortarlo antes de ponerlo
      expect(w.findComponent({ name: 'StudyCue' }).props('draft')).toMatchObject({
        t: 50,
        lane: 'g1',
        file: '/guias/importada.wav',
        seconds: 3
      })
      expect(w.findComponent({ name: 'StudyCue' }).props('songLength')).toBe(200)
    } finally {
      held.audioPath = null
    }
  })

  it('while playing, cues cannot be placed from the lane menu', async () => {
    const w = await mountView()
    await w.find('.tl-lane-more').trigger('click')
    expect(option('Guía aquí…').disabled).toBe(true)
    expect(option('Importar un audio…').note).toBe('para la canción')
  })

  it('right-click on a cue: preview, edit, move to another lane or remove', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          cues: [guide(10, 'g1', '', '/guias/grabada-1.wav')],
          cueLanes: [
            { key: 'g1', name: 'Banda' },
            { key: 'g2', name: 'Baterista' }
          ]
        })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    await w.find('.tl-guide-clip').trigger('contextmenu')
    expect(menuLabels()).toEqual([
      'Escuchar',
      'Editar…',
      'Ir aquí',
      'Pasar a otro carril',
      'Quitar'
    ])
    expect(option('Editar…').note).toBe('recortar, fundidos, limpiar')
    await option('Escuchar').action()
    expect(held.api.previewCue).toHaveBeenCalledWith('/guias/grabada-1.wav')
    option('Pasar a otro carril').children[0].action()
    await flushPromises()
    const stems = w.findAll('.tl-guide-track')
    expect(stems[1].findAll('.tl-guide-clip')).toHaveLength(1)
    await stems[1].find('.tl-guide-clip').trigger('contextmenu')
    await option('Quitar').action()
    await flushPromises()
    expect(w.findAll('.tl-guide-clip')).toHaveLength(0)
  })

  it('the cue list: «Guías», and clicking one goes to it in its lane', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          cues: [guide(150, '', 'Final'), guide(20, '', '', '/guias/g.wav')]
        })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    const list = w.findAll('.study-cue')
    expect(w.text()).not.toContain('Guías habladas')
    expect(list.map((c) => c.find('.study-pick-name').text())).toEqual(['audio', 'Final'])
    await list[1].find('.study-pick').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(150)
    const final = w.findAll('.tl-guide-clip').find((c) => c.text() === 'Final')
    expect(final.classes()).toContain('flash')
    // escucharla desde la lista
    await list[1].find('[aria-label="Escucharla"]').trigger('click')
    expect(held.api.previewCue).toHaveBeenCalledWith('/guias/Final.wav')
  })

  it('each cue shows with its waveform and its real length', async () => {
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({ cues: [guide(10, '', '', '/guias/larga.wav')] })
      })
    ]
    held.playback.emit({ playing: false })
    const w = await mountView()
    await flushPromises()
    expect(held.api.cueWave).toHaveBeenCalledWith('/guias/larga.wav')
    const block = w.find('.tl-guide-clip')
    // 8 s de 200: el 4 % del ancho
    expect(block.attributes('style')).toContain('width: 4%')
    expect(block.find('.tl-guide-wave path').attributes('d')).toMatch(/^M0,50 L/)
    expect(block.classes()).toContain('waved')
  })
})

describe('exporting', () => {
  const withGuides = (extra = {}) =>
    JSON.stringify({
      cues: [{ t: 12, file: '/guias/coro.wav', text: 'Coro' }],
      cueLanes: [{ key: 'g1', name: 'Banda', gain: 1.5 }],
      ...extra
    })
  /** Abre «Exportar…» del menú del estudio (la ventana). */
  async function open(w) {
    const { menu } = useContextMenu()
    await w.find('.study-more').trigger('click')
    const item = menu.value.items.find((i) => i.label === 'Exportar…')
    expect(item, 'un solo «Exportar…»').toBeTruthy()
    expect(menu.value.items.filter((i) => /export/i.test(i.label || ''))).toHaveLength(1)
    item.action()
    await flushPromises()
  }
  const extra = (name) =>
    [...document.querySelectorAll('.exp-extra')].find((e) => e.textContent.includes(name))
  async function save() {
    document.querySelector('.exp-go').click()
    await flushPromises()
    await flushPromises()
  }
  beforeEach(() => localStorage.removeItem('danplay.export'))
  afterEach(() => {
    held.savePath = null
  })

  it('the song as is, with its cues if wanted', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: withGuides() })]
    held.playback.emit({ playing: false })
    const w = await mountView()
    held.savePath = '/musica/Mi Gozo (con guias).mp3'
    await open(w)
    // qué: la canción (sin pistas, lo demás no se puede); con qué: las guías
    const kinds = [...document.querySelectorAll('.exp-kind')]
    expect(kinds.map((k) => k.disabled)).toEqual([false, true, true])
    expect(extra('Las guías').getAttribute('aria-checked')).toBe('true')
    await save()
    expect(held.api.exportMix).toHaveBeenLastCalledWith(7, {
      tracks: [{ source: 'song', gain: 1, pan: 0 }],
      path: '/musica/Mi Gozo (con guias).mp3',
      cues: [{ at: 12, file: '/guias/coro.wav', gain: 1.5 }]
    })
  })

  it('with cues unticked, no cues; and with no cues made, they are not even offered', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: withGuides() })]
    held.playback.emit({ playing: false })
    let w = await mountView()
    held.savePath = '/musica/Mi Gozo.mp3'
    await open(w)
    extra('Las guías').click()
    await flushPromises()
    await save()
    expect(held.api.exportMix.mock.calls.at(-1)[1].cues).toBeUndefined()
    // y se recuerda para la próxima
    expect(JSON.parse(localStorage.getItem('danplay.export')).guides).toBe(false)
    w.unmount()

    held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
    w = await mountView()
    await open(w)
    expect(extra('Las guías'), 'sin guías no hay nada que elegir').toBeUndefined()
    expect(document.querySelector('.exp-none').textContent).toContain('Tal cual')
    await save()
    expect(held.api.exportMix).toHaveBeenLastCalledWith(7, {
      tracks: [{ source: 'song', gain: 1, pan: 0 }],
      path: '/musica/Mi Gozo.mp3'
    })
  })

  it('with the channels: each thing on its own side, as it plays', async () => {
    const { resetRouting, useRouting } = await import('../src/composables/useRouting.js')
    localStorage.removeItem('danplay.outputs')
    resetRouting()
    held.state.songs = [song(7, { title: 'Mi Gozo', study: withGuides() })]
    held.playback.emit({ playing: false })
    const w = await mountView()
    // la canción a la derecha y las guías a la izquierda, como una secuencia
    const r = useRouting()
    await r.setRoute('song', '', 'R')
    await r.setRoute('stem', 'cues_g1', 'L')
    await flushPromises()
    held.savePath = '/musica/Mi Gozo (secuencia).mp3'
    await open(w)
    extra('Los canales').click()
    await flushPromises()
    await save()
    expect(held.api.exportMix).toHaveBeenLastCalledWith(7, {
      tracks: [{ source: 'song', gain: 1, pan: 0, side: 'R', mono: true }],
      path: '/musica/Mi Gozo (secuencia).mp3',
      cues: [{ at: 12, file: '/guias/coro.wav', gain: 1.5, side: 'L' }]
    })
    localStorage.removeItem('danplay.outputs')
    resetRouting()
  })
})
