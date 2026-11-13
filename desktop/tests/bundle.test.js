import { describe, it, expect, vi, beforeEach } from 'vitest'

// Abrir un `.danpista` en la PC (`useBundle`): lo trae a la biblioteca con su
// avance, dice lo que entró y, si la canción ya estaba, que no se trae otra.
const held = vi.hoisted(() => ({ api: null, picked: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  held.api = {
    importBundle: vi.fn(async () => ({ job: { name: 'bundle-import', active: true } })),
    runJob: vi.fn(async (start, name, options) => {
      const first = await start()
      if (options?.busy && first?.already_running) throw new actual.ApiError(options.busy, 409)
      options?.onProgress?.({ message: 'Mezclando «Mi Gozo»…', done: 1, total: 8 })
      return held.result
    })
  }
  return { ...actual, api: held.api, pickBundle: vi.fn(async () => held.picked) }
})

import { useBundle } from '../src/composables/useBundle.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'

const said = () => useNotices().notices.value.map((n) => n.message)

beforeEach(() => {
  vi.clearAllMocks()
  clearNotices()
  held.picked = null
  held.result = { id: 12, title: 'Mi Gozo', artist: 'Barak', tracks: 6 }
})

describe('opening a .danpista on the PC', () => {
  it('the chosen file is imported and what came in is said', async () => {
    held.picked = '/descargas/Barak - Mi Gozo.danpista'
    const { open, state } = useBundle()
    const made = await open()
    expect(held.api.importBundle).toHaveBeenCalledWith('/descargas/Barak - Mi Gozo.danpista')
    expect(held.api.runJob.mock.calls[0][1]).toBe('bundle-import')
    expect(made).toEqual({ id: 12, title: 'Mi Gozo', artist: 'Barak', tracks: 6 })
    expect(said().at(-1)).toContain('«Mi Gozo» lista: 6 pistas')
    expect(state.busy).toBe(false)
  })

  it('with a path (opened from outside) nothing is asked', async () => {
    const { pickBundle } = await import('../src/api.js')
    const made = await useBundle().open('/usb/Shekinah.danpista')
    expect(pickBundle).not.toHaveBeenCalled()
    expect(held.api.importBundle).toHaveBeenCalledWith('/usb/Shekinah.danpista')
    expect(made.id).toBe(12)
  })

  it('a song that was already in the library is not brought again: it is shown', async () => {
    held.result = { id: 7, title: 'Mi Gozo', artist: 'Barak', existing: true }
    const made = await useBundle().open('/usb/Mi Gozo.danpista')
    expect(made.existing).toBe(true)
    expect(said().at(-1)).toContain('ya estaba en tu biblioteca')
  })

  it('cancelling the file dialog does nothing', async () => {
    expect(await useBundle().open()).toBe(null)
    expect(held.api.importBundle).not.toHaveBeenCalled()
  })

  it('a failure is said, and it can be tried again', async () => {
    held.api.runJob.mockRejectedValueOnce(new Error('no es un .danpista'))
    expect(await useBundle().open('/usb/otra-cosa.danpista')).toBe(null)
    expect(said().at(-1)).toContain('No se pudo abrir el .danpista: no es un .danpista')
    expect(useBundle().state.busy).toBe(false)
  })
})
