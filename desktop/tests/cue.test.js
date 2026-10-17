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

const montados = []
async function tarjeta(draft = {}, props = {}) {
  const w = mount(StudyCue, {
    props: {
      draft: { t: 12, text: '', voice: '', cue: null, ...draft },
      songId: 7,
      songLength: 240,
      ...props
    },
    attachTo: document.body
  })
  montados.push(w)
  await flushPromises()
  return w
}
const boton = (w, texto) =>
  w.findAll('button').find((b) => b.text().trim() === texto || b.attributes('aria-label') === texto)
const ultimo = () => notices.value.at(-1)?.message || ''

beforeEach(() => {
  held.voices = [{ ...NATURAL }, { ...DAVID }]
  held.unavailable = ''
  held.picked = null
  clearNotices()
  vi.clearAllMocks()
})
afterEach(() => {
  for (const w of montados.splice(0)) w.unmount()
  document.body.innerHTML = ''
})

describe('escribir una guía', () => {
  it('con la voz que ya está bajada: se elige sola, se escucha y se guarda', async () => {
    const w = await tarjeta()
    expect(w.find('.cue-head').text()).toContain('Guía')
    const guardar = boton(w, 'Guardar')
    expect(guardar.attributes('disabled'), 'sin texto no se guarda').toBeDefined()

    // una de las sugeridas
    await boton(w, 'Coro').trigger('click')
    expect(w.find('input').element.value).toBe('Coro')
    expect(boton(w, 'Guardar').attributes('disabled')).toBeUndefined()

    await boton(w, 'Escuchar').trigger('click')
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
    await boton(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(held.api.makeCue).toHaveBeenLastCalledWith(7, { text: 'Coro', voice: 'supertonic-M1' })
    expect(w.emitted('made')[0]).toEqual([
      { t: 11.5, text: 'Coro', voice: 'supertonic-M1', file: '/guias/coro.wav', cue: null }
    ])
  })

  it('«aquí» la pone donde va la canción', async () => {
    const w = await tarjeta({ t: 40 })
    await w.find('button[title="Donde va la canción"]').trigger('click')
    await boton(w, 'Final').trigger('click')
    await boton(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(w.emitted('made')[0][0].t).toBe(0)
  })

  it('una voz sin bajar: se dice cuánto pesa, se baja y se ve cómo va', async () => {
    const w = await tarjeta({ voice: DAVID.id, text: 'Puente' })
    expect(w.text()).toContain('Esta voz se baja una sola vez (63 MB)')
    expect(boton(w, 'Guardar').attributes('disabled'), 'sin la voz no se guarda').toBeDefined()
    // con Enter tampoco: se dice qué falta
    await w.find('input').trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(ultimo()).toBe('Baja antes la voz')

    await w.find('.cue-get button').trigger('click')
    await flushPromises()
    expect(held.api.installCueVoice).toHaveBeenCalledWith(DAVID.id)
    expect(w.text()).toContain('Bajando la voz… 40 %')

    // termina: la siguiente pregunta (cada segundo) ya la ve lista
    held.voices[1].installed = true
    await new Promise((r) => setTimeout(r, 1100))
    await flushPromises()
    expect(w.text()).not.toContain('Bajando la voz')
    expect(boton(w, 'Guardar').attributes('disabled')).toBeUndefined()
  })

  it('las voces naturales dicen que vienen las diez', async () => {
    held.voices = [{ ...NATURAL, installed: false, error: 'Se cortó la descarga' }]
    const w = await tarjeta()
    expect(w.text()).toContain('Se cortó la descarga.')
    expect(w.text()).toContain('y con ella las diez voces naturales')
  })

  it('si en esta instalación no se puede, lo dice', async () => {
    held.unavailable = 'falta onnxruntime'
    const w = await tarjeta()
    expect(w.text()).toContain('No se pueden decir guías aquí: falta onnxruntime')
  })

  it('sin texto no se hace, y un fallo del núcleo se cuenta', async () => {
    const w = await tarjeta()
    await w.find('input').trigger('keydown', { key: 'Enter' })
    await flushPromises()
    expect(ultimo()).toBe('Escribe lo que se dirá')
    expect(held.api.makeCue).not.toHaveBeenCalled()

    held.api.makeCue.mockRejectedValueOnce(new Error('sin espacio'))
    await boton(w, 'Coro').trigger('click')
    await boton(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(ultimo()).toBe('No se pudo hacer la guía: sin espacio')
    expect(w.emitted('made')).toBeFalsy()
  })

  it('bajar una voz que falla lo dice', async () => {
    held.api.installCueVoice.mockRejectedValueOnce(new Error('sin red'))
    const w = await tarjeta({ voice: DAVID.id })
    await w.find('.cue-get button').trigger('click')
    await flushPromises()
    expect(ultimo()).toBe('No se pudo bajar la voz: sin red')
  })
})

describe('grabarla con el micrófono', () => {
  it('grabar y parar: lo grabado se trae ya, con su onda, para escucharlo y guardarlo', async () => {
    const w = await tarjeta()
    await boton(w, 'Grabar o importar').trigger('click')
    expect(w.text()).toContain('Graba tu voz o trae un audio corto')
    expect(boton(w, 'Guardar').attributes('disabled')).toBeDefined()

    await boton(w, 'Grabar').trigger('click')
    await flushPromises()
    expect(held.app.recordStart).toHaveBeenCalled()
    expect(w.text()).toContain('Habla ahora…')
    // el medidor y lo que lleva se mueven mientras se graba
    await new Promise((r) => setTimeout(r, 120))
    await flushPromises()
    expect(held.app.recordLevel).toHaveBeenCalled()
    expect(w.find('.cue-rec-live').exists()).toBe(true)

    await boton(w, 'Parar').trigger('click')
    await flushPromises()
    expect(held.app.recordStop).toHaveBeenCalled()
    // se trae a las guías en cuanto se para (hasta lo que dura la canción)
    expect(held.api.importCue).toHaveBeenCalledWith(7, '/tmp/grabacion.wav', 240)
    expect(held.api.cueWave).toHaveBeenCalledWith('/guias/grabada.wav', 240)
    expect(w.find('.cue-editor').exists()).toBe(true)
    expect(w.find('.cue-wave-shape').attributes('d')).toMatch(/^M0,50 L/)
    expect(boton(w, 'Grabar otra vez')).toBeTruthy()

    // sin tocar nada, se escucha y se guarda tal cual
    await boton(w, 'Escuchar').trigger('click')
    await flushPromises()
    expect(held.api.previewCue).toHaveBeenCalledWith('/guias/grabada.wav')
    expect(held.api.editCue).not.toHaveBeenCalled()
    expect(boton(w, 'Parar'), 'mientras suena, se puede parar').toBeTruthy()
    await boton(w, 'Parar').trigger('click')
    expect(held.api.stopPreview).toHaveBeenCalled()

    await boton(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(held.api.makeCue).not.toHaveBeenCalled()
    expect(w.emitted('made')[0][0]).toMatchObject({
      text: '',
      voice: '',
      file: '/guias/grabada.wav'
    })
  })

  it('si el micrófono falla, lo dice y no se queda grabando', async () => {
    held.app.recordStart.mockRejectedValueOnce(new Error('sin micrófono'))
    const w = await tarjeta()
    await boton(w, 'Grabar o importar').trigger('click')
    await boton(w, 'Grabar').trigger('click')
    await flushPromises()
    expect(ultimo()).toBe('No se pudo grabar: sin micrófono')
    expect(w.text()).not.toContain('Habla ahora')
  })

  it('si lo grabado no vale (demasiado largo), lo dice', async () => {
    held.api.importCue.mockRejectedValueOnce(new Error('ese audio dura más que la canción'))
    const w = await tarjeta()
    await boton(w, 'Grabar o importar').trigger('click')
    await boton(w, 'Grabar').trigger('click')
    await flushPromises()
    await boton(w, 'Parar').trigger('click')
    await flushPromises()
    expect(ultimo()).toBe('No se pudo usar lo grabado: ese audio dura más que la canción')
    expect(w.find('.cue-editor').exists()).toBe(false)
  })

  it('al cerrar a media grabación, se para', async () => {
    const w = await tarjeta()
    await boton(w, 'Grabar o importar').trigger('click')
    await boton(w, 'Grabar').trigger('click')
    await flushPromises()
    w.unmount()
    montados.splice(montados.indexOf(w), 1)
    expect(held.app.recordStop).toHaveBeenCalled()
  })
})

describe('importar un audio y arreglarlo', () => {
  it('un audio del equipo, hasta lo que dura la canción', async () => {
    held.picked = '/home/yo/cuenta.mp3'
    const w = await tarjeta()
    await boton(w, 'Grabar o importar').trigger('click')
    expect(boton(w, 'Importar un audio').attributes('title')).toContain('como mucho 4:00')
    await boton(w, 'Importar un audio').trigger('click')
    await flushPromises()
    expect(held.api.importCue).toHaveBeenCalledWith(7, '/home/yo/cuenta.mp3', 240)
    expect(w.find('.cue-editor').exists()).toBe(true)
    expect(boton(w, 'Importar otro')).toBeTruthy()
  })

  it('cancelar el selector no hace nada; un audio que no vale se cuenta', async () => {
    const w = await tarjeta()
    await boton(w, 'Grabar o importar').trigger('click')
    await boton(w, 'Importar un audio').trigger('click')
    await flushPromises()
    expect(held.api.importCue).not.toHaveBeenCalled()
    held.picked = '/home/yo/largo.wav'
    held.api.importCue.mockRejectedValueOnce(new Error('ese audio dura más que la canción'))
    await boton(w, 'Importar un audio').trigger('click')
    await flushPromises()
    expect(ultimo()).toBe('No se pudo importar: ese audio dura más que la canción')
  })

  it('recortar, fundidos y limpiarla: se escucha y se guarda como una nueva', async () => {
    const w = await tarjeta({ file: '/guias/grabada.wav', seconds: 4 })
    expect(w.find('.study-tab.on').text()).toBe('Grabar o importar')
    expect(w.find('.cue-card').classes()).toContain('wide')
    // los bordes, con las flechas (Mayús: de medio segundo)
    const desde = w.find('.cue-edge.start')
    const hasta = w.find('.cue-edge.end')
    expect(hasta.attributes('aria-valuenow')).toBe('4')
    await desde.trigger('keydown', { key: 'ArrowRight', shiftKey: true })
    await hasta.trigger('keydown', { key: 'ArrowLeft', shiftKey: true })
    await hasta.trigger('keydown', { key: 'ArrowLeft', shiftKey: true })
    expect(desde.attributes('aria-valuenow')).toBe('0.5')
    expect(hasta.attributes('aria-valuenow')).toBe('3')
    expect(w.find('.cue-len').text()).toContain('2,50 s')
    // y lo que se quita, sombreado
    expect(w.findAll('.cue-cut')[0].attributes('style')).toContain('width: 12.5%')

    // los filtros: se encienden y se apagan
    await boton(w, 'Quitar ruido').trigger('click')
    await boton(w, 'Igualar volumen').trigger('click')
    expect(boton(w, 'Quitar ruido').attributes('aria-pressed')).toBe('true')
    expect(boton(w, 'Deshacer los cambios'), 'con cambios se pueden deshacer').toBeTruthy()

    // escucharla: una de prueba con los cambios
    await boton(w, 'Escuchar').trigger('click')
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

    await boton(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(held.api.editCue).toHaveBeenLastCalledWith(
      expect.objectContaining({ start: 0.5, end: 3, denoise: true })
    )
    expect(held.api.editCue.mock.calls.at(-1)[0].preview).toBeUndefined()
    expect(w.emitted('made')[0][0].file).toBe('/guias/editada.wav')
  })

  it('arrastrar sobre la onda mueve el borde más cercano', async () => {
    const w = await tarjeta({ file: '/guias/grabada.wav', seconds: 4 })
    const onda = w.find('.cue-wave')
    onda.element.getBoundingClientRect = () => ({ left: 0, width: 400, top: 0, height: 80 })
    onda.element.setPointerCapture = () => {}
    const puntero = async (type, clientX) => {
      onda.element.dispatchEvent(
        new PointerEvent(type, { clientX, button: 0, pointerId: 1, bubbles: true })
      )
      await flushPromises()
    }
    await puntero('pointerdown', 300)
    await puntero('pointermove', 350)
    await puntero('pointerup', 350)
    expect(w.find('.cue-edge.end').attributes('aria-valuenow')).toBe('3.5')
    expect(w.find('.cue-edge.start').attributes('aria-valuenow')).toBe('0')
    // deshacer: como venía
    await boton(w, 'Deshacer los cambios').trigger('click')
    expect(w.find('.cue-edge.end').attributes('aria-valuenow')).toBe('4')
  })
})

describe('una guía que ya estaba', () => {
  it('grabada, abre en su editor; «Quitar» la quita', async () => {
    const cue = { t: 30, file: '/guias/vieja.wav', text: '' }
    const w = await tarjeta({ t: 30, cue })
    expect(w.find('.study-tab.on').text()).toBe('Grabar o importar')
    await flushPromises()
    expect(held.api.cueWave).toHaveBeenCalledWith('/guias/vieja.wav', 240)
    expect(w.find('.cue-editor').exists()).toBe(true)
    await boton(w, 'Quitar').trigger('click')
    expect(w.emitted('remove')[0]).toEqual([cue])
  })

  it('con el imán, antes y después van de pulso en pulso', async () => {
    const w = await tarjeta({ t: 12 }, { beats: [10, 10.5, 11, 11.5, 12, 12.5] })
    const antes = w.find('button[aria-label="Antes"]')
    expect(antes.attributes('title')).toBe('Al pulso de antes (imán puesto)')
    await antes.trigger('click')
    await antes.trigger('click')
    await w.find('button[aria-label="Después"]').trigger('click')
    await boton(w, 'Coro').trigger('click')
    await boton(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(w.emitted('made')[0][0].t).toBe(11.5)
  })

  it('Escape o un clic fuera la cierran', async () => {
    const w = await tarjeta()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    expect(w.emitted('close')).toHaveLength(1)
    await w.find('.cue-veil').trigger('click')
    expect(w.emitted('close')).toHaveLength(2)
    await w.find('.cue-close').trigger('click')
    expect(w.emitted('close')).toHaveLength(3)
  })
})
