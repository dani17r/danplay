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

const FILA = 30
const PANEL = 600

const song = (i) => ({
  id: i, title: 'Cancion ' + i, artist: 'Artista ' + (i % 50), album: 'Album',
  duration: 210, bitrate: 320000, stars: 0, favorite: 0, feat: '',
  folder: 'x', key: 'C', bpm: 120, file: i + '.mp3'
})
const lista = (n) => Array.from({ length: n }, (_, i) => song(i))

let originales = null

/** Simula que el navegador ha maquetado: filas de 30 px dentro de un panel. */
function conMaquetacion () {
  originales = {
    offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
    offsetTop: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop'),
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    overflow: window.getComputedStyle
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get () {
      // los separadores llevan su alto puesto a mano
      const propio = parseInt(this.style?.height || '', 10)
      if (propio > 0) return propio
      return this.tagName === 'TR' ? FILA : 0
    }
  })
  // cada fila empieza donde acaba la anterior: es lo que mira el composable
  // para saber cuantas caben por linea y cuanto baja de una a otra
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get () {
      let t = 0
      for (let el = this.previousElementSibling; el; el = el.previousElementSibling) {
        t += el.offsetHeight
      }
      return t
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get () { return this.classList?.contains('table-wrap') ? PANEL : 0 }
  })
  // el panel con scroll es .table-wrap
  window.getComputedStyle = (el) =>
    ({ overflowY: el.classList?.contains('table-wrap') ? 'auto' : 'visible' })
}

function sinMaquetacion () {
  if (!originales) return
  // jsdom no siempre define estas en HTMLElement.prototype: si no habia nada
  // que guardar, se quita lo que pusimos en vez de restaurar «undefined».
  for (const prop of ['offsetHeight', 'offsetTop', 'clientHeight']) {
    if (originales[prop]) Object.defineProperty(HTMLElement.prototype, prop, originales[prop])
    else delete HTMLElement.prototype[prop]
  }
  window.getComputedStyle = originales.overflow
  originales = null
}

const filas = (w) => w.findAll('tbody tr:not([data-spacer])')
const separadores = (w) => w.findAll('tbody tr[data-spacer]')
const alto = (tr) => parseInt(tr.attributes('style')?.match(/height:\s*(\d+)px/)?.[1] || '0', 10)

describe('lista larga: solo se pinta lo que se ve', () => {
  beforeEach(conMaquetacion)
  afterEach(sinMaquetacion)

  it('con mil canciones pinta una ventana, no las mil', async () => {
    const songs = lista(1000)
    const w = mount(SongTable, { props: { songs, selected: null, playing: null },
                                 attachTo: document.body })
    await flushPromises(); await nextTick()
    const n = filas(w).length
    expect(n).toBeGreaterThan(10)
    expect(n).toBeLessThan(200)          // ni de lejos las mil
    w.unmount()
  })

  it('los separadores rellenan justo lo que no se pinta', async () => {
    const songs = lista(1000)
    const w = mount(SongTable, { props: { songs, selected: null, playing: null },
                                 attachTo: document.body })
    await flushPromises(); await nextTick()
    const pintadas = filas(w).length
    const hueco = separadores(w).reduce((t, tr) => t + alto(tr), 0)
    // lo pintado mas los huecos tiene que sumar la lista entera
    expect(hueco + pintadas * FILA).toBe(songs.length * FILA)
    w.unmount()
  })

  it('empieza por el principio de la lista', async () => {
    const w = mount(SongTable, { props: { songs: lista(1000), selected: null, playing: null },
                                 attachTo: document.body })
    await flushPromises(); await nextTick()
    expect(w.text()).toContain('Cancion 0')
    w.unmount()
  })

  it('la numeracion de las filas es la real, no la de la ventana', async () => {
    const w = mount(SongTable, { props: { songs: lista(1000), selected: null, playing: null },
                                 attachTo: document.body })
    await flushPromises(); await nextTick()
    expect(filas(w)[0].find('.row-n').text()).toBe('1')
    w.unmount()
  })

  it('«ir a lo que suena» trae a la ventana una fila que no estaba pintada', async () => {
    const songs = lista(1000)
    const w = mount(SongTable, { props: { songs, selected: null, playing: null, jumpTo: null },
                                 attachTo: document.body })
    await flushPromises(); await nextTick()
    expect(w.text()).not.toContain('Cancion 900')       // aun no esta pintada
    await w.setProps({ jumpTo: 900 })
    await flushPromises(); await nextTick()
    expect(w.text()).toContain('Cancion 900')
    w.unmount()
  })

  it('una lista corta se pinta entera y sin separadores', async () => {
    const w = mount(SongTable, { props: { songs: lista(20), selected: null, playing: null },
                                 attachTo: document.body })
    await flushPromises(); await nextTick()
    expect(filas(w).length).toBe(20)
    expect(separadores(w).length).toBe(0)
    w.unmount()
  })
})

