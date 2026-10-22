// Pintar solo lo que se ve.
//
// jsdom no maqueta: todo mide cero. Para poder probar esto de verdad se
// simulan las alturas (una fila 30 px, el panel 600 px) y se comprueba que
// se pinta una ventana, que los separadores rellenan el resto, y —lo mas
// importante— que si NO se puede medir se pinta la lista entera.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import SongTable from '../src/components/SongTable.vue'

const ROW_H = 30
const PANEL = 600

const song = (i) => ({
  id: i,
  title: 'Cancion ' + i,
  artist: 'Artista ' + (i % 50),
  album: 'Album',
  duration: 210,
  bitrate: 320000,
  stars: 0,
  favorite: 0,
  feat: '',
  folder: 'x',
  key: 'C',
  bpm: 120,
  file: i + '.mp3'
})
const list = (n) => Array.from({ length: n }, (_, i) => song(i))

let originals = null

/** Simula que el navegador ha maquetado: filas de 30 px dentro de un panel. */
function withLayout() {
  originals = {
    offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
    offsetTop: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop'),
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    overflow: window.getComputedStyle
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      // los separadores llevan su alto puesto a mano
      const own = parseInt(this.style?.height || '', 10)
      if (own > 0) return own
      return this.tagName === 'TR' ? ROW_H : 0
    }
  })
  // cada fila empieza donde acaba la anterior: es lo que mira el composable
  // para saber cuantas caben por linea y cuanto baja de una a otra
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get() {
      let t = 0
      for (let el = this.previousElementSibling; el; el = el.previousElementSibling) {
        t += el.offsetHeight
      }
      return t
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get() {
      return this.classList?.contains('table-wrap') ? PANEL : 0
    }
  })
  // el panel con scroll es .table-wrap
  window.getComputedStyle = (el) => ({
    overflowY: el.classList?.contains('table-wrap') ? 'auto' : 'visible'
  })
}

function withoutLayout() {
  if (!originals) return
  // jsdom no siempre define estas en HTMLElement.prototype: si no habia nada
  // que guardar, se quita lo que pusimos en vez de restaurar «undefined».
  for (const prop of ['offsetHeight', 'offsetTop', 'clientHeight']) {
    if (originals[prop]) Object.defineProperty(HTMLElement.prototype, prop, originals[prop])
    else delete HTMLElement.prototype[prop]
  }
  window.getComputedStyle = originals.overflow
  originals = null
}

const rows = (w) => w.findAll('tbody tr:not([data-spacer])')
const spacers = (w) => w.findAll('tbody tr[data-spacer]')
const height = (tr) => parseInt(tr.attributes('style')?.match(/height:\s*(\d+)px/)?.[1] || '0', 10)

