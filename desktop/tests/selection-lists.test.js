// Las casillas del modo «Seleccionar» en las cuatro vistas de una lista
// (tabla, lista fina, fichas y cuadrícula) y en la agrupada.
//
// Lo que se promete: con `selectMode` cada fila lleva su casilla DENTRO de la
// propia fila (el virtualizado solo pinta las que se ven); es decorativa
// (`aria-hidden`), la verdad de cada fila sigue siendo su `aria-selected`; un
// clic en ella es un clic en la fila (`select(id, $event)`, la app decide si
// alterna). Fuera del modo no queda rastro: ni casillas ni columna. La tabla
// lleva además en la cabecera la casilla de «todas», de tres estados.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import SongTable from '../src/components/SongTable.vue'
import SongRows from '../src/components/SongRows.vue'
import SongCards from '../src/components/SongCards.vue'
import SongGrid from '../src/components/SongGrid.vue'
import GroupedSongs from '../src/components/GroupedSongs.vue'
import { allCss } from './support/css.js'
import { song } from './support/backend.js'

const CSS = allCss()
const STYLES = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'styles')

afterEach(() => {
  vi.restoreAllMocks()
  localStorage.clear()
})

const VIEWS = { table: SongTable, rows: SongRows, cards: SongCards, grid: SongGrid }
const three = () => [
  song(1, { title: 'A Una Voz' }),
  song(2, { title: 'Mi Gozo', favorite: 1 }),
  song(3, { title: 'Shekinah' })
]
const rows = (w) => w.findAll('[data-song-row]')
/** La casilla de una fila (la de la cabecera de la tabla no es de ninguna). */
const checkbox = (row) => row.find('.pick-box')

describe('las casillas en las cuatro vistas', () => {
  for (const [name, View] of Object.entries(VIEWS)) {
    describe(name, () => {
      it('fuera del modo no hay ninguna casilla', () => {
        const w = mount(View, { props: { songs: three(), selectedIds: [1, 2] } })
        expect(w.findAll('.pick-box')).toHaveLength(0)
        expect(w.findAll('.col-check')).toHaveLength(0)
      })

      it('en el modo, una por fila, dentro de la fila y decorativa', () => {
        const w = mount(View, { props: { songs: three(), selectMode: true } })
        for (const f of rows(w)) {
          expect(f.findAll('.pick-box'), 'una casilla por fila').toHaveLength(1)
          expect(checkbox(f).attributes('aria-hidden')).toBe('true')
          // ni foco propio ni papel propio: la fila es la que se elige
          expect(checkbox(f).attributes('tabindex')).toBeUndefined()
          expect(checkbox(f).attributes('role')).toBeUndefined()
        }
      })

      it('marcada la de las elegidas, como su aria-selected', async () => {
        const w = mount(View, {
          props: { songs: three(), selectMode: true, selected: 3, selectedIds: [1, 3] }
        })
        const rowStates = () =>
          rows(w).map((f) => [
            f.attributes('aria-selected'),
            checkbox(f).classes().includes('on'),
            checkbox(f).find('svg').exists()
          ])
        expect(rowStates()).toEqual([
          ['true', true, true],
          ['false', false, false],
          ['true', true, true]
        ])
        // cambia la selección: cambian con ella
        await w.setProps({ selected: 2, selectedIds: [2] })
        expect(rowStates().map(([sel, on]) => [sel, on])).toEqual([
          ['false', false],
          ['true', true],
          ['false', false]
        ])
      })

      it('un clic en la casilla es un clic en la fila: select(id, el evento)', async () => {
        const w = mount(View, { props: { songs: three(), selectMode: true } })
        await checkbox(rows(w)[1]).trigger('click', { shiftKey: true })
        const [id, ev] = w.emitted('select').at(-1)
        expect(id).toBe(2)
        // el evento entero, con sus teclas: Mayús sigue haciendo tramo
        expect(ev).toBeInstanceOf(MouseEvent)
        expect(ev.shiftKey).toBe(true)
        await rows(w)[2].trigger('click')
        expect(w.emitted('select').at(-1)[0]).toBe(3)
      })

      it('el doble clic en la fila sigue poniéndola; en la casilla, no', async () => {
        const w = mount(View, { props: { songs: three(), selectMode: true } })
        // marcar y desmarcar deprisa no tiene que ponerla a sonar
        await checkbox(rows(w)[0]).trigger('dblclick')
        expect(w.emitted('play')).toBeUndefined()
        await rows(w)[0].trigger('dblclick')
        expect(w.emitted('play')).toEqual([[three()[0]]])
      })

      it('el botón de reproducir de la fila sigue a lo suyo, sin elegir', async () => {
        const w = mount(View, { props: { songs: three(), selectMode: true } })
        await rows(w)[2].find('.row-play, .row-go, .card-play, .tile-play').trigger('click')
        expect(w.emitted('play')).toEqual([[three()[2]]])
        expect(w.emitted('select')).toBeUndefined()
      })

      it('declara los avisos de la selección (Ctrl+A, Supr y Escape)', () => {
        const w = mount(View, { props: { songs: three(), selectMode: true } })
        // declarados, Vue no los deja caer al elemento raíz como atributos
        for (const ev of ['selectAll', 'remove', 'clearSelection']) {
          expect(View.emits, `${name} no declara ${ev}`).toContain(ev)
        }
        expect(w.props('selectMode')).toBe(true)
      })
    })
  }

  it('el corazón y las estrellas tampoco eligen la fila', async () => {
    for (const View of [SongTable, SongRows, SongCards]) {
      const w = mount(View, { props: { songs: three(), selectMode: true } })
      await rows(w)[1].find('.heart').trigger('click')
      expect(w.emitted('toggleFavorite')).toHaveLength(1)
      await rows(w)[1].findAll('.stars .ico').at(0).trigger('click')
      expect(w.emitted('setStars')).toHaveLength(1)
      expect(w.emitted('select')).toBeUndefined()
    }
  })
})

