import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { settle } from './support/pages.js'

// La app entera, mirando lo que hace y no como esta escrita. Estas pruebas
// buscaban antes texto en App.vue («¿aparece `<transition` antes del menu?»,
// «¿dice `play(song, quick.value`?»), y un texto puede estar y el
// comportamiento no.
const held = vi.hoisted(() => ({ state: null, api: null, playback: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble, createAppDouble } =
    await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: held.playback.bridge,
    app: createAppDouble(),
    core: { onStatus: v.fn(async () => () => {}), onChanged: v.fn(async () => () => {}) },
    projection: { show: v.fn(async () => {}), hide: v.fn(async () => {}) }
  }
})

import App from '../src/App.vue'
import { song } from './support/backend.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetPreferences } from '../src/composables/usePreferences.js'
import { resetChat } from '../src/composables/useChat.js'
import { dialogCancel } from '../src/composables/useDialog.js'
import { clearNotices, notify } from '../src/composables/useNotices.js'
import { allCss } from './support/css.js'
import { SEARCH_DELAY } from '../src/composables/useSearch.js'

const api = held.api
const state = held.state
const playback = held.playback

beforeEach(() => {
  state.songs = [song(1, { title: 'Mi Gozo' }), song(2, { title: 'Shekinah' }), song(3)]
  state.playlists = []
  state.status.configured = true
  resetPlayback()
  resetPreferences()
  resetChat()
  clearNotices()
  dialogCancel()
  playback.reset()
  localStorage.clear()
  // sin el dialogo de «¿abrir las canciones con DanPlay?», que se pone delante
  localStorage.setItem('danplay.default-player-asked', '1')
  vi.clearAllMocks()
})

