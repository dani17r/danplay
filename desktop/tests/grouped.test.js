// La lista agrupada es UNA sola lista, con la cabecera de cada grupo entre
// sus filas. Antes era una lista por grupo y, con grupos de menos de 80
// canciones («Artistas», casi siempre), se pintaba la biblioteca entera; las
// flechas además se paraban al final de cada grupo.
import { describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import GroupedSongs from '../src/components/GroupedSongs.vue'
import { song } from './support/backend.js'

const tecla = (el, key, extra = {}) => el.trigger('keydown', { key, ...extra })
const filas = (w) => w.findAll('[data-song-row]')
/** El título de una fila, en cualquiera de las cuatro vistas. */
const titulo = (f) => f.find('td.title, .row-title, .card-title, .name').text()

/** Dos de Barak, una de Miel San Marcos y dos de New Wine, llegadas por título. */
const cinco = () => [
  song(1, { title: 'A Una Voz', artist: 'New Wine', duration: 200 }),
  song(2, { title: 'Mi Gozo', artist: 'Barak', duration: 240 }),
  song(3, { title: 'Que Se Abra El Cielo', artist: 'Miel San Marcos', duration: 180 }),
  song(4, { title: 'Sera Llena La Tierra', artist: 'Barak', duration: 60 }),
  song(5, { title: 'Shekinah', artist: 'New Wine', duration: 300 })
]

describe('una sola lista con cabeceras', () => {
  for (const layout of ['table', 'rows', 'cards', 'grid']) {
    it(`${layout}: las cabeceras van entre las filas, en el orden de los grupos`, () => {
      const w = mount(GroupedSongs, { props: { songs: cinco(), by: 'artist', layout } })
      // una lista, no una por grupo
      expect(w.findAll('[role="grid"], [role="listbox"]')).toHaveLength(1)
      const orden = w
        .findAll('[data-group-row], [data-song-row]')
        .map((e) =>
          e.attributes('data-group-row') !== undefined
            ? '# ' + e.find('.group-name').text()
            : titulo(e)
        )
      expect(orden).toEqual([
        '# Barak',
        'Mi Gozo',
        'Sera Llena La Tierra',
        '# Miel San Marcos',
        'Que Se Abra El Cielo',
        '# New Wine',
        'A Una Voz',
        'Shekinah'
      ])
    })
  }

  it('las flechas pasan de un grupo al siguiente, y vuelven', async () => {
    const w = mount(GroupedSongs, {
      props: { songs: cinco(), by: 'artist', layout: 'table' },
      attachTo: document.body
    })
    await tecla(filas(w)[1], 'ArrowDown') // la última de Barak
    expect(w.emitted('select').at(-1)).toEqual([3, { shiftKey: false }])
    await tecla(filas(w)[2], 'ArrowUp')
    expect(w.emitted('select').at(-1)).toEqual([4, { shiftKey: false }])
    await tecla(filas(w)[0], 'End')
    expect(w.emitted('select').at(-1)).toEqual([5, { shiftKey: false }])
  })

  it('una sola parada del tabulador para toda la lista', () => {
    const w = mount(GroupedSongs, { props: { songs: cinco(), by: 'artist', layout: 'rows' } })
    expect(filas(w).filter((f) => f.attributes('tabindex') === '0')).toHaveLength(1)
  })

  it('el número de cada fila es el de dentro de su grupo', () => {
    const w = mount(GroupedSongs, { props: { songs: cinco(), by: 'artist', layout: 'table' } })
    expect(filas(w).map((f) => f.find('.row-n').text())).toEqual(['1', '2', '1', '1', '2'])
  })

  it('plegar un grupo quita sus filas y deja su cabecera', async () => {
    const w = mount(GroupedSongs, { props: { songs: cinco(), by: 'artist', layout: 'grid' } })
    const barak = w.findAll('.group-head')[0]
    await barak.trigger('click')
    expect(barak.attributes('aria-expanded')).toBe('false')
    expect(w.findAll('.group-head')).toHaveLength(3)
    expect(filas(w).map(titulo)).toEqual(['Que Se Abra El Cielo', 'A Una Voz', 'Shekinah'])
    // todo plegado: quedan las cabeceras, no «Nada por aquí»
    for (const h of w.findAll('.group-head').slice(1)) await h.trigger('click')
    expect(filas(w)).toHaveLength(0)
    expect(w.findAll('.group-head')).toHaveLength(3)
    expect(w.text()).not.toContain('Nada por aquí')
  })

  it('la tabla cuenta las cabeceras como filas', () => {
    const w = mount(GroupedSongs, { props: { songs: cinco(), by: 'artist', layout: 'table' } })
    const tabla = w.find('[role="grid"]')
    expect(tabla.attributes('aria-rowcount')).toBe('8') // 5 canciones y 3 cabeceras
    const indices = w
      .findAll('tbody tr')
      .map((tr) => tr.attributes('aria-rowindex'))
      .map(Number)
    expect(indices).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
  })

  it('en las fichas, cada grupo es un grupo con nombre', () => {
    const w = mount(GroupedSongs, { props: { songs: cinco(), by: 'artist', layout: 'cards' } })
    const grupos = w.findAll('[role="group"]')
    expect(grupos).toHaveLength(3)
    for (const g of grupos) {
      const id = g.attributes('aria-labelledby')
      expect(id).toBeTruthy()
      expect(g.find(`#${CSS.escape(id)}`).classes()).toContain('group-head')
      expect(g.findAll('[role="option"]').length).toBeGreaterThan(0)
    }
  })
})

// ---------------------------------------------------------------------------
// Pintar solo lo que se ve, con grupos. jsdom no maqueta: se simula que cada
// fila mide 30 px, cada cabecera 40 y el panel 600.
const FILA = 30
const CABECERA = 40
const PANEL = 600
let originales = null

function conMaquetacion() {
  originales = {
    offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
    offsetTop: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop'),
    clientHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight'),
    overflow: window.getComputedStyle
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get() {
      const propio = parseInt(this.style?.height || '', 10)
      if (propio > 0) return propio
      if (this.tagName !== 'TR') return 0
      return this.hasAttribute('data-group-row') ? CABECERA : FILA
    }
  })
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
  window.getComputedStyle = (el) => ({
    overflowY: el.classList?.contains('table-wrap') ? 'auto' : 'visible'
  })
}

