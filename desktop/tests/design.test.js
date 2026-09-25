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
function componentes(dir = SRC, out = []) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    if (e.isDirectory()) componentes(p, out)
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
  const VALIDAS = new Set(Object.keys(CATALOG.night.v))

  it('el catalogo define las mismas claves en todos los temas', () => {
    for (const [nombre, t] of Object.entries(CATALOG)) {
      expect(new Set(Object.keys(t.v)), `el tema «${nombre}» no tiene las mismas claves`).toEqual(
        VALIDAS
      )
    }
  })

  // que ningun componente lea un color que no existe se mira donde se
  // pintan: las tarjetas de Ajustes (settings.test.js), el editor de temas
  // (theme-editor.test.js) y el selector de tema del menu de Vista (abajo)

  it('cada clave de color es tambien una variable del css', () => {
    // applyTheme escribe --<clave>, asi que el css tiene que usarlas
    const faltan = [...VALIDAS].filter((k) => !CSS.includes(`var(--${k})`))
    expect(faltan, 'claves de tema que el css no usa nunca').toEqual([])
  })

  it('el tipo de tema se enseña traducido, no en ingles', () => {
    for (const t of Object.values(CATALOG)) {
      expect(KIND_LABEL[t.kind], `falta como se dice «${t.kind}»`).toBeTruthy()
    }
  })
})

