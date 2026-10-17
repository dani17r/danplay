import { describe, it, expect, vi, beforeEach, afterEach, onTestFinished } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El modo estudio de la 1.30: cada cosa por su lado (L, R, estéreo o mono,
// en su carril), «ir con un clic» con la onda bloqueada, ocultar la mezcla y
// los canales, el carril del clic, la guía automática y la voz de las guías,
// los menús cortos (lo puesto, marcado) y el recorrido.
const held = vi.hoisted(() => ({ playback: null, api: null, state: null, savePath: null }))

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
    pickSavePath: async () => held.savePath
  }
})

import StudyBar from '../src/components/StudyBar.vue'
import ContextMenu from '../src/components/ui/ContextMenu.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { useContextMenu } from '../src/composables/useContextMenu.js'
import {
  useRouting,
  resetRouting,
  channelsOf,
  destOfChannels,
  sideOf,
  targetOf
} from '../src/composables/useRouting.js'
import { stopStudyTour } from '../src/composables/useStudyTour.js'
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
const { menu } = useContextMenu()
const opcion = (label) => menu.value.items.find((i) => i.label === label)

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  resetPlayback()
  resetClick()
  resetRouting()
  held.playback.reset()
  held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
  held.playback.emit({
    track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
    playing: false,
    position: 12,
    ...base
  })
})
const mounted = []
const montar = async () => {
  const w = mount(StudyBar, { attachTo: document.body })
  mounted.push(w)
  await flushPromises()
  await flushPromises()
  return w
}
afterEach(() => {
  stopStudyTour()
  for (const w of mounted.splice(0)) {
    try {
      w.unmount()
    } catch {
      /* ya estaba */
    }
  }
  document.body.innerHTML = ''
})
const routing = () => held.playback.bridge.setRouting.mock.calls.at(-1)[0]

describe('los canales, en números', () => {
  it('lo que se enciende y el destino van y vuelven', () => {
    expect(channelsOf('main')).toEqual([1, 2])
    expect(channelsOf('L')).toEqual([1])
    expect(channelsOf('R')).toEqual([2])
    expect(channelsOf('p3')).toEqual([3, 4])
    expect(channelsOf('m4')).toEqual([4])
    expect(channelsOf('off')).toEqual([])
    expect(destOfChannels([2, 1])).toBe('main')
    expect(destOfChannels([1])).toBe('L')
    expect(destOfChannels([2])).toBe('R')
    expect(destOfChannels([3, 4])).toBe('p3')
    expect(destOfChannels([3])).toBe('m3')
    expect(destOfChannels([2, 3])).toBe('x2-3')
    expect(destOfChannels([])).toBe('off')
    // dos cualesquiera: el primero a la izquierda
    expect(targetOf('x2-3')).toEqual({ left: 1, right: 2 })
    expect(targetOf('x2-3', true)).toEqual({ left: 1, right: 2, mono: true })
    expect(channelsOf('x2-3')).toEqual([2, 3])
  })

  it('al exportar a un archivo estéreo: los dos lados, la izquierda o la derecha', () => {
    expect(sideOf('main')).toBe('both')
    expect(sideOf('L')).toBe('L')
    expect(sideOf('R')).toBe('R')
    // de una interfaz: las impares a la izquierda, las pares a la derecha
    expect(sideOf('p3')).toBe('both')
    expect(sideOf('m3')).toBe('L')
    expect(sideOf('m4')).toBe('R')
    expect(sideOf('off')).toBe(null)
  })

  it('cambiar una cosa desde un reparto de fábrica no mueve las demás', async () => {
    const r = useRouting()
    await r.update({ preset: 'click-left' })
    // la canción, a los dos lados: el clic y las guías siguen a la izquierda
    await r.setRoute('song', '', 'main')
    expect(r.prefs.value.preset).toBe('custom')
    expect(r.routeOf('click').dest).toBe('L')
    expect(r.routeOf('cues').dest).toBe('L')
    expect(r.routeOf('song').dest).toBe('main')
  })
})