describe('la tabla: la columna de las casillas', () => {
  const columns = (w) => w.findAll('col').map((c) => c.classes()[0])

  it('solo existe en el modo, y es la primera en todas partes', async () => {
    const w = mount(SongTable, { props: { songs: three() } })
    const withoutCheck = columns(w)
    expect(withoutCheck).not.toContain('col-check')
    await w.setProps({ selectMode: true })
    // en el <colgroup>, en la cabecera y en cada fila: los tres beben de `cols`
    expect(columns(w)).toEqual(['col-check', ...withoutCheck])
    expect(w.find('thead th').attributes('data-col')).toBe('check')
    for (const f of rows(w)) {
      expect(f.find('td').classes()).toContain('col-check')
      expect(f.findAll('td')).toHaveLength(columns(w).length)
    }
    expect(w.findAll('thead th')).toHaveLength(columns(w).length)
    // al salir del modo se va entera: WebKitGTK guarda el ancho de una <col>
    // escondida, así que no basta con ocultarla
    await w.setProps({ selectMode: false })
    expect(columns(w)).toEqual(withoutCheck)
    expect(w.findAll('.col-check')).toHaveLength(0)
  })

  it('no se ensancha: su cabecera no lleva tirador', () => {
    const w = mount(SongTable, { props: { songs: three(), selectMode: true } })
    expect(w.find('th[data-col="check"] .col-resize').exists()).toBe(false)
    expect(w.find('th[data-col="n"] .col-resize').exists()).toBe(true)
  })

  it('la casilla de «todas»: un checkbox de tres estados que sigue a la selección', async () => {
    const w = mount(SongTable, { props: { songs: three(), selectMode: true } })
    const allBox = () => w.find('th[data-col="check"] [role="checkbox"]')
    expect(allBox().element.tagName).toBe('BUTTON')
    expect(allBox().attributes('aria-label')).toBe('Seleccionar todas')
    expect(allBox().attributes('aria-checked')).toBe('false')
    await w.setProps({ selectedIds: [1, 2] })
    expect(allBox().attributes('aria-checked')).toBe('mixed')
    expect(allBox().find('.pick-box').classes()).toContain('mixed')
    // la principal (la de la ficha) cuenta, igual que en su fila
    await w.setProps({ selected: 3 })
    expect(allBox().attributes('aria-checked')).toBe('true')
    expect(allBox().find('.pick-box').classes()).toContain('on')
    expect(allBox().find('svg').exists()).toBe(true)
    await w.setProps({ selected: null, selectedIds: [] })
    expect(allBox().attributes('aria-checked')).toBe('false')
  })

  it('pulsarla avisa a la app con selectAll, que alterna; no elige ninguna fila ni ordena', async () => {
    const w = mount(SongTable, { props: { songs: three(), selectMode: true } })
    await w.find('.pick-all').trigger('click')
    expect(w.emitted('selectAll')).toEqual([[{ toggle: true }]])
    expect(w.emitted('select')).toBeUndefined()
    expect(w.emitted('sortBy')).toBeUndefined()
  })

  it('agrupada no hay cabecera, pero sí la columna, y el grupo la abarca', () => {
    const w = mount(SongTable, {
      props: {
        songs: three(),
        selectMode: true,
        noHeader: true,
        groups: [{ key: 'Barak', start: 0, size: 3, count: 3, seconds: 600, open: true }]
      }
    })
    expect(w.find('thead').exists()).toBe(false)
    expect(w.find('[role="checkbox"]').exists()).toBe(false)
    expect(w.find('col').classes()).toContain('col-check')
    expect(w.find('.group-cell').attributes('colspan')).toBe(String(w.findAll('col').length))
    expect(rows(w).every((f) => f.find('td.col-check .pick-box').exists())).toBe(true)
  })

  it('la celda del grupo abarca solo las columnas que se ven (sin columnas fantasma)', async () => {
    // jsdom no aplica las media queries: se dice a mano cuáles esconden
    const hidden = new Set(['col-fav', 'col-album', 'col-stars', 'col-key', 'col-bpm', 'col-kbps'])
    const real = window.getComputedStyle
    vi.spyOn(window, 'getComputedStyle').mockImplementation((el, p) => {
      const s = real.call(window, el, p)
      if (el.tagName !== 'COL' || !hidden.has(el.className)) return s
      return new Proxy(s, {
        get: (t, k) => (k === 'display' ? 'none' : typeof t[k] === 'function' ? t[k].bind(t) : t[k])
      })
    })
    const group = [{ key: 'Barak', start: 0, size: 3, count: 3, seconds: 600, open: true }]
    const w = mount(SongTable, { props: { songs: three(), noHeader: true, groups: group } })
    await nextTick()
    const span = () => Number(w.find('.group-cell').attributes('colspan'))
    // n, título, artista, duración y copiar: cinco de once
    expect(span()).toBe(5)
    // con el modo, una más (la casilla, que no se esconde nunca)
    await w.setProps({ selectMode: true })
    await nextTick()
    expect(span()).toBe(6)
    // la ventana se estrecha: las media queries esconden más, y se recuenta
    for (const c of ['col-artist', 'col-dur', 'col-copy']) hidden.add(c)
    window.dispatchEvent(new Event('resize'))
    await nextTick()
    expect(span()).toBe(3)
    // sin grupos ni se mira (no hay celda que abarque nada)
    const plain = mount(SongTable, { props: { songs: three() } })
    expect(plain.find('.group-cell').exists()).toBe(false)
  })

  it('respeta los anchos guardados, que van por nombre, y el suyo no se guarda', async () => {
    localStorage.setItem('danplay.colWidths', JSON.stringify({ title: 300, artist: 150 }))
    const w = mount(SongTable, { props: { songs: three(), selectMode: true } })
    const width = (cls) => w.find(`col.${cls}`).attributes('style') || ''
    expect(width('col-title')).toContain('width: 300px')
    expect(width('col-artist')).toContain('width: 150px')
    expect(width('col-check')).toBe('')
    // la primera vez que se arrastra se toma la foto de lo que se ve: la
    // casilla, que es fija, se queda fuera
    localStorage.clear()
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
      const w = this.tagName === 'TH' ? 100 : 0
      return { width: w, height: 20, top: 0, left: 0, right: w, bottom: 20, x: 0, y: 0 }
    })
    const other = mount(SongTable, { props: { songs: three(), selectMode: true } })
    await other.find('th[data-col="album"] .col-resize').trigger('keydown', { key: 'ArrowRight' })
    const saved = JSON.parse(localStorage.getItem('danplay.colWidths'))
    expect(saved.album).toBe(110)
    expect(saved.title).toBe(100)
    expect(saved).not.toHaveProperty('check')
  })
})

