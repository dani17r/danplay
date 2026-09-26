import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// La pagina de Descargas: el historial es lo unico que hay que mirar (la
// tarjeta de «Resultado» decia lo mismo), y desde el se pone a sonar y se para
// lo descargado.
const held = vi.hoisted(() => ({
  history: [],
  total: 0,
  youtube: null,
  playback: null,
  folder: null
}))

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
      items: held.history.slice(offset, offset + limit),
      total: held.total
    })),
    clearDownloadHistory: v.fn(async () => {
      held.history = []
      held.total = 0
      return { removed: 3 }
    }),
    song: v.fn(async (id) => ({ id, title: 'Tema ' + id, artist: 'Alguien', duration: 100 })),
    youtubeDownload: v.fn(async () => ({ ok: true, active: true })),
    youtubeCancel: v.fn(async () => ({ ok: true })),
    moveSong: v.fn(async (id) => ({ id, folder: 'Pistas' })),
    downloadFolder: v.fn(async () => ({ ...held.folder })),
    setDownloadFolder: v.fn(async (path) => {
      held.folder = { path, ready: true, reason: '', suggested: path }
      return { ...held.folder }
    }),
    addFolder: v.fn(async () => ({ action: 'already_there', notice: { message: 'ya esta' } })),
    // trae el yt-dlp nuevo: una tarea que aqui ya viene terminada
    youtubeUpdate: v.fn(async () => ({
      job: {
        name: 'yt-dlp',
        active: false,
        done: 1,
        total: 1,
        message: '',
        error: '',
        result: { previous: '2026.08.01', version: '2026.09.20', updated: true }
      }
    }))
  }
  return { ...actual, api, inTauri: true, playback: held.playback.bridge }
})

import DownloadsPage from '../src/components/DownloadsPage.vue'
import { resetDownloads, useDownloads } from '../src/composables/useDownloads.js'
import { api } from '../src/api.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { dialogOk, dialogCancel, useDialog } from '../src/composables/useDialog.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'

const row = (id, extra = {}) => ({
  id,
  at: 1789087991 - id,
  source: 'assistant',
  ok: 1,
  already: 0,
  song_id: 200 + id,
  artist: 'Barak',
  song: 'Tema ' + id,
  title: 'Tema ' + id,
  target: '/musica/x.mp3',
  ...extra
})

beforeEach(() => {
  held.history = [
    row(1),
    row(2, { ok: 0, already: 1, song_id: null }),
    row(3, { ok: 0, song_id: null, reason: 'sin red' })
  ]
  held.total = 3
  held.folder = { path: '/musica', ready: true, reason: '', suggested: '/musica' }
  vi.clearAllMocks()
  resetDownloads()
  resetPlayback()
  held.playback.reset()
  localStorage.clear()
})

