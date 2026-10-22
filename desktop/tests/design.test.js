// Guardias de diseño.
//
// Casi todos los fallos que cubre este archivo son el mismo: un nombre que
// dejó de existir al pasar el código a inglés y que nadie notó porque en
// JavaScript leer una propiedad que no está no da error, simplemente vale
// `undefined`. El resultado eran bordes invisibles, puntos de color que no
// salían y avisos que nunca se coloreaban.
//
// Muchas de estas pruebas buscaban texto en el código («¿pone `sort:` en la
// tabla?») y por eso se escapó que la vista agrupada perdía el Ctrl del clic:
// el texto estaba y el comportamiento no. Ahora montan los componentes y
// miran lo que hacen. Las que siguen leyendo archivos son las del CSS (sin
// motor de maquetación, la hoja es lo que hay que mirar) y cuatro cruces
// entre archivos que no tienen otra forma de comprobarse (ver al final).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { allCss } from './support/css.js'
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { mount, flushPromises } from '@vue/test-utils'
import { CATALOG, KIND_LABEL } from '../src/themes.js'
import { coreSource } from './support/core.js'
import SelectField from '../src/components/ui/SelectField.vue'
import CopyButton from '../src/components/ui/CopyButton.vue'
import CoverArt from '../src/components/ui/CoverArt.vue'
import SongTable from '../src/components/SongTable.vue'
import SongCards from '../src/components/SongCards.vue'
import SongGrid from '../src/components/SongGrid.vue'
import GroupedSongs from '../src/components/GroupedSongs.vue'
import SearchPanel from '../src/components/SearchPanel.vue'
import SearchResults from '../src/components/SearchResults.vue'
import DuplicateGroup from '../src/components/DuplicateGroup.vue'
import DetailsPanel from '../src/components/DetailsPanel.vue'
import { useDragSong, cancelDrag } from '../src/composables/useDragSong.js'

const SRC = join(dirname(fileURLToPath(import.meta.url)), '..', 'src')
const CSS = allCss()

/** Todos los .vue del proyecto, con su ruta y su contenido. */
function components(dir = SRC, out = []) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    if (e.isDirectory()) components(p, out)
    else if (e.name.endsWith('.vue'))
      out.push({ file: p.slice(SRC.length + 1), code: readFileSync(p, 'utf8') })
  }
  return out
}

const song = (id, extra = {}) => ({
  id,
  title: 'Cancion ' + id,
  artist: 'Barak',
  album: 'Album',
  file: id + '.mp3',
  duration: 200,
  bitrate: 320000,
  stars: 0,
  favorite: 0,
  folder: 'Artistas/Barak',
  key: 'G',
  bpm: 120,
  ...extra
})

afterEach(() => {
  vi.restoreAllMocks()
  localStorage.clear()
  cancelDrag()
})

describe('colores de tema', () => {
  const VALID = new Set(Object.keys(CATALOG.night.v))

  it('el catalogo define las mismas claves en todos los temas', () => {
    for (const [name, t] of Object.entries(CATALOG)) {
      expect(new Set(Object.keys(t.v)), `el tema «${name}» no tiene las mismas claves`).toEqual(
        VALID
      )
    }
  })

  // que ningun componente lea un color que no existe se mira donde se
  // pintan: las tarjetas de Ajustes (settings.test.js), el editor de temas
  // (theme-editor.test.js) y el selector de tema del menu de Vista (abajo)

  it('cada clave de color es tambien una variable del css', () => {
    // applyTheme escribe --<clave>, asi que el css tiene que usarlas
    const missing = [...VALID].filter((k) => !CSS.includes(`var(--${k})`))
    expect(missing, 'claves de tema que el css no usa nunca').toEqual([])
  })

  it('el tipo de tema se enseña traducido, no en ingles', () => {
    for (const t of Object.values(CATALOG)) {
      expect(KIND_LABEL[t.kind], `falta como se dice «${t.kind}»`).toBeTruthy()
    }
  })
})

describe('clases que la interfaz pinta', () => {
  /** ¿El css define esta clase en alguna parte? */
  const defined = (className) => new RegExp(`\\.${className}[\\s,{:.>+~]`).test(CSS)

  it('los avisos flotantes tienen estilo (y los pinta la app: app.test.js)', () => {
    // Antes emitia notice-ok / notice-ambar y el css definia hint-ok /
    // hint-amber: los avisos salian siempre grises.
    for (const c of ['toast', 'hint-ok', 'hint-amber']) {
      expect(defined(c), `el css no define .${c}`).toBe(true)
    }
  })

  it('la flecha de los grupos tiene estilo para girar', () => {
    expect(defined('group-chevron')).toBe(true)
    expect(CSS).toMatch(/\.group-chevron\.open\{[^}]*rotate/)
  })
})

describe('animaciones', () => {
  it('el modo estudio sube por encima de la lista, sin recolocar nada', () => {
    // Antes entraba en el flujo de la columna y le quitaba alto a la lista:
    // al abrirlo y al cerrarlo se recolocaba media pantalla. (Que entra con
    // su transicion se prueba montando la app: app.test.js.)
    expect(CSS).toMatch(/\.study\{[^}]*position:fixed/)
    expect(CSS).toMatch(/\.study\{[^}]*bottom:var\(--player-h\)/)
    // media pantalla, y las tres columnas
    expect(CSS).toMatch(/\.study\{[^}]*height:min\(5\dvh/)
    expect(CSS).toMatch(/\.study-body\{[^}]*grid-template-columns:[^;}]*fr[^;}]*fr[^;}]*fr/)
    expect(CSS).toMatch(/\.study-enter-from[^{]*\{[^}]*translateY/)
    // y lo de dentro asoma escalonado
    expect(CSS).toMatch(/\.study-enter-active \.study-col\{[^}]*animation:/)
    // por debajo de la cola del reproductor, que se abre desde un boton suyo
    const z = (sel) =>
      Number(
        (CSS.match(new RegExp(sel.replace(/[.-]/g, '\\$&') + '\\{[^}]*z-index:(\\d+)')) || [])[1]
      )
    expect(z('.study')).toBeLessThan(z('.queue'))
  })

  it('los botones reaccionan al raton de forma suave', () => {
    expect(CSS).toMatch(/\.btn\{[^}]*transition:/)
  })

  it('quien pide movimiento reducido lo obtiene en TODA la app', () => {
    const block = CSS.slice(CSS.lastIndexOf('prefers-reduced-motion'))
    expect(block, 'falta la regla global').toMatch(/\*,\s*\*::before,\s*\*::after/)
    // ...pero los indicadores de «esto sigue en marcha» no se congelan
    expect(block).toMatch(/\.spinner[^{]*\{[^}]*infinite/)
  })
})