async function montar() {
  const w = mount(App, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const pulsar = async (w, texto) => {
  await w
    .findAll('.nav-link')
    .find((b) => b.text().includes(texto))
    .trigger('click')
  await settle()
}
const abrirVista = async (w) => {
  await w
    .findAll('button')
    .find((b) => b.text().includes('Vista'))
    .trigger('click')
  await flushPromises()
}
/** El nombre de la transicion con la que entra ese elemento (en las pruebas, `transition-stub`). */
const transicion = (el) => el.closest('transition-stub')?.getAttribute('name') ?? null

describe('lo que se ve', () => {
  it('los avisos se pintan con las clases que el css conoce', async () => {
    // Antes salian con notice-ok / notice-ambar y el css definia hint-ok /
    // hint-amber: los avisos eran siempre grises.
    const w = await montar()
    notify('Guardado', 'ok')
    notify('No se pudo')
    await flushPromises()
    const [bien, mal] = w.findAll('.toast')
    expect(bien.classes()).toContain('hint-ok')
    expect(mal.classes()).toContain('hint-amber')
    const css = allCss()
    for (const c of ['hint-ok', 'hint-amber'])
      expect(css).toMatch(new RegExp(`\\.${c}[\\s,{:.>+~]`))
  })

  it('el menu de Vista entra con su transicion, como los demas paneles', async () => {
    // aparecia de golpe mientras el resto se desvanecia
    const w = await montar()
    await abrirVista(w)
    expect(transicion(w.find('.view-menu').element)).toBe('dropdown')
  })

  it('el modo estudio sube desde el reproductor con su transicion', async () => {
    const w = await montar()
    await w.find('.pl-study').trigger('click')
    await flushPromises()
    expect(transicion(w.find('.study').element)).toBe('study')
  })

  it('cada vista se pinta con lo suyo', async () => {
    const w = await montar()
    const caja = { rows: '.rows', cards: '.cards', grid: '.grid', table: 'table.song-table' }
    for (const [i, vista] of ['rows', 'cards', 'grid', 'table'].entries()) {
      await w.findAll('.view-switch button')[i].trigger('click')
      await flushPromises()
      expect(w.find(caja[vista]).exists(), `«${vista}» no pinta nada`).toBe(true)
      expect(w.findAll('[data-song-row]')).toHaveLength(3)
    }
  })

  it('donde el conmutador no cabe, la vista se elige en el menu', async () => {
    const w = await montar()
    await abrirVista(w)
    const como = w.findAll('.view-menu .field').find((f) => f.text().includes('Cómo se ve'))
    await como.find('.select-box').trigger('click')
    await como
      .findAll('.select-opt')
      .find((o) => o.text().includes('Cuadrícula'))
      .trigger('click')
    await flushPromises()
    expect(w.find('.grid').exists()).toBe(true)
    // y el conmutador de la barra se esconde en estrecho
    expect(allCss()).toMatch(/\.view-switch\{display:none\}/)
  })

  it('el selector de tema enseña el color de cada uno', async () => {
    const w = await montar()
    await abrirVista(w)
    const tema = w.findAll('.view-menu .field').find((f) => f.text().includes('Tema'))
    await tema.find('.select-box').trigger('click')
    const puntos = tema.findAll('.select-dot')
    expect(puntos.length).toBeGreaterThan(5)
    for (const p of puntos) expect(p.attributes('style')).toMatch(/background:\s*rgb/)
  })

  it('en Entrada no se reserva sitio para la ficha; en Duplicados si', async () => {
    const w = await montar()
    expect(w.find('.details').exists()).toBe(true)
    await pulsar(w, 'Entrada')
    expect(w.find('.details').exists(), 'Entrada no tiene canciones que enseñar').toBe(false)
    // desde Duplicados se escuchan las copias y la ficha se llena
    await pulsar(w, 'Duplicados')
    expect(w.find('.details').exists()).toBe(true)
  })
})

describe('ordenar', () => {
  it('pulsar la misma columna invierte el orden; otra empieza por lo natural', async () => {
    const w = await montar()
    const columna = (k) => w.find(`th[data-col="${k}"] .th-sort`)
    const ultimo = () => api.search.mock.calls.at(-1)[0]
    await columna('title').trigger('click')
    await flushPromises()
    expect([ultimo().sort, ultimo().desc]).toEqual(['title', false])
    await columna('title').trigger('click')
    await flushPromises()
    expect([ultimo().sort, ultimo().desc], 'la misma columna deberia invertirse').toEqual([
      'title',
      true
    ])
    await columna('dur').trigger('click')
    await flushPromises()
    expect([ultimo().sort, ultimo().desc], 'por duracion, lo mas largo primero').toEqual([
      'duration',
      true
    ])
  })
})

describe('el buscador', () => {
  // Con temporizadores de verdad: los falsos adelantan `Date.now()` y Vue
  // descarta despues los clics sobre lo que se monto «en el futuro».
  const buscar = async (w, texto) => {
    await w.find('.topbar input').setValue(texto)
    await new Promise((r) => setTimeout(r, SEARCH_DELAY + 20))
    await flushPromises()
  }

  it('en la biblioteca filtra la propia lista; fuera de ella, despliega resultados', async () => {
    const w = await montar()
    api.search.mockClear()
    await buscar(w, 'gozo')
    expect(api.search.mock.calls.at(-1)[0].q).toBe('gozo')
    expect(w.find('.search-results').exists()).toBe(false)
    // en Favoritos esa lista es otra cosa: se busca en toda la biblioteca
    await w.find('.chip.x').trigger('click') // quitar la busqueda
    await pulsar(w, 'Favoritos')
    await buscar(w, 'shekinah')
    expect(api.quickSearch).toHaveBeenCalledWith('shekinah', 60)
    expect(w.find('.search-results').exists()).toBe(true)
    // la caja anuncia la lista de resultados que maneja
    const caja = w.find('.topbar input')
    expect(caja.attributes('role')).toBe('combobox')
    expect(caja.attributes('aria-expanded')).toBe('true')
    expect(document.getElementById(caja.attributes('aria-controls'))).toBeTruthy()
  })

  it('al reproducir un resultado, la cola pasa a ser lo encontrado', async () => {
    // si no, «siguiente» no recorreria los resultados, que es lo que se
    // espera despues de buscar
    const w = await montar()
    await pulsar(w, 'Favoritos')
    await buscar(w, 'barak')
    await w.findAll('.sr-play')[1].trigger('click')
    await flushPromises()
    await flushPromises()
    const [items, start, origin] = playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([1, 2, 3])
    expect(start).toBe(2)
    expect(origin.label).toContain('barak')
  })

  it('la busqueda avanzada se abre desde un boton dentro del propio campo', async () => {
    const w = await montar()
    const boton = w.find('.search-box .field-box .search-more')
    expect(boton.exists(), 'el boton no va dentro del campo').toBe(true)
    await boton.trigger('click')
    expect(w.find('.search-panel').exists()).toBe(true)
  })
})

describe('portadas difuminadas', () => {
  const menu = async (w, fila = 0) => {
    await w.findAll('tbody tr')[fila].trigger('contextmenu')
    await flushPromises()
    return w.findAll('.ctx-item')
  }

  it('se difumina y se vuelve a ver desde el menu de la cancion', async () => {
    const w = await montar()
    const difuminar = (await menu(w)).find((b) => b.text().includes('Difuminar la portada'))
    expect(difuminar, 'falta la accion en el menu').toBeTruthy()
    await difuminar.trigger('click')
    await flushPromises()
    expect(api.setBlur).toHaveBeenCalledWith(1, true)
    expect(w.find('.toast').text()).toContain('Portada difuminada')
    const ver = (await menu(w)).find((b) => b.text().includes('Ver la portada'))
    expect(ver, 'no hay forma de volver a verla').toBeTruthy()
    await ver.trigger('click')
    await flushPromises()
    expect(api.setBlur).toHaveBeenLastCalledWith(1, false)
  })

  it('el fantasma que se lleva al arrastrar tambien la respeta', async () => {
    state.songs = [song(1, { blur: 1 })]
    const w = await montar()
    const fila = w.find('tbody tr').element
    fila.dispatchEvent(
      new MouseEvent('pointerdown', { bubbles: true, button: 0, clientX: 5, clientY: 5 })
    )
    fila.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 80, clientY: 90 }))
    await flushPromises()
    expect(document.querySelector('.drag-ghost .cover-art').classList.contains('blurred')).toBe(
      true
    )
    window.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
  })
})

describe('valorar con el teclado', () => {
  it('las estrellas de una fila tambien se ponen desde su menu', async () => {
    // dentro de la fila no son parada del tabulador: con el teclado se abre
    // el menu de la cancion (Mayus+F10) y se valora desde ahi
    const w = await montar()
    const fila = w.find('tbody tr')
    await fila.trigger('keydown', { key: 'F10', shiftKey: true })
    await flushPromises()
    await w
      .findAll('.ctx-item')
      .find((b) => b.text().includes('Valorar'))
      .trigger('click')
    await w
      .findAll('.ctx-item')
      .find((b) => b.text().includes('4 estrellas'))
      .trigger('click')
    await flushPromises()
    expect(api.setStars).toHaveBeenCalledWith(1, 4)
  })
})