describe('long list: only what is visible is rendered', () => {
  beforeEach(withLayout)
  afterEach(withoutLayout)

  it('with a thousand songs it renders a window, not all thousand', async () => {
    const songs = list(1000)
    const w = mount(SongTable, {
      props: { songs, selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    const n = rows(w).length
    expect(n).toBeGreaterThan(10)
    expect(n).toBeLessThan(200) // ni de lejos las mil
    w.unmount()
  })

  it('the spacers fill exactly what is not rendered', async () => {
    const songs = list(1000)
    const w = mount(SongTable, {
      props: { songs, selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    const rendered = rows(w).length
    const gap = spacers(w).reduce((t, tr) => t + height(tr), 0)
    // lo pintado mas los huecos tiene que sumar la lista entera
    expect(gap + rendered * ROW_H).toBe(songs.length * ROW_H)
    w.unmount()
  })

  it('starts at the top of the list', async () => {
    const w = mount(SongTable, {
      props: { songs: list(1000), selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    expect(w.text()).toContain('Cancion 0')
    w.unmount()
  })

  it('row numbering is the real one, not the window one', async () => {
    const w = mount(SongTable, {
      props: { songs: list(1000), selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    expect(rows(w)[0].find('.row-n').text()).toBe('1')
    w.unmount()
  })

  it('«ir a lo que suena» brings into the window a row that was not rendered', async () => {
    const songs = list(1000)
    const w = mount(SongTable, {
      props: { songs, selected: null, playing: null, jumpTo: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    expect(w.text()).not.toContain('Cancion 900') // aun no esta pintada
    await w.setProps({ jumpTo: 900 })
    await flushPromises()
    await nextTick()
    expect(w.text()).toContain('Cancion 900')
    w.unmount()
  })

  it('a list that arrives after mounting follows the scroll', async () => {
    // Sin canciones no hay tabla, ni ancla desde la que buscar el panel. Antes
    // solo se escuchaba al panel encontrado al montar: una lista que llegaba
    // después (el núcleo aún arrancando) medía bien, pero al bajar se quedaba
    // en blanco. Lo pilló Playwright en la CI, donde todo va más lento.
    const w = mount(SongTable, {
      props: { songs: [], selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await w.setProps({ songs: list(1000) })
    await flushPromises()
    await nextTick()
    expect(w.text()).not.toContain('Cancion 500')

    const panel = w.find('.table-wrap').element
    Object.defineProperty(panel, 'scrollTop', { configurable: true, value: 500 * ROW_H })
    // jsdom no mueve nada al desplazarse: las filas suben lo que baja el panel
    w.find('tbody').element.getBoundingClientRect = () =>
      /** @type {DOMRect} */ ({ top: -panel.scrollTop, height: 0 })
    panel.dispatchEvent(new Event('scroll'))
    await new Promise((r) => requestAnimationFrame(r))
    await nextTick()
    expect(w.text()).toContain('Cancion 500')
    w.unmount()
  })

  it('a short list renders whole and without spacers', async () => {
    const w = mount(SongTable, {
      props: { songs: list(20), selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    expect(rows(w).length).toBe(20)
    expect(spacers(w).length).toBe(0)
    w.unmount()
  })
})

describe('if it cannot be measured, everything is rendered', () => {
  it('without layout (or if the panel has no height yet) nothing is windowed', async () => {
    // sin conMaquetacion(): todo mide cero, como en un primer pintado
    const w = mount(SongTable, {
      props: { songs: list(300), selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    expect(rows(w).length).toBe(300)
    expect(w.text()).toContain('Cancion 299')
    w.unmount()
  })
})

// ---------------------------------------------------------------------------
// La cuadricula es el mismo mecanismo, pero con varias fichas por linea. No
// se calcula del css: se mira cuantas de las pintadas empiezan a la misma
// altura. Aqui se simula una rejilla de 4 columnas y fichas de 200 px.
import SongGrid from '../src/components/SongGrid.vue'

const COLS = 4
const TILE_H = 200

function withGrid() {
  originals = {
    offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
    offsetTop: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop'),
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    overflow: window.getComputedStyle
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      const own = parseInt(this.style?.height || '', 10)
      if (own > 0) return own
      return this.classList?.contains('tile') ? TILE_H : 0
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get() {
      // altura de los separadores que haya antes, mas la linea que le toca
      let gap = 0
      let index = 0
      for (let el = this.previousElementSibling; el; el = el.previousElementSibling) {
        if (el.dataset && el.dataset.spacer !== undefined) gap += el.offsetHeight
        else index++
      }
      return gap + Math.floor(index / COLS) * TILE_H
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get() {
      return this.classList?.contains('grid') ? PANEL : 0
    }
  })
  window.getComputedStyle = (el) => ({
    overflowY: el.classList?.contains('grid') ? 'auto' : 'visible'
  })
}

describe('grid: several cards per line', () => {
  beforeEach(withGrid)
  afterEach(withoutLayout)

  it('with a thousand cards it renders only a few lines', async () => {
    const w = mount(SongGrid, {
      props: { songs: list(1000), selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    const cards = w.findAll('.tile').length
    expect(cards).toBeGreaterThan(COLS) // al menos una linea
    expect(cards).toBeLessThan(200) // ni de lejos las mil
    expect(cards % COLS).toBe(0) // lineas enteras
    w.unmount()
  })

  it('the spacers keep the total height correct', async () => {
    const songs = list(1000)
    const w = mount(SongGrid, {
      props: { songs, selected: null, playing: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    const rendered = w.findAll('.tile').length
    const gap = w.findAll('.grid > [data-spacer]').reduce((t, d) => t + height(d), 0)
    const totalLines = Math.ceil(songs.length / COLS)
    expect(gap + (rendered / COLS) * TILE_H).toBe(totalLines * TILE_H)
    w.unmount()
  })

  it('«ir a lo que suena» brings in a far card', async () => {
    const w = mount(SongGrid, {
      props: { songs: list(1000), selected: null, playing: null, jumpTo: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    expect(w.text()).not.toContain('Cancion 800')
    await w.setProps({ jumpTo: 800 })
    await flushPromises()
    await nextTick()
    expect(w.text()).toContain('Cancion 800')
    w.unmount()
  })
})

// ---------------------------------------------------------------------------
// Las cuatro vistas comparten el virtualizado: montadas con mil canciones en
// un panel de 600 px, ninguna pinta las mil.
import SongRows from '../src/components/SongRows.vue'
import SongCards from '../src/components/SongCards.vue'

/** Cada fila mide 30 px y va debajo de la anterior; el panel que se desplaza es `caja`. */
function withPanel(box) {
  originals = {
    offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
    offsetTop: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop'),
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    overflow: window.getComputedStyle
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      const own = parseInt(this.style?.height || '', 10)
      if (own > 0) return own
      return this.hasAttribute?.('data-song-row') ? ROW_H : 0
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get() {
      let t = 0
      for (let el = this.previousElementSibling; el; el = el.previousElementSibling)
        t += el.offsetHeight
      return t
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get() {
      return this.classList?.contains(box) ? PANEL : 0
    }
  })
  window.getComputedStyle = (el) => ({
    overflowY: el.classList?.contains(box) ? 'auto' : 'visible'
  })
}

describe('the four views render only what is visible', () => {
  afterEach(withoutLayout)
  for (const [name, View, box] of [
    ['the table', SongTable, 'table-wrap'],
    ['the compact list', SongRows, 'rows'],
    ['the cards', SongCards, 'cards'],
    ['the grid', SongGrid, 'grid']
  ]) {
    it(name, async () => {
      withPanel(box)
      const w = mount(View, { props: { songs: list(1000) }, attachTo: document.body })
      await flushPromises()
      await nextTick()
      const rendered = w.findAll('[data-song-row]').length
      expect(rendered, 'no pinta nada').toBeGreaterThan(10)
      expect(rendered, 'pinta la lista entera').toBeLessThan(200)
      w.unmount()
    })
  }
})