describe('los botones de canal de cada carril', () => {
  it('con una interfaz de cuatro salidas: 1, 2, 3, 4, y un tercero apaga el más antiguo', async () => {
    const before = held.playback.bridge.outputDevices
    held.playback.bridge.outputDevices = vi.fn(async () => [
      { id: 'iface', name: 'Interfaz', channels: 4, default: true }
    ])
    onTestFinished(() => {
      held.playback.bridge.outputDevices = before
    })
    const w = await montar()
    await flushPromises()
    const cancion = w.find('.tl-song-head .chans')
    const b = (n) => cancion.findAll('.chan').find((x) => x.text() === n)
    expect(cancion.findAll('.chan:not(.chan-mode)').map((x) => x.text())).toEqual([
      '1',
      '2',
      '3',
      '4'
    ])
    // de 1-2 a 2-3 (encender la 3 apaga la 1, la más antigua)
    await b('3').trigger('click')
    await flushPromises()
    expect(routing().buses[routing().song]).toEqual({ left: 1, right: 2 })
    // y la 3 sola, en mono
    await b('2').trigger('click')
    await flushPromises()
    expect(routing().buses[routing().song]).toEqual({ left: 2, right: 2 })
    expect(held.playback.bridge.setOutput).toHaveBeenLastCalledWith({ device: null, channels: 3 })
  })

  it('apagarlos todos la calla en la salida', async () => {
    const w = await montar()
    const cancion = () => w.find('.tl-song-head .chans')
    await cancion().find('[aria-label="Izquierda"]').trigger('click')
    await flushPromises()
    await cancion().find('[aria-label="Derecha"]').trigger('click')
    await flushPromises()
    expect(routing().buses[routing().song]).toBe(null)
    expect(cancion().attributes('title')).toContain('no suena')
  })
})

describe('la onda', () => {
  function conAnchura(w) {
    w.find('.tl-box').element.getBoundingClientRect = () => ({
      left: 0,
      width: 400,
      right: 400,
      top: 0,
      height: 80,
      bottom: 80,
      x: 0,
      y: 0
    })
  }
  const pulsa = async (w, x) => {
    const box = w.find('.tl-box').element
    box.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, clientX: x, clientY: 20 }))
    window.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, clientX: x, clientY: 20 }))
    await flushPromises()
  }

  it('con el candado, «ir con un clic» deja saltar sonando sin quitar el tramo', async () => {
    held.state.songs = [song(7, { title: 'Mi Gozo', study: JSON.stringify({ loop: [50, 60] }) })]
    held.playback.emit({ playing: true })
    const w = await montar()
    conAnchura(w)
    // sin el candado puesto no se ofrece
    expect(w.find('.tl-lock').classes()).toContain('on')
    const ir = w.find('.tl-seek')
    expect(ir.exists()).toBe(true)
    expect(ir.attributes('aria-pressed')).toBe('false')
    // con «ir con un clic», bloqueada y sonando, un clic lleva ahí y el tramo sigue
    await ir.trigger('click')
    expect(JSON.parse(localStorage.getItem('danplay.study.irConClic'))).toBe(true)
    held.playback.bridge.seek.mockClear()
    held.playback.bridge.setLoop.mockClear()
    await pulsa(w, 100)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(50)
    expect(held.playback.bridge.setLoop, 'el tramo no se quita').not.toHaveBeenCalled()
    // sin él, bloqueada y sonando, un clic no mueve la canción
    await ir.trigger('click')
    held.playback.bridge.seek.mockClear()
    await pulsa(w, 100)
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    // quitar el candado lo esconde
    await w.find('.tl-lock').trigger('click')
    expect(w.find('.tl-seek').exists()).toBe(false)
  })

  it('«Mezcla» y «Canales» ocultan sus mandos de los carriles, y se recuerda', async () => {
    const w = await montar()
    const tl = () => w.find('.tl')
    expect(tl().classes()).not.toContain('tl-no-mix')
    const anchoCon = tl().attributes('style')
    await w.find('.study-show-mix').trigger('click')
    expect(tl().classes()).toContain('tl-no-mix')
    await w.find('.study-show-routes').trigger('click')
    expect(w.find('.chans').exists()).toBe(false)
    expect(tl().attributes('style')).not.toBe(anchoCon)
    expect(JSON.parse(localStorage.getItem('danplay.study.verMezcla'))).toBe(false)
    expect(JSON.parse(localStorage.getItem('danplay.study.verCanales'))).toBe(false)
  })

  it('el carril del clic: encenderlo y su volumen', async () => {
    const w = await montar()
    const clic = w.find('.tl-click-lane')
    expect(clic.classes()).toContain('off')
    await clic.find('.tl-lane-btn').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ on: true })
    )
    held.playback.bridge.setMetronome.mockClear()
    const slider = w
      .findAllComponents({ name: 'SliderField' })
      .find((c) => c.element.closest?.('.tl-click-lane'))
    slider.vm.$emit('update:modelValue', 0.5)
    await flushPromises()
    expect(held.playback.bridge.setMetronome).toHaveBeenCalledWith(
      expect.objectContaining({ volume: 0.5 })
    )
  })
})

