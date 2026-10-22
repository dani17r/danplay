// La tarjeta de una guía (StudyCue): escribirla y que la diga una voz
// (bajándola la primera vez), o grabarla con el micrófono o importar un
// audio, y arreglarlo (recortar, fundidos, quitar ruido y graves, igualar el
// volumen); escucharla, guardarla, moverla de sitio y quitarla.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

const held = vi.hoisted(() => ({ voices: [], unavailable: '', api: null, app: null, picked: null }))
vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { vi: v } = await import('vitest')
  held.api = {
    cueVoices: v.fn(async () => ({
      voices: held.voices.map((x) => ({ ...x })),
      available: held.unavailable
    })),
    installCueVoice: v.fn(async (id) => {
      const voice = held.voices.find((x) => x.id === id)
      voice.progress = 0.4
      return { ok: true }
    }),
    makeCue: v.fn(async (id, body) => ({
      file: body.preview ? '/tmp/prueba.wav' : '/guias/coro.wav'
    })),
    previewCue: v.fn(async () => 1.5),
    stopPreview: v.fn(async () => {}),
    importCue: v.fn(async () => ({ file: '/guias/grabada.wav', seconds: 4 })),
    cueWave: v.fn(async () => ({ seconds: 4, peaks: [0.2, 1, 0.5, 0.1] })),
    editCue: v.fn(async (body) => ({
      file: body.preview ? '/guias/prueba/editada.wav' : '/guias/editada.wav',
      seconds: 2
    }))
  }
  held.app = {
    canRecord: true,
    recordStart: v.fn(async () => {}),
    recordLevel: v.fn(async () => 0.3),
    recordStop: v.fn(async () => '/tmp/grabacion.wav')
  }
  return { ...actual, api: held.api, app: held.app, pickAudio: v.fn(async () => held.picked) }
})

import StudyCue from '../src/components/StudyCue.vue'
import { useNotices, clearNotices } from '../src/composables/useNotices.js'

const { notices } = useNotices()
const NATURAL = {
  id: 'supertonic-M1',
  name: 'Alberto',
  lang: 'es',
  engine: 'supertonic',
  installed: true,
  size: 401e6,
  progress: null
}
const DAVID = {
  id: 'piper-es_ES-davefx-medium',
  name: 'David · España',
  lang: 'es-ES',
  engine: 'piper',
  installed: false,
  size: 63e6,
  progress: null
}

const mounted = []
async function card(draft = {}, props = {}) {
  const w = mount(StudyCue, {
    props: {
      draft: { t: 12, text: '', voice: '', cue: null, ...draft },
      songId: 7,
      songLength: 240,
      ...props
    },
    attachTo: document.body
  })
  mounted.push(w)
  await flushPromises()
  return w
}
const button = (w, text) =>
  w.findAll('button').find((b) => b.text().trim() === text || b.attributes('aria-label') === text)
const last = () => notices.value.at(-1)?.message || ''

beforeEach(() => {
  held.voices = [{ ...NATURAL }, { ...DAVID }]
  held.unavailable = ''
  held.picked = null
  clearNotices()
  vi.clearAllMocks()
})
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
  document.body.innerHTML = ''
})