describe('la agrupada', () => {
  const five = () => [
    song(1, { title: 'A Una Voz', artist: 'New Wine' }),
    song(2, { title: 'Mi Gozo', artist: 'Barak' }),
    song(3, { title: 'Que Se Abra El Cielo', artist: 'Miel San Marcos' }),
    song(4, { title: 'Sera Llena La Tierra', artist: 'Barak' }),
    song(5, { title: 'Shekinah', artist: 'New Wine' })
  ]

  for (const [layout, View] of Object.entries(VIEWS)) {
    it(`${layout}: pasa el modo a la vista y saca sus avisos tal cual`, async () => {
      const w = mount(GroupedSongs, { props: { songs: five(), by: 'artist', layout } })
      expect(w.findAll('.pick-box')).toHaveLength(0)
      await w.setProps({ selectMode: true, selectedIds: [2, 4] })
      expect(w.findComponent(View).props('selectMode')).toBe(true)
      expect(rows(w).every((f) => f.find('.pick-box').exists())).toBe(true)
      expect(rows(w).filter((f) => f.find('.pick-box.on').exists())).toHaveLength(2)
      // la vista avisa; la agrupada lo reenvía sin tocarlo
      const view = w.findComponent(View)
      view.vm.$emit('selectAll', { toggle: false })
      view.vm.$emit('remove')
      view.vm.$emit('clearSelection')
      expect(w.emitted('selectAll')).toEqual([[{ toggle: false }]])
      expect(w.emitted('remove')).toEqual([[]])
      expect(w.emitted('clearSelection')).toEqual([[]])
      // y el clic en una casilla llega a la app como el de una fila
      await checkbox(rows(w)[0]).trigger('click')
      expect(w.emitted('select').at(-1)[0]).toBe(2)
    })
  }

  it('la tabla agrupada, sin la casilla de «todas» (no tiene cabecera)', () => {
    const w = mount(GroupedSongs, {
      props: { songs: five(), by: 'artist', layout: 'table', selectMode: true }
    })
    expect(w.find('thead').exists()).toBe(false)
    expect(w.find('col.col-check').exists()).toBe(true)
  })

  it('con el teclado: Ctrl+A en una fila sale de la agrupada como selectAll', async () => {
    const w = mount(GroupedSongs, {
      props: { songs: five(), by: 'artist', layout: 'rows', selectMode: true },
      attachTo: document.body
    })
    await rows(w)[0].trigger('keydown', { key: 'a', ctrlKey: true })
    expect(w.emitted('selectAll')).toHaveLength(1)
  })
})