describe('los menús', () => {
  it('lo puesto va marcado, y lo largo, en su aviso', async () => {
    const w = mount(ContextMenu, {
      props: {
        open: true,
        title: 'Estilo de la onda',
        items: [
          { label: 'Barras', active: true, hint: 'La de siempre' },
          { label: 'Suave', note: 'una nota larga que no cabe junto al nombre' }
        ]
      },
      attachTo: document.body
    })
    await flushPromises()
    const [barras, suave] = w.findAll('.ctx-item')
    expect(barras.classes()).toContain('on')
    expect(barras.attributes('aria-current')).toBe('true')
    expect(barras.find('.ctx-check').exists()).toBe(true)
    expect(barras.attributes('title')).toBe('La de siempre')
    expect(suave.attributes('title')).toContain('una nota larga')
    expect(suave.find('.ctx-check').exists()).toBe(false)
    w.unmount()
  })

  it('el del estudio: corto, con un solo «Exportar…»', async () => {
    const w = await montar()
    await w.find('.study-more').trigger('click')
    const labels = menu.value.items.filter((i) => i.label).map((i) => i.label)
    expect(labels).toEqual([
      'Exportar…',
      'Guía automática',
      'Salidas…',
      'Estilo de la onda',
      'Cómo se usa',
      'Salir del modo estudio'
    ])
    expect(opcion('Salir del modo estudio').note).toBeUndefined()
    expect(opcion('Salir del modo estudio').hint).toContain('clic')
    // el estilo puesto, marcado
    expect(opcion('Estilo de la onda').children.filter((c) => c.active)).toHaveLength(1)
  })

  it('para el celular, desde la ventana de exportar', async () => {
    const { stemsOf } = await import('./support/backend.js')
    held.state.songs = [
      song(7, { title: 'Mi Gozo', study: JSON.stringify({ mixer: { on: true } }), has_stems: true })
    ]
    held.state.stems = { 7: stemsOf(7) }
    const w = await montar()
    await w.find('.study-more').trigger('click')
    opcion('Exportar…').action()
    await flushPromises()
    const cel = [...document.querySelectorAll('.exp-kind')].find((k) =>
      k.textContent.includes('Para el celular')
    )
    expect(cel.disabled).toBe(false)
    cel.click()
    await flushPromises()
    expect(document.querySelector('.exp-extras')).toBeNull()
    expect(document.querySelector('.exp-go').textContent).toContain('celular')
  })
})