describe('writing a cue', () => {
  it('with the already downloaded voice: it is picked automatically, previewed and saved', async () => {
    const w = await card()
    expect(w.find('.cue-head').text()).toContain('Guía')
    const save = button(w, 'Guardar')
    expect(save.attributes('disabled'), 'sin texto no se guarda').toBeDefined()

    // una de las sugeridas
    await button(w, 'Coro').trigger('click')
    expect(w.find('input').element.value).toBe('Coro')
    expect(button(w, 'Guardar').attributes('disabled')).toBeUndefined()

    await button(w, 'Escuchar').trigger('click')
    await flushPromises()
    expect(held.api.makeCue).toHaveBeenCalledWith(7, {
      text: 'Coro',
      voice: 'supertonic-M1',
      preview: true
    })
    expect(held.api.previewCue).toHaveBeenCalledWith('/tmp/prueba.wav')

    // un poco antes de donde entra la parte
    await w.find('button[title="Medio segundo antes"]').trigger('click')
    await w.find('button[title="Medio segundo antes"]').trigger('click')
    await w.find('button[title="Medio segundo después"]').trigger('click')
    await button(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(held.api.makeCue).toHaveBeenLastCalledWith(7, { text: 'Coro', voice: 'supertonic-M1' })
    expect(w.emitted('made')[0]).toEqual([
      { t: 11.5, text: 'Coro', voice: 'supertonic-M1', file: '/guias/coro.wav', cue: null }
    ])
  })

  it('«aquí» places it at the current song position', async () => {
    const w = await card({ t: 40 })
    await w.find('button[title="Donde va la canción"]').trigger('click')
    await button(w, 'Final').trigger('click')
    await button(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(w.emitted('made')[0][0].t).toBe(0)
  })

  it('a voice not yet downloaded: its size is shown, it downloads and progress is shown', async () => {
    const w = await card({ voice: DAVID.id, text: 'Puente' })
    expect(w.text()).toContain('Esta voz se baja una sola vez (63 MB)')
    expect(button(w, 'Guardar').attributes('disabled'), 'sin la voz no se guarda').toBeDefined()
    // con Enter tampoco: se dice qué falta
    await w.find('input').trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(last()).toBe('Baja antes la voz')

    await w.find('.cue-get button').trigger('click')
    await flushPromises()
    expect(held.api.installCueVoice).toHaveBeenCalledWith(DAVID.id)
    expect(w.text()).toContain('Bajando la voz… 40 %')

    // termina: la siguiente pregunta (cada segundo) ya la ve lista
    held.voices[1].installed = true
    await new Promise((r) => setTimeout(r, 1100))
    await flushPromises()
    expect(w.text()).not.toContain('Bajando la voz')
    expect(button(w, 'Guardar').attributes('disabled')).toBeUndefined()
  })

  it('the natural voices warn that all ten download together', async () => {
    held.voices = [{ ...NATURAL, installed: false, error: 'Se cortó la descarga' }]
    const w = await card()
    expect(w.text()).toContain('Se cortó la descarga.')
    expect(w.text()).toContain('y con ella las diez voces naturales')
  })

  it('if this installation cannot do it, it says so', async () => {
    held.unavailable = 'falta onnxruntime'
    const w = await card()
    expect(w.text()).toContain('No se pueden decir guías aquí: falta onnxruntime')
  })

  it('without text nothing is made, and a core failure is reported', async () => {
    const w = await card()
    await w.find('input').trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(last()).toBe('Escribe lo que se dirá')
    expect(held.api.makeCue).not.toHaveBeenCalled()

    held.api.makeCue.mockRejectedValueOnce(new Error('sin espacio'))
    await button(w, 'Coro').trigger('click')
    await button(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(last()).toBe('No se pudo hacer la guía: sin espacio')
    expect(w.emitted('made')).toBeFalsy()
  })

  it('a failed voice download says so', async () => {
    held.api.installCueVoice.mockRejectedValueOnce(new Error('sin red'))
    const w = await card({ voice: DAVID.id })
    await w.find('.cue-get button').trigger('click')
    await flushPromises()
    expect(last()).toBe('No se pudo bajar la voz: sin red')
  })
})

describe('recording it with the microphone', () => {
  it('record and stop: the recording comes back right away, with its waveform, to listen to and save', async () => {
    const w = await card()
    await button(w, 'Grabar o importar').trigger('click')
    expect(w.text()).toContain('Graba tu voz o trae un audio corto')
    expect(button(w, 'Guardar').attributes('disabled')).toBeDefined()

    await button(w, 'Grabar').trigger('click')
    await flushPromises()
    expect(held.app.recordStart).toHaveBeenCalled()
    expect(w.text()).toContain('Habla ahora…')
    // el medidor y lo que lleva se mueven mientras se graba
    await new Promise((r) => setTimeout(r, 120))
    await flushPromises()
    expect(held.app.recordLevel).toHaveBeenCalled()
    expect(w.find('.cue-rec-live').exists()).toBe(true)

    await button(w, 'Parar').trigger('click')
    await flushPromises()
    expect(held.app.recordStop).toHaveBeenCalled()
    // se trae a las guías en cuanto se para (hasta lo que dura la canción)
    expect(held.api.importCue).toHaveBeenCalledWith(7, '/tmp/grabacion.wav', 240)
    expect(held.api.cueWave).toHaveBeenCalledWith('/guias/grabada.wav', 240)
    expect(w.find('.cue-editor').exists()).toBe(true)
    expect(w.find('.cue-wave-shape').attributes('d')).toMatch(/^M0,50 L/)
    expect(button(w, 'Grabar otra vez')).toBeTruthy()

    // sin tocar nada, se escucha y se guarda tal cual
    await button(w, 'Escuchar').trigger('click')
    await flushPromises()
    expect(held.api.previewCue).toHaveBeenCalledWith('/guias/grabada.wav')
    expect(held.api.editCue).not.toHaveBeenCalled()
    expect(button(w, 'Parar'), 'mientras suena, se puede parar').toBeTruthy()
    await button(w, 'Parar').trigger('click')
    expect(held.api.stopPreview).toHaveBeenCalled()

    await button(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(held.api.makeCue).not.toHaveBeenCalled()
    expect(w.emitted('made')[0][0]).toMatchObject({
      text: '',
      voice: '',
      file: '/guias/grabada.wav'
    })
  })

  it('if the microphone fails, it says so and does not keep recording', async () => {
    held.app.recordStart.mockRejectedValueOnce(new Error('sin micrófono'))
    const w = await card()
    await button(w, 'Grabar o importar').trigger('click')
    await button(w, 'Grabar').trigger('click')
    await flushPromises()
    expect(last()).toBe('No se pudo grabar: sin micrófono')
    expect(w.text()).not.toContain('Habla ahora')
  })

  it('if the recording is not valid (too long), it says so', async () => {
    held.api.importCue.mockRejectedValueOnce(new Error('ese audio dura más que la canción'))
    const w = await card()
    await button(w, 'Grabar o importar').trigger('click')
    await button(w, 'Grabar').trigger('click')
    await flushPromises()
    await button(w, 'Parar').trigger('click')
    await flushPromises()
    expect(last()).toBe('No se pudo usar lo grabado: ese audio dura más que la canción')
    expect(w.find('.cue-editor').exists()).toBe(false)
  })

  it('closing mid-recording stops it', async () => {
    const w = await card()
    await button(w, 'Grabar o importar').trigger('click')
    await button(w, 'Grabar').trigger('click')
    await flushPromises()
    w.unmount()
    mounted.splice(mounted.indexOf(w), 1)
    expect(held.app.recordStop).toHaveBeenCalled()
  })
})

describe('importing an audio file and fixing it', () => {
  it('an audio file from the computer, up to the song length', async () => {
    held.picked = '/home/yo/cuenta.mp3'
    const w = await card()
    await button(w, 'Grabar o importar').trigger('click')
    expect(button(w, 'Importar un audio').attributes('title')).toContain('como mucho 4:00')
    await button(w, 'Importar un audio').trigger('click')
    await flushPromises()
    expect(held.api.importCue).toHaveBeenCalledWith(7, '/home/yo/cuenta.mp3', 240)
    expect(w.find('.cue-editor').exists()).toBe(true)
    expect(button(w, 'Importar otro')).toBeTruthy()
  })

  it('cancelling the picker does nothing; an invalid audio file is reported', async () => {
    const w = await card()
    await button(w, 'Grabar o importar').trigger('click')
    await button(w, 'Importar un audio').trigger('click')
    await flushPromises()
    expect(held.api.importCue).not.toHaveBeenCalled()
    held.picked = '/home/yo/largo.wav'
    held.api.importCue.mockRejectedValueOnce(new Error('ese audio dura más que la canción'))
    await button(w, 'Importar un audio').trigger('click')
    await flushPromises()
    expect(last()).toBe('No se pudo importar: ese audio dura más que la canción')
  })

  it('trim, fades and clean-up: it is previewed and saved as a new one', async () => {
    const w = await card({ file: '/guias/grabada.wav', seconds: 4 })
    expect(w.find('.study-tab.on').text()).toBe('Grabar o importar')
    expect(w.find('.cue-card').classes()).toContain('wide')
    // los bordes, con las flechas (Mayús: de medio segundo)
    const startEdge = w.find('.cue-edge.start')
    const endEdge = w.find('.cue-edge.end')
    expect(endEdge.attributes('aria-valuenow')).toBe('4')
    await startEdge.trigger('keydown', { key: 'ArrowRight', shiftKey: true })
    await endEdge.trigger('keydown', { key: 'ArrowLeft', shiftKey: true })
    await endEdge.trigger('keydown', { key: 'ArrowLeft', shiftKey: true })
    expect(startEdge.attributes('aria-valuenow')).toBe('0.5')
    expect(endEdge.attributes('aria-valuenow')).toBe('3')
    expect(w.find('.cue-len').text()).toContain('2,50 s')
    // y lo que se quita, sombreado
    expect(w.findAll('.cue-cut')[0].attributes('style')).toContain('width: 12.5%')

    // los filtros: se encienden y se apagan
    await button(w, 'Quitar ruido').trigger('click')
    await button(w, 'Igualar volumen').trigger('click')
    expect(button(w, 'Quitar ruido').attributes('aria-pressed')).toBe('true')
    expect(button(w, 'Deshacer los cambios'), 'con cambios se pueden deshacer').toBeTruthy()

    // escucharla: una de prueba con los cambios
    await button(w, 'Escuchar').trigger('click')
    await flushPromises()
    expect(held.api.editCue).toHaveBeenLastCalledWith({
      file: '/guias/grabada.wav',
      start: 0.5,
      end: 3,
      fade_in: 0,
      fade_out: 0,
      denoise: true,
      highpass: false,
      normalize: true,
      preview: true
    })
    expect(held.api.previewCue).toHaveBeenLastCalledWith('/guias/prueba/editada.wav')

    await button(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(held.api.editCue).toHaveBeenLastCalledWith(
      expect.objectContaining({ start: 0.5, end: 3, denoise: true })
    )
    expect(held.api.editCue.mock.calls.at(-1)[0].preview).toBeUndefined()
    expect(w.emitted('made')[0][0].file).toBe('/guias/editada.wav')
  })

  it('dragging over the waveform moves the nearest edge', async () => {
    const w = await card({ file: '/guias/grabada.wav', seconds: 4 })
    const wave = w.find('.cue-wave')
    wave.element.getBoundingClientRect = () => ({ left: 0, width: 400, top: 0, height: 80 })
    wave.element.setPointerCapture = () => {}
    const pointer = async (type, clientX) => {
      wave.element.dispatchEvent(
        new PointerEvent(type, { clientX, button: 0, pointerId: 1, bubbles: true })
      )
      await flushPromises()
    }
    await pointer('pointerdown', 300)
    await pointer('pointermove', 350)
    await pointer('pointerup', 350)
    expect(w.find('.cue-edge.end').attributes('aria-valuenow')).toBe('3.5')
    expect(w.find('.cue-edge.start').attributes('aria-valuenow')).toBe('0')
    // deshacer: como venía
    await button(w, 'Deshacer los cambios').trigger('click')
    expect(w.find('.cue-edge.end').attributes('aria-valuenow')).toBe('4')
  })
})

describe('an existing cue', () => {
  it('recorded, it opens in its editor; «Quitar» removes it', async () => {
    const cue = { t: 30, file: '/guias/vieja.wav', text: '' }
    const w = await card({ t: 30, cue })
    expect(w.find('.study-tab.on').text()).toBe('Grabar o importar')
    await flushPromises()
    expect(held.api.cueWave).toHaveBeenCalledWith('/guias/vieja.wav', 240)
    expect(w.find('.cue-editor').exists()).toBe(true)
    await button(w, 'Quitar').trigger('click')
    expect(w.emitted('remove')[0]).toEqual([cue])
  })

  it('with snapping on, before and after move beat by beat', async () => {
    const w = await card({ t: 12 }, { beats: [10, 10.5, 11, 11.5, 12, 12.5] })
    const before = w.find('button[aria-label="Antes"]')
    expect(before.attributes('title')).toBe('Al pulso de antes (imán puesto)')
    await before.trigger('click')
    await before.trigger('click')
    await w.find('button[aria-label="Después"]').trigger('click')
    await button(w, 'Coro').trigger('click')
    await button(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(w.emitted('made')[0][0].t).toBe(11.5)
  })

  it('Escape or a click outside closes it', async () => {
    const w = await card()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(w.emitted('close')).toHaveLength(1)
    await w.find('.cue-veil').trigger('click')
    expect(w.emitted('close')).toHaveLength(2)
    await w.find('.cue-close').trigger('click')
    expect(w.emitted('close')).toHaveLength(3)
  })
})
