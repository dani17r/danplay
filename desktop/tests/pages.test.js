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
const montar = async (Page, props = {}) => {
  w = mount(Page, { props, attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const boton = (text) => w.findAll('button').find((b) => b.text().includes(text))
const avisos = () => useNotices().notices.value.map((n) => n.message)
/** Una tarea que va por donde se le diga y acaba cuando se le diga. */
function tareaEnMarcha(name, pasos, result) {
  const cola = [...pasos, finishedJob(name, result)]
  let soltar = null
  api().job.mockImplementation(
    () =>
      new Promise((r) => {
        soltar = () => r(cola.shift())
      })
  )
  return {
    first: { job: { name, active: true, done: 0, total: 0, message: '', result: null, error: '' } },
    siguiente: async () => {
      await vi.waitFor(() => expect(soltar).toBeTypeOf('function'))
      const s = soltar
      soltar = null
      s()
      await flushPromises()
      await flushPromises()
    }
  }
}

describe('la Entrada', () => {
  it('importar es una tarea: se ve como va y al acabar el resultado', async () => {
    const t = tareaEnMarcha(
      'importacion',
      [
        {
          name: 'importacion',
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
    await montar(InboxPage, { waiting: 2 })
    await boton('Importar 2 archivos').trigger('click')
    await flushPromises()
    expect(api().runImport).toHaveBeenCalledWith({ dry_run: false })
    expect(w.find('.job-progress').exists()).toBe(true)
    expect(boton('Importar').attributes('disabled')).toBeDefined()
    await t.siguiente()
    expect(w.find('.job-progress').text()).toContain('1 / 2')
    expect(w.find('.job-progress').text()).toContain('identificando «x.mp3»')
    await t.siguiente()
    expect(w.find('.job-progress').exists()).toBe(false)
    expect(w.text()).toContain('1 archivadas · 1 a revisar')
    expect(w.text()).toContain('Barak — Mi Gozo')
    expect(w.text()).toContain('! sin artista')
    expect(w.emitted('changed')).toBeTruthy()
    api().job.mockReset()
  })

  it('«probar sin tocar nada» es la misma tarea, en prueba', async () => {
    await montar(InboxPage, { waiting: 1 })
    await boton('Probar sin tocar nada').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(api().runImport).toHaveBeenCalledWith({ dry_run: true })
  })

  it('si falla, lo dice', async () => {
    api().runImport.mockResolvedValueOnce({
      job: finishedJob('importacion', null, { error: 'la Entrada no existe' })
    })
    await montar(InboxPage, { waiting: 1 })
    await boton('Importar 1 archivo').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(avisos()).toContain('No se pudo importar: la Entrada no existe')
  })
})

describe('los duplicados', () => {
  const grupo = {
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

  it('buscarlos es una tarea: se ve como va y luego los grupos', async () => {
    const t = tareaEnMarcha(
      'duplicados',
      [
        {
          name: 'duplicados',
          active: true,
          done: 300,
          total: 900,
          message: 'comparando nombres',
          result: null,
          error: ''
        }
      ],
      { identical: [], similar: [grupo] }
    )
    api().duplicatesScan.mockResolvedValueOnce(t.first)
    await montar(DuplicatesPage)
    expect(w.find('.job-progress').exists()).toBe(true)
    await t.siguiente()
    expect(w.find('.job-progress').text()).toContain('300 / 900')
    await t.siguiente()
    expect(w.findAll('.dup-row')).toHaveLength(2)
    expect(w.text()).toContain('Misma canción, archivo distinto (1)')
    api().job.mockReset()
  })

  it('escuchar una copia, y la que suena se pausa en vez de volver a empezar', async () => {
    held.state.songs = [song(1), song(2)]
    held.state.duplicates = { identical: [grupo], similar: [] }
    await montar(DuplicatesPage)
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

  it('quedarse con una pregunta antes y manda las demas a la papelera', async () => {
    held.state.duplicates = { identical: [grupo], similar: [] }
    await montar(DuplicatesPage)
    await w.findAll('.dup-keep')[1].trigger('click')
    await flushPromises()
    expect(useDialog().dialog.value.title).toBe('Conservar solo esta')
    expect(useDialog().dialog.value.detail).toBe('a.mp3')
    dialogOk()
    await flushPromises()
    await flushPromises()
    expect(api().resolveDuplicate).toHaveBeenCalledWith('/m/b.mp3', ['/m/a.mp3'])
    expect(avisos().some((m) => m.includes('Conservada'))).toBe(true)
    expect(w.emitted('changed')).toBeTruthy()
  })

  it('sin duplicados lo dice', async () => {
    await montar(DuplicatesPage)
    expect(w.text()).toContain('Sin duplicados.')
  })
})

describe('la bienvenida', () => {
  it('al elegir la carpeta, el analisis se ve avanzar', async () => {
    const t = tareaEnMarcha(
      'escaneo',
      [
        {
          name: 'escaneo',
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
    await montar(WelcomePage)
    await w.find('input').setValue('/musica')
    await boton('Analizar').trigger('click')
    await flushPromises()
    expect(api().addFolder).toHaveBeenCalledWith('/musica', '', false)
    expect(w.text()).toContain('Leyendo etiquetas')
    await t.siguiente()
    expect(w.find('.job-progress').text()).toContain('120 / 480')
    await t.siguiente()
    expect(w.emitted('ready')).toBeTruthy()
    api().job.mockReset()
  })

  it('si el analisis falla, la carpeta queda añadida y se dice por que', async () => {
    api().scan.mockResolvedValueOnce({
      job: finishedJob('escaneo', null, { error: 'no se puede leer la carpeta' })
    })
    await montar(WelcomePage)
    await w.find('input').setValue('/musica')
    await boton('Analizar').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.text()).toContain(
      'Carpeta añadida, pero no se pudo analizar: no se puede leer la carpeta'
    )
    expect(w.emitted('ready')).toBeTruthy()
  })
})