describe('el desplegable', () => {
  const themes = () =>
    Object.entries(CATALOG).map(([k, t]) => ({
      v: k,
      n: t.name,
      note: KIND_LABEL[t.kind],
      color: t.v.accent
    }))

  it('enseña sus opciones con el punto de color de cada una', async () => {
    const w = mount(SelectField, {
      props: { modelValue: 'night', label: 'Tema', options: themes() }
    })
    await w.find('.select-box').trigger('click')
    await flushPromises()
    expect(w.findAll('.select-opt').length).toBe(Object.keys(CATALOG).length)
    // El punto de color tiene que tener un color de VERDAD. Antes se leia
    // `t.v.acento`, que ya no existe, y salia `background: undefined`.
    // jsdom normaliza el hex a rgb(), asi que vale cualquiera de las dos.
    for (const dot of w.findAll('.select-menu .select-dot')) {
      const style = dot.attributes('style') || ''
      expect(style).not.toContain('undefined')
      expect(style, `color no valido: ${style}`).toMatch(/background:\s*(#[0-9a-f]{3,8}|rgb)/i)
    }
  })

  it('al abrirse lleva a la vista la opcion elegida', async () => {
    const view = vi.fn()
    Element.prototype.scrollIntoView = view
    const w = mount(SelectField, { props: { modelValue: 'wine', options: themes() } })
    await w.find('.select-box').trigger('click')
    await flushPromises()
    expect(view).toHaveBeenCalled()
    expect(view.mock.contexts[0].textContent).toContain(CATALOG.wine.name)
    delete Element.prototype.scrollIntoView
  })

  it('se puede quedar sin elegir: la «x» vuelve al valor vacio', async () => {
    const options = [
      { v: '', n: 'Cualquiera' },
      { v: 'Barak', n: 'Barak' }
    ]
    const empty = mount(SelectField, {
      props: { modelValue: '', options: options, clearable: true }
    })
    expect(empty.find('.select-clear').exists(), 'sin nada elegido no hay nada que quitar').toBe(
      false
    )
    const w = mount(SelectField, {
      props: { modelValue: 'Barak', options: options, clearable: true }
    })
    await w.find('.select-clear').trigger('click')
    expect(w.emitted('update:modelValue')[0]).toEqual([''])
    // y la «x» no le tapa la flecha
    expect(CSS).toMatch(/\.select-clear\{/)
    expect(CSS).toMatch(/\.field:has\(\.select-clear\) \.select-box\{[^}]*padding-right/)
  })
})

describe('la columna de estrellas cabe', () => {
  it('es lo bastante ancha para las cinco estrellas', () => {
    // cinco iconos de 15px + cuatro huecos + el relleno de la celda
    const rule = CSS.match(/\.col-stars\{([^}]*)\}/)
    expect(rule, 'no hay regla para la columna de estrellas').toBeTruthy()
    const width = Number(rule[1].match(/width:\s*(\d+)px/)?.[1])
    const gap = Number(CSS.match(/\.stars\{[^}]*gap:\s*([\d.]+)px/)?.[1] || 2.5)
    const padding = 20 // 10px a cada lado en `td`
    const needed = 5 * 15 + 4 * gap + padding
    expect(width, `necesita al menos ${needed}px`).toBeGreaterThanOrEqual(needed)
  })

  it('la columna de estrellas se define una sola vez fuera de media queries', () => {
    // habia dos reglas, 82px y 104px, y la de abajo pisaba a la de arriba
    const withoutMedia = CSS.replace(/@media[^{]*\{(?:[^{}]*\{[^}]*\})*[^}]*\}/g, '')
    expect((withoutMedia.match(/\.col-stars\s*\{/g) || []).length).toBe(1)
  })
})

describe('arrastrar una cancion', () => {
  // En el WebView de escritorio, arrastrar una fila iba dejando media lista
  // seleccionada en azul. El `user-select:none` del css no basta ahi, asi que
  // se corta el evento que inicia la seleccion.
  it('mientras se arrastra no se puede empezar a seleccionar texto', () => {
    const { startDrag } = useDragSong()
    const row = document.createElement('div')
    document.body.appendChild(row)
    startDrag(
      { id: 1, title: 'Mi Gozo' },
      { pointerType: 'mouse', button: 0, clientX: 10, clientY: 10, target: row }
    )
    const selection = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(selection)
    expect(selection.defaultPrevented, 'deja que el motor empiece a seleccionar').toBe(true)
    // y tampoco el arrastre nativo del texto, que pinta su propio fantasma
    const native = new window.Event('dragstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(native)
    expect(native.defaultPrevented).toBe(true)
  })

  it('al soltar deja de estorbar a la seleccion normal', () => {
    const { startDrag } = useDragSong()
    startDrag(
      { id: 1 },
      { pointerType: 'mouse', button: 0, clientX: 0, clientY: 0, target: document.body }
    )
    cancelDrag()
    const selection = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(selection)
    expect(selection.defaultPrevented, 'sigue bloqueando la seleccion sin arrastrar nada').toBe(
      false
    )
  })

  it('con el dedo no se arrastra: eso es desplazar la lista', () => {
    const { startDrag } = useDragSong()
    startDrag(
      { id: 1 },
      { pointerType: 'touch', button: 0, clientX: 0, clientY: 0, target: document.body }
    )
    const selection = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(selection)
    expect(selection.defaultPrevented, 'el toque no deberia armar un arrastre').toBe(false)
  })
})

describe('portadas difuminadas', () => {
  it('la portada se pinta borrosa cuando la cancion esta marcada, y lo dice', () => {
    expect(
      mount(CoverArt, { props: { id: 1 } })
        .find('img')
        .classes()
    ).not.toContain('blurred')
    const blurred = mount(CoverArt, { props: { id: 1, blur: true, alt: 'Mi Gozo' } })
    expect(blurred.find('img').classes(), 'no se marca como difuminada').toContain('blurred')
    // quien lee la portada se entera de que esta difuminada
    expect(blurred.find('img').attributes('alt')).toContain('difuminada')
  })

  it('el css la difumina de verdad, no le baja la opacidad', () => {
    const rule = CSS.match(/\.cover-art\.blurred\{([^}]*)\}/)
    expect(rule, 'no hay estilo para las portadas difuminadas').toBeTruthy()
    expect(rule[1], 'deberia usar blur()').toMatch(/filter:\s*blur\(\s*\d+px/)
    // Bajar la opacidad dejaria la imagen reconocible, que es justo lo que se
    // quiere evitar.
    expect(rule[1]).not.toMatch(/(^|;)\s*opacity:/)
    // Y nada de agrandarla: se saldria de su caja y taparia lo de al lado.
    expect(rule[1]).not.toMatch(/transform:\s*scale/)
  })

  it('todas las listas y la ficha la respetan', async () => {
    // si una se olvida, la imagen sigue viendose donde menos se espera (el
    // reproductor y la ventanita, en sus propias pruebas; el menu que la
    // activa, en app.test.js)
    const blurred = song(1, { blur: 1 })
    for (const [name, w] of [
      ['las fichas', mount(SongCards, { props: { songs: [blurred] } })],
      ['la cuadricula', mount(SongGrid, { props: { songs: [blurred] } })],
      [
        'los resultados del buscador',
        mount(SearchResults, { props: { songs: [blurred], query: 'x' } })
      ],
      ['la ficha', mount(DetailsPanel, { props: { song: blurred, aiReady: false } })]
    ]) {
      await flushPromises()
      expect(w.find('.cover-art').classes(), `${name} no la difumina`).toContain('blurred')
    }
  })
})

describe('campos de formulario', () => {
  it('el input de dentro no pinta su propio anillo de foco', () => {
    // La regla base `input:focus` le ponia su sombra al input, y el envoltorio
    // ponia la suya: se veian DOS anillos verdes, uno dentro del otro, con un
    // hueco oscuro en medio.
    const rule = CSS.match(/\.field-box input[^{]*\{[^}]*box-shadow:\s*none[^}]*\}/)
    expect(rule, 'el input de un campo deberia anular su sombra de foco').toBeTruthy()
    for (const sel of ['.slider-track input', '.color-swatch input']) {
      const className = sel.split(' ')[0].slice(1)
      expect(CSS, `${sel} tambien la hereda`).toMatch(
        new RegExp(`\\.${className}[^{]*input[^{]*\\{[^}]*box-shadow:\\s*none`)
      )
    }
  })

  it('solo hay un anillo por campo, el del envoltorio', () => {
    const wrapper = CSS.match(/\.field\.focused \.field-box\{([^}]*)\}/)
    expect(wrapper, 'el campo enfocado deberia tener su sombra').toBeTruthy()
    expect(wrapper[1]).toMatch(/box-shadow:/)
  })
})

describe('iconos con el significado correcto', () => {
  // que el boton de enviar del chat lleve este icono se comprueba montando
  // el chat (chat.test.js); aqui, que el icono exista
  it('el icono de enviar existe de verdad', async () => {
    const { ICONS } = await import('../src/icons.js')
    expect(ICONS.send, 'falta el icono «send»').toBeTruthy()
    expect(ICONS.send.d).toContain('<path')
  })
})

describe('informe de duplicados', () => {
  const group = {
    suggested: '/m/Barak - Mi Gozo - r.mp3',
    items: [
      {
        id: 2,
        path: '/m/Barak - Mi Gozo - r.mp3',
        relative: 'Artistas/Barak/Barak - Mi Gozo - r.mp3',
        file: 'Barak - Mi Gozo - r.mp3',
        duration: 201,
        bitrate: 320000,
        size: 7023456,
        stars: 0,
        favorite: 0,
        has_suffix: true
      }
    ]
  }

  it('los datos del archivo van juntos y no se parten por la mitad', () => {
    // «6.7» quedaba en una linea y «MB» en la siguiente, con la ruta en medio:
    // el texto suelto dentro de un flex se rompe por cualquier espacio.
    const w = mount(DuplicateGroup, { props: { group: group, playing: null, busy: false } })
    expect(w.find('.dup-stats').text().replace(/\s+/g, ' ')).toBe('320 kbps · 3:21 · 6.7 MB')
    expect(CSS).toMatch(/\.dup-stats\{[^}]*white-space:\s*nowrap/)
  })

  it('lee las mismas claves que manda el nucleo', () => {
    // `relative`, `has_suffix` y `suggested`: con los nombres viejos en
    // castellano no salia ni la ruta ni las marcas
    const w = mount(DuplicateGroup, { props: { group: group, playing: null, busy: false } })
    expect(w.find('.dup-path').text()).toBe('Artistas/Barak/Barak - Mi Gozo - r.mp3')
    expect(w.text()).toContain('mejor calidad')
    expect(w.text()).toContain('lleva « - r»')
  })
})

describe('las cuatro vistas', () => {
  it('hay exactamente cuatro y cada una tiene nombre e icono', async () => {
    const { VIEWS, VIEW_NAMES, VIEW_ICONS } = await import('../src/composables/usePreferences.js')
    expect(VIEWS).toEqual(['rows', 'cards', 'grid', 'table'])
    for (const v of VIEWS) {
      expect(VIEW_NAMES[v], `falta el nombre de «${v}»`).toBeTruthy()
      expect(VIEW_ICONS[v], `falta el icono de «${v}»`).toBeTruthy()
    }
  })

  it('quien tenia elegida una vista vieja no la pierde', async () => {
    const { savedLayout, resetPreferences } = await import('../src/composables/usePreferences.js')
    resetPreferences()
    for (const [legacy, current] of [
      ['list', 'table'],
      ['compact', 'rows']
    ]) {
      localStorage.setItem('danplay.layout', legacy)
      expect(savedLayout(), `«${legacy}» deberia traducirse a «${current}»`).toBe(current)
    }
    // y una vista que no existe no deja la app en blanco
    localStorage.setItem('danplay.layout', '{"topbar":"bottom"}')
    expect(savedLayout()).toBe('table')
  })

  it('las cuatro tambien valen agrupadas, con sus cabeceras de grupo', () => {
    const songs = [song(1), song(2, { artist: 'New Wine' }), song(3)]
    for (const [layout, box] of [
      ['rows', '.rows'],
      ['cards', '.cards'],
      ['grid', '.grid'],
      ['table', 'table']
    ]) {
      const w = mount(GroupedSongs, { props: { songs, by: 'artist', layout } })
      expect(w.findAll('.group-head').map((g) => g.find('span').text())).toEqual([
        'Barak',
        'New Wine'
      ])
      expect(w.find(box).exists(), `agrupado no sabe pintar «${layout}»`).toBe(true)
      expect(w.findAll('[data-song-row]')).toHaveLength(3)
    }
  })

  it('todas se adaptan al ancho', () => {
    // cada vista tiene que ceder algo cuando falta sitio
    expect(CSS, 'la lista fina no se adapta').toMatch(/@media[^{]*\{\s*\.row-/)
    expect(CSS, 'las fichas no se acoplan solas').toMatch(/\.cards\{[^}]*auto-fill/)
    expect(CSS, 'la cuadricula no se acopla sola').toMatch(/\.grid\{[^}]*auto-fill/)
  })

  // que las cuatro pintan solo lo que se ve se prueba montandolas con una
  // maquetacion simulada (virtual-rows.test.js); que la app pinta cada una
  // con su componente, en app.test.js
})

describe('la cabecera de la tabla', () => {
  // Estaba definida DOS veces: la primera la dejaba fija (`position:sticky`) y
  // la segunda, ochenta lineas mas abajo, la pisaba con `position:relative`.
  // Resultado: al desplazar una lista larga la cabecera se iba con ella y no
  // se sabia que columna era cual. Nadie lo vio porque las dos reglas estaban
  // lejos y ninguna herramienta miraba los selectores repetidos.
  it('se queda fija al desplazar la lista', () => {
    const rules = [...CSS.matchAll(/(^|\n)thead th\{([^}]*)\}/g)].map((m) => m[2])
    expect(rules.length, 'thead th deberia definirse una sola vez').toBe(1)
    expect(rules[0]).toMatch(/position:\s*sticky/)
    expect(rules[0], 'sin top no se pega a ningun sitio').toMatch(/top:\s*0/)
    expect(rules[0], 'tiene que quedar por encima de las filas').toMatch(/z-index/)
    expect(rules[0], 'sin fondo se ven las filas por debajo').toMatch(/background:/)
  })

  it('solo las columnas que ordenan parecen pulsables', () => {
    expect(CSS).toMatch(/thead th\.sortable\{[^}]*cursor:\s*pointer/)
  })
})

describe('ordenar por una columna', () => {
  it('la cabecera enseña por donde va el orden y hacia donde', async () => {
    const w = mount(SongTable, { props: { songs: [song(1)], sort: 'title', desc: false } })
    const title = w.find('th[data-col="title"]')
    expect(title.attributes('aria-sort')).toBe('ascending')
    expect(title.find('.th-arrow').classes()).toContain('up')
    expect(w.find('th[data-col="artist"]').attributes('aria-sort')).toBeUndefined()
    await w.setProps({ desc: true })
    expect(title.attributes('aria-sort')).toBe('descending')
    expect(title.find('.th-arrow').classes()).not.toContain('up')
    expect(CSS, 'la flecha no se da la vuelta').toMatch(/\.th-arrow\.up\{[^}]*rotate\(180deg\)/)
  })

  it('cada columna ordena por un campo que el nucleo conoce, empezando por lo natural', async () => {
    // los nombres salen del nucleo (SORT_FIELDS); si aqui se inventa uno, la
    // API lo ignora y la lista no cambia. (Que pulsar dos veces la misma
    // invierte el orden, en app.test.js.)
    const w = mount(SongTable, { props: { songs: [song(1)] } })
    for (const b of w.findAll('thead .th-sort')) await b.trigger('click')
    const requests = w.emitted('sortBy')
    expect(requests.length).toBeGreaterThan(4)
    const core = coreSource()
    for (const [field] of requests) {
      expect(core, `el nucleo no sabe ordenar por «${field}»`).toMatch(
        new RegExp(`"${field}":\\s*\\(`)
      )
    }
    // por texto se espera de la A a la Z; por duracion, lo mas largo primero
    expect(Object.fromEntries(requests)).toMatchObject({
      title: false,
      duration: true,
      stars: true
    })
  })
})

describe('columnas ajustables', () => {
  /** jsdom no maqueta: cada cabecera dice que mide 100 px. */
  const withWidths = () =>
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
      const w = this.tagName === 'TH' ? 100 : 0
      return { width: w, height: 20, top: 0, left: 0, right: w, bottom: 20, x: 0, y: 0 }
    })
  const pointer = (target, type, x) =>
    target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x }))

  /** Los anchos puestos a mano: los `col` con estilo propio. */
  const manual = (w) => w.findAll('col').filter((c) => c.attributes('style'))

  it('sin tocarlas no se fija ningun ancho: la tabla se reparte sola', () => {
    const w = mount(SongTable, { props: { songs: [song(1)] } })
    // los `col` estan siempre (llevan la clase de su columna, con el ancho
    // de fabrica), pero ninguno con un ancho propio
    expect(w.findAll('col').map((c) => c.classes()[0])).toContain('col-title')
    expect(manual(w)).toHaveLength(0)
    expect(w.find('table').classes()).not.toContain('medida')
  })

  it('se ensanchan arrastrando el borde, se recuerdan y el doble clic las olvida', async () => {
    withWidths()
    const w = mount(SongTable, { props: { songs: [song(1)] }, attachTo: document.body })
    const edge = w.find('th[data-col="title"] .col-resize').element
    pointer(edge, 'pointerdown', 100)
    pointer(window, 'pointermove', 160)
    pointer(window, 'pointerup', 160)
    await flushPromises()
    expect(JSON.parse(localStorage.getItem('danplay.colWidths')).title).toBe(160)
    expect(w.find('col:nth-child(3)').attributes('style')).toContain('width: 160px')
    // al volver, siguen como se dejaron
    const other = mount(SongTable, { props: { songs: [song(1)] } })
    expect(other.find('col:nth-child(3)').attributes('style')).toContain('width: 160px')
    // doble clic en el borde: vuelve el reparto de siempre
    await w.find('th[data-col="title"] .col-resize').trigger('dblclick')
    expect(manual(w)).toHaveLength(0)
    expect(w.find('table').classes()).not.toContain('medida')
    expect(localStorage.getItem('danplay.colWidths')).toBeNull()
  })

  it('con el teclado, las flechas lo ensanchan de a poco', async () => {
    withWidths()
    const w = mount(SongTable, { props: { songs: [song(1)] } })
    await w.find('th[data-col="album"] .col-resize').trigger('keydown', { key: 'ArrowRight' })
    expect(JSON.parse(localStorage.getItem('danplay.colWidths')).album).toBe(110)
  })

  it('el tirador queda dentro de la celda', () => {
    // `th` recorta lo que se salga: a caballo del borde no recibia el puntero
    expect(CSS).toMatch(/\.col-resize\{[^}]*right:0/)
  })
})

