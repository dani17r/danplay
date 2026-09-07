// Guardias de diseño.
//
// Todos los fallos que cubre este archivo son el mismo: un nombre que dejo de
// existir al pasar el codigo a ingles y que nadie noto porque en JavaScript
// leer una propiedad que no esta no da error, simplemente vale `undefined`.
// El resultado eran bordes invisibles, puntos de color que no salian y avisos
// que nunca se coloreaban.
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { mount, flushPromises } from '@vue/test-utils'
import { CATALOG, KIND_LABEL } from '../src/themes.js'

const SRC = join(dirname(fileURLToPath(import.meta.url)), '..', 'src')
const CSS = readFileSync(join(SRC, 'style.css'), 'utf8')

/** Todos los .vue del proyecto, con su ruta y su contenido. */
function componentes (dir = SRC, out = []) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    if (e.isDirectory()) componentes(p, out)
    else if (e.name.endsWith('.vue')) out.push({ file: p.slice(SRC.length + 1), code: readFileSync(p, 'utf8') })
  }
  return out
}

describe('colores de tema', () => {
  const VALIDAS = new Set(Object.keys(CATALOG.night.v))

  it('el catalogo define las mismas claves en todos los temas', () => {
    for (const [nombre, t] of Object.entries(CATALOG)) {
      expect(new Set(Object.keys(t.v)), `el tema «${nombre}» no tiene las mismas claves`)
        .toEqual(VALIDAS)
    }
  })

  it('ningun componente lee un color de tema que no existe', () => {
    const malos = []
    for (const { file, code } of componentes()) {
      // t.v.algo / theme.v.algo : el objeto de colores de un tema
      for (const m of code.matchAll(/\b\w+\.v\.(\w+)/g)) {
        if (!VALIDAS.has(m[1])) malos.push(`${file}: .v.${m[1]}`)
      }
    }
    expect(malos, 'colores de tema inexistentes (dan undefined y el estilo se pierde)')
      .toEqual([])
  })

  it('cada clave de color es tambien una variable del css', () => {
    // applyTheme escribe --<clave>, asi que el css tiene que usarlas
    const faltan = [...VALIDAS].filter(k => !CSS.includes(`var(--${k})`))
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

  it('los avisos flotantes usan clases que el css conoce', async () => {
    // Antes emitia notice-ok / notice-ambar y el css definia hint-ok /
    // hint-amber: los avisos salian siempre grises.
    for (const c of ['toast', 'hint-ok', 'hint-amber']) {
      expect(definida(c), `el css no define .${c}`).toBe(true)
    }
    const App = (await import('../src/App.vue')).default
    expect(readFileSync(join(SRC, 'App.vue'), 'utf8')).toContain("'hint-ok'")
    expect(App).toBeTruthy()
  })

  it('la flecha de los grupos tiene estilo para girar', () => {
    expect(definida('group-chevron')).toBe(true)
    expect(CSS).toMatch(/\.group-chevron\.open\{[^}]*rotate/)
  })
})

describe('animaciones', () => {
  it('todo panel flotante entra con una transicion', () => {
    // el menu de vista aparecia de golpe mientras el resto se desvanecia
    const app = readFileSync(join(SRC, 'App.vue'), 'utf8')
    const menu = app.indexOf('class="card view-menu"')
    expect(menu).toBeGreaterThan(-1)
    expect(app.slice(Math.max(0, menu - 220), menu),
      'al menu de vista le falta su <transition>').toContain('<transition')
  })

  it('cada transicion usada en las plantillas existe en el css', () => {
    const usadas = new Set()
    for (const { code } of componentes()) {
      for (const m of code.matchAll(/<transition(?:-group)?[^>]*\bname="([\w-]+)"/g)) {
        usadas.add(m[1])
      }
    }
    expect(usadas.size).toBeGreaterThan(0)
    const faltan = [...usadas].filter(n => !CSS.includes(`.${n}-enter-active`))
    expect(faltan, 'transiciones nombradas en las plantillas y sin css').toEqual([])
  })

  it('los botones reaccionan al raton de forma suave', () => {
    expect(CSS).toMatch(/\.btn\{[^}]*transition:/)
  })

  it('quien pide movimiento reducido lo obtiene en TODA la app', () => {
    const bloque = CSS.slice(CSS.lastIndexOf('prefers-reduced-motion'))
    expect(bloque, 'falta la regla global').toMatch(/\*,\s*\*::before,\s*\*::after/)
    // ...pero los indicadores de «esto sigue en marcha» no se congelan
    expect(bloque).toMatch(/\.spinner\{[^}]*infinite/)
  })
})

describe('el menu de vista', () => {
  it('se abre al pulsar «Vista» y trae sus controles', async () => {
    const { default: SelectField } = await import('../src/components/ui/SelectField.vue')
    // se prueba el desplegable suelto: es la pieza que compone el menu
    const w = mount(SelectField, {
      props: {
        modelValue: 'night',
        label: 'Tema',
        options: Object.entries(CATALOG).map(([k, t]) =>
          ({ v: k, n: t.name, note: KIND_LABEL[t.kind], color: t.v.accent }))
      }
    })
    await w.find('.select-box').trigger('click')
    await flushPromises()
    const opciones = w.findAll('.select-opt')
    expect(opciones.length).toBe(Object.keys(CATALOG).length)
    // El punto de color tiene que tener un color de VERDAD. Antes se leia
    // `t.v.acento`, que ya no existe, y salia `background: undefined`.
    // jsdom normaliza el hex a rgb(), asi que vale cualquiera de las dos.
    const punto = w.find('.select-menu .select-dot')
    expect(punto.exists(), 'no se pinta el punto de color del tema').toBe(true)
    const estilo = punto.attributes('style') || ''
    expect(estilo).not.toContain('undefined')
    expect(estilo, `color no valido: ${estilo}`).toMatch(/background:\s*(#[0-9a-f]{3,8}|rgb)/i)
    w.unmount()
  })

  it('al abrirse busca la opcion marcada con la clase que de verdad existe', () => {
    const code = readFileSync(join(SRC, 'components/ui/SelectField.vue'), 'utf8')
    const m = code.match(/querySelector\('\.select-opt\.(\w+)'\)/)
    expect(m, 'ya no se busca la opcion actual').toBeTruthy()
    // la clase buscada tiene que ser una que la plantilla ponga de verdad
    expect(code).toMatch(new RegExp(`\\b${m[1]}:`))
  })
})

describe('props que se pasan a los componentes', () => {
  // `limpiable` en vez de `clearable` hacia que el dialogo enseñara la «x» de
  // limpiar donde no debia, y nada avisaba: una prop que no existe se ignora
  // en silencio. Esto compara lo que se pasa con lo que cada componente
  // declara de verdad.
  const NATIVAS = new Set(['class', 'style', 'ref', 'key', 'id', 'title', 'role',
    'tabindex', 'type', 'name', 'value', 'placeholder', 'disabled', 'is'])

  /** «icon-size» y «iconSize» son la misma prop. */
  const camel = (s) => s.replace(/-(\w)/g, (_, c) => c.toUpperCase())

  /**
   * Claves del primer nivel de un objeto literal. Hay que contar llaves: si
   * se buscan «palabra:» a secas tambien salen `type` y `default` de los
   * descriptores de dentro, y si solo se miran las de principio de linea se
   * pierden las que van agrupadas («label: String, width: String»).
   */
  function clavesDePrimerNivel (texto) {
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
  function declaradas () {
    const mapa = new Map()
    for (const { file, code } of componentes()) {
      const nombre = file.split('/').pop().replace('.vue', '')
      const props = new Set()
      const obj = code.match(/defineProps\(\{([\s\S]*?)\n\}\)/)
      if (obj) for (const k of clavesDePrimerNivel(obj[1])) props.add(k)
      const arr = code.match(/defineProps\(\[([\s\S]*?)\]\)/)
      if (arr) for (const m of arr[1].matchAll(/['"](\w+)['"]/g)) props.add(m[1])
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
          if (nombre.startsWith('v-') || nombre.startsWith('@') ||
              nombre.startsWith('data-') || nombre.startsWith('aria-')) continue
          if (NATIVAS.has(nombre)) continue
          if (!props.has(camel(nombre))) {
            malos.push(`${file}: <${uso[1]} ${attr[1]}${nombre}=…>`)
          }
        }
      }
    }
    expect(malos, 'props inexistentes (se ignoran en silencio)').toEqual([])
  })
})

describe('el dialogo se maneja con el teclado', () => {
  it('Escape cierra tambien una confirmacion, no solo los que piden texto', async () => {
    const { default: ModalDialog } = await import('../src/components/ui/ModalDialog.vue')
    const w = mount(ModalDialog, {
      props: { open: true, kind: 'confirm', title: 'Borrar', message: '¿Seguro?' },
      attachTo: document.body
    })
    await flushPromises()
    // la tecla llega al documento, que es donde cae el foco cuando no hay campo
    document.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(w.emitted('cancel'), 'Escape no cerro la confirmacion').toBeTruthy()
    w.unmount()
  })

  it('Enter acepta una confirmacion', async () => {
    const { default: ModalDialog } = await import('../src/components/ui/ModalDialog.vue')
    const w = mount(ModalDialog, {
      props: { open: true, kind: 'confirm', title: 'Borrar' },
      attachTo: document.body
    })
    await flushPromises()
    document.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Enter' }))
    await flushPromises()
    expect(w.emitted('ok')).toBeTruthy()
    w.unmount()
  })

  it('deja de escuchar el teclado al cerrarse', async () => {
    const { default: ModalDialog } = await import('../src/components/ui/ModalDialog.vue')
    const w = mount(ModalDialog, { props: { open: true, kind: 'confirm' }, attachTo: document.body })
    await flushPromises()
    await w.setProps({ open: false })
    await flushPromises()
    document.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(w.emitted('cancel'), 'sigue reaccionando con el dialogo cerrado').toBeFalsy()
    w.unmount()
  })
})

describe('nombres de clase que chocan', () => {
  // `.mini` era a la vez el modificador de los botones pequeños («btn mini»)
  // y la raiz del mini reproductor, que pone height:100vh. Las dos reglas
  // pesan igual, asi que ganaba la ultima del archivo: los veintitantos
  // botones pequeños de la app se volvian columnas de la altura de la
  // pantalla. Eso descolocaba el menu de vista, porque su boton estiraba el
  // bloque que le sirve de referencia y la barra lo centraba fuera de vista.
  it('ningun modificador de boton trae un alto relativo a la ventana', () => {
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
    expect(chocan, 'reglas sueltas que se cuelan en todos los .btn con ese modificador')
      .toEqual([])
  })

  it('el mini reproductor no se llama como un modificador de boton', () => {
    const mini = readFileSync(join(SRC, 'MiniPlayer.vue'), 'utf8')
    const raiz = mini.match(/<div class="([\w-]+)">/)
    expect(raiz, 'no encuentro la raiz del mini reproductor').toBeTruthy()
    expect(raiz[1]).not.toBe('mini')
    expect(CSS).toContain(`.${raiz[1]}{`)
  })
})

describe('la columna de estrellas cabe', () => {
  it('es lo bastante ancha para las cinco estrellas', () => {
    // cinco iconos de 15px + cuatro huecos + el relleno de la celda
    const regla = CSS.match(/\.col-stars\{([^}]*)\}/)
    expect(regla, 'no hay regla para la columna de estrellas').toBeTruthy()
    const ancho = Number(regla[1].match(/width:\s*(\d+)px/)?.[1])
    const hueco = Number(CSS.match(/\.stars\{[^}]*gap:\s*([\d.]+)px/)?.[1] || 2.5)
    const relleno = 20                          // 10px a cada lado en `td`
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
  it('mientras se arrastra no se puede empezar a seleccionar texto', async () => {
    const { startDrag, cancelDrag } = await import('../src/composables/useDragSong.js')
    cancelDrag()
    const cancion = { id: 1, title: 'Mi Gozo' }
    const fila = document.createElement('div')
    document.body.appendChild(fila)
    try {
      startDrag(cancion, { pointerType: 'mouse', button: 0, clientX: 10, clientY: 10, target: fila })

      const seleccion = new window.Event('selectstart', { cancelable: true, bubbles: true })
      document.dispatchEvent(seleccion)
      expect(seleccion.defaultPrevented, 'deja que el motor empiece a seleccionar').toBe(true)

      // y tampoco el arrastre nativo del texto, que pinta su propio fantasma
      const nativo = new window.Event('dragstart', { cancelable: true, bubbles: true })
      document.dispatchEvent(nativo)
      expect(nativo.defaultPrevented).toBe(true)
    } finally {
      cancelDrag()
      fila.remove()
    }
  })

  it('al soltar deja de estorbar a la seleccion normal', async () => {
    const { startDrag, cancelDrag } = await import('../src/composables/useDragSong.js')
    startDrag({ id: 1 }, { pointerType: 'mouse', button: 0, clientX: 0, clientY: 0, target: document.body })
    cancelDrag()
    const seleccion = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(seleccion)
    expect(seleccion.defaultPrevented, 'sigue bloqueando la seleccion sin arrastrar nada').toBe(false)
  })

  it('con el dedo no se arrastra: eso es desplazar la lista', async () => {
    const { startDrag, cancelDrag } = await import('../src/composables/useDragSong.js')
    cancelDrag()
    startDrag({ id: 1 }, { pointerType: 'touch', button: 0, clientX: 0, clientY: 0, target: document.body })
    const seleccion = new window.Event('selectstart', { cancelable: true, bubbles: true })
    document.dispatchEvent(seleccion)
    expect(seleccion.defaultPrevented, 'el toque no deberia armar un arrastre').toBe(false)
  })
})

describe('portadas difuminadas', () => {
  it('la portada se pinta borrosa cuando la cancion esta marcada', async () => {
    const { default: CoverArt } = await import('../src/components/ui/CoverArt.vue')
    const normal = mount(CoverArt, { props: { id: 1 } })
    expect(normal.find('img').classes()).not.toContain('blurred')
    normal.unmount()

    const borrosa = mount(CoverArt, { props: { id: 1, blur: true } })
    expect(borrosa.find('img').classes(), 'no se marca como difuminada').toContain('blurred')
    borrosa.unmount()
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

  it('quien lee la portada avisa de que esta difuminada', async () => {
    const { default: CoverArt } = await import('../src/components/ui/CoverArt.vue')
    const w = mount(CoverArt, { props: { id: 1, blur: true, alt: 'Mi Gozo' } })
    expect(w.find('img').attributes('alt')).toContain('difuminada')
    w.unmount()
  })

  it('se puede activar y quitar desde el menu de la cancion', () => {
    const app = readFileSync(join(SRC, 'App.vue'), 'utf8')
    expect(app, 'falta la accion en el menu contextual').toMatch(/Difuminar la portada/)
    expect(app, 'falta como volver a verla').toMatch(/Ver la portada/)
    expect(app).toMatch(/api\.setBlur\(/)
  })

  it('todas las portadas de la app la respetan', () => {
    // si una se olvida, la imagen sigue viendose donde menos se espera
    const olvidadas = []
    for (const { file, code } of componentes()) {
      for (const uso of code.matchAll(/<CoverArt\b[^>]*>/g)) {
        if (!/:blur=/.test(uso[0])) olvidadas.push(`${file}: ${uso[0].slice(0, 60)}`)
      }
    }
    expect(olvidadas, 'portadas que no respetan el difuminado').toEqual([])
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
        new RegExp(`\\.${clase}[^{]*input[^{]*\\{[^}]*box-shadow:\\s*none`))
    }
  })

  it('solo hay un anillo por campo, el del envoltorio', () => {
    const envoltorio = CSS.match(/\.field\.focused \.field-box\{([^}]*)\}/)
    expect(envoltorio, 'el campo enfocado deberia tener su sombra').toBeTruthy()
    expect(envoltorio[1]).toMatch(/box-shadow:/)
  })
})

describe('iconos con el significado correcto', () => {
  it('enviar en el chat no usa el icono de «cancion siguiente»', () => {
    const chat = readFileSync(join(SRC, 'components/ChatPage.vue'), 'utf8')
    const boton = chat.match(/<button[^>]*@click="send\(\)"[\s\S]{0,200}?<\/button>/)
    expect(boton, 'no encuentro el boton de enviar').toBeTruthy()
    expect(boton[0], 'usa el icono del reproductor').not.toMatch(/n="(next|previous|play)"/)
    expect(boton[0]).toMatch(/n="send"/)
  })

  it('el icono de enviar existe de verdad', async () => {
    const { ICONS } = await import('../src/icons.js')
    expect(ICONS.send, 'falta el icono «send»').toBeTruthy()
    expect(ICONS.send.d).toContain('<path')
  })
})

describe('informe de duplicados', () => {
  it('los datos del archivo no se parten por la mitad', () => {
    // «6.7» quedaba en una linea y «MB» en la siguiente, con la ruta en medio:
    // el texto suelto dentro de un flex se rompe por cualquier espacio.
    const vue = readFileSync(join(SRC, 'components/DuplicateGroup.vue'), 'utf8')
    expect(vue, 'los datos deberian ir en su propio span').toMatch(/class="dup-stats"/)
    expect(CSS).toMatch(/\.dup-stats\{[^}]*white-space:\s*nowrap/)
  })

  it('lee las mismas claves que manda el nucleo', () => {
    const vue = readFileSync(join(SRC, 'components/DuplicateGroup.vue'), 'utf8')
    for (const viejo of ['relativa', 'tiene_sufijo', 'sugerida']) {
      expect(vue, `quedo «${viejo}», que el nucleo ya no manda`).not.toContain(viejo)
    }
    expect(vue).toContain('group.suggested')
    expect(vue).toContain('t.relative')
    expect(vue).toContain('t.has_suffix')
  })
})

describe('paginas sin canciones', () => {
  it('Entrada no reserva sitio para una ficha que no se puede llenar', () => {
    const app = readFileSync(join(SRC, 'App.vue'), 'utf8')
    const lista = app.match(/const SIN_DETALLE = \[([^\]]*)\]/)
    expect(lista, 'no encuentro que paginas ocultan la ficha').toBeTruthy()
    expect(lista[1]).toContain("'inbox'")
    // en Duplicados SI se queda: desde ahi se escuchan las copias
    expect(lista[1]).not.toContain("'duplicates'")
  })
})