describe('la guía automática', () => {
  const resultado = {
    sections: [
      { start: 0, end: 8, label: 'Intro' },
      { start: 8, end: 40, label: 'Estrofa 1' },
      { start: 40, end: 70, label: 'Coro 1' }
    ],
    cues: [
      { t: 6.5, text: 'Estrofa 1', voice: 'st-F1', file: '/guias/estrofa-1.wav', seconds: 1 },
      { t: 38.4, text: 'Coro 1', voice: 'st-F1', file: '/guias/coro-1.wav', seconds: 0.8 }
    ]
  }
  function preparar() {
    held.api.cueVoices.mockImplementation(async () => ({
      voices: [
        { id: 'st-F1', name: 'Mujer 1', engine: 'supertonic', installed: true },
        { id: 'st-M1', name: 'Hombre 1', engine: 'supertonic', installed: true }
      ],
      available: ''
    }))
    held.api.autoCues.mockImplementation(async () => ({ job: { name: 'guia-automatica' } }))
    held.api.runJob.mockImplementation(async (start) => {
      await start()
      return resultado
    })
  }

  it('con la voz ya elegida, la hace sin preguntar, en su carril «Partes»', async () => {
    preparar()
    localStorage.setItem('danplay.guias.voz', JSON.stringify('st-F1'))
    const w = await montar()
    await w.find('.study-auto-guide').trigger('click')
    await flushPromises()
    expect(document.querySelector('.vp-list'), 'sin preguntar la voz').toBeNull()
    expect(held.api.autoCues).toHaveBeenCalledWith(7, expect.objectContaining({ voice: 'st-F1' }))
    // el carril de fábrica estaba vacío: pasa a ser el de las partes
    expect(w.findAll('.tl-guide-name').map((n) => n.text())).toEqual(['Partes'])
    expect(w.findAll('.tl-guide-clip').map((c) => c.text())).toEqual(['Estrofa 1', 'Coro 1'])
    // otra vez: la rehace (no se duplican)
    await w.find('.study-auto-guide').trigger('click')
    await flushPromises()
    expect(w.findAll('.tl-guide-clip')).toHaveLength(2)
    await new Promise((r) => setTimeout(r, 700))
    await flushPromises()
    const body = held.api.setStudy.mock.calls.at(-1)[1]
    expect(body.cueLanes[0]).toMatchObject({ name: 'Partes', auto: true })
  })

  it('sin voz elegida, pregunta cuál (y se recuerda)', async () => {
    preparar()
    const w = await montar()
    await w.find('.study-auto-guide').trigger('click')
    await flushPromises()
    const voces = [...document.querySelectorAll('.vp-pick')]
    expect(voces.map((v) => v.textContent)).toEqual([
      expect.stringContaining('Mujer 1'),
      expect.stringContaining('Hombre 1')
    ])
    voces[1].click()
    await flushPromises()
    document.querySelector('.vp-go').click()
    await flushPromises()
    await flushPromises()
    expect(held.api.autoCues).toHaveBeenCalledWith(7, expect.objectContaining({ voice: 'st-M1' }))
    expect(JSON.parse(localStorage.getItem('danplay.guias.voz'))).toBe('st-M1')
    expect(w.findAll('.tl-guide-clip')).toHaveLength(2)
  })

  it('sonando no se hace (las guías, con la canción parada)', async () => {
    preparar()
    held.playback.emit({ playing: true })
    const w = await montar()
    expect(w.find('.study-auto-guide').attributes('disabled')).toBeDefined()
  })

  it('«Cambiar la voz…» en un carril: sus guías escritas, dichas con otra', async () => {
    preparar()
    held.api.makeCue.mockImplementation(async (id, b) => ({
      file: `/guias/${b.text.toLowerCase()}-${b.voice}.wav`,
      seconds: 1
    }))
    held.state.songs = [
      song(7, {
        title: 'Mi Gozo',
        study: JSON.stringify({
          cues: [
            { t: 10, file: '/guias/coro.wav', text: 'Coro', voice: 'st-F1', lane: 'g1' },
            { t: 20, file: '/guias/grabada-1.wav', lane: 'g1' }
          ],
          cueLanes: [{ key: 'g1', name: 'Banda' }]
        })
      })
    ]
    const w = await montar()
    await w.find('.tl-lane-more').trigger('click')
    expect(opcion('Cambiar la voz…')).toBeTruthy()
    opcion('Cambiar la voz…').action()
    await flushPromises()
    ;[...document.querySelectorAll('.vp-pick')][1].click()
    await flushPromises()
    document.querySelector('.vp-go').click()
    await flushPromises()
    await flushPromises()
    // solo la escrita (la grabada no tiene voz que cambiar)
    expect(held.api.makeCue).toHaveBeenCalledTimes(1)
    expect(held.api.makeCue).toHaveBeenCalledWith(7, { text: 'Coro', voice: 'st-M1' })
    const sent = held.playback.bridge.setCues.mock.calls.at(-1)[1]
    expect(sent.map((c) => c.path)).toContain('/guias/coro-st-M1.wav')
  })
})

describe('el recorrido', () => {
  it('«?» lo abre, con sus pasos y sus animaciones', async () => {
    const w = await montar()
    await w.find('.study-help').trigger('click')
    await flushPromises()
    const pop = document.querySelector('.driver-popover')
    expect(pop).not.toBeNull()
    expect(pop.textContent).toContain('El modo estudio')
    expect(pop.querySelector('.tour-demo')).not.toBeNull()
    stopStudyTour()
    expect(localStorage.getItem('danplay.study.recorridoVisto')).toBe('true')
  })

  it('no sale solo en las pruebas (sin medidas), y una vez visto no vuelve a salir solo', async () => {
    await montar()
    await new Promise((r) => setTimeout(r, 1000))
    expect(document.querySelector('.driver-popover')).toBeNull()
  })
})