describe('los resultados del buscador', () => {
  const eight = () => Array.from({ length: 8 }, (_, i) => song(i + 1))

  it('se ven cinco y el resto con la rueda, midiendo una fila de verdad', async () => {
    // la altura sale de MEDIR una fila, no de un numero a ojo: asi sigue
    // cuadrando aunque cambie la densidad o el tamaño de la app
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
      const h = this.classList.contains('sr-row') ? 41 : 0
      return { height: h, width: 300, top: 0, left: 0, right: 300, bottom: h, x: 0, y: 0 }
    })
    const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
    await flushPromises()
    expect(w.find('.sr-list').attributes('style')).toContain('max-height: 205px')
    // con pocas no hace falta rueda
    await w.setProps({ songs: eight().slice(0, 3) })
    await flushPromises()
    expect(w.find('.sr-list').attributes('style')).toBeUndefined()
    expect(CSS).toMatch(/\.sr-list\{[^}]*overflow-y:auto/)
  })

  it('se manejan con el teclado desde la caja de busqueda', async () => {
    const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
    const press = (key) => w.vm.onKey(new KeyboardEvent('keydown', { key, cancelable: true }))
    expect(press('ArrowDown')).toBe(true)
    expect(press('ArrowDown')).toBe(true)
    expect(press('ArrowUp')).toBe(true)
    await flushPromises()
    expect(w.find('.sr-row.on').text()).toContain('Cancion 2')
    // la caja sabe cual esta resaltada, para quien no ve la pantalla
    expect(document.getElementById !== undefined && w.vm.activeId).toBe(
      w.find('.sr-row.on').attributes('id')
    )
    expect(press('Enter')).toBe(true)
    expect(w.emitted('pick')[0][0].id).toBe(2)
    expect(press('Escape')).toBe(true)
    expect(w.emitted('close')).toBeTruthy()
    expect(press('a')).toBe(false)
  })

  it('se pueden reproducir, no solo mirar', async () => {
    const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
    await w.findAll('.sr-play')[3].trigger('click')
    expect(w.emitted('play')[0][0].id).toBe(4)
    expect(w.emitted('pick'), 'reproducir no es elegir').toBeFalsy()
  })

  it('el clic derecho y el «⋯» abren las opciones de esa fila, sin elegirla ni reproducirla', async () => {
    // antes no hacian nada: no se podia añadir un resultado a un repertorio
    const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
    await w.findAll('.sr-row')[2].trigger('contextmenu')
    expect(w.emitted('menu')).toHaveLength(1)
    expect(w.emitted('menu')[0][0].id).toBe(3)
    expect(w.emitted('menu')[0][1]).toBeInstanceOf(MouseEvent)
    // la fila queda resaltada: es sobre la que actua el menu
    expect(w.find('.sr-row.on').text()).toContain('Cancion 3')

    const more = w.findAll('.sr-more')[5]
    // dentro de un «option» sus hijos son presentacionales: el nombre ya lo dice la opcion
    expect(more.attributes('aria-hidden')).toBe('true')
    expect(more.attributes('title')).toBe('Opciones')
    await more.trigger('click')
    expect(w.emitted('menu')).toHaveLength(2)
    expect(w.emitted('menu')[1][0].id).toBe(6)
    expect(w.emitted('pick'), 'abrir las opciones no es elegir la fila').toBeFalsy()
    expect(w.emitted('play'), 'ni reproducirla').toBeFalsy()
  })

  it('la tecla de menu o Mayus+F10 abren las opciones de la fila resaltada', async () => {
    // el foco se queda en la caja de busqueda: el menu sale bajo el «⋯» de la fila
    const w = mount(SearchResults, {
      props: { songs: eight(), query: 'x' },
      attachTo: document.body
    })
    const press = (init) => w.vm.onKey(new KeyboardEvent('keydown', { cancelable: true, ...init }))
    w.vm.onKey(new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true }))
    await flushPromises()
    expect(press({ key: 'ContextMenu' })).toBe(true)
    const [song, where] = w.emitted('menu')[0]
    expect(song.id).toBe(2)
    // un sitio, no un evento de raton: el menu sabe que viene del teclado
    expect(where).not.toBeInstanceOf(MouseEvent)
    expect(where).toEqual({ clientX: expect.any(Number), clientY: expect.any(Number) })
    expect(press({ key: 'F10', shiftKey: true })).toBe(true)
    expect(w.emitted('menu')).toHaveLength(2)
    // F10 a secas no es el menu
    expect(press({ key: 'F10' })).toBe(false)
  })

  it('el doble clic en el «⋯» o en reproducir no llega a la fila (que lo toma por reproducir)', async () => {
    const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
    await w.findAll('.sr-more')[2].trigger('dblclick')
    await w.findAll('.sr-play')[3].trigger('dblclick')
    expect(w.emitted('play'), 'un doble clic en un boton de la fila no reproduce').toBeFalsy()
    // en la propia fila sigue valiendo
    await w.findAll('.sr-row')[1].trigger('dblclick')
    expect(w.emitted('play')).toHaveLength(1)
  })

  it('mientras carga no hay fila sobre la que actuar: ni Enter ni la tecla de menu', () => {
    // la lista no se pinta pero `songs` conserva la busqueda anterior: la tecla
    // de menu abria las opciones de una cancion que no se ve, en la esquina
    const w = mount(SearchResults, { props: { songs: eight(), query: 'x', loading: true } })
    const press = (init) => w.vm.onKey(new KeyboardEvent('keydown', { cancelable: true, ...init }))
    expect(press({ key: 'ContextMenu' })).toBe(false)
    expect(press({ key: 'F10', shiftKey: true })).toBe(false)
    expect(press({ key: 'Enter' })).toBe(false)
    expect(w.emitted('menu')).toBeFalsy()
    expect(w.emitted('pick')).toBeFalsy()
  })

  it('la cabecera es corta y no se parte: el recuento, un aviso corto y «Seleccionar»; en estrecho cuelga de la ventana', async () => {
    // «8 resultados en toda la biblioteca» y el aviso largo no cabian a 460 px
    // con el boton: el recuento se partia y el aviso se cortaba con «…»
    const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
    expect(w.find('.sr-head .sr-count').text()).toBe('8 resultados')
    expect(w.find('.sr-tip').text()).toBe('clic derecho: opciones')
    expect(w.find('.sr-head .sr-select').text()).toBe('Seleccionar')
    expect(CSS).toMatch(/\.sr-count\{[^}]*flex-shrink:0[^}]*white-space:nowrap/)
    expect(CSS).toMatch(/\.sr-tools\{[^}]*white-space:nowrap/)
    // sin raton, «clic derecho» no dice nada
    expect(CSS).toMatch(/@media \(hover:none\)\{[^@]*\.sr-tip\{display:none/)
    // con la caja a ~59 px del borde, 94vw se salia por la derecha y el «⋯» quedaba medio fuera
    expect(CSS).toMatch(
      /@media \(max-width:700px\)\{\.search-results\{position:fixed;top:56px;left:8px;right:8px;width:auto/
    )
  })

  it('el «⋯» se ve con el puntero encima, en la fila resaltada y donde no hay puntero', () => {
    expect(CSS).toMatch(/\.sr-more\{[^}]*opacity:0/)
    expect(CSS).toMatch(/\.sr-row\.on \.sr-more[^{]*\{[^}]*opacity:1/)
    expect(CSS).toMatch(/@media \(hover:none\)\{\.sr-more\{opacity:1/)
  })

  it('se pueden arrastrar a un repertorio', async () => {
    // una fila-boton nunca podria arrastrarse: el arrastre se aparta de los
    // botones (ahi el clic tiene otra cosa que hacer)
    const w = mount(SearchResults, {
      props: { songs: eight(), query: 'x' },
      attachTo: document.body
    })
    const row = w.findAll('.sr-row')[1].element
    expect(row.tagName).not.toBe('BUTTON')
    row.dispatchEvent(
      new MouseEvent('pointerdown', { bubbles: true, button: 0, clientX: 5, clientY: 5 })
    )
    window.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 60, clientY: 90 }))
    expect(useDragSong().drag.song?.id).toBe(2)
    // y mientras se arrastra, el desplegable no tapa el lateral
    expect(CSS).toMatch(/body\.dragging-song \.search-results\{[^}]*pointer-events:none/)
  })

  // ------------------------------------------------- varios a la vez
  describe('elegir varios', () => {
    /** Los ids de las filas elegidas, en el orden en que se ven. */
    const picked = (w) =>
      w.findAll('.sr-row').flatMap((f, i) => (f.classes().includes('picked') ? [i + 1] : []))
    const rows = (w) => w.findAll('.sr-row')
    const button = (w, label) => w.find(`.sr-head button[aria-label="${label}"]`)
    const press = (w, key, extra = {}) => {
      const e = new KeyboardEvent('keydown', { key, cancelable: true, ...extra })
      return { handled: w.vm.onKey(e), prevented: e.defaultPrevented }
    }

    it('«Seleccionar» pone una casilla en cada fila y el clic las marca en vez de abrir la ficha', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      const mode = () => w.find('.sr-select')
      expect(mode().attributes('aria-pressed')).toBe('false')
      expect(w.findAll('.sr-row .pick-box')).toHaveLength(0)
      expect(w.find('[role="listbox"]').attributes('aria-multiselectable')).toBeUndefined()

      await mode().trigger('click')
      expect(mode().attributes('aria-pressed')).toBe('true')
      expect(w.findAll('.sr-row .pick-box')).toHaveLength(8)
      // decorativas: la verdad, para el lector de pantalla, es el aria-selected
      // de cada fila en una lista de eleccion multiple
      expect(w.find('.sr-row .pick-box').attributes('aria-hidden')).toBe('true')
      expect(w.find('[role="listbox"]').attributes('aria-multiselectable')).toBe('true')
      expect(w.find('.sr-tip').text()).toBe('marca los que quieras')

      await rows(w)[1].trigger('click')
      await rows(w)[4].trigger('click')
      expect(w.emitted('pick'), 'en el modo un clic no abre la ficha').toBeFalsy()
      expect(picked(w)).toEqual([2, 5])
      expect(w.findAll('.pick-box.on')).toHaveLength(2)
      expect(rows(w).map((f) => f.attributes('aria-selected'))).toEqual([
        'false',
        'true',
        'false',
        'false',
        'true',
        'false',
        'false',
        'false'
      ])
      await rows(w)[1].trigger('click') // la desmarca, sin soltar la otra
      expect(picked(w)).toEqual([5])
      // marcar y desmarcar deprisa en la casilla no pone la cancion
      await w.findAll('.pick-box')[2].trigger('dblclick')
      expect(w.emitted('play')).toBeFalsy()
      expect(CSS).toMatch(/\.sr-select\.on\{[^}]*var\(--accent\)/)
    })

    it('sin el modo: Ctrl+clic pone o quita uno y Mayús+clic coge el tramo; el clic a secas abre la ficha', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      await rows(w)[1].trigger('click', { ctrlKey: true })
      await rows(w)[3].trigger('click', { metaKey: true }) // Cmd, en el Mac
      expect(picked(w)).toEqual([2, 4])
      // Mayús: el tramo desde la ultima pulsada, solo
      await rows(w)[6].trigger('click', { shiftKey: true })
      expect(picked(w)).toEqual([4, 5, 6, 7])
      // Ctrl+Mayús: el tramo se suma a lo que habia
      await rows(w)[1].trigger('click', { ctrlKey: true, shiftKey: true })
      expect(picked(w)).toEqual([2, 3, 4, 5, 6, 7])
      await rows(w)[3].trigger('click', { ctrlKey: true })
      expect(picked(w)).toEqual([2, 3, 5, 6, 7])
      expect(w.emitted('pick'), 'Ctrl y Mayús no abren la ficha').toBeFalsy()
      // sin modo no hay casillas: lo elegido se ve por el color de la fila
      expect(w.findAll('.pick-box')).toHaveLength(0)
      expect(CSS).toMatch(/\.sr-row\.picked\{[^}]*var\(--accent\)/)
      // el clic normal sigue siendo elegir la cancion (su ficha)
      await rows(w)[0].trigger('click')
      expect(w.emitted('pick')[0][0].id).toBe(1)
    })

    it('desde la caja: Ctrl+Espacio pone o quita la resaltada y Mayús+flechas amplían o encogen', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      press(w, 'ArrowDown') // la segunda
      expect(press(w, ' ', { ctrlKey: true })).toEqual({ handled: true, prevented: true })
      await flushPromises()
      expect(picked(w)).toEqual([2])
      press(w, 'ArrowDown')
      press(w, 'ArrowDown') // la cuarta
      // amplía desde donde esta, sin soltar lo de antes; la caja no selecciona texto
      expect(press(w, 'ArrowDown', { shiftKey: true })).toEqual({ handled: true, prevented: true })
      press(w, 'ArrowDown', { shiftKey: true })
      await flushPromises()
      expect(picked(w)).toEqual([2, 4, 5, 6])
      press(w, 'ArrowUp', { shiftKey: true }) // encoge el tramo, no lo de antes
      await flushPromises()
      expect(picked(w)).toEqual([2, 4, 5])
      expect(w.find('.sr-row.on').text()).toContain('Cancion 5')
      press(w, ' ', { ctrlKey: true }) // quita la resaltada
      await flushPromises()
      expect(picked(w)).toEqual([2, 4])
      // el espacio a secas se escribe en la caja
      expect(press(w, ' ').handled).toBe(false)
      expect(w.emitted('pick')).toBeFalsy()
    })

    it('Ctrl+A en la caja es de su texto: no elige resultados', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      expect(press(w, 'a', { ctrlKey: true })).toEqual({ handled: false, prevented: false })
      expect(press(w, 'A', { metaKey: true }).handled).toBe(false)
      await flushPromises()
      expect(picked(w)).toEqual([])
      expect(w.find('.sr-picking').exists()).toBe(false)
    })

    it('con alguno elegido la cabecera es una barra: cuántos, añadir a una lista, «⋯» y Ninguno', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      await rows(w)[4].trigger('click', { ctrlKey: true })
      expect(w.find('.sr-count').text()).toBe('1 seleccionado')
      await rows(w)[1].trigger('click', { ctrlKey: true })
      const header = w.find('.sr-head')
      expect(header.classes()).toContain('sr-picking')
      // el lector de pantalla anuncia la cuenta al cambiar
      expect(w.find('.sr-count').text()).toBe('2 seleccionados')
      expect(w.find('.sr-count').attributes('aria-live')).toBe('polite')
      expect(w.find('.sr-select').exists(), 'la barra ocupa el sitio del boton').toBe(false)
      expect(w.find('.sr-tip').exists()).toBe(false)

      // los elegidos van en el orden de los resultados, no en el de los clics
      await button(w, 'Añadir a una lista').trigger('click')
      const [list, where] = w.emitted('addToList')[0]
      expect(list.map((c) => c.id)).toEqual([2, 5])
      expect(where).toBeInstanceOf(MouseEvent)
      expect(button(w, 'Añadir a una lista').attributes('aria-haspopup')).toBe('menu')
      await button(w, 'Más opciones').trigger('click')
      expect(w.emitted('groupMenu')[0][0].map((c) => c.id)).toEqual([2, 5])
      expect(w.emitted('pick'), 'nada de esto cierra ni elige').toBeFalsy()

      // «Ninguno»: vuelve la cabecera de siempre
      await button(w, 'No seleccionar ninguno').trigger('click')
      expect(picked(w)).toEqual([])
      expect(w.find('.sr-count').text()).toBe('8 resultados')
      expect(w.find('.sr-select').exists()).toBe(true)
      // y la barra no se parte: si falta sitio cede la palabra, no el numero
      expect(CSS).toMatch(/\.sr-picking \.sr-count\{[^}]*text-overflow:ellipsis/)
      expect(CSS).toMatch(
        /@media \(max-width:460px\)\{\.sr-long\{display:none\}\s*\.sr-short\{display:inline/
      )
    })

    it('sus botones no se quedan el foco al pulsarlos: el teclado sigue en la caja', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      const mousedown = (el) => {
        const e = new MouseEvent('mousedown', { bubbles: true, cancelable: true })
        el.element.dispatchEvent(e)
        return e.defaultPrevented
      }
      expect(mousedown(w.find('.sr-select'))).toBe(true)
      expect(mousedown(rows(w)[2])).toBe(true)
      await rows(w)[2].trigger('click', { ctrlKey: true })
      for (const label of ['Añadir a una lista', 'Más opciones', 'No seleccionar ninguno']) {
        expect(mousedown(button(w, label)), label).toBe(true)
      }
    })

    it('el clic derecho, el «⋯» y la tecla de menú sobre una elegida son las opciones de todas; sobre otra, las suyas', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      await rows(w)[2].trigger('click', { ctrlKey: true })
      await rows(w)[5].trigger('click', { ctrlKey: true })
      await rows(w)[5].trigger('contextmenu')
      expect(w.emitted('groupMenu')[0][0].map((c) => c.id)).toEqual([3, 6])
      expect(w.emitted('menu')).toBeFalsy()
      await w.findAll('.sr-more')[2].trigger('click')
      expect(w.emitted('groupMenu')).toHaveLength(2)
      // sobre una que no esta elegida: la de esa, como siempre
      await rows(w)[0].trigger('contextmenu')
      expect(w.emitted('menu')[0][0].id).toBe(1)
      expect(picked(w), 'abrir un menu no cambia lo elegido').toEqual([3, 6])
      // con el teclado, sobre la resaltada
      press(w, 'ArrowDown')
      press(w, 'ArrowDown') // la tercera, elegida
      expect(press(w, 'ContextMenu').handled).toBe(true)
      const [list, where] = w.emitted('groupMenu')[2]
      expect(list.map((c) => c.id)).toEqual([3, 6])
      expect(where).toEqual({ clientX: expect.any(Number), clientY: expect.any(Number) })
    })

    it('Escape suelta lo elegido (y el modo) antes de cerrar', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      await w.find('.sr-select').trigger('click')
      await rows(w)[3].trigger('click')
      const e = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true })
      const stop = vi.spyOn(e, 'stopPropagation')
      expect(w.vm.onKey(e)).toBe(true)
      // el «Escape cierra» del documento no se entera
      expect(stop).toHaveBeenCalled()
      await flushPromises()
      expect(w.emitted('close')).toBeFalsy()
      expect(picked(w)).toEqual([])
      expect(w.find('.sr-select').attributes('aria-pressed')).toBe('false')
      // con nada que soltar, Escape cierra
      expect(press(w, 'Escape').handled).toBe(true)
      expect(w.emitted('close')).toHaveLength(1)
    })

    it('otra búsqueda olvida lo elegido (el modo sigue); lo que se va de los resultados se suelta', async () => {
      const w = mount(SearchResults, { props: { songs: eight(), query: 'x' } })
      await w.find('.sr-select').trigger('click')
      await rows(w)[1].trigger('click')
      await rows(w)[2].trigger('click')
      await rows(w)[3].trigger('click')
      // la misma busqueda pierde una fila (a la papelera): las demas siguen elegidas
      await w.setProps({ songs: eight().filter((c) => c.id !== 3) })
      await flushPromises()
      expect(w.find('.sr-count').text()).toBe('2 seleccionados')
      // otra consulta: nada elegido, pero quien puso el modo sigue eligiendo
      await w.setProps({ query: 'xy' })
      await flushPromises()
      expect(w.findAll('.sr-row.picked')).toHaveLength(0)
      expect(w.find('.sr-select').attributes('aria-pressed')).toBe('true')
    })

    it('arrastrar una de las elegidas lleva todas, en su orden; una sin elegir va sola', async () => {
      const w = mount(SearchResults, {
        props: { songs: eight(), query: 'x' },
        attachTo: document.body
      })
      await rows(w)[4].trigger('click', { ctrlKey: true })
      await rows(w)[1].trigger('click', { ctrlKey: true })
      const drag = (i) => {
        rows(w)[i].element.dispatchEvent(
          new MouseEvent('pointerdown', { bubbles: true, button: 0, clientX: 5, clientY: 5 })
        )
        window.dispatchEvent(
          new MouseEvent('pointermove', { bubbles: true, clientX: 60, clientY: 90 })
        )
      }
      drag(4)
      expect(useDragSong().drag.songs.map((c) => c.id)).toEqual([2, 5])
      await flushPromises()
      expect(w.findAll('.sr-row.dragged')).toHaveLength(2)
      cancelDrag()
      drag(6)
      expect(useDragSong().drag.songs.map((c) => c.id)).toEqual([7])
      expect(picked(w), 'arrastrar otra no cambia lo elegido').toEqual([2, 5])
    })
  })
})

