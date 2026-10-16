// La tarjeta de una guía hablada (StudyCue): escribirla y que la diga una
// voz (bajándola la primera vez), o grabarla con el micrófono; escucharla,
// guardarla, moverla de sitio y quitarla.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

const held = vi.hoisted(() => ({ voices: [], unavailable: '', api: null, app: null }))
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
    previewCue: v.fn(async () => {}),
    importCue: v.fn(async () => ({ file: '/guias/grabada.wav' }))
  }
  held.app = {
    canRecord: true,
    recordStart: v.fn(async () => {}),
    recordLevel: v.fn(async () => 0.3),
    recordStop: v.fn(async () => '/tmp/grabacion.wav')
  }
  return { ...actual, api: held.api, app: held.app }
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
async function tarjeta(draft = {}) {
  const w = mount(StudyCue, {
    props: { draft: { t: 12, text: '', voice: '', cue: null, ...draft }, songId: 7 },
    attachTo: document.body
  })
  montados.push(w)
  await flushPromises()
  return w
}
const boton = (w, texto) => w.findAll('button').find((b) => b.text().trim() === texto)
const ultimo = () => notices.value.at(-1)?.message || ''

beforeEach(() => {
  held.voices = [{ ...NATURAL }, { ...DAVID }]
  held.unavailable = ''
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
    expect(w.text()).toContain('Guía hablada')
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
  it('grabar, parar, escuchar y guardar lo grabado', async () => {
    const w = await tarjeta()
    await boton(w, 'Grabar mi voz').trigger('click')
    expect(w.text()).toContain('Con el micrófono del equipo')
    expect(boton(w, 'Guardar').attributes('disabled')).toBeDefined()

    await boton(w, 'Grabar').trigger('click')
    await flushPromises()
    expect(held.app.recordStart).toHaveBeenCalled()
    expect(w.text()).toContain('Habla ahora…')
    // el medidor se mueve mientras se graba
    await new Promise((r) => setTimeout(r, 120))
    await flushPromises()
    expect(held.app.recordLevel).toHaveBeenCalled()

    await boton(w, 'Parar').trigger('click')
    await flushPromises()
    expect(held.app.recordStop).toHaveBeenCalled()
    expect(w.text()).toContain('Listo: escúchala o guárdala.')
    expect(boton(w, 'Grabar otra vez')).toBeTruthy()

    await boton(w, 'Escuchar').trigger('click')
    await flushPromises()
    expect(held.api.previewCue).toHaveBeenCalledWith('/tmp/grabacion.wav')
    expect(held.api.makeCue).not.toHaveBeenCalled()

    await boton(w, 'Guardar').trigger('click')
    await flushPromises()
    expect(held.api.importCue).toHaveBeenCalledWith(7, '/tmp/grabacion.wav')
    expect(w.emitted('made')[0][0]).toMatchObject({
      text: '',
      voice: '',
      file: '/guias/grabada.wav'
    })
  })

  it('si el micrófono falla, lo dice y no se queda grabando', async () => {
    held.app.recordStart.mockRejectedValueOnce(new Error('sin micrófono'))
    const w = await tarjeta()
    await boton(w, 'Grabar mi voz').trigger('click')
    await boton(w, 'Grabar').trigger('click')
    await flushPromises()
    expect(ultimo()).toBe('No se pudo grabar: sin micrófono')
    expect(w.text()).not.toContain('Habla ahora')
  })

  it('al cerrar a media grabación, se para', async () => {
    const w = await tarjeta()
    await boton(w, 'Grabar mi voz').trigger('click')
    await boton(w, 'Grabar').trigger('click')
    await flushPromises()
    w.unmount()
    montados.splice(montados.indexOf(w), 1)
    expect(held.app.recordStop).toHaveBeenCalled()
  })
})

describe('una guía que ya estaba', () => {
  it('grabada, abre en «Grabar mi voz»; «Quitar» la quita', async () => {
    const cue = { t: 30, file: '/guias/vieja.wav', text: '' }
    const w = await tarjeta({ t: 30, cue })
    expect(w.find('.study-tab.on').text()).toBe('Grabar mi voz')
    await boton(w, 'Quitar').trigger('click')
    expect(w.emitted('remove')[0]).toEqual([cue])
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