function sinMaquetacion() {
  if (!originales) return
  for (const prop of ['offsetHeight', 'offsetTop', 'clientHeight']) {
    if (originales[prop]) Object.defineProperty(HTMLElement.prototype, prop, originales[prop])
    else delete HTMLElement.prototype[prop]
  }
  window.getComputedStyle = originales.overflow
  originales = null
}

/** 1000 canciones de 500 artistas: grupos de dos, como «Artistas». */
const biblioteca = () =>
  Array.from({ length: 1000 }, (_, i) =>
    song(i + 1, {
      title: 'Cancion ' + i,
      artist: 'Artista ' + String(Math.floor(i / 2)).padStart(3, '0')
    })
  )

describe('agrupada y larga: solo se pinta lo que se ve', () => {
  afterEach(sinMaquetacion)

  it('con 500 grupos pequeños pinta una ventana, no la biblioteca', async () => {
    conMaquetacion()
    const w = mount(GroupedSongs, {
      props: { songs: biblioteca(), by: 'artist', layout: 'table' },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    const pintadas = filas(w).length
    expect(pintadas, 'no pinta nada').toBeGreaterThan(10)
    expect(pintadas, 'pinta la biblioteca entera').toBeLessThan(200)
    // lo pintado más los huecos tiene que medir la lista entera
    const alto = (tr) => parseInt(tr.attributes('style')?.match(/height:\s*(\d+)px/)?.[1] || '0')
    const huecos = w.findAll('tbody tr[data-spacer]').reduce((t, tr) => t + alto(tr), 0)
    const cabeceras = w.findAll('tbody tr[data-group-row]').length
    expect(huecos + pintadas * FILA + cabeceras * CABECERA).toBe(1000 * FILA + 500 * CABECERA)
    w.unmount()
  })

  it('«ir a lo que suena» trae un grupo lejano, con su cabecera', async () => {
    conMaquetacion()
    const w = mount(GroupedSongs, {
      props: { songs: biblioteca(), by: 'artist', layout: 'table', jumpTo: null },
      attachTo: document.body
    })
    await flushPromises()
    await nextTick()
    expect(w.text()).not.toContain('Cancion 901')
    await w.setProps({ jumpTo: 902 }) // la 901, del «Artista 450»
    await flushPromises()
    await nextTick()
    expect(w.text()).toContain('Cancion 901')
    const cabeceras = w.findAll('.group-name').map((n) => n.text())
    expect(cabeceras).toContain('Artista 450')
    w.unmount()
  })
})
