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
import { dialogCancel, dialogOk, useDialog } from '../src/composables/useDialog.js'
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

async function mountView() {
  const w = mount(App, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const clickButton = async (w, text) => {
  await w
    .findAll('.nav-link')
    .find((b) => b.text().includes(text))
    .trigger('click')
  await settle()
}
const openView = async (w) => {
  await w
    .findAll('button')
    .find((b) => b.text().includes('Vista'))
    .trigger('click')
  await flushPromises()
}
/** El nombre de la transicion con la que entra ese elemento (en las pruebas, `transition-stub`). */
const transition = (el) => el.closest('transition-stub')?.getAttribute('name') ?? null

describe('what is shown', () => {
  it('notices render with the classes the CSS knows', async () => {
    // Antes salian con notice-ok / notice-ambar y el css definia hint-ok /
    // hint-amber: los avisos eran siempre grises.
    const w = await mountView()
    notify('Guardado', 'ok')
    notify('No se pudo')
    await flushPromises()
    const [good, bad] = w.findAll('.toast')
    expect(good.classes()).toContain('hint-ok')
    expect(bad.classes()).toContain('hint-amber')
    const css = allCss()
    for (const c of ['hint-ok', 'hint-amber'])
      expect(css).toMatch(new RegExp(`\\.${c}[\\s,{:.>+~]`))
  })

  it('the View menu comes in with its transition, like the other panels', async () => {
    // aparecia de golpe mientras el resto se desvanecia
    const w = await mountView()
    await openView(w)
    expect(transition(w.find('.view-menu').element)).toBe('dropdown')
  })

  it('study mode slides up from the player with its transition', async () => {
    const w = await mountView()
    await w.find('.pl-study').trigger('click')
    await flushPromises()
    expect(transition(w.find('.study').element)).toBe('study')
  })

  it('each view renders with its own markup', async () => {
    const w = await mountView()
    const box = { rows: '.rows', cards: '.cards', grid: '.grid', table: 'table.song-table' }
    for (const [i, view] of ['rows', 'cards', 'grid', 'table'].entries()) {
      await w.findAll('.view-switch button')[i].trigger('click')
      await flushPromises()
      expect(w.find(box[view]).exists(), `«${view}» no pinta nada`).toBe(true)
      expect(w.findAll('[data-song-row]')).toHaveLength(3)
    }
  })

  it('where the switcher does not fit, the view is chosen from the menu', async () => {
    const w = await mountView()
    await openView(w)
    const layoutField = w.findAll('.view-menu .field').find((f) => f.text().includes('Cómo se ve'))
    await layoutField.find('.select-box').trigger('click')
    await layoutField
      .findAll('.select-opt')
      .find((o) => o.text().includes('Cuadrícula'))
      .trigger('click')
    await flushPromises()
    expect(w.find('.grid').exists()).toBe(true)
    // y el conmutador de la barra se esconde en estrecho
    expect(allCss()).toMatch(/\.view-switch\{display:none\}/)
  })

  it('the theme picker shows the color of each theme', async () => {
    const w = await mountView()
    await openView(w)
    const theme = w.findAll('.view-menu .field').find((f) => f.text().includes('Tema'))
    await theme.find('.select-box').trigger('click')
    const dots = theme.findAll('.select-dot')
    expect(dots.length).toBeGreaterThan(5)
    for (const p of dots) expect(p.attributes('style')).toMatch(/background:\s*rgb/)
  })

  it('Inbox reserves no room for the details panel; Duplicates does', async () => {
    const w = await mountView()
    expect(w.find('.details').exists()).toBe(true)
    await clickButton(w, 'Entrada')
    expect(w.find('.details').exists(), 'Entrada no tiene canciones que enseñar').toBe(false)
    // desde Duplicados se escuchan las copias y la ficha se llena
    await clickButton(w, 'Duplicados')
    expect(w.find('.details').exists()).toBe(true)
  })
})

describe('sorting', () => {
  it('clicking the same column reverses the order; another one starts with its natural order', async () => {
    const w = await mountView()
    const column = (k) => w.find(`th[data-col="${k}"] .th-sort`)
    const last = () => api.search.mock.calls.at(-1)[0]
    await column('title').trigger('click')
    await flushPromises()
    expect([last().sort, last().desc]).toEqual(['title', false])
    await column('title').trigger('click')
    await flushPromises()
    expect([last().sort, last().desc], 'la misma columna deberia invertirse').toEqual([
      'title',
      true
    ])
    await column('dur').trigger('click')
    await flushPromises()
    expect([last().sort, last().desc], 'por duracion, lo mas largo primero').toEqual([
      'duration',
      true
    ])
  })
})

describe('the search box', () => {
  // Con temporizadores de verdad: los falsos adelantan `Date.now()` y Vue
  // descarta despues los clics sobre lo que se monto «en el futuro».
  const search = async (w, text) => {
    await w.find('.topbar input').setValue(text)
    await new Promise((r) => setTimeout(r, SEARCH_DELAY + 20))
    await flushPromises()
  }

  it('in the library it filters the list itself; elsewhere it opens a dropdown of results', async () => {
    const w = await mountView()
    api.search.mockClear()
    await search(w, 'gozo')
    expect(api.search.mock.calls.at(-1)[0].q).toBe('gozo')
    expect(w.find('.search-results').exists()).toBe(false)
    // en Favoritos esa lista es otra cosa: se busca en toda la biblioteca
    await w.find('.chip.x').trigger('click') // quitar la busqueda
    await clickButton(w, 'Favoritos')
    await search(w, 'shekinah')
    expect(api.quickSearch).toHaveBeenCalledWith('shekinah', 60)
    expect(w.find('.search-results').exists()).toBe(true)
    // la caja anuncia la lista de resultados que maneja
    const box = w.find('.topbar input')
    expect(box.attributes('role')).toBe('combobox')
    expect(box.attributes('aria-expanded')).toBe('true')
    expect(document.getElementById(box.attributes('aria-controls'))).toBeTruthy()
  })

  it('playing a result makes the search results the queue', async () => {
    // si no, «siguiente» no recorreria los resultados, que es lo que se
    // espera despues de buscar
    const w = await mountView()
    await clickButton(w, 'Favoritos')
    await search(w, 'barak')
    await w.findAll('.sr-play')[1].trigger('click')
    await flushPromises()
    await flushPromises()
    const [items, start, origin] = playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([1, 2, 3])
    expect(start).toBe(2)
    expect(origin.label).toContain('barak')
  })

  it('advanced search opens from a button inside the field itself', async () => {
    const w = await mountView()
    const button = w.find('.search-box .field-box .search-more')
    expect(button.exists(), 'el boton no va dentro del campo').toBe(true)
    await button.trigger('click')
    expect(w.find('.search-panel').exists()).toBe(true)
  })

  // Las opciones de un resultado del desplegable: antes el clic derecho no
  // hacia nada, y un menu abierto fuera del desplegable lo cerraba al pulsar
  // en el (el clic cae «fuera» de la caja de busqueda).
  describe('the options of a result', () => {
    const prepare = async () => {
      const w = await mountView()
      await clickButton(w, 'Favoritos')
      await search(w, 'barak')
      expect(w.find('.search-results').exists()).toBe(true)
      return w
    }
    const openOptions = async (w, row) => {
      await w.findAll('.sr-row')[row].trigger('contextmenu')
      await flushPromises()
    }
    // como lo hace el raton: se pulsa (mousedown) y se suelta (click)
    const pick = async (w, text) => {
      const item = w.findAll('.ctx-item').find((b) => b.text().includes(text))
      expect(item, `falta «${text}» en el menu`).toBeTruthy()
      await item.trigger('mousedown')
      await item.trigger('click')
      await flushPromises()
    }

    it('allow adding several results to playlists without closing the dropdown', async () => {
      state.playlists = [
        { id: 7, name: 'Domingo', n: 0 },
        { id: 8, name: 'Ensayo', n: 2 }
      ]
      const w = await mountView()
      await clickButton(w, 'Favoritos')
      await search(w, 'barak')
      await openOptions(w, 1)
      await pick(w, 'Añadir a una lista')
      await pick(w, 'Ensayo')
      expect(api.addToPlaylist).toHaveBeenCalledWith(8, [2])
      expect(w.find('.ctx').exists(), 'el menu se cierra al elegir').toBe(false)
      expect(w.find('.search-results').exists(), 'el desplegable sigue ahi').toBe(true)
      // y se puede seguir: otra cancion, otro repertorio
      await openOptions(w, 0)
      await pick(w, 'Añadir a una lista')
      await pick(w, 'Domingo')
      expect(api.addToPlaylist).toHaveBeenLastCalledWith(7, [1])
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('the row «⋯» opens the same options, without selecting it or closing anything', async () => {
      const w = await prepare()
      api.song.mockClear()
      await w.findAll('.sr-more')[1].trigger('click')
      await flushPromises()
      expect(w.find('.ctx').exists()).toBe(true)
      expect(w.find('.ctx-head').text()).toContain('Shekinah')
      expect(w.find('.search-results').exists()).toBe(true)
      // no es la lista: la ficha de al lado no cambia a esa cancion
      expect(api.song).not.toHaveBeenCalled()
      // y lo suyo (no lo de una fila de la lista) esta en el menu
      const texts = w.findAll('.ctx-item').map((b) => b.text())
      expect(texts.some((t) => t.includes('Añadir a una lista'))).toBe(true)
      expect(texts.some((t) => t.includes('Quitar de esta lista'))).toBe(false)
    })

    it('Play from the menu queues the search results, like the row button', async () => {
      const w = await prepare()
      await openOptions(w, 1)
      await pick(w, 'Reproducir')
      await flushPromises()
      const [items, start, origin] = playback.bridge.setQueue.mock.calls.at(-1)
      expect(items.map((t) => t.id)).toEqual([1, 2, 3])
      expect(start).toBe(2)
      expect(origin.label).toContain('barak')
    })

    it('marking a favorite from there shows in its own menu next time', async () => {
      const w = await prepare()
      await openOptions(w, 0)
      await pick(w, 'Marcar como favorito')
      expect(api.toggleFavorite).toHaveBeenCalledWith(1, true)
      await openOptions(w, 0)
      const texts = w.findAll('.ctx-item').map((b) => b.text())
      expect(texts.some((t) => t.includes('Quitar de favoritos'))).toBe(true)
    })

    it('Escape closes the menu but not the dropdown; with the menu closed, the dropdown', async () => {
      const w = await prepare()
      await openOptions(w, 0)
      expect(w.find('.ctx').exists()).toBe(true)
      const escape = () =>
        document.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
        )
      escape()
      await flushPromises()
      expect(w.find('.ctx').exists()).toBe(false)
      expect(w.find('.search-results').exists(), 'Escape era para el menu').toBe(true)
      escape()
      await flushPromises()
      expect(w.find('.search-results').exists()).toBe(false)
    })

    it('a click anywhere else closes everything', async () => {
      const w = await prepare()
      await openOptions(w, 0)
      document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
      await flushPromises()
      expect(w.find('.ctx').exists()).toBe(false)
      expect(w.find('.search-results').exists()).toBe(false)
    })

    it('with the keyboard: the menu key in the box opens the options of the highlighted result', async () => {
      const w = await prepare()
      const box = w.find('.topbar input')
      await box.trigger('keydown', { key: 'ArrowDown' })
      await box.trigger('keydown', { key: 'ContextMenu' })
      await flushPromises()
      expect(w.find('.ctx').exists()).toBe(true)
      expect(w.find('.ctx-head').text()).toContain('Shekinah')
    })

    it('on the song that is playing the menu pauses: it neither restarts it nor changes the queue', async () => {
      const w = await prepare()
      await w.findAll('.sr-play')[1].trigger('click') // suena «Shekinah»
      await flushPromises()
      await flushPromises()
      // reproducir cierra el desplegable: se busca otra vez
      await search(w, 'barak otra vez')
      playback.bridge.setQueue.mockClear()
      playback.bridge.toggle.mockClear()
      await openOptions(w, 1)
      const texts = w.findAll('.ctx-item').map((b) => b.text())
      expect(texts.some((t) => t.startsWith('Pausar'))).toBe(true)
      await pick(w, 'Pausar')
      expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
      expect(
        playback.bridge.setQueue,
        'pausar no es poner la cancion otra vez'
      ).not.toHaveBeenCalled()
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('marking a favorite does not move the highlighted row (the keyboard stays where it was)', async () => {
      const w = await prepare()
      const box = w.find('.topbar input')
      await box.trigger('keydown', { key: 'ArrowDown' })
      await box.trigger('keydown', { key: 'ArrowDown' })
      expect(w.find('.sr-row.on').text()).toContain('Cancion 3')
      await box.trigger('keydown', { key: 'ContextMenu' })
      await flushPromises()
      await pick(w, 'Marcar como favorito')
      expect(api.toggleFavorite).toHaveBeenCalledWith(3, true)
      // se parchea en sitio: con un array nuevo se volvia a resaltar la primera
      expect(w.find('.sr-row.on').text()).toContain('Cancion 3')
    })

    it('a dialog opened from the menu does not close the dropdown (Nueva lista… → Crear)', async () => {
      const w = await prepare()
      await openOptions(w, 0)
      await pick(w, 'Añadir a una lista')
      await pick(w, 'Nueva lista')
      expect(useDialog().dialog.value.open).toBe(true)
      // el primer clic dentro del dialogo cae «fuera» de la caja de busqueda
      document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
      await flushPromises()
      expect(w.find('.search-results').exists(), 'pulsar en el dialogo no es pulsar fuera').toBe(
        true
      )
      // Escape cancela el dialogo, no el desplegable
      document.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
      )
      await flushPromises()
      expect(useDialog().dialog.value.open).toBe(false)
      expect(w.find('.search-results').exists(), 'Escape era para el dialogo').toBe(true)
      // y se puede seguir: otra vez, esta vez aceptando
      await openOptions(w, 0)
      await pick(w, 'Añadir a una lista')
      await pick(w, 'Nueva lista')
      dialogOk('Mi lista')
      await flushPromises()
      await flushPromises()
      expect(api.addToPlaylist).toHaveBeenCalledWith(expect.any(Number), [1])
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('choosing an option with the mouse returns focus to the box', async () => {
      // sin esto el foco se perdia en el <body>: lo escrito no llegaba a la caja y
      // la flecha abajo bajaba el volumen del reproductor
      const w = await prepare()
      await openOptions(w, 0)
      await pick(w, 'Marcar como favorito')
      await flushPromises()
      expect(document.activeElement).toBe(w.find('.topbar input').element)
      // pero si la opcion abre un dialogo, el foco es del dialogo
      await openOptions(w, 0)
      await pick(w, 'Añadir a una lista')
      await pick(w, 'Nueva lista')
      await flushPromises()
      expect(document.activeElement).not.toBe(w.find('.topbar input').element)
      dialogCancel()
      await flushPromises()
    })

    it('moving to the trash removes the row from the dropdown (no ghost song left behind)', async () => {
      const w = await prepare()
      expect(w.findAll('.sr-row')).toHaveLength(3)
      await openOptions(w, 0)
      await pick(w, 'Mandar a la papelera')
      expect(useDialog().dialog.value.open).toBe(true)
      dialogOk()
      await flushPromises()
      await flushPromises()
      expect(api.deleteSong).toHaveBeenCalledWith(1, { fromLists: false })
      expect(w.findAll('.sr-row')).toHaveLength(2)
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('«Ver detalles» on a result selects the song in the panel without opening a dialog', async () => {
      const w = await prepare()
      api.song.mockClear()
      await openOptions(w, 1)
      await pick(w, 'Ver detalles')
      await flushPromises()
      expect(api.song).toHaveBeenCalledWith(2)
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('double-clicking the «⋯» neither plays nor changes the queue', async () => {
      const w = await prepare()
      playback.bridge.setQueue.mockClear()
      await w.findAll('.sr-more')[0].trigger('dblclick')
      expect(playback.bridge.setQueue).not.toHaveBeenCalled()
    })
  })

  // Varios resultados a la vez (el botón «Seleccionar» del desplegable, Ctrl o
  // Mayús + clic, Ctrl+Espacio): lo mismo que con varias canciones de la lista,
  // sin cerrar el desplegable.
  describe('several results at once', () => {
    const prepare = async () => {
      const w = await mountView()
      await clickButton(w, 'Favoritos')
      await search(w, 'barak')
      expect(w.findAll('.sr-row')).toHaveLength(3)
      return w
    }
    const rows = (w) => w.findAll('.sr-row')
    const picked = (w) => rows(w).flatMap((f, i) => (f.classes().includes('picked') ? [i + 1] : []))
    const button = (w, label) => w.find(`.sr-head button[aria-label="${label}"]`)
    const box = (w) => w.find('.topbar input')
    const pick = async (w, text) => {
      const item = w.findAll('.ctx-item').find((b) => b.text().includes(text))
      expect(item, `falta «${text}» en el menu`).toBeTruthy()
      await item.trigger('mousedown')
      await item.trigger('click')
      await flushPromises()
    }

    it('two results to a playlist with «Añadir a una lista ▾»: a single request and the dropdown stays open', async () => {
      state.playlists = [
        { id: 7, name: 'Domingo', n: 0 },
        { id: 8, name: 'Ensayo', n: 2 }
      ]
      const w = await prepare()
      await w.find('.sr-select').trigger('click')
      await rows(w)[0].trigger('click')
      await rows(w)[2].trigger('click')
      expect(w.find('.sr-count').text()).toBe('2 seleccionados')
      await button(w, 'Añadir a una lista').trigger('click')
      await flushPromises()
      expect(w.find('.ctx-head').text()).toBe('Añadir 2 a una lista')
      await pick(w, 'Ensayo')
      expect(api.addToPlaylist).toHaveBeenCalledTimes(1)
      expect(api.addToPlaylist).toHaveBeenCalledWith(8, [1, 3])
      expect(w.find('.ctx').exists()).toBe(false)
      expect(w.find('.search-results').exists(), 'el desplegable sigue ahi').toBe(true)
      expect(picked(w), 'y lo elegido, para seguir haciendo cosas').toEqual([1, 3])
      // el foco vuelve a la caja: lo que se escriba sigue llegando
      expect(document.activeElement).toBe(box(w).element)
      // la ficha de al lado no se ha movido a ninguna
      expect(w.findAll('[data-song-row].selected')).toHaveLength(0)
    })

    it('with Ctrl+click and the bar «⋯»: the options for all of them, without «quitar de esta lista»', async () => {
      state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
      state.playlistSongs = [song(1), song(2), song(3)]
      const w = await mountView()
      await w.find('.nav-playlist').trigger('click')
      await settle()
      await search(w, 'barak')
      api.song.mockClear()
      await rows(w)[1].trigger('click', { ctrlKey: true })
      await rows(w)[2].trigger('click', { ctrlKey: true })
      await flushPromises()
      expect(api.song, 'Ctrl+clic no abre la ficha').not.toHaveBeenCalled()
      await button(w, 'Más opciones').trigger('click')
      await flushPromises()
      expect(w.find('.ctx-head').text()).toBe('2 canciones')
      const texts = w.findAll('.ctx-item').map((b) => b.text())
      expect(texts.some((t) => t.startsWith('Reproducir estas 2'))).toBe(true)
      expect(texts.some((t) => t.startsWith('Enviar 2 por Telegram'))).toBe(true)
      // habla de la lista abierta, no de lo encontrado
      expect(texts.some((t) => t.includes('de esta lista'))).toBe(false)
      await pick(w, 'Añadir 2 a una lista')
      await pick(w, 'Domingo')
      expect(api.addToPlaylist).toHaveBeenCalledWith(7, [2, 3])
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('right-clicking a selected one opens the menu for all; on another one, its own', async () => {
      const w = await prepare()
      await rows(w)[0].trigger('click', { ctrlKey: true })
      await rows(w)[1].trigger('click', { ctrlKey: true })
      await rows(w)[1].trigger('contextmenu')
      await flushPromises()
      expect(w.find('.ctx-head').text()).toBe('2 canciones')
      await rows(w)[2].trigger('contextmenu')
      await flushPromises()
      expect(w.find('.ctx-head').text()).toContain('Cancion 3')
      expect(picked(w)).toEqual([1, 2])
    })

    it('«Reproducir estas 2» queues the selected ones, in results order', async () => {
      const w = await prepare()
      await rows(w)[2].trigger('click', { ctrlKey: true })
      await rows(w)[0].trigger('click', { ctrlKey: true })
      await button(w, 'Más opciones').trigger('click')
      await flushPromises()
      await pick(w, 'Reproducir estas 2')
      await flushPromises()
      const [items, start, origin] = playback.bridge.setQueue.mock.calls.at(-1)
      expect(items.map((t) => t.id)).toEqual([1, 3])
      expect(start).toBe(1)
      expect(origin.label).toContain('barak')
    })

    it('the trash asks for confirmation: cancelled deletes nothing; accepted, they leave the dropdown', async () => {
      const w = await prepare()
      await rows(w)[0].trigger('click', { ctrlKey: true })
      await rows(w)[1].trigger('click', { shiftKey: true })
      expect(picked(w)).toEqual([1, 2])
      const trash = async () => {
        await button(w, 'Más opciones').trigger('click')
        await flushPromises()
        await pick(w, 'Mandar 2 a la papelera')
        expect(useDialog().dialog.value.open).toBe(true)
        expect(useDialog().dialog.value.title).toBe('Mandar 2 canciones a la papelera')
      }
      await trash()
      dialogCancel()
      await flushPromises()
      await flushPromises()
      expect(api.deleteSong).not.toHaveBeenCalled()
      expect(rows(w)).toHaveLength(3)
      expect(picked(w)).toEqual([1, 2])
      expect(w.find('.search-results').exists()).toBe(true)

      await trash()
      dialogOk()
      await flushPromises()
      await flushPromises()
      expect(api.deleteSong.mock.calls).toEqual([
        [1, { fromLists: false }],
        [2, { fromLists: false }]
      ])
      expect(rows(w)).toHaveLength(1)
      expect(w.find('.sr-picking').exists(), 'lo que ya no esta, no esta elegido').toBe(false)
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('from the box: Ctrl+Space, Shift+arrows, and Escape clears the selection before closing', async () => {
      const w = await prepare()
      await box(w).trigger('keydown', { key: ' ', ctrlKey: true }) // la primera
      await box(w).trigger('keydown', { key: 'ArrowDown', shiftKey: true })
      expect(picked(w)).toEqual([1, 2])
      await box(w).trigger('keydown', { key: 'Escape' })
      await flushPromises()
      expect(picked(w)).toEqual([])
      expect(w.find('.search-results').exists(), 'Escape era para lo elegido').toBe(true)
      await box(w).trigger('keydown', { key: 'Escape' })
      await flushPromises()
      expect(w.find('.search-results').exists()).toBe(false)
    })

    it('Ctrl+A in the box belongs to the text: it selects neither results nor songs in the list', async () => {
      const w = await prepare()
      const e = new KeyboardEvent('keydown', {
        key: 'a',
        ctrlKey: true,
        bubbles: true,
        cancelable: true
      })
      box(w).element.dispatchEvent(e)
      await flushPromises()
      expect(e.defaultPrevented, 'el navegador selecciona el texto').toBe(false)
      expect(picked(w)).toEqual([])
      expect(w.find('.sel-bar').exists()).toBe(false)
      expect(w.findAll('[data-song-row].selected')).toHaveLength(0)
    })

    it('another search or closing the dropdown forgets the selection', async () => {
      const w = await prepare()
      await rows(w)[0].trigger('click', { ctrlKey: true })
      await search(w, 'barak otra vez')
      expect(picked(w)).toEqual([])
      await rows(w)[1].trigger('click', { ctrlKey: true })
      document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
      await flushPromises()
      expect(w.find('.search-results').exists()).toBe(false)
      await search(w, 'barak')
      expect(picked(w)).toEqual([])
    })
  })
})

describe('blurred covers', () => {
  const menu = async (w, row = 0) => {
    await w.findAll('tbody tr')[row].trigger('contextmenu')
    await flushPromises()
    return w.findAll('.ctx-item')
  }

  it('it is blurred and unblurred from the song menu', async () => {
    const w = await mountView()
    const blur = (await menu(w)).find((b) => b.text().includes('Difuminar la portada'))
    expect(blur, 'falta la accion en el menu').toBeTruthy()
    await blur.trigger('click')
    await flushPromises()
    expect(api.setBlur).toHaveBeenCalledWith(1, true)
    expect(w.find('.toast').text()).toContain('Portada difuminada')
    const viewCover = (await menu(w)).find((b) => b.text().includes('Ver la portada'))
    expect(viewCover, 'no hay forma de volver a verla').toBeTruthy()
    await viewCover.trigger('click')
    await flushPromises()
    expect(api.setBlur).toHaveBeenLastCalledWith(1, false)
  })

  it('the drag ghost respects it too', async () => {
    state.songs = [song(1, { blur: 1 })]
    const w = await mountView()
    const row = w.find('tbody tr').element
    row.dispatchEvent(
      new MouseEvent('pointerdown', { bubbles: true, button: 0, clientX: 5, clientY: 5 })
    )
    row.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 80, clientY: 90 }))
    await flushPromises()
    expect(document.querySelector('.drag-ghost .cover-art').classList.contains('blurred')).toBe(
      true
    )
    window.dispatchEvent(new MouseEvent('pointerup', { bubbles: true }))
  })
})

describe('rating with the keyboard', () => {
  it('the stars of a row can also be set from its menu', async () => {
    // dentro de la fila no son parada del tabulador: con el teclado se abre
    // el menu de la cancion (Mayus+F10) y se valora desde ahi
    const w = await mountView()
    const row = w.find('tbody tr')
    await row.trigger('keydown', { key: 'F10', shiftKey: true })
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

// La selección múltiple: el botón «Seleccionar» (casillas), la barra con lo
// que se hace con las elegidas y las teclas con el foco en el <body> (tras un
// clic el foco se suelta solo). Lo de dentro de cada lista (las casillas, la
// de «todas» de la cabecera) se prueba aparte.
describe('selecting several', () => {
  const rows = (w) => w.findAll('tbody tr')
  const selectedCount = (w) => rows(w).filter((f) => f.classes().includes('selected')).length
  const bar = (w) => w.find('.sel-bar')
  const button = (w) => w.find('.filters .select-toggle')
  const press = async (key, extra = {}, target = document.body) => {
    target.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })
    )
    await flushPromises()
  }
  const pick = async (w, text) => {
    const item = w.findAll('.ctx-item').find((b) => b.text().includes(text))
    expect(item, `falta «${text}» en el menu`).toBeTruthy()
    await item.trigger('mousedown')
    await item.trigger('click')
    await flushPromises()
  }

  it('the button turns on the mode: each click on a row ticks or unticks it, and the bar counts them', async () => {
    const w = await mountView()
    expect(bar(w).exists()).toBe(false)
    expect(button(w).attributes('aria-pressed')).toBe('false')
    await button(w).trigger('click')
    expect(button(w).attributes('aria-pressed')).toBe('true')
    expect(bar(w).find('.sel-status').text()).toBe('Elige canciones')
    // las casillas, una por fila
    expect(w.findAll('tbody .pick-box')).toHaveLength(3)
    await rows(w)[0].trigger('click')
    await rows(w)[2].trigger('click')
    await flushPromises()
    expect(selectedCount(w)).toBe(2)
    expect(bar(w).find('.sel-status').text()).toBe('2 seleccionadas')
    await rows(w)[0].trigger('click') // la desmarca, sin soltar la otra
    await flushPromises()
    expect(selectedCount(w)).toBe(1)
    expect(bar(w).find('.sel-status').text()).toBe('1 seleccionada')
    // «Listo» lo termina todo
    await bar(w).find('[data-act="done"]').trigger('click')
    expect(bar(w).exists()).toBe(false)
    expect(button(w).attributes('aria-pressed')).toBe('false')
    expect(selectedCount(w)).toBe(0)
  })

  it('adding two to a playlist from the bar sends both at once', async () => {
    state.playlists = [
      { id: 7, name: 'Domingo', n: 0 },
      { id: 8, name: 'Ensayo', n: 2 }
    ]
    const w = await mountView()
    await button(w).trigger('click')
    await rows(w)[0].trigger('click')
    await rows(w)[2].trigger('click')
    await flushPromises()
    await bar(w).find('[data-act="list"]').trigger('click')
    await flushPromises()
    expect(w.find('.ctx-head').text()).toBe('Añadir 2 a una lista')
    await pick(w, 'Ensayo')
    expect(api.addToPlaylist).toHaveBeenCalledWith(8, [1, 3])
    expect(bar(w).exists(), 'se pueden seguir haciendo cosas con ellas').toBe(true)
  })

  it('Telegram from the bar opens Telegram with their files', async () => {
    const w = await mountView()
    await button(w).trigger('click')
    await rows(w)[1].trigger('click')
    await rows(w)[2].trigger('click')
    await flushPromises()
    await bar(w).find('[data-act="telegram"]').trigger('click')
    await flushPromises()
    const { app } = await import('../src/api.js')
    expect(app.sendToTelegram).toHaveBeenCalledWith([
      '/musica/cancion-2.mp3',
      '/musica/cancion-3.mp3'
    ])
  })

  it('Ctrl+A selects all loaded songs and the bar counts them, without the mode', async () => {
    const w = await mountView()
    await press('a', { ctrlKey: true })
    expect(selectedCount(w)).toBe(3)
    expect(bar(w).find('.sel-status').text()).toBe('3 seleccionadas')
    expect(bar(w).find('[data-act="none"]').exists(), 'ya están todas: «Ninguna»').toBe(true)
    await bar(w).find('[data-act="none"]').trigger('click')
    expect(selectedCount(w)).toBe(0)
    expect(bar(w).exists()).toBe(false)
  })

  it('Delete in the library asks about the trash; cancelling deletes nothing', async () => {
    const w = await mountView()
    await press('a', { ctrlKey: true })
    await press('Delete')
    const { dialog } = useDialog()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.title).toBe('Mandar 3 canciones a la papelera')
    dialogCancel()
    await flushPromises()
    expect(api.deleteSong).not.toHaveBeenCalled()
    expect(selectedCount(w), 'cancelar no suelta lo elegido').toBe(3)
  })

  it('Delete with a single selected song (no mode) asks about that one, with its file', async () => {
    const w = await mountView()
    await rows(w)[1].trigger('click')
    await flushPromises()
    await press('Delete')
    const { dialog } = useDialog()
    expect(dialog.value.title).toBe('Mandar a la papelera')
    dialogCancel()
    await flushPromises()
    expect(api.deleteSong).not.toHaveBeenCalled()
  })

  it('Delete in a playlist removes from the playlist (without touching files or asking)', async () => {
    state.playlists = [{ id: 7, name: 'Domingo', n: 3 }]
    state.playlistSongs = [song(1), song(2), song(3)]
    const w = await mountView()
    await w.find('.nav-playlist').trigger('click')
    await settle()
    await button(w).trigger('click')
    await rows(w)[0].trigger('click')
    await rows(w)[1].trigger('click')
    await flushPromises()
    expect(bar(w).find('[data-act="remove"]').attributes('aria-label')).toBe('Quitar de «Domingo»')
    await press('Delete')
    const { dialog } = useDialog()
    expect(dialog.value.open).toBe(false)
    expect(api.removeFromPlaylist.mock.calls).toEqual([
      [7, 1],
      [7, 2]
    ])
    expect(api.deleteSong).not.toHaveBeenCalled()
    expect(w.findAll('.toast').at(-1).text()).toContain('2 quitadas de «Domingo»')
  })

  it('Escape clears the selection and leaves the mode; with nothing selected it does nothing', async () => {
    const w = await mountView()
    await button(w).trigger('click')
    await rows(w)[0].trigger('click')
    await rows(w)[1].trigger('click')
    await flushPromises()
    await press('Escape')
    expect(bar(w).exists()).toBe(false)
    expect(button(w).attributes('aria-pressed')).toBe('false')
    expect(selectedCount(w)).toBe(0)
    // nada que soltar: Escape no se toca (es de quien cierre otra cosa)
    const ev = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    document.body.dispatchEvent(ev)
    expect(ev.defaultPrevented).toBe(false)
  })

  it('with focus in the search box none of this fires', async () => {
    const w = await mountView()
    await button(w).trigger('click')
    await rows(w)[0].trigger('click')
    await flushPromises()
    const box = w.find('.topbar input').element
    box.focus()
    await press('a', { ctrlKey: true }, box)
    expect(selectedCount(w), 'Ctrl+A es del texto de la caja').toBe(1)
    await press('Delete', {}, box)
    expect(useDialog().dialog.value.open).toBe(false)
    await press('Escape', {}, box)
    expect(button(w).attributes('aria-pressed'), 'Escape en la caja es de la caja').toBe('true')
    expect(bar(w).exists()).toBe(true)
  })

  it('with focus on a row, the list handles the keys and they still arrive', async () => {
    const w = await mountView()
    await rows(w)[0].trigger('keydown', { key: 'a', ctrlKey: true })
    await flushPromises()
    expect(selectedCount(w)).toBe(3)
    await rows(w)[0].trigger('keydown', { key: 'Escape' })
    await flushPromises()
    expect(bar(w).exists()).toBe(false)
  })

  it('switching view leaves the mode', async () => {
    const w = await mountView()
    await button(w).trigger('click')
    await rows(w)[0].trigger('click')
    await flushPromises()
    await clickButton(w, 'Favoritos')
    expect(bar(w).exists()).toBe(false)
    expect(button(w).attributes('aria-pressed')).toBe('false')
  })

  it('on mobile, tapping in the mode ticks without opening the details panel over the list', async () => {
    const width = window.innerWidth
    window.innerWidth = 400
    window.dispatchEvent(new Event('resize'))
    try {
      const w = await mountView()
      await button(w).trigger('click')
      await rows(w)[1].trigger('click')
      await flushPromises()
      expect(selectedCount(w)).toBe(1)
      expect(document.querySelector('.drawer'), 'la ficha taparía la lista a cada toque').toBe(null)
      // fuera del modo, tocar una canción sí abre su ficha (como siempre)
      await bar(w).find('[data-act="done"]').trigger('click')
      await rows(w)[2].trigger('click')
      await flushPromises()
      expect(document.querySelector('.drawer')).not.toBe(null)
    } finally {
      window.innerWidth = width
      window.dispatchEvent(new Event('resize'))
    }
  })

  it('in the mode, playing one with its button does not tick it', async () => {
    const w = await mountView()
    await button(w).trigger('click')
    await rows(w)[0].trigger('click')
    await flushPromises()
    await rows(w)[2].find('.row-play').trigger('click')
    await flushPromises()
    expect(selectedCount(w)).toBe(1)
    expect(bar(w).find('.sel-status').text()).toBe('1 seleccionada')
  })
})