describe('clases que la interfaz pinta', () => {
  /** ¿El css define esta clase en alguna parte? */
  const definida = (clase) => new RegExp(`\\.${clase}[\\s,{:.>+~]`).test(CSS)

  it('los avisos flotantes tienen estilo (y los pinta la app: app.test.js)', () => {
    // Antes emitia notice-ok / notice-ambar y el css definia hint-ok /
    // hint-amber: los avisos salian siempre grises.
    for (const c of ['toast', 'hint-ok', 'hint-amber']) {
      expect(definida(c), `el css no define .${c}`).toBe(true)
    }
  })

  it('la flecha de los grupos tiene estilo para girar', () => {
    expect(definida('group-chevron')).toBe(true)
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
    const bloque = CSS.slice(CSS.lastIndexOf('prefers-reduced-motion'))
    expect(bloque, 'falta la regla global').toMatch(/\*,\s*\*::before,\s*\*::after/)
    // ...pero los indicadores de «esto sigue en marcha» no se congelan
    expect(bloque).toMatch(/\.spinner[^{]*\{[^}]*infinite/)
  })
})

describe('el desplegable', () => {
  const temas = () =>
    Object.entries(CATALOG).map(([k, t]) => ({
      v: k,
      n: t.name,
      note: KIND_LABEL[t.kind],
      color: t.v.accent
    }))

  it('enseña sus opciones con el punto de color de cada una', async () => {
    const w = mount(SelectField, {
      props: { modelValue: 'night', label: 'Tema', options: temas() }
    })
    await w.find('.select-box').trigger('click')
    await flushPromises()
    expect(w.findAll('.select-opt').length).toBe(Object.keys(CATALOG).length)
    // El punto de color tiene que tener un color de VERDAD. Antes se leia
    // `t.v.acento`, que ya no existe, y salia `background: undefined`.
    // jsdom normaliza el hex a rgb(), asi que vale cualquiera de las dos.
    for (const punto of w.findAll('.select-menu .select-dot')) {
      const estilo = punto.attributes('style') || ''
      expect(estilo).not.toContain('undefined')
      expect(estilo, `color no valido: ${estilo}`).toMatch(/background:\s*(#[0-9a-f]{3,8}|rgb)/i)
    }
  })

  it('al abrirse lleva a la vista la opcion elegida', async () => {
    const vista = vi.fn()
    Element.prototype.scrollIntoView = vista
    const w = mount(SelectField, { props: { modelValue: 'wine', options: temas() } })
    await w.find('.select-box').trigger('click')
    await flushPromises()
    expect(vista).toHaveBeenCalled()
    expect(vista.mock.contexts[0].textContent).toContain(CATALOG.wine.name)
    delete Element.prototype.scrollIntoView
  })

  it('se puede quedar sin elegir: la «x» vuelve al valor vacio', async () => {
    const opciones = [
      { v: '', n: 'Cualquiera' },
      { v: 'Barak', n: 'Barak' }
    ]
    const vacio = mount(SelectField, {
      props: { modelValue: '', options: opciones, clearable: true }
    })
    expect(vacio.find('.select-clear').exists(), 'sin nada elegido no hay nada que quitar').toBe(
      false
    )
    const w = mount(SelectField, {
      props: { modelValue: 'Barak', options: opciones, clearable: true }
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
    const regla = CSS.match(/\.col-stars\{([^}]*)\}/)
    expect(regla, 'no hay regla para la columna de estrellas').toBeTruthy()
    const ancho = Number(regla[1].match(/width:\s*(\d+)px/)?.[1])
    const hueco = Number(CSS.match(/\.stars\{[^}]*gap:\s*([\d.]+)px/)?.[1] || 2.5)
    const relleno = 20 // 10px a cada lado en `td`
    const necesita = 5 * 15 + 4 * hueco + relleno
    expect(ancho, `necesita al menos ${necesita}px`).toBeGreaterThanOrEqual(necesita)
  })

  it('la columna de estrellas se define una sola vez fuera de media queries', () => {
    // habia dos reglas, 82px y 104px, y la de abajo pisaba a la de arriba
    const sinMedia = CSS.replace(/@media[^{]*\{(?:[^{}]*\{[^}]*\})*[^}]*\}/g, '')
    expect((sinMedia.match(/\.col-stars\s*\{/g) || []).length).toBe(1)
  })
})

describe('arrastrar una cancion', () => {
  // En el WebView de escritorio, arrastrar una fila iba dejando media lista
  // seleccionada en azul. El `user-select:none` del css no basta ahi, asi que
  // se corta el evento que inicia la seleccion.
  it('mientras se arrastra no se puede empezar a seleccionar texto', () => {
    const { startDrag } = useDragSong()
    const fila = document.createElement('div')
    document.body.appendChild(fila)
    startDrag(
      { id: 1, title: 'Mi Gozo' },
      { pointerType: 'mouse', button: 0, clientX: 10, clientY: 10, target: fila }
    )
    const seleccion = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(seleccion)
    expect(seleccion.defaultPrevented, 'deja que el motor empiece a seleccionar').toBe(true)
    // y tampoco el arrastre nativo del texto, que pinta su propio fantasma
    const nativo = new window.Event('dragstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(nativo)
    expect(nativo.defaultPrevented).toBe(true)
  })

  it('al soltar deja de estorbar a la seleccion normal', () => {
    const { startDrag } = useDragSong()
    startDrag(
      { id: 1 },
      { pointerType: 'mouse', button: 0, clientX: 0, clientY: 0, target: document.body }
    )
    cancelDrag()
    const seleccion = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(seleccion)
    expect(seleccion.defaultPrevented, 'sigue bloqueando la seleccion sin arrastrar nada').toBe(
      false
    )
  })

  it('con el dedo no se arrastra: eso es desplazar la lista', () => {
    const { startDrag } = useDragSong()
    startDrag(
      { id: 1 },
      { pointerType: 'touch', button: 0, clientX: 0, clientY: 0, target: document.body }
    )
    const seleccion = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(seleccion)
    expect(seleccion.defaultPrevented, 'el toque no deberia armar un arrastre').toBe(false)
  })
})

describe('portadas difuminadas', () => {
  it('la portada se pinta borrosa cuando la cancion esta marcada, y lo dice', () => {
    expect(
      mount(CoverArt, { props: { id: 1 } })
        .find('img')
        .classes()
    ).not.toContain('blurred')
    const borrosa = mount(CoverArt, { props: { id: 1, blur: true, alt: 'Mi Gozo' } })
    expect(borrosa.find('img').classes(), 'no se marca como difuminada').toContain('blurred')
    // quien lee la portada se entera de que esta difuminada
    expect(borrosa.find('img').attributes('alt')).toContain('difuminada')
  })

  it('el css la difumina de verdad, no le baja la opacidad', () => {
    const regla = CSS.match(/\.cover-art\.blurred\{([^}]*)\}/)
    expect(regla, 'no hay estilo para las portadas difuminadas').toBeTruthy()
    expect(regla[1], 'deberia usar blur()').toMatch(/filter:\s*blur\(\s*\d+px/)
    // Bajar la opacidad dejaria la imagen reconocible, que es justo lo que se
    // quiere evitar.
    expect(regla[1]).not.toMatch(/(^|;)\s*opacity:/)
    // Y nada de agrandarla: se saldria de su caja y taparia lo de al lado.
    expect(regla[1]).not.toMatch(/transform:\s*scale/)
  })

  it('todas las listas y la ficha la respetan', async () => {
    // si una se olvida, la imagen sigue viendose donde menos se espera (el
    // reproductor y la ventanita, en sus propias pruebas; el menu que la
    // activa, en app.test.js)
    const borrosa = song(1, { blur: 1 })
    for (const [nombre, w] of [
      ['las fichas', mount(SongCards, { props: { songs: [borrosa] } })],
      ['la cuadricula', mount(SongGrid, { props: { songs: [borrosa] } })],
      [
        'los resultados del buscador',
        mount(SearchResults, { props: { songs: [borrosa], query: 'x' } })
      ],
      ['la ficha', mount(DetailsPanel, { props: { song: borrosa, aiReady: false } })]
    ]) {
      await flushPromises()
      expect(w.find('.cover-art').classes(), `${nombre} no la difumina`).toContain('blurred')
    }
  })
})

describe('campos de formulario', () => {
  it('el input de dentro no pinta su propio anillo de foco', () => {
    // La regla base `input:focus` le ponia su sombra al input, y el envoltorio
    // ponia la suya: se veian DOS anillos verdes, uno dentro del otro, con un
    // hueco oscuro en medio.
    const regla = CSS.match(/\.field-box input[^{]*\{[^}]*box-shadow:\s*none[^}]*\}/)
    expect(regla, 'el input de un campo deberia anular su sombra de foco').toBeTruthy()
    for (const sel of ['.slider-track input', '.color-swatch input']) {
      const clase = sel.split(' ')[0].slice(1)
      expect(CSS, `${sel} tambien la hereda`).toMatch(
        new RegExp(`\\.${clase}[^{]*input[^{]*\\{[^}]*box-shadow:\\s*none`)
      )
    }
  })

  it('solo hay un anillo por campo, el del envoltorio', () => {
    const envoltorio = CSS.match(/\.field\.focused \.field-box\{([^}]*)\}/)
    expect(envoltorio, 'el campo enfocado deberia tener su sombra').toBeTruthy()
    expect(envoltorio[1]).toMatch(/box-shadow:/)
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
  const grupo = {
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
    const w = mount(DuplicateGroup, { props: { group: grupo, playing: null, busy: false } })
    expect(w.find('.dup-stats').text().replace(/\s+/g, ' ')).toBe('320 kbps · 3:21 · 6.7 MB')
    expect(CSS).toMatch(/\.dup-stats\{[^}]*white-space:\s*nowrap/)
  })

  it('lee las mismas claves que manda el nucleo', () => {
    // `relative`, `has_suffix` y `suggested`: con los nombres viejos en
    // castellano no salia ni la ruta ni las marcas
    const w = mount(DuplicateGroup, { props: { group: grupo, playing: null, busy: false } })
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
    for (const [viejo, nuevo] of [
      ['list', 'table'],
      ['compact', 'rows']
    ]) {
      localStorage.setItem('danplay.layout', viejo)
      expect(savedLayout(), `«${viejo}» deberia traducirse a «${nuevo}»`).toBe(nuevo)
    }
    // y una vista que no existe no deja la app en blanco
    localStorage.setItem('danplay.layout', '{"topbar":"bottom"}')
    expect(savedLayout()).toBe('table')
  })

  it('las cuatro tambien valen agrupadas, con sus cabeceras de grupo', () => {
    const songs = [song(1), song(2, { artist: 'New Wine' }), song(3)]
    for (const [layout, caja] of [
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
      expect(w.find(caja).exists(), `agrupado no sabe pintar «${layout}»`).toBe(true)
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
    const reglas = [...CSS.matchAll(/(^|\n)thead th\{([^}]*)\}/g)].map((m) => m[2])
    expect(reglas.length, 'thead th deberia definirse una sola vez').toBe(1)
    expect(reglas[0]).toMatch(/position:\s*sticky/)
    expect(reglas[0], 'sin top no se pega a ningun sitio').toMatch(/top:\s*0/)
    expect(reglas[0], 'tiene que quedar por encima de las filas').toMatch(/z-index/)
    expect(reglas[0], 'sin fondo se ven las filas por debajo').toMatch(/background:/)
  })

  it('solo las columnas que ordenan parecen pulsables', () => {
    expect(CSS).toMatch(/thead th\.sortable\{[^}]*cursor:\s*pointer/)
  })
})

describe('ordenar por una columna', () => {
  it('la cabecera enseña por donde va el orden y hacia donde', async () => {
    const w = mount(SongTable, { props: { songs: [song(1)], sort: 'title', desc: false } })
    const titulo = w.find('th[data-col="title"]')
    expect(titulo.attributes('aria-sort')).toBe('ascending')
    expect(titulo.find('.th-arrow').classes()).toContain('up')
    expect(w.find('th[data-col="artist"]').attributes('aria-sort')).toBeUndefined()
    await w.setProps({ desc: true })
    expect(titulo.attributes('aria-sort')).toBe('descending')
    expect(titulo.find('.th-arrow').classes()).not.toContain('up')
    expect(CSS, 'la flecha no se da la vuelta').toMatch(/\.th-arrow\.up\{[^}]*rotate\(180deg\)/)
  })

  it('cada columna ordena por un campo que el nucleo conoce, empezando por lo natural', async () => {
    // los nombres salen del nucleo (SORT_FIELDS); si aqui se inventa uno, la
    // API lo ignora y la lista no cambia. (Que pulsar dos veces la misma
    // invierte el orden, en app.test.js.)
    const w = mount(SongTable, { props: { songs: [song(1)] } })
    for (const b of w.findAll('thead .th-sort')) await b.trigger('click')
    const pedidos = w.emitted('sortBy')
    expect(pedidos.length).toBeGreaterThan(4)
    const core = coreSource()
    for (const [campo] of pedidos) {
      expect(core, `el nucleo no sabe ordenar por «${campo}»`).toMatch(
        new RegExp(`"${campo}":\\s*\\(`)
      )
    }
    // por texto se espera de la A a la Z; por duracion, lo mas largo primero
    expect(Object.fromEntries(pedidos)).toMatchObject({ title: false, duration: true, stars: true })
  })
})

describe('columnas ajustables', () => {
  /** jsdom no maqueta: cada cabecera dice que mide 100 px. */
  const conAnchos = () =>
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
      const w = this.tagName === 'TH' ? 100 : 0
      return { width: w, height: 20, top: 0, left: 0, right: w, bottom: 20, x: 0, y: 0 }
    })
  const puntero = (target, tipo, x) =>
    target.dispatchEvent(new MouseEvent(tipo, { bubbles: true, cancelable: true, clientX: x }))

  it('sin tocarlas no se fija ningun ancho: la tabla se reparte sola', () => {
    const w = mount(SongTable, { props: { songs: [song(1)] } })
    expect(w.find('colgroup').exists()).toBe(false)
    expect(w.find('table').classes()).not.toContain('medida')
  })

  it('se ensanchan arrastrando el borde, se recuerdan y el doble clic las olvida', async () => {
    conAnchos()
    const w = mount(SongTable, { props: { songs: [song(1)] }, attachTo: document.body })
    const borde = w.find('th[data-col="title"] .col-resize').element
    puntero(borde, 'pointerdown', 100)
    puntero(window, 'pointermove', 160)
    puntero(window, 'pointerup', 160)
    await flushPromises()
    expect(JSON.parse(localStorage.getItem('danplay.colWidths')).title).toBe(160)
    expect(w.find('col:nth-child(3)').attributes('style')).toContain('width: 160px')
    // al volver, siguen como se dejaron
    const otra = mount(SongTable, { props: { songs: [song(1)] } })
    expect(otra.find('colgroup').exists()).toBe(true)
    // doble clic en el borde: vuelve el reparto de siempre
    await w.find('th[data-col="title"] .col-resize').trigger('dblclick')
    expect(w.find('colgroup').exists()).toBe(false)
    expect(localStorage.getItem('danplay.colWidths')).toBeNull()
  })

  it('con el teclado, las flechas lo ensanchan de a poco', async () => {
    conAnchos()
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
  const ocho = () => Array.from({ length: 8 }, (_, i) => song(i + 1))

  it('se ven cinco y el resto con la rueda, midiendo una fila de verdad', async () => {
    // la altura sale de MEDIR una fila, no de un numero a ojo: asi sigue
    // cuadrando aunque cambie la densidad o el tamaño de la app
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function () {
      const h = this.classList.contains('sr-row') ? 41 : 0
      return { height: h, width: 300, top: 0, left: 0, right: 300, bottom: h, x: 0, y: 0 }
    })
    const w = mount(SearchResults, { props: { songs: ocho(), query: 'x' } })
    await flushPromises()
    expect(w.find('.sr-list').attributes('style')).toContain('max-height: 205px')
    // con pocas no hace falta rueda
    await w.setProps({ songs: ocho().slice(0, 3) })
    await flushPromises()
    expect(w.find('.sr-list').attributes('style')).toBeUndefined()
    expect(CSS).toMatch(/\.sr-list\{[^}]*overflow-y:auto/)
  })

  it('se manejan con el teclado desde la caja de busqueda', async () => {
    const w = mount(SearchResults, { props: { songs: ocho(), query: 'x' } })
    const tecla = (key) => w.vm.onKey(new KeyboardEvent('keydown', { key, cancelable: true }))
    expect(tecla('ArrowDown')).toBe(true)
    expect(tecla('ArrowDown')).toBe(true)
    expect(tecla('ArrowUp')).toBe(true)
    await flushPromises()
    expect(w.find('.sr-row.on').text()).toContain('Cancion 2')
    // la caja sabe cual esta resaltada, para quien no ve la pantalla
    expect(document.getElementById !== undefined && w.vm.activeId).toBe(
      w.find('.sr-row.on').attributes('id')
    )
    expect(tecla('Enter')).toBe(true)
    expect(w.emitted('pick')[0][0].id).toBe(2)
    expect(tecla('Escape')).toBe(true)
    expect(w.emitted('close')).toBeTruthy()
    expect(tecla('a')).toBe(false)
  })

  it('se pueden reproducir, no solo mirar', async () => {
    const w = mount(SearchResults, { props: { songs: ocho(), query: 'x' } })
    await w.findAll('.sr-play')[3].trigger('click')
    expect(w.emitted('play')[0][0].id).toBe(4)
    expect(w.emitted('pick'), 'reproducir no es elegir').toBeFalsy()
  })

  it('se pueden arrastrar a un repertorio', async () => {
    // una fila-boton nunca podria arrastrarse: el arrastre se aparta de los
    // botones (ahi el clic tiene otra cosa que hacer)
    const w = mount(SearchResults, {
      props: { songs: ocho(), query: 'x' },
      attachTo: document.body
    })
    const fila = w.findAll('.sr-row')[1].element
    expect(fila.tagName).not.toBe('BUTTON')
    fila.dispatchEvent(
      new MouseEvent('pointerdown', { bubbles: true, button: 0, clientX: 5, clientY: 5 })
    )
    window.dispatchEvent(new MouseEvent('pointermove', { bubbles: true, clientX: 60, clientY: 90 }))
    expect(useDragSong().drag.song?.id).toBe(2)
    // y mientras se arrastra, el desplegable no tapa el lateral
    expect(CSS).toMatch(/body\.dragging-song \.search-results\{[^}]*pointer-events:none/)
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
  const montar = (query = '') => mount(SearchPanel, { props: { query, facets } })
  /** El campo con esa etiqueta. */
  const campo = (w, etiqueta) =>
    w
      .findAll('.field')
      .find((f) => f.find('.field-label').exists() && f.find('.field-label').text() === etiqueta)

  it('ofrece filtrar, comparar y ordenar', () => {
    const w = montar()
    const etiquetas = w.findAll('.field-label').map((l) => l.text())
    for (const e of ['Texto', 'Artista', 'Album', 'Genero', 'Tono', 'Carpeta', 'Por']) {
      expect(etiquetas, `falta «${e}»`).toContain(e)
    }
    for (const e of ['BPM', 'Duración (s)', 'Calidad (bps)', 'Año']) {
      expect(etiquetas, `falta el rango de «${e}»`).toContain(e)
    }
  })

  it('escribe en la misma caja que se escribe a mano, sin perder lo que habia', async () => {
    // asi no hay dos estados que puedan discrepar, y de paso se aprende la
    // sintaxis viendo lo que aparece escrito
    const w = montar('mi gozo bpm>100')
    const artista = campo(w, 'Artista')
    await artista.find('.select-box').trigger('click')
    await artista
      .findAll('.select-opt')
      .find((o) => o.text().includes('New Wine'))
      .trigger('click')
    expect(w.emitted('update:query').at(-1)).toEqual(['mi gozo artista:NewWine bpm>100'])
    // y lee lo ya escrito: el rango aparece en su casilla
    const desde = w.findAll('.rango')[0].findAll('input')[0]
    expect(desde.element.value).toBe('100')
    await desde.setValue('90')
    expect(w.emitted('update:query').at(-1)).toEqual(['mi gozo bpm>90'])
  })

  it('cada orden que ofrece es uno que el nucleo sabe hacer', async () => {
    const w = montar()
    const orden = campo(w, 'Por')
    await orden.find('.select-box').trigger('click')
    const nombres = orden.findAll('.select-opt').map((o) => o.text())
    expect(nombres.length).toBeGreaterThan(8)
    // el menu ya esta abierto: se elige la primera y luego se abre para cada una
    for (let i = 0; i < nombres.length; i++) {
      if (i) await orden.find('.select-box').trigger('click')
      await orden.findAll('.select-opt')[i].trigger('click')
    }
    const campos = w.emitted('sort').map(([campo]) => campo)
    const core = coreSource()
    for (const campo of campos) {
      expect(core, `el nucleo no sabe ordenar por «${campo}»`).toMatch(
        new RegExp(`"${campo}":\\s*\\(`)
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
    const escrito = []
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: async (t) => escrito.push(t) }
    })
    const w = mount(CopyButton, { props: { text: '  Mi Gozo ', what: 'el titulo' } })
    expect(w.attributes('aria-label')).toBe('Copiar el titulo')
    await w.trigger('click')
    await flushPromises()
    expect(escrito).toEqual(['Mi Gozo'])
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
    const chords = JSON.stringify({ progression: 'G D Em C', confidence: 0.9 })
    const w = mount(DetailsPanel, {
      props: { song: song(1, { lyrics: 'Mi gozo', chords }), aiReady: true }
    })
    const que = w.findAll('.copy-btn').map((b) => b.attributes('aria-label'))
    for (const q of ['el título', 'el artista', 'la letra', 'los acordes']) {
      expect(que, `no se puede copiar ${q}`).toContain('Copiar ' + q)
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
    const usadas = new Set()
    for (const { code } of componentes()) {
      for (const m of code.matchAll(/<transition(?:-group)?[^>]*\bname="([\w-]+)"/g)) {
        usadas.add(m[1])
      }
    }
    expect(usadas.size).toBeGreaterThan(0)
    const faltan = [...usadas].filter((n) => !CSS.includes(`.${n}-enter-active`))
    expect(faltan, 'transiciones nombradas en las plantillas y sin css').toEqual([])
  })

  it('todos los campos de texto se pueden vaciar', () => {
    // `clearable` viene puesto; lo que se revisa es que nadie lo apague
    const apagados = componentes()
      .filter(({ code }) => /:clearable="false"/.test(code))
      .map(({ file }) => file)
    expect(apagados, 'campos sin forma de vaciarlos').toEqual([])
  })

  it('ningun modificador de boton trae un alto relativo a la ventana', () => {
    // `.mini` era a la vez el modificador de los botones pequeños («btn mini»)
    // y la raiz del mini reproductor, que pone height:100vh. Las dos reglas
    // pesan igual, asi que ganaba la ultima del archivo: los veintitantos
    // botones pequeños de la app se volvian columnas de la altura de la
    // pantalla.
    const modificadores = new Set()
    for (const { code } of componentes()) {
      for (const m of code.matchAll(/class="([^"]*\bbtn\b[^"]*)"/g)) {
        for (const c of m[1].split(/\s+/)) if (c && c !== 'btn') modificadores.add(c)
      }
    }
    expect(modificadores.size).toBeGreaterThan(0)
    const chocan = []
    for (const c of modificadores) {
      // una regla que empieza por `.clase{` o `.clase ,` : sin `.btn` delante
      const suelta = new RegExp(`(^|[},])\\s*\\.${c}\\s*\\{([^}]*)\\}`, 'm')
      const m = CSS.match(suelta)
      // Un alto fijo en px es normal en un boton cuadrado (.icon-btn y
      // compañia). Lo que no puede estar bien nunca es un alto relativo a la
      // ventana o al padre: eso delata que la clase es en realidad la raiz de
      // otra pantalla y se esta colando en los botones por compartir nombre.
      if (m && /(^|;)\s*height\s*:\s*[\d.]+(vh|vw|%)/.test(m[2])) {
        chocan.push(`.${c} -> ${m[2].trim().slice(0, 60)}`)
      }
    }
    expect(chocan, 'reglas sueltas que se cuelan en todos los .btn con ese modificador').toEqual([])
  })

  describe('props que se pasan a los componentes', () => {
    // `limpiable` en vez de `clearable` hacia que el dialogo enseñara la «x» de
    // limpiar donde no debia, y nada avisaba: una prop que no existe se ignora
    // en silencio. Esto compara lo que se pasa con lo que cada componente
    // declara de verdad.
    const NATIVAS = new Set([
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
    function clavesDePrimerNivel(texto) {
      const claves = new Set()
      let hondo = 0
      for (const t of texto.matchAll(/[{}[\]]|(\w+)\s*:/g)) {
        if (t[0] === '{' || t[0] === '[') hondo++
        else if (t[0] === '}' || t[0] === ']') hondo--
        else if (hondo === 0 && t[1]) claves.add(t[1])
      }
      return claves
    }

    /** Nombre del componente -> props que declara. */
    function declaradas() {
      const mapa = new Map()
      for (const { file, code } of componentes()) {
        const nombre = file.split('/').pop().replace('.vue', '')
        const props = new Set()
        const obj = code.match(/defineProps\(\{([\s\S]*?)\n\}\)/)
        if (obj) for (const k of clavesDePrimerNivel(obj[1])) props.add(k)
        // `defineModel()` declara `modelValue`; `defineModel('x')`, `x`
        for (const m of code.matchAll(/defineModel\(\s*(?:'(\w+)')?/g)) {
          props.add(m[1] || 'modelValue')
        }
        if (props.size) mapa.set(nombre, props)
      }
      return mapa
    }

    it('ninguna plantilla pasa una prop que el componente no declara', () => {
      const mapa = declaradas()
      expect(mapa.size).toBeGreaterThan(5)
      const malos = []
      for (const { file, code } of componentes()) {
        for (const uso of code.matchAll(/<([A-Z]\w+)([^>]*?)\/?>/g)) {
          const props = mapa.get(uso[1])
          if (!props) continue
          // Se vacian los valores entrecomillados antes de buscar nombres: si
          // no, un manejador como @enter="save(x); x=''" parece traer una prop
          // llamada `x`.
          const limpio = uso[2].replace(/"[^"]*"/g, '""').replace(/'[^']*'/g, "''")
          for (const attr of limpio.matchAll(/(?:^|\s)(:?)([\w-]+)\s*=/g)) {
            const nombre = attr[2]
            if (
              nombre.startsWith('v-') ||
              nombre.startsWith('@') ||
              nombre.startsWith('data-') ||
              nombre.startsWith('aria-')
            )
              continue
            if (NATIVAS.has(nombre)) continue
            if (!props.has(camel(nombre))) malos.push(`${file}: <${uso[1]} ${attr[1]}${nombre}=…>`)
          }
        }
      }
      expect(malos, 'props inexistentes (se ignoran en silencio)').toEqual([])
    })
  })
})