describe('si no se puede medir, se pinta todo', () => {
  it('sin maquetacion (o si el panel aun no tiene alto) no se recorta nada', async () => {
    // sin conMaquetacion(): todo mide cero, como en un primer pintado
    const w = mount(SongTable, { props: { songs: lista(300), selected: null, playing: null },
                                 attachTo: document.body })
    await flushPromises(); await nextTick()
    expect(filas(w).length).toBe(300)
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
const FICHA = 200

function conRejilla () {
  originales = {
    offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
    offsetTop: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop'),
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    overflow: window.getComputedStyle
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get () {
      const propio = parseInt(this.style?.height || '', 10)
      if (propio > 0) return propio
      return this.classList?.contains('tile') ? FICHA : 0
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
    configurable: true,
    get () {
      // altura de los separadores que haya antes, mas la linea que le toca
      let hueco = 0
      let indice = 0
      for (let el = this.previousElementSibling; el; el = el.previousElementSibling) {
        if (el.dataset && el.dataset.spacer !== undefined) hueco += el.offsetHeight
        else indice++
      }
      return hueco + Math.floor(indice / COLS) * FICHA
    }
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get () { return this.classList?.contains('grid') ? PANEL : 0 }
  })
  window.getComputedStyle = (el) =>
    ({ overflowY: el.classList?.contains('grid') ? 'auto' : 'visible' })
}

describe('cuadricula: varias fichas por linea', () => {
  beforeEach(conRejilla)
  afterEach(sinMaquetacion)

  it('con mil fichas pinta solo unas cuantas lineas', async () => {
    const w = mount(SongGrid, { props: { songs: lista(1000), selected: null, playing: null },
                                attachTo: document.body })
    await flushPromises(); await nextTick()
    const fichas = w.findAll('.tile').length
    expect(fichas).toBeGreaterThan(COLS)          // al menos una linea
    expect(fichas).toBeLessThan(200)              // ni de lejos las mil
    expect(fichas % COLS).toBe(0)                 // lineas enteras
    w.unmount()
  })

  it('los separadores dejan el alto total correcto', async () => {
    const songs = lista(1000)
    const w = mount(SongGrid, { props: { songs, selected: null, playing: null },
                                attachTo: document.body })
    await flushPromises(); await nextTick()
    const pintadas = w.findAll('.tile').length
    const hueco = w.findAll('.grid > [data-spacer]')
      .reduce((t, d) => t + alto(d), 0)
    const lineasTotales = Math.ceil(songs.length / COLS)
    expect(hueco + (pintadas / COLS) * FICHA).toBe(lineasTotales * FICHA)
    w.unmount()
  })

  it('«ir a lo que suena» trae una ficha lejana', async () => {
    const w = mount(SongGrid, { props: { songs: lista(1000), selected: null, playing: null, jumpTo: null },
                                attachTo: document.body })
    await flushPromises(); await nextTick()
    expect(w.text()).not.toContain('Cancion 800')
    await w.setProps({ jumpTo: 800 })
    await flushPromises(); await nextTick()
    expect(w.text()).toContain('Cancion 800')
    w.unmount()
  })
})