describe('el css de las casillas', () => {
  const OWN = readFileSync(join(STYLES, 'selection.css'), 'utf8')

  it('va cargado, tras las fichas', () => {
    const index = readFileSync(join(STYLES, 'index.css'), 'utf8')
    expect(index.indexOf("'./selection.css'")).toBeGreaterThan(index.indexOf("'./list-cards.css'"))
  })

  it('un cuadrado redondeado de 16 px que no se encoge', () => {
    const rule = CSS.match(/\n\.pick-box\{([^}]*)\}/)?.[1] || ''
    expect(rule).toMatch(/width:16px/)
    expect(rule).toMatch(/height:16px/)
    expect(rule).toMatch(/border-radius:4px/)
    expect(rule).toMatch(/flex:0 0 auto/)
    expect(rule).toMatch(/border:1px solid [^;]*var\(--border2\)/)
  })

  it('marcada con el acento; nada de colores fijos, para los temas claros y oscuros', () => {
    expect(CSS).toMatch(/\.pick-box\.on,\.pick-box\.mixed\{[^}]*background:var\(--accent\)/)
    // ni #hex, ni rgb(), ni hsl(): todo sale de las variables del tema
    const noComments = OWN.replace(/\/\*[\s\S]*?\*\//g, '')
    expect(noComments).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i)
  })

  it('en fichas y cuadrícula, en la esquina de la carátula y sin z-index', () => {
    expect(CSS).toMatch(/\.card-art > \.pick-box,\.tile > \.pick-box\{[^}]*position:absolute/)
    expect(CSS).toMatch(/\.card-art > \.pick-box\{[^}]*top:[^}]*left:/)
    expect(CSS).toMatch(/\.tile > \.pick-box\{[^}]*top:[^}]*left:/)
    // con uno se colaba por encima de la cabecera de grupo pegada arriba
    expect(OWN.replace(/\/\*[\s\S]*?\*\//g, '')).not.toMatch(/z-index/)
  })

  it('la columna de la tabla es fija y ningún tramo la esconde', () => {
    expect(CSS).toMatch(/\.col-check\{[^}]*width:34px/)
    // ni en ancho ni en estrecho: es la forma de elegir sin teclado
    expect(CSS).not.toMatch(/\.col-check[^{]*\{[^}]*display:none/)
    // sin relleno a los lados, gane quien gane (en estrecho `td,th` lo pone)
    expect(CSS).toMatch(
      /thead th\.col-check,td\.col-check\{[^}]*padding-left:0[^}]*padding-right:0/
    )
  })

  it('la casilla de «todas» se ve con el foco del teclado', () => {
    expect(CSS).toMatch(/\.pick-all:focus-visible\{[^}]*outline:2px solid var\(--accent\)/)
  })

  it('en la lista fina las estrellas se van en estrecho (le dejan sitio al título)', () => {
    // `.stars{display:inline-flex}` va después: con `.row-stars` a secas
    // ganaba él y no se escondían nunca (medido en WebKitGTK)
    const specificity = (sel) =>
      sel.split(' ').reduce((n, p) => n + (p.match(/\./g) || []).length, 0)
    const hides = CSS.match(/@media \(max-width:\s*820px\)\{([^{]+)\{display:none/)?.[1] || ''
    const shows = CSS.match(/\n(\.stars)\{[^}]*display:inline-flex/)?.[1] || ''
    expect(hides).toMatch(/\.row-stars/)
    expect(specificity(hides)).toBeGreaterThan(specificity(shows))
  })

  it('el movimiento reducido apaga su transición (la regla general)', () => {
    expect(CSS).toMatch(/\n\.pick-box\{[^}]*transition:/)
    const motion = CSS.slice(CSS.lastIndexOf('prefers-reduced-motion'))
    expect(motion).toMatch(/\*,\*::before,\*::after\{[^}]*transition-duration:0\.01ms/)
  })
})