describe('la busqueda avanzada', () => {
  const facets = {
    artists: [
      { value: 'Barak', n: 12 },
      { value: 'New Wine', n: 3 }
    ],
    albums: [],
    genres: [],
    keys: [],
    folders: []
  }
  const mountView = (query = '') => mount(SearchPanel, { props: { query, facets } })
  /** El campo con esa etiqueta. */
  const field = (w, labelText) =>
    w
      .findAll('.field')
      .find((f) => f.find('.field-label').exists() && f.find('.field-label').text() === labelText)

  it('ofrece filtrar, comparar y ordenar', () => {
    const w = mountView()
    const labels = w.findAll('.field-label').map((l) => l.text())
    for (const e of ['Texto', 'Artista', 'Album', 'Genero', 'Tono', 'Carpeta', 'Por']) {
      expect(labels, `falta «${e}»`).toContain(e)
    }
    for (const e of ['BPM', 'Duración (s)', 'Calidad (bps)', 'Año']) {
      expect(labels, `falta el rango de «${e}»`).toContain(e)
    }
  })

  it('escribe en la misma caja que se escribe a mano, sin perder lo que habia', async () => {
    // asi no hay dos estados que puedan discrepar, y de paso se aprende la
    // sintaxis viendo lo que aparece escrito
    const w = mountView('mi gozo bpm>100')
    const artist = field(w, 'Artista')
    await artist.find('.select-box').trigger('click')
    await artist
      .findAll('.select-opt')
      .find((o) => o.text().includes('New Wine'))
      .trigger('click')
    expect(w.emitted('update:query').at(-1)).toEqual(['mi gozo artista:NewWine bpm>100'])
    // y lee lo ya escrito: el rango aparece en su casilla
    const fromInput = w.findAll('.rango')[0].findAll('input')[0]
    expect(fromInput.element.value).toBe('100')
    await fromInput.setValue('90')
    expect(w.emitted('update:query').at(-1)).toEqual(['mi gozo bpm>90'])
  })

  it('cada orden que ofrece es uno que el nucleo sabe hacer', async () => {
    const w = mountView()
    const order = field(w, 'Por')
    await order.find('.select-box').trigger('click')
    const names = order.findAll('.select-opt').map((o) => o.text())
    expect(names.length).toBeGreaterThan(8)
    // el menu ya esta abierto: se elige la primera y luego se abre para cada una
    for (let i = 0; i < names.length; i++) {
      if (i) await order.find('.select-box').trigger('click')
      await order.findAll('.select-opt')[i].trigger('click')
    }
    const fields = w.emitted('sort').map(([field]) => field)
    const core = coreSource()
    for (const field of fields) {
      expect(core, `el nucleo no sabe ordenar por «${field}»`).toMatch(
        new RegExp(`"${field}":\\s*\\(`)
      )
    }
  })

  it('«Limpiar todo» deja la busqueda como al principio', async () => {
    const w = mount(SearchPanel, {
      props: { query: 'barak', facets, onlyFavorites: true, minStars: 3 }
    })
    await w
      .findAll('button')
      .find((b) => b.text() === 'Limpiar todo')
      .trigger('click')
    expect(w.emitted('update:query').at(-1)).toEqual([''])
    expect(w.emitted('update:onlyFavorites').at(-1)).toEqual([false])
    expect(w.emitted('update:minStars').at(-1)).toEqual([0])
    expect(w.emitted('sort').at(-1)).toEqual(['artist', false])
  })
})

