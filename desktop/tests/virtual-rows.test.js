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
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    overflow: window.getComputedStyle
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get () { return this.tagName === 'TR' ? FILA : 0 }
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
  for (const prop of ['offsetHeight', 'clientHeight']) {
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
