import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Las paginas que arrancan tareas largas del nucleo (importar, buscar
// duplicados, analizar la carpeta nueva): se siguen hasta que acaban y se
// enseña como van. Antes esperaban dentro de una peticion y el puente de
// Rust la cortaba al minuto.
const held = vi.hoisted(() => ({ state: null, api: null, playback: null, pickFolder: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  held.pickFolder = v.fn(async () => null)
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: held.playback.bridge,
    pickFolder: held.pickFolder
  }
})

import InboxPage from '../src/components/InboxPage.vue'
import DuplicatesPage from '../src/components/DuplicatesPage.vue'
import WelcomePage from '../src/components/WelcomePage.vue'
import { finishedJob, song } from './support/backend.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'
import { dialogOk, dialogCancel, useDialog } from '../src/composables/useDialog.js'

const api = () => held.api
let w = null
beforeEach(() => {
  vi.clearAllMocks()
  clearNotices()
  dialogCancel()
  resetPlayback()
  held.playback.reset()
  held.state.jobs = {}
  held.state.songs = []
  held.state.addedFolders = []
  held.state.duplicates = { identical: [], similar: [] }
})
afterEach(() => {
  w?.unmount()
  w = null
})
const mountView = async (Page, props = {}) => {
  w = mount(Page, { props, attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const button = (text) => w.findAll('button').find((b) => b.text().includes(text))
const notices = () => useNotices().notices.value.map((n) => n.message)
/** Una tarea que va por donde se le diga y acaba cuando se le diga. */
function runningJob(name, steps, result) {
  const queue = [...steps, finishedJob(name, result)]
  let release = null
  api().job.mockImplementation(
    () =>
      new Promise((r) => {
        release = () => r(queue.shift())
      })
  )
  return {
    first: { job: { name, active: true, done: 0, total: 0, message: '', result: null, error: '' } },
    next: async () => {
      await vi.waitFor(() => expect(release).toBeTypeOf('function'))
      const s = release
      release = null
      s()
      await flushPromises()
      await flushPromises()
    }
  }
}

describe('the Inbox', () => {
  it('importing is a job: its progress shows and, when done, the result', async () => {
    const t = runningJob(
      'import',
      [
        {
          name: 'import',
          active: true,
          done: 1,
          total: 2,
          message: 'identificando «x.mp3»',
          result: null,
          error: ''
        }
      ],
      {
        results: [
          {
            source_path: 'x.mp3',
            target: 'Artistas/Barak/Barak - Mi Gozo.mp3',
            artist: 'Barak',
            title: 'Mi Gozo',
            source: 'tags',
            confidence: 0.95,
            action: 'moved',
            warnings: []
          },
          {
            source_path: 'y.mp3',
            target: 'Revisar/y.mp3',
            artist: '',
            title: '',
            source: '',
            confidence: 0,
            action: 'review',
            warnings: ['sin artista']
          }
        ]
      }
    )
    api().runImport.mockResolvedValueOnce(t.first)
    await mountView(InboxPage, { waiting: 2 })
    await button('Importar 2 archivos').trigger('click')
    await flushPromises()
    expect(api().runImport).toHaveBeenCalledWith({ dry_run: false })
    expect(w.find('.job-progress').exists()).toBe(true)
    expect(button('Importar').attributes('disabled')).toBeDefined()
    await t.next()
    expect(w.find('.job-progress').text()).toContain('1 / 2')
    expect(w.find('.job-progress').text()).toContain('identificando «x.mp3»')
    await t.next()
    expect(w.find('.job-progress').exists()).toBe(false)
    expect(w.text()).toContain('1 archivadas · 1 a revisar')
    expect(w.text()).toContain('Barak — Mi Gozo')
    expect(w.text()).toContain('! sin artista')
    expect(w.emitted('changed')).toBeTruthy()
    api().job.mockReset()
  })

  it('«probar sin tocar nada» is the same job, as a dry run', async () => {
    await mountView(InboxPage, { waiting: 1 })
    await button('Probar sin tocar nada').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(api().runImport).toHaveBeenCalledWith({ dry_run: true })
  })

  it('if it fails, it says so', async () => {
    api().runImport.mockResolvedValueOnce({
      job: finishedJob('import', null, { error: 'la Entrada no existe' })
    })
    await mountView(InboxPage, { waiting: 1 })
    await button('Importar 1 archivo').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(notices()).toContain('No se pudo importar: la Entrada no existe')
  })
})

describe('duplicates', () => {
  const group = {
    suggested: '/m/b.mp3',
    items: [
      {
        id: 1,
        path: '/m/a.mp3',
        relative: 'a.mp3',
        file: 'a.mp3',
        artist: 'X',
        title: 'A',
        duration: 100,
        bitrate: 128000,
        size: 1e6,
        stars: 0,
        favorite: 0,
        has_suffix: false
      },
      {
        id: 2,
        path: '/m/b.mp3',
        relative: 'b.mp3',
        file: 'b.mp3',
        artist: 'X',
        title: 'A',
        duration: 100,
        bitrate: 320000,
        size: 3e6,
        stars: 0,
        favorite: 0,
        has_suffix: true
      }
    ]
  }

  it('finding them is a job: its progress shows and then the groups', async () => {
    const t = runningJob(
      'duplicates',
      [
        {
          name: 'duplicates',
          active: true,
          done: 300,
          total: 900,
          message: 'comparando nombres',
          result: null,
          error: ''
        }
      ],
      { identical: [], similar: [group] }
    )
    api().duplicatesScan.mockResolvedValueOnce(t.first)
    await mountView(DuplicatesPage)
    expect(w.find('.job-progress').exists()).toBe(true)
    await t.next()
    expect(w.find('.job-progress').text()).toContain('300 / 900')
    await t.next()
    expect(w.findAll('.dup-row')).toHaveLength(2)
    expect(w.text()).toContain('Misma canción, archivo distinto (1)')
    api().job.mockReset()
  })

  it('playing a copy, and the playing one pauses instead of starting over', async () => {
    held.state.songs = [song(1), song(2)]
    held.state.duplicates = { identical: [group], similar: [] }
    await mountView(DuplicatesPage)
    await w.findAll('.dup-play')[1].trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    expect(held.playback.bridge.setQueue.mock.calls[0][0].map((t) => t.id)).toEqual([2])
    expect(w.findAll('.dup-play')[1].attributes('title')).toBe('Pausar')
    await w.findAll('.dup-play')[1].trigger('click')
    await flushPromises()
    expect(held.playback.bridge.toggle).toHaveBeenCalledTimes(1)
    expect(held.playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    expect(w.findAll('.dup-play')[1].attributes('title')).toBe('Reanudar')
  })

  it('keeping one asks first and sends the others to the trash', async () => {
    held.state.duplicates = { identical: [group], similar: [] }
    await mountView(DuplicatesPage)
    await w.findAll('.dup-keep')[1].trigger('click')
    await flushPromises()
    expect(useDialog().dialog.value.title).toBe('Conservar solo esta')
    expect(useDialog().dialog.value.detail).toBe('a.mp3')
    dialogOk()
    await flushPromises()
    await flushPromises()
    expect(api().resolveDuplicate).toHaveBeenCalledWith('/m/b.mp3', ['/m/a.mp3'])
    expect(notices().some((m) => m.includes('Conservada'))).toBe(true)
    expect(w.emitted('changed')).toBeTruthy()
  })

  it('with no duplicates it says so', async () => {
    await mountView(DuplicatesPage)
    expect(w.text()).toContain('Sin duplicados.')
  })
})

describe('the welcome page', () => {
  it('after picking the folder, the scan shows its progress', async () => {
    const t = runningJob(
      'scan',
      [
        {
          name: 'scan',
          active: true,
          done: 120,
          total: 480,
          message: 'leyendo etiquetas',
          result: null,
          error: ''
        }
      ],
      { stats: { total: 480 } }
    )
    api().scan.mockResolvedValueOnce(t.first)
    await mountView(WelcomePage)
    await w.find('input').setValue('/musica')
    await button('Analizar').trigger('click')
    await flushPromises()
    expect(api().addFolder).toHaveBeenCalledWith('/musica', '', false)
    expect(w.text()).toContain('Leyendo etiquetas')
    await t.next()
    expect(w.find('.job-progress').text()).toContain('120 / 480')
    await t.next()
    expect(w.emitted('ready')).toBeTruthy()
    api().job.mockReset()
  })

  it('if the scan fails, the folder stays added and the reason is given', async () => {
    api().scan.mockResolvedValueOnce({
      job: finishedJob('scan', null, { error: 'no se puede leer la carpeta' })
    })
    await mountView(WelcomePage)
    await w.find('input').setValue('/musica')
    await button('Analizar').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.text()).toContain(
      'Carpeta añadida, pero no se pudo analizar: no se puede leer la carpeta'
    )
    expect(w.emitted('ready')).toBeTruthy()
  })
})