describe('copiar textos', () => {
  it('copia al portapapeles y acusa que lo hizo', async () => {
    const written = []
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: async (t) => written.push(t) }
    })
    const w = mount(CopyButton, { props: { text: '  Mi Gozo ', what: 'el titulo' } })
    expect(w.attributes('aria-label')).toBe('Copiar el titulo')
    await w.trigger('click')
    await flushPromises()
    expect(written).toEqual(['Mi Gozo'])
    expect(w.emitted('copied')[0]).toEqual([true, 'el titulo'])
    // el portapapeles no se ve: el icono cambia un momento para decirlo
    expect(w.classes()).toContain('done')
    expect(w.attributes('title')).toBe('Copiado')
  })

  it('donde el navegador niega el portapapeles, se copia por la otra via', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: async () => Promise.reject(new Error('sin permiso')) }
    })
    document.execCommand = vi.fn(() => true)
    const w = mount(CopyButton, { props: { text: 'G D Em C' } })
    await w.trigger('click')
    await flushPromises()
    expect(document.execCommand).toHaveBeenCalledWith('copy')
    expect(w.emitted('copied')[0][0]).toBe(true)
    delete document.execCommand
  })

  it('no aparece si no hay nada que copiar', () => {
    expect(
      mount(CopyButton, { props: { text: '   ' } })
        .find('button')
        .exists()
    ).toBe(false)
    expect(
      mount(CopyButton, { props: { text: '' } })
        .find('button')
        .exists()
    ).toBe(false)
  })

  it('está donde hay texto que copiar: título, artista, letra y acordes', () => {
    const chords = JSON.stringify({
      sheet: {
        source: 'LaCuerda',
        url: 'https://lc.example/x',
        key: 'G',
        sections: [{ name: '', chords: ['G', 'D'], lines: [] }]
      }
    })
    const w = mount(DetailsPanel, {
      props: { song: song(1, { lyrics: 'Mi gozo', chords }), aiReady: true }
    })
    const what = w.findAll('.copy-btn').map((b) => b.attributes('aria-label'))
    for (const q of ['el título', 'el artista', 'la letra', 'los acordes']) {
      expect(what, `no se puede copiar ${q}`).toContain('Copiar ' + q)
    }
  })
})

