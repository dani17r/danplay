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

  // Las opciones de un resultado del desplegable: antes el clic derecho no
  // hacia nada, y un menu abierto fuera del desplegable lo cerraba al pulsar
  // en el (el clic cae «fuera» de la caja de busqueda).
  describe('las opciones de un resultado', () => {
    const preparar = async () => {
      const w = await montar()
      await pulsar(w, 'Favoritos')
      await buscar(w, 'barak')
      expect(w.find('.search-results').exists()).toBe(true)
      return w
    }
    const abrirOpciones = async (w, fila) => {
      await w.findAll('.sr-row')[fila].trigger('contextmenu')
      await flushPromises()
    }
    // como lo hace el raton: se pulsa (mousedown) y se suelta (click)
    const elegir = async (w, texto) => {
      const item = w.findAll('.ctx-item').find((b) => b.text().includes(texto))
      expect(item, `falta «${texto}» en el menu`).toBeTruthy()
      await item.trigger('mousedown')
      await item.trigger('click')
      await flushPromises()
    }

    it('dejan añadir varios resultados a repertorios sin cerrar el desplegable', async () => {
      state.playlists = [
        { id: 7, name: 'Domingo', n: 0 },
        { id: 8, name: 'Ensayo', n: 2 }
      ]
      const w = await montar()
      await pulsar(w, 'Favoritos')
      await buscar(w, 'barak')
      await abrirOpciones(w, 1)
      await elegir(w, 'Añadir a una lista')
      await elegir(w, 'Ensayo')
      expect(api.addToPlaylist).toHaveBeenCalledWith(8, [2])
      expect(w.find('.ctx').exists(), 'el menu se cierra al elegir').toBe(false)
      expect(w.find('.search-results').exists(), 'el desplegable sigue ahi').toBe(true)
      // y se puede seguir: otra cancion, otro repertorio
      await abrirOpciones(w, 0)
      await elegir(w, 'Añadir a una lista')
      await elegir(w, 'Domingo')
      expect(api.addToPlaylist).toHaveBeenLastCalledWith(7, [1])
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('el «⋯» de la fila abre las mismas opciones, sin elegirla ni cerrar nada', async () => {
      const w = await preparar()
      api.song.mockClear()
      await w.findAll('.sr-more')[1].trigger('click')
      await flushPromises()
      expect(w.find('.ctx').exists()).toBe(true)
      expect(w.find('.ctx-head').text()).toContain('Shekinah')
      expect(w.find('.search-results').exists()).toBe(true)
      // no es la lista: la ficha de al lado no cambia a esa cancion
      expect(api.song).not.toHaveBeenCalled()
      // y lo suyo (no lo de una fila de la lista) esta en el menu
      const textos = w.findAll('.ctx-item').map((b) => b.text())
      expect(textos.some((t) => t.includes('Añadir a una lista'))).toBe(true)
      expect(textos.some((t) => t.includes('Quitar de esta lista'))).toBe(false)
    })

    it('Reproducir desde el menu pone lo encontrado como cola, como el boton de la fila', async () => {
      const w = await preparar()
      await abrirOpciones(w, 1)
      await elegir(w, 'Reproducir')
      await flushPromises()
      const [items, start, origin] = playback.bridge.setQueue.mock.calls.at(-1)
      expect(items.map((t) => t.id)).toEqual([1, 2, 3])
      expect(start).toBe(2)
      expect(origin.label).toContain('barak')
    })

    it('marcar una favorita desde ahi se nota en su propio menu la proxima vez', async () => {
      const w = await preparar()
      await abrirOpciones(w, 0)
      await elegir(w, 'Marcar como favorito')
      expect(api.toggleFavorite).toHaveBeenCalledWith(1, true)
      await abrirOpciones(w, 0)
      const textos = w.findAll('.ctx-item').map((b) => b.text())
      expect(textos.some((t) => t.includes('Quitar de favoritos'))).toBe(true)
    })

    it('Escape cierra el menu y no el desplegable; con el menu cerrado, el desplegable', async () => {
      const w = await preparar()
      await abrirOpciones(w, 0)
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

    it('un clic en cualquier otro sitio lo cierra todo', async () => {
      const w = await preparar()
      await abrirOpciones(w, 0)
      document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
      await flushPromises()
      expect(w.find('.ctx').exists()).toBe(false)
      expect(w.find('.search-results').exists()).toBe(false)
    })

    it('con el teclado: la tecla de menu en la caja abre las opciones del resaltado', async () => {
      const w = await preparar()
      const caja = w.find('.topbar input')
      await caja.trigger('keydown', { key: 'ArrowDown' })
      await caja.trigger('keydown', { key: 'ContextMenu' })
      await flushPromises()
      expect(w.find('.ctx').exists()).toBe(true)
      expect(w.find('.ctx-head').text()).toContain('Shekinah')
    })

    it('sobre la que suena el menu pausa: no la reinicia ni cambia la cola', async () => {
      const w = await preparar()
      await w.findAll('.sr-play')[1].trigger('click') // suena «Shekinah»
      await flushPromises()
      await flushPromises()
      // reproducir cierra el desplegable: se busca otra vez
      await buscar(w, 'barak otra vez')
      playback.bridge.setQueue.mockClear()
      playback.bridge.toggle.mockClear()
      await abrirOpciones(w, 1)
      const textos = w.findAll('.ctx-item').map((b) => b.text())
      expect(textos.some((t) => t.startsWith('Pausar'))).toBe(true)
      await elegir(w, 'Pausar')
      expect(playback.bridge.toggle).toHaveBeenCalledTimes(1)
      expect(
        playback.bridge.setQueue,
        'pausar no es poner la cancion otra vez'
      ).not.toHaveBeenCalled()
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('marcar una favorita no mueve la fila resaltada (el teclado sigue donde estaba)', async () => {
      const w = await preparar()
      const caja = w.find('.topbar input')
      await caja.trigger('keydown', { key: 'ArrowDown' })
      await caja.trigger('keydown', { key: 'ArrowDown' })
      expect(w.find('.sr-row.on').text()).toContain('Cancion 3')
      await caja.trigger('keydown', { key: 'ContextMenu' })
      await flushPromises()
      await elegir(w, 'Marcar como favorito')
      expect(api.toggleFavorite).toHaveBeenCalledWith(3, true)
      // se parchea en sitio: con un array nuevo se volvia a resaltar la primera
      expect(w.find('.sr-row.on').text()).toContain('Cancion 3')
    })

    it('un dialogo abierto desde el menu no cierra el desplegable (Nueva lista… → Crear)', async () => {
      const w = await preparar()
      await abrirOpciones(w, 0)
      await elegir(w, 'Añadir a una lista')
      await elegir(w, 'Nueva lista')
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
      await abrirOpciones(w, 0)
      await elegir(w, 'Añadir a una lista')
      await elegir(w, 'Nueva lista')
      dialogOk('Mi lista')
      await flushPromises()
      await flushPromises()
      expect(api.addToPlaylist).toHaveBeenCalledWith(expect.any(Number), [1])
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('elegir una opcion con el raton devuelve el foco a la caja', async () => {
      // sin esto el foco se perdia en el <body>: lo escrito no llegaba a la caja y
      // la flecha abajo bajaba el volumen del reproductor
      const w = await preparar()
      await abrirOpciones(w, 0)
      await elegir(w, 'Marcar como favorito')
      await flushPromises()
      expect(document.activeElement).toBe(w.find('.topbar input').element)
      // pero si la opcion abre un dialogo, el foco es del dialogo
      await abrirOpciones(w, 0)
      await elegir(w, 'Añadir a una lista')
      await elegir(w, 'Nueva lista')
      await flushPromises()
      expect(document.activeElement).not.toBe(w.find('.topbar input').element)
      dialogCancel()
      await flushPromises()
    })

    it('mandar a la papelera quita la fila del desplegable (no deja una cancion fantasma)', async () => {
      const w = await preparar()
      expect(w.findAll('.sr-row')).toHaveLength(3)
      await abrirOpciones(w, 0)
      await elegir(w, 'Mandar a la papelera')
      expect(useDialog().dialog.value.open).toBe(true)
      dialogOk()
      await flushPromises()
      await flushPromises()
      expect(api.deleteSong).toHaveBeenCalledWith(1)
      expect(w.findAll('.sr-row')).toHaveLength(2)
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('«Ver detalles» de un resultado elige la cancion en el panel, sin abrir una ventana', async () => {
      const w = await preparar()
      api.song.mockClear()
      await abrirOpciones(w, 1)
      await elegir(w, 'Ver detalles')
      await flushPromises()
      expect(api.song).toHaveBeenCalledWith(2)
      expect(w.find('.search-results').exists()).toBe(true)
    })

    it('el doble clic en el «⋯» no reproduce ni cambia la cola', async () => {
      const w = await preparar()
      playback.bridge.setQueue.mockClear()
      await w.findAll('.sr-more')[0].trigger('dblclick')
      expect(playback.bridge.setQueue).not.toHaveBeenCalled()
    })
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
