import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'

// La carpeta de descargas: la primera vez se pregunta, se hace de tus
// carpetas de música si no lo era, y a partir de ahí no se vuelve a preguntar.
const held = vi.hoisted(() => ({ status: null, added: [] }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { vi: v } = await import('vitest')
  const api = {
    ...actual.api,
    downloadFolder: v.fn(async () => ({ ...held.status })),
    setDownloadFolder: v.fn(async (path) => {
      held.status = { path, ready: true, reason: '', suggested: path }
      return { ...held.status }
    }),
    addFolder: v.fn(async (path, label, force) => {
      held.added.push([path, force])
      if (path === '/copia' && !force)
        return { action: 'confirm', notice: { message: 'es una copia de /musica' } }
      return { action: 'already_there', notice: { message: 'ya esta' } }
    })
  }
  return { ...actual, api, inTauri: false }
})

import { api } from '../src/api.js'
import { ensureDownloadFolder, chooseDownloadFolder } from '../src/utils/downloadFolder.js'
import { useDialog, dialogOk, dialogCancel } from '../src/composables/useDialog.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'

const { dialog } = useDialog()

beforeEach(() => {
  held.status = { path: '/home/x/Música', ready: false, reason: 'unset', suggested: '/musica' }
  held.added = []
  vi.clearAllMocks()
  clearNotices()
})

describe('la carpeta de descargas', () => {
  it('ya elegida: se baja sin preguntar nada', async () => {
    held.status = { path: '/musica', ready: true, reason: '', suggested: '/musica' }
    expect(await ensureDownloadFolder()).toBe(true)
    expect(dialog.value.open).toBe(false)
  })

  it('la primera vez la pregunta, propone tu carpeta de música y la recuerda', async () => {
    const done = ensureDownloadFolder()
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.title).toBe('¿Dónde guardo lo que descargues?')
    expect(dialog.value.value).toBe('/musica')
    expect(dialog.value.message).toContain('Artistas, Pistas, Secuencias')
    dialogOk('/musica')
    expect(await done).toBe(true)
    expect(held.added).toEqual([['/musica', false]])
    expect(api.setDownloadFolder).toHaveBeenCalledWith('/musica')
    expect(useNotices().notices.value.some((n) => n.message.includes('/musica'))).toBe(true)
    // la siguiente, sin preguntar
    expect(await ensureDownloadFolder()).toBe(true)
    expect(api.setDownloadFolder).toHaveBeenCalledTimes(1)
  })

  it('si cancelas, no se baja nada', async () => {
    const done = ensureDownloadFolder()
    await flushPromises()
    dialogCancel()
    expect(await done).toBe(false)
    expect(api.setDownloadFolder).not.toHaveBeenCalled()
  })

  it('si parece una copia de otra carpeta, pregunta antes de usarla', async () => {
    const done = chooseDownloadFolder(held.status)
    await flushPromises()
    dialogOk('/copia')
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.message).toContain('es una copia de /musica')
    dialogOk()
    expect(await done).toBe(true)
    expect(held.added).toEqual([
      ['/copia', false],
      ['/copia', true]
    ])
    expect(api.setDownloadFolder).toHaveBeenCalledWith('/copia')
  })

  it('si el núcleo no la acepta, lo dice y no se baja', async () => {
    api.setDownloadFolder.mockRejectedValueOnce(new Error('esa carpeta no existe'))
    const done = chooseDownloadFolder(held.status)
    await flushPromises()
    dialogOk('/musica')
    expect(await done).toBe(false)
    expect(
      useNotices().notices.value.some((n) => n.message.includes('No se pudo usar esa carpeta'))
    ).toBe(true)
  })
})
