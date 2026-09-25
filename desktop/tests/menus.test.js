import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El doble del nucleo y de Rust se construye a partir del `api` de verdad
// (tests/support/backend.js). Aqui habia uno escrito a mano que se habia
// quedado con nombres muertos (`resolverDuplicado`, `tray`, `estadoFalso.ia`):
// las pruebas seguian en verde contra un contrato que ya no existia.
const held = vi.hoisted(() => ({ state: null, api: null }))
vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble, createAppDouble } =
    await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: createPlaybackDouble().bridge,
    app: createAppDouble({ shareTargets: v.fn(async () => ({ telegram: false })) }),
    core: { onStatus: v.fn(async () => () => {}), onChanged: v.fn(async () => () => {}) },
    projection: { show: v.fn(async () => {}), hide: v.fn(async () => {}) }
  }
})
import App from '../src/App.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetPreferences } from '../src/composables/usePreferences.js'
import { resetChat } from '../src/composables/useChat.js'
import { dialogCancel } from '../src/composables/useDialog.js'
import { song } from './support/backend.js'

const status = held.state

beforeEach(() => {
  status.songs = Array.from({ length: 6 }, (_, i) =>
    song(i + 1, { title: 'Tema ' + (i + 1), artist: 'Artista ' + (i + 1) })
  )
  status.playlists = []
  status.playlistSongs = status.songs.map((s) => ({ ...s }))
  status.status.configured = true
  resetPlayback()
  resetPreferences()
  resetChat()
  dialogCancel()
  localStorage.clear()
  localStorage.setItem('danplay.default-player-asked', '1')
  vi.clearAllMocks()
})

const montar = async () => {
  const w = mount(App, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const fueraClic = async () => {
  document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  await flushPromises()
}

describe('la lista completa sigue visible al reproducir', () => {
  it('poner una cancion no reduce la lista', async () => {
    const w = await montar()
    expect(w.findAll('tbody tr')).toHaveLength(6)
    await w.findAll('tbody tr')[2].trigger('dblclick')
    await flushPromises()
    expect(w.findAll('tbody tr'), 'la lista se encogio al reproducir').toHaveLength(6)
  })

  it('"Ver todo" desde la cola devuelve la lista entera', async () => {
    const w = await montar()
    await w.findAll('tbody tr')[1].trigger('dblclick')
    await flushPromises()
    // abrir la cola
    const queueButton = w.findAll('.player .pl-btn').at(-1)
    await queueButton.trigger('click')
    await flushPromises()
    expect(w.findAll('.queue-row')).toHaveLength(3) // antes, ahora, luego
    await w.find('.queue-more').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.findAll('tbody tr'), 'no volvio la lista completa').toHaveLength(6)
  })

  it('en un repertorio tambien se ven todas', async () => {
    status.playlists = [{ id: 7, name: 'Domingo', n: 6, seconds: 600 }]
    const w = await montar()
    await w
      .findAll('.nav-link')
      .find((e) => e.text().includes('Domingo'))
      .trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.findAll('tbody tr')).toHaveLength(6)
    await w.findAll('tbody tr')[0].trigger('dblclick')
    await flushPromises()
    expect(w.findAll('tbody tr')).toHaveLength(6)
  })
})

// Estaba todo revuelto: el tema entre «agrupar» y «densidad». Ahora va en dos
// grupos, lo que cambia la lista que ves y lo que cambia la app entera.
describe('el menu de Vista', () => {
  const abrir = async (w) => {
    await w
      .findAll('button')
      .find((b) => b.text().includes('Vista'))
      .trigger('click')
    await flushPromises()
  }

  it('separa lo de la lista de lo de la app', async () => {
    const w = await montar()
    await abrir(w)
    const grupos = w.findAll('.view-menu .menu-block')
    expect(grupos).toHaveLength(2)
    expect(grupos[0].text()).toContain('Agrupar')
    expect(grupos[0].text()).toContain('Densidad')
    expect(grupos[1].text()).toContain('Tema')
    expect(grupos[1].text()).toContain('Tamaño de la app')
    expect(grupos[0].text(), 'el tema no pinta nada entre lo de la lista').not.toContain('Tema')
  })

  // El mini reproductor ya no se abre desde aqui: sale al pulsar el icono de
  // la bandeja, que es lo que se pidio. Lo que si tiene que haber es una
  // forma de salir, porque cerrar la ventana ahora solo la esconde.
  it('ofrece salir de la aplicacion', async () => {
    const w = await montar()
    await abrir(w)
    const salir = w.findAll('button').find((b) => b.text().includes('Salir de DanPlay'))
    expect(salir, 'sin esto no hay forma de cerrar DanPlay desde la ventana').toBeTruthy()
  })
})

describe('los menus se cierran al pulsar fuera', () => {
  it('el menu de Vista', async () => {
    const w = await montar()
    await w
      .findAll('button')
      .find((b) => b.text().includes('Vista'))
      .trigger('click')
    await flushPromises()
    expect(w.findAll('.view-menu')).toHaveLength(1)
    await fueraClic()
    expect(w.findAll('.view-menu'), 'el menu de Vista sigue abierto').toHaveLength(0)
  })

  it('la cola del reproductor', async () => {
    const w = await montar()
    await w.findAll('tbody tr')[0].trigger('dblclick')
    await flushPromises()
    await w.findAll('.player .pl-btn').at(-1).trigger('click')
    await flushPromises()
    expect(w.findAll('.queue')).toHaveLength(1)
    await fueraClic()
    expect(w.findAll('.queue'), 'la cola sigue abierta').toHaveLength(0)
  })

  it('el desplegable de un selector', async () => {
    const w = await montar()
    await w
      .findAll('button')
      .find((b) => b.text().includes('Vista'))
      .trigger('click')
    await flushPromises()
    await w.findAll('.select-box')[0].trigger('click')
    await flushPromises()
    expect(w.findAll('.select-menu')).toHaveLength(1)
    await fueraClic()
    expect(w.findAll('.select-menu')).toHaveLength(0)
  })
})