// ---------------------------------------------------------------------------
// Cruces entre archivos. Estas cuatro siguen leyendo el código porque lo que
// comprueban no se ve montando nada: una prop mal escrita se ignora en
// silencio (no hay error que atrapar), y las otras tres cruzan todas las
// plantillas con la hoja de estilos o con una norma de la casa.
describe('cruces entre las plantillas y el resto', () => {
  it('cada transicion usada en las plantillas existe en el css', () => {
    const used = new Set()
    for (const { code } of components()) {
      for (const m of code.matchAll(/<transition(?:-group)?[^>]*\bname="([\w-]+)"/g)) {
        used.add(m[1])
      }
    }
    expect(used.size).toBeGreaterThan(0)
    const missing = [...used].filter((n) => !CSS.includes(`.${n}-enter-active`))
    expect(missing, 'transiciones nombradas en las plantillas y sin css').toEqual([])
  })

  it('todos los campos de texto se pueden vaciar', () => {
    // `clearable` viene puesto; lo que se revisa es que nadie lo apague
    const notClearable = components()
      .filter(({ code }) => /:clearable="false"/.test(code))
      .map(({ file }) => file)
    expect(notClearable, 'campos sin forma de vaciarlos').toEqual([])
  })

  it('ningun modificador de boton trae un alto relativo a la ventana', () => {
    // `.mini` era a la vez el modificador de los botones pequeños («btn mini»)
    // y la raiz del mini reproductor, que pone height:100vh. Las dos reglas
    // pesan igual, asi que ganaba la ultima del archivo: los veintitantos
    // botones pequeños de la app se volvian columnas de la altura de la
    // pantalla.
    const modifiers = new Set()
    for (const { code } of components()) {
      for (const m of code.matchAll(/class="([^"]*\bbtn\b[^"]*)"/g)) {
        for (const c of m[1].split(/\s+/)) if (c && c !== 'btn') modifiers.add(c)
      }
    }
    expect(modifiers.size).toBeGreaterThan(0)
    const clashes = []
    for (const c of modifiers) {
      // una regla que empieza por `.clase{` o `.clase ,` : sin `.btn` delante
      const loose = new RegExp(`(^|[},])\\s*\\.${c}\\s*\\{([^}]*)\\}`, 'm')
      const m = CSS.match(loose)
      // Un alto fijo en px es normal en un boton cuadrado (.icon-btn y
      // compañia). Lo que no puede estar bien nunca es un alto relativo a la
      // ventana o al padre: eso delata que la clase es en realidad la raiz de
      // otra pantalla y se esta colando en los botones por compartir nombre.
      if (m && /(^|;)\s*height\s*:\s*[\d.]+(vh|vw|%)/.test(m[2])) {
        clashes.push(`.${c} -> ${m[2].trim().slice(0, 60)}`)
      }
    }
    expect(clashes, 'reglas sueltas que se cuelan en todos los .btn con ese modificador').toEqual(
      []
    )
  })

  describe('props que se pasan a los componentes', () => {
    // `limpiable` en vez de `clearable` hacia que el dialogo enseñara la «x» de
    // limpiar donde no debia, y nada avisaba: una prop que no existe se ignora
    // en silencio. Esto compara lo que se pasa con lo que cada componente
    // declara de verdad.
    const NATIVE = new Set([
      'class',
      'style',
      'ref',
      'key',
      'id',
      'title',
      'role',
      'tabindex',
      'type',
      'name',
      'value',
      'placeholder',
      'disabled',
      'is'
    ])

    /** «icon-size» y «iconSize» son la misma prop. */
    const camel = (s) => s.replace(/-(\w)/g, (_, c) => c.toUpperCase())

    /**
     * Claves del primer nivel de un objeto literal. Hay que contar llaves: si
     * se buscan «palabra:» a secas tambien salen `type` y `default` de los
     * descriptores de dentro.
     */
    function topLevelKeys(text) {
      const keys = new Set()
      let depth = 0
      for (const t of text.matchAll(/[{}[\]]|(\w+)\s*:/g)) {
        if (t[0] === '{' || t[0] === '[') depth++
        else if (t[0] === '}' || t[0] === ']') depth--
        else if (depth === 0 && t[1]) keys.add(t[1])
      }
      return keys
    }

    /** Nombre del componente -> props que declara. */
    function declared() {
      const byName = new Map()
      for (const { file, code } of components()) {
        const name = file.split('/').pop().replace('.vue', '')
        const props = new Set()
        const obj = code.match(/defineProps\(\{([\s\S]*?)\n\}\)/)
        if (obj) for (const k of topLevelKeys(obj[1])) props.add(k)
        // `defineModel()` declara `modelValue`; `defineModel('x')`, `x`
        for (const m of code.matchAll(/defineModel\(\s*(?:'(\w+)')?/g)) {
          props.add(m[1] || 'modelValue')
        }
        if (props.size) byName.set(name, props)
      }
      return byName
    }

    it('ninguna plantilla pasa una prop que el componente no declara', () => {
      const byName = declared()
      expect(byName.size).toBeGreaterThan(5)
      const bad = []
      for (const { file, code } of components()) {
        for (const usage of code.matchAll(/<([A-Z]\w+)([^>]*?)\/?>/g)) {
          const props = byName.get(usage[1])
          if (!props) continue
          // Se vacian los valores entrecomillados antes de buscar nombres: si
          // no, un manejador como @enter="save(x); x=''" parece traer una prop
          // llamada `x`.
          const clean = usage[2].replace(/"[^"]*"/g, '""').replace(/'[^']*'/g, "''")
          for (const attr of clean.matchAll(/(?:^|\s)(:?)([\w-]+)\s*=/g)) {
            const name = attr[2]
            if (
              name.startsWith('v-') ||
              name.startsWith('@') ||
              name.startsWith('data-') ||
              name.startsWith('aria-')
            )
              continue
            if (NATIVE.has(name)) continue
            if (!props.has(camel(name))) bad.push(`${file}: <${usage[1]} ${attr[1]}${name}=…>`)
          }
        }
      }
      expect(bad, 'props inexistentes (se ignoran en silencio)').toEqual([])
    })
  })
})
