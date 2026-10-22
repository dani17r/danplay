import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// «Transformar»: la propuesta de cada canción, de una en una; aplicar,
// corregir antes, saltar, deshacer, y las seguras de una vez.
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

import TransformPanel from '../src/components/TransformPanel.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'

const DISK = {
  id: 7,
  from: 'Artistas/Barak/Generacion Radical (2018)/06 - Hasta Ver Tu Gloria.mp3',
  to: 'Artistas/Barak/Barak - Hasta Ver Tu Gloria.mp3',
  artist: 'Barak',
  title: 'Hasta Ver Tu Gloria',
  why: ['dentro de una subcarpeta', 'al nombre le falta el artista'],
  sure: true
}
const LOOSE = {
  id: 8,
  from: 'Barak - Nueva (Official Video).mp3',
  to: 'Artistas/Barak/Barak - Nueva.mp3',
  artist: 'Barak',
  title: 'Nueva',
  why: ['suelta en la raíz de tu música'],
  sure: true
}
const CAM = {
  id: 9,
  from: 'Artistas/Barak/Barak - Danzar (Drum Cam).mp3',
  to: 'Pistas/Barak - Danzar (Drum Cam).mp3',
  artist: 'Barak',
  title: 'Danzar (Drum Cam)',
  why: ['es de Pistas'],
  sure: false
}

beforeEach(() => {
  vi.clearAllMocks()
  resetPlayback()
  held.playback.reset()
  held.state.tidy = [DISK, LOOSE, CAM].map((p) => ({ ...p }))
  held.state.tidyError = ''
})

const mountView = async () => {
  const w = mount(TransformPanel, { props: { open: true }, attachTo: document.body })
  await flushPromises()
  return w
}
const button = (w, text) => w.findAll('button').find((b) => b.text().includes(text))

describe('Transformar', () => {
  it('enseña de una en una dónde está, dónde iría y por qué; aplicar la quita y se puede deshacer', async () => {
    const w = await mountView()
    expect(w.text()).toContain('Canción 1 de 3')
    expect(w.text()).toContain('2 seguras')
    const card = () => w.find('.tidy-card')
    expect(card().text()).toContain(DISK.from)
    expect(card().text()).toContain(DISK.to)
    expect(card().text()).toContain('dentro de una subcarpeta')
    expect(card().find('.tidy-badge.ok').exists()).toBe(true)
    await w.find('.tidy-apply').trigger('click')
    await flushPromises()
    expect(held.api.tidy).toHaveBeenCalledWith(7, {
      artist: 'Barak',
      title: 'Hasta Ver Tu Gloria',
      tags: false
    })
    expect(w.emitted('changed')).toBeTruthy()
    expect(w.text()).toContain('Canción 1 de 2')
    expect(card().text()).toContain(LOOSE.from)
    expect(w.find('.tidy-last').text()).toContain('Barak - Hasta Ver Tu Gloria.mp3')
    // deshacer: vuelve a la ruta de antes
    await button(w, 'Deshacer').trigger('click')
    await flushPromises()
    expect(held.api.tidy).toHaveBeenLastCalledWith(7, { to: DISK.from })
    expect(held.api.tidyList).toHaveBeenCalledTimes(2)
    w.unmount()
  })

  it('corregido a mano: lo dice, se ve dónde quedaría y va también a las etiquetas', async () => {
    const w = await mountView()
    const [artist, title] = w.findAll('.tidy-fields input')
    await artist.setValue('Barak y Amigos')
    await title.setValue('Hasta Ver Tu Gloria (Live)')
    expect(w.find('.tidy-path.to').text()).toBe(
      'Artistas/Barak y Amigos/Barak y Amigos - Hasta Ver Tu Gloria (Live).mp3'
    )
    expect(w.find('.tidy-note').text()).toContain('etiquetas')
    await w.find('.tidy-apply').trigger('click')
    await flushPromises()
    expect(held.api.tidy).toHaveBeenCalledWith(7, {
      artist: 'Barak y Amigos',
      title: 'Hasta Ver Tu Gloria (Live)',
      tags: true
    })
    w.unmount()
  })

  it('una Drum Cam va a su carpeta por su ruta; saltar y volver no tocan nada', async () => {
    const w = await mountView()
    await button(w, 'Saltar').trigger('click')
    await button(w, 'Saltar').trigger('click')
    await flushPromises()
    expect(w.find('.tidy-card').text()).toContain(CAM.from)
    expect(w.find('.tidy-badge.doubt').exists(), 'para revisar').toBe(true)
    expect(w.find('.tidy-fields').exists(), 'el nombre no cambia').toBe(false)
    expect(held.api.tidy).not.toHaveBeenCalled()
    await w.find('.tidy-apply').trigger('click')
    await flushPromises()
    expect(held.api.tidy).toHaveBeenCalledWith(9, { to: CAM.to })
    w.unmount()
  })

  it('«Aplicar las seguras» las hace una tras otra y deja las de revisar', async () => {
    const w = await mountView()
    await button(w, 'Aplicar las seguras').trigger('click')
    await flushPromises()
    expect(held.api.tidy.mock.calls.map((c) => c[0])).toEqual([7, 8])
    expect(w.text()).toContain('Canción 1 de 1')
    expect(w.find('.tidy-card').text()).toContain(CAM.from)
    w.unmount()
  })

  it('si no se puede (en Windows, abierta por otro programa), lo dice en la tarjeta', async () => {
    held.state.tidyError = 'no se pudo mover: el archivo está en uso'
    const w = await mountView()
    await w.find('.tidy-apply').trigger('click')
    await flushPromises()
    expect(w.find('.tidy-error').text()).toContain('en uso')
    expect(w.text()).toContain('Canción 1 de 3')
    w.unmount()
  })

  it('«¿Qué hace?» lo explica; sin nada que hacer, lo dice', async () => {
    const w = await mountView()
    expect(w.find('.tidy-help').exists()).toBe(false)
    await button(w, '¿Qué hace?').trigger('click')
    expect(w.find('.tidy-help').text()).toContain('Una a una, y tú decides')
    w.unmount()
    held.state.tidy = []
    const empty = await mountView()
    expect(empty.text()).toContain('Todo en su sitio')
    expect(empty.text()).toContain('Se miraron 9 canciones')
    empty.unmount()
  })

  it('la que suena lo dice: seguirá sonando', async () => {
    held.playback.emit({ track: { id: 7, title: 'Hasta Ver Tu Gloria', artist: 'Barak' } })
    const w = await mountView()
    expect(w.find('.tidy-badge.playing').text()).toContain('seguirá sonando')
    w.unmount()
  })
})