const montar = async () => {
  const w = mount(DownloadsPage, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}

describe('el historial de descargas', () => {
  it('esta siempre a la vista y no hay tarjeta de Resultado aparte', async () => {
    const w = await montar()
    expect(w.text()).toContain('Historial')
    expect(w.findAll('.hist-row')).toHaveLength(3)
    expect(w.text()).not.toContain('Resultado')
    // ya no hay que abrirlo: no existe el boton de Ver/Ocultar
    expect(w.findAll('button').some((b) => ['Ver', 'Ocultar'].includes(b.text()))).toBe(false)
  })

  it('lo bajado se pone a sonar desde su fila, y la misma fila lo pausa', async () => {
    const w = await montar()
    const rows = w.findAll('.hist-row')
    expect(rows[0].find('.hist-play').exists()).toBe(true)
    expect(rows[1].find('.hist-play').exists()).toBe(false) // «ya la tenias»: nada que poner
    expect(rows[2].find('.hist-play').exists()).toBe(false) // fallo: nada que poner

    await rows[0].find('.hist-play').trigger('click')
    await flushPromises()
    await flushPromises()
    const [items, start, origin] = held.playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([201])
    expect(start).toBe(201)
    expect(origin.kind).toBe('downloads')
    expect(w.findAll('.hist-row')[0].find('.hist-play').attributes('title')).toBe('Pausar')
    expect(w.findAll('.hist-row')[0].classes()).toContain('sounding')

    await w.findAll('.hist-row')[0].find('.hist-play').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(held.playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    expect(w.findAll('.hist-row')[0].find('.hist-play').attributes('title')).toBe('Reanudar')
  })

  it('se vacia como en un navegador, pero no mientras baja algo', async () => {
    const w = await montar()
    const vaciar = () => w.findAll('button').find((b) => b.text() === 'Vaciar')
    expect(vaciar().attributes('disabled')).toBeUndefined()

    // algo bajando: el boton se bloquea
    held.youtube.mockResolvedValueOnce({
      available: true,
      active: true,
      phase: 'downloading',
      index: 1,
      total: 1
    })
    await w.vm.$.setupState.downloads.refresh()
    await flushPromises()
    expect(vaciar().attributes('disabled')).toBeDefined()

    held.youtube.mockResolvedValueOnce({ available: true, active: false, results: [] })
    await w.vm.$.setupState.downloads.refresh()
    await flushPromises()
    await vaciar().trigger('click')
    await flushPromises()
    dialogOk()
    await flushPromises()
    await flushPromises()
    expect(w.findAll('.hist-row')).toHaveLength(0)
  })

  it('a partir de una tanda, ofrece cargar mas en vez de paginar', async () => {
    held.history = Array.from({ length: 45 }, (_, i) => row(i + 1))
    held.total = 45
    const w = await montar()
    expect(w.findAll('.hist-row')).toHaveLength(30)
    const mas = w.findAll('button').find((b) => b.text().startsWith('Cargar mas'))
    expect(mas.text()).toContain('15')
    await mas.trigger('click')
    await flushPromises()
    expect(w.findAll('.hist-row')).toHaveLength(45)
    expect(w.findAll('button').find((b) => b.text().startsWith('Cargar mas'))).toBeUndefined()
  })
})

describe('yt-dlp', () => {
  beforeEach(clearNotices)

  it('se ve que version se usa y se puede traer la nueva', async () => {
    held.youtube.mockResolvedValue({
      available: true,
      active: false,
      results: [],
      version: '2026.08.01',
      bundled_version: '2026.08.01',
      js_runtime: 'deno',
      js_runtime_hint: ''
    })
    const w = await montar()
    expect(w.find('.yt-version').text()).toBe('2026.08.01')
    expect(w.text()).toContain('Motor de JavaScript: deno')
    held.youtube.mockResolvedValue({
      available: true,
      active: false,
      results: [],
      version: '2026.09.20',
      bundled_version: '2026.08.01',
      js_runtime: 'deno',
      js_runtime_hint: ''
    })
    await w
      .findAll('button')
      .find((b) => b.text().includes('Actualizar yt-dlp'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    const { notices } = useNotices()
    expect(
      notices.value.some((n) => n.message.includes('yt-dlp actualizado: 2026.08.01 → 2026.09.20'))
    ).toBe(true)
    expect(w.find('.yt-version').text()).toBe('2026.09.20')
    // la que viaja con la app se dice aparte
    expect(w.text()).toContain('La que trae DanPlay es la 2026.08.01')
    held.youtube.mockResolvedValue({ available: true, active: false, results: [] })
  })

  it('sin motor de JavaScript lo avisa, con lo que hay que instalar', async () => {
    held.youtube.mockResolvedValue({
      available: true,
      active: false,
      results: [],
      version: '2026.09.20',
      js_runtime: null,
      js_runtime_hint: 'Instala Deno (deno.com) o Node 20 o superior'
    })
    const w = await montar()
    expect(w.find('.yt-js').text()).toContain('Instala Deno')
    held.youtube.mockResolvedValue({ available: true, active: false, results: [] })
  })

  it('mientras baja algo, no se actualiza', async () => {
    held.youtube.mockResolvedValue({
      available: true,
      active: true,
      phase: 'downloading',
      index: 1,
      total: 1,
      results: []
    })
    const w = await montar()
    expect(
      w
        .findAll('button')
        .find((b) => b.text().includes('Actualizar yt-dlp'))
        .attributes('disabled')
    ).toBeDefined()
    held.youtube.mockResolvedValue({ available: true, active: false, results: [] })
  })
})

describe('lo que parece una Drum Cam', () => {
  beforeEach(clearNotices)

  const drumCam = {
    ok: true,
    id: 7,
    artist: 'Ish Melton',
    song: 'Que Se Abra El Cielo (Drum Cam)',
    title: 'QUE SE ABRA EL CIELO - ISH MELTON DRUM CAM',
    kind: { category: 'track', what: 'una Drum Cam', folder: 'Pistas' }
  }
  const terminar = async (results) => {
    held.youtube.mockResolvedValue({ available: true, active: true, phase: 'filing', results: [] })
    const w = await montar()
    held.youtube.mockResolvedValue({ available: true, active: false, results })
    await useDownloads().refresh()
    await flushPromises()
    return w
  }
  afterEach(() => held.youtube.mockResolvedValue({ available: true, active: false, results: [] }))

  it('al terminar ofrece llevarla a su carpeta, y con un clic la mueve', async () => {
    const w = await terminar([drumCam, { ok: true, id: 8, artist: 'Barak', song: 'Mi Gozo' }])
    const card = w.find('.dl-kind')
    expect(card.text()).toContain('Parece una Drum Cam: ¿moverla a Pistas?')
    // solo la que lo parece: la cancion de siempre no sale
    expect(card.findAll('.dl-result')).toHaveLength(1)
    await card
      .findAll('button')
      .find((b) => b.text() === 'Mover a Pistas')
      .trigger('click')
    await flushPromises()
    expect(api.moveSong).toHaveBeenCalledWith(7, 'track')
    expect(w.find('.dl-kind').exists()).toBe(false)
    expect(w.emitted('reload')).toBeTruthy()
    const { notices } = useNotices()
    expect(notices.value.some((n) => n.message === 'Movida a Pistas')).toBe(true)
  })

  it('«No, déjala en Artistas» la quita sin mover nada', async () => {
    const w = await terminar([drumCam])
    await w
      .findAll('button')
      .find((b) => b.text() === 'No, déjala en Artistas')
      .trigger('click')
    expect(w.find('.dl-kind').exists()).toBe(false)
    expect(api.moveSong).not.toHaveBeenCalled()
  })

  it('si no se puede mover, lo dice y la sugerencia sigue ahi', async () => {
    api.moveSong.mockRejectedValueOnce(new Error('su archivo ya no esta donde decia el indice'))
    const w = await terminar([drumCam])
    await w
      .findAll('button')
      .find((b) => b.text() === 'Mover a Pistas')
      .trigger('click')
    await flushPromises()
    const { notices } = useNotices()
    expect(notices.value.some((n) => n.message.startsWith('No se pudo mover'))).toBe(true)
    expect(w.find('.dl-kind').exists()).toBe(true)
  })
})

describe('la carpeta de descargas', () => {
  it('se ve donde se guarda', async () => {
    const w = await montar()
    expect(w.find('.dl-folder').text()).toContain('Se guarda en /musica')
  })

  it('la primera vez pregunta donde, y luego baja ahi', async () => {
    held.folder = { path: '/home/x/Música', ready: false, reason: 'unset', suggested: '/musica' }
    const w = await montar()
    expect(w.find('.dl-folder').text()).toContain('La primera vez que descargues te pregunto')
    await w.find('input').setValue('barak mi gozo')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Descargar')
      .trigger('click')
    await flushPromises()
    const { dialog } = useDialog()
    expect(dialog.value.open && dialog.value.title).toBe('¿Dónde guardo lo que descargues?')
    expect(api.youtubeDownload).not.toHaveBeenCalled()
    dialogOk('/musica')
    await flushPromises()
    await flushPromises()
    expect(api.setDownloadFolder).toHaveBeenCalledWith('/musica')
    expect(api.youtubeDownload).toHaveBeenCalledTimes(1)
    expect(w.find('.dl-folder').text()).toContain('Se guarda en /musica')
  })

  it('sin carpeta no se baja nada', async () => {
    held.folder = { path: '', ready: false, reason: 'unset', suggested: '' }
    const w = await montar()
    await w.find('input').setValue('barak mi gozo')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Descargar')
      .trigger('click')
    await flushPromises()
    dialogCancel()
    await flushPromises()
    expect(api.youtubeDownload).not.toHaveBeenCalled()
  })
})
