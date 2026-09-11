import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// La pagina de Descargas: el historial es lo unico que hay que mirar (la
// tarjeta de «Resultado» decia lo mismo), y desde el se pone a sonar y se para
// lo descargado.
const held = vi.hoisted(() => ({ history: [], total: 0, youtube: null, playback: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createPlaybackDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.playback = createPlaybackDouble()
  held.youtube = v.fn(async () => ({ available: true, active: false, results: [] }))
  const api = {
    ...actual.api,
    inTauri: true,
    youtube: held.youtube,
    downloadHistory: v.fn(async (limit, offset = 0) => ({
      items: held.history.slice(offset, offset + limit), total: held.total
    })),
    clearDownloadHistory: v.fn(async () => { held.history = []; held.total = 0; return { removed: 3 } }),
    song: v.fn(async (id) => ({ id, title: 'Tema ' + id, artist: 'Alguien', duration: 100 })),
    youtubeDownload: v.fn(async () => ({ ok: true, active: true })),
    youtubeCancel: v.fn(async () => ({ ok: true }))
  }
  return { ...actual, api, inTauri: true, playback: held.playback.bridge }
})

import DownloadsPage from '../src/components/DownloadsPage.vue'
import { resetDownloads } from '../src/composables/useDownloads.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { dialogOk } from '../src/composables/useDialog.js'

const row = (id, extra = {}) => ({
  id, at: 1789087991 - id, source: 'assistant', ok: 1, already: 0, song_id: 200 + id,
  artist: 'Barak', song: 'Tema ' + id, title: 'Tema ' + id, target: '/musica/x.mp3', ...extra
})

beforeEach(() => {
  held.history = [row(1), row(2, { ok: 0, already: 1, song_id: null }), row(3, { ok: 0, song_id: null, reason: 'sin red' })]
  held.total = 3
  vi.clearAllMocks()
  resetDownloads()
  resetPlayback()
  held.playback.reset()
  localStorage.clear()
})

const montar = async () => {
  const w = mount(DownloadsPage, { attachTo: document.body })
  await flushPromises(); await flushPromises()
  return w
}

describe('el historial de descargas', () => {
  it('esta siempre a la vista y no hay tarjeta de Resultado aparte', async () => {
    const w = await montar()
    expect(w.text()).toContain('Historial')
    expect(w.findAll('.hist-row')).toHaveLength(3)
    expect(w.text()).not.toContain('Resultado')
    // ya no hay que abrirlo: no existe el boton de Ver/Ocultar
    expect(w.findAll('button').some(b => ['Ver', 'Ocultar'].includes(b.text()))).toBe(false)
  })

  it('lo bajado se pone a sonar desde su fila, y la misma fila lo pausa', async () => {
    const w = await montar()
    const rows = w.findAll('.hist-row')
    expect(rows[0].find('.hist-play').exists()).toBe(true)
    expect(rows[1].find('.hist-play').exists()).toBe(false)   // «ya la tenias»: nada que poner
    expect(rows[2].find('.hist-play').exists()).toBe(false)   // fallo: nada que poner

    await rows[0].find('.hist-play').trigger('click')
    await flushPromises(); await flushPromises()
    const [items, start, origin] = held.playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map(t => t.id)).toEqual([201])
    expect(start).toBe(201)
    expect(origin.kind).toBe('downloads')
    expect(w.findAll('.hist-row')[0].find('.hist-play').attributes('title')).toBe('Pausar')
    expect(w.findAll('.hist-row')[0].classes()).toContain('sounding')

    await w.findAll('.hist-row')[0].find('.hist-play').trigger('click')
    await flushPromises(); await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(held.playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    expect(w.findAll('.hist-row')[0].find('.hist-play').attributes('title')).toBe('Reanudar')
  })

  it('se vacia como en un navegador, pero no mientras baja algo', async () => {
    const w = await montar()
    const vaciar = () => w.findAll('button').find(b => b.text() === 'Vaciar')
    expect(vaciar().attributes('disabled')).toBeUndefined()

    // algo bajando: el boton se bloquea
    held.youtube.mockResolvedValueOnce({ available: true, active: true, phase: 'downloading', index: 1, total: 1 })
    await w.vm.$.setupState.downloads.refresh()
    await flushPromises()
    expect(vaciar().attributes('disabled')).toBeDefined()

    held.youtube.mockResolvedValueOnce({ available: true, active: false, results: [] })
    await w.vm.$.setupState.downloads.refresh()
    await flushPromises()
    await vaciar().trigger('click')
    await flushPromises()
    dialogOk()
    await flushPromises(); await flushPromises()
    expect(w.findAll('.hist-row')).toHaveLength(0)
  })

  it('a partir de una tanda, ofrece cargar mas en vez de paginar', async () => {
    held.history = Array.from({ length: 45 }, (_, i) => row(i + 1))
    held.total = 45
    const w = await montar()
    expect(w.findAll('.hist-row')).toHaveLength(30)
    const mas = w.findAll('button').find(b => b.text().startsWith('Cargar mas'))
    expect(mas.text()).toContain('15')
    await mas.trigger('click')
    await flushPromises()
    expect(w.findAll('.hist-row')).toHaveLength(45)
    expect(w.findAll('button').find(b => b.text().startsWith('Cargar mas'))).toBeUndefined()
  })
})
