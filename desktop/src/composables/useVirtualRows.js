// @ts-check
// Pintar solo las filas que se ven.
//
// Una lista de mil canciones son casi treinta mil nodos en la página: la
// tabla tiene una veintena larga por fila (los iconos de play, el corazón,
// las cinco estrellas...). Montar eso tarda, y se monta ENTERO cada vez que
// cambias de búsqueda, de orden o de vista. En un equipo modesto se nota como
// un tirón cada vez que escribes una letra.
//
// Aquí solo se pintan las filas de la ventana visible más un margen; el resto
// del alto lo ocupan dos separadores, uno arriba y otro abajo. La barra de
// desplazamiento mide lo mismo que antes y todo se comporta igual.
//
// Las filas miden todas lo mismo (el css las deja en una sola línea, sin
// partir), así que basta con medir una para saber dónde cae cada índice. Si
// por lo que sea no se puede medir —el contenedor aún no tiene alto, o
// estamos en un entorno sin maquetación como las pruebas— se pinta la lista
// entera: más vale de más que de menos.
//
// Sirve igual para la tabla y para la cuadrícula. La diferencia es cuántos
// elementos caben en una línea, y eso no se deduce del css sino que se mira
// en lo ya pintado: los que empiezan a la misma altura son una línea.
//
// Y sirve para la lista AGRUPADA (por artista, por álbum...): cada grupo es
// una cabecera y sus líneas de canciones, y la cabecera mide lo suyo. Antes
// cada grupo era una lista aparte y, con grupos de menos de 80 canciones
// —«Artistas», casi siempre—, no se recortaba nada: se pintaba la biblioteca
// entera. La cabecera del primer grupo de la ventana se pinta siempre, justo
// antes de su primera línea pintada: así se queda pegada arriba (es
// `sticky`) aunque la de verdad haya quedado muy por encima.
import { ref, shallowRef, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'

/**
 * El elemento que tiene la barra de desplazamiento.
 *
 * Empieza por el propio: en la tabla el ancla es el <tbody> y quien se
 * desplaza es un antepasado, pero en la cuadrícula el ancla ES la rejilla y
 * ella misma es la que se desplaza.
 * @param {Element|null|undefined} el
 * @returns {HTMLElement|null}
 */
export function nearestScroller(el) {
  let n = /** @type {HTMLElement|null|undefined} */ (el)
  while (n && n !== document.body) {
    const o = getComputedStyle(n).overflowY
    if (o === 'auto' || o === 'scroll') return n
    n = n.parentElement
  }
  return null
}

/**
 * Un grupo de la lista agrupada, visto desde aquí: dónde empiezan sus
 * canciones en la lista y cuántas se pintan (0 si está plegado).
 * @typedef {Object} VirtualGroup
 * @property {number} start
 * @property {number} size
 */

/**
 * La ventana que se pinta: del grupo `gFirst` desde su línea `lineFirst`,
 * hasta el grupo `gLast` antes de su línea `lineLast`. Sin agrupar hay un
 * solo grupo, sin cabecera.
 * @typedef {Object} VirtualWindow
 * @property {VirtualGroup[]} list
 * @property {boolean} heads   si los grupos llevan cabecera
 * @property {number} cols
 * @property {number} gFirst
 * @property {number} lineFirst
 * @property {number} gLast
 * @property {number} lineLast  exclusiva
 */

/** Cuántas columnas tiene la rejilla, si el navegador ya las ha resuelto. */
function gridTracks(/** @type {Element} */ el) {
  const t = String(getComputedStyle(el).gridTemplateColumns || '').trim()
  return /^[\d.]+px(\s+[\d.]+px)*$/.test(t) ? t.split(/\s+/).length : 0
}

/** El hueco entre líneas de una rejilla (0 en una tabla o una lista). */
function rowGap(/** @type {Element} */ el) {
  return parseFloat(getComputedStyle(el).rowGap) || 0
}

/** El último índice de `tops` (de 0 a n-1) que empieza en `y` o antes. */
function lastAtOrBefore(/** @type {Float64Array} */ tops, /** @type {number} */ n, y) {
  let lo = 0
  let hi = n - 1
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1
    if (tops[mid] <= y) lo = mid
    else hi = mid - 1
  }
  return lo
}

/**
 * @param {() => (HTMLElement|null|undefined)} getAnchor
 *   el elemento que contiene las filas
 * @param {() => number} getCount  cuántas canciones hay en total
 * @param {{ minimum?: number, overscan?: number, initial?: number,
 *           groups?: () => (VirtualGroup[]|null|undefined) }} [options]
 *   minimum: por debajo de esto se pinta todo sin más; overscan: líneas de
 *   margen por arriba y por abajo; groups: los grupos, si la lista va agrupada
 */
export function useVirtualRows(getAnchor, getCount, options = {}) {
  const minimum = options.minimum ?? 80
  const overscan = options.overscan ?? 10
  // Cuántas se pintan ANTES de haber medido nada. Tiene que dar para llenar
  // una pantalla grande y para poder medir una fila; en cuanto se mide (el
  // mismo cuadro) la ventana se ajusta a lo que de verdad se ve.
  const initial = options.initial ?? 120
  const getGroups = options.groups || (() => null)

  const rowHeight = ref(0) // lo que baja de una línea de canciones a la siguiente
  const headHeight = ref(0) // lo que ocupa la cabecera de un grupo
  const columns = ref(1) // cuántos elementos caben en una línea
  // Solo se pone a true si, DESPUÉS de intentarlo, resulta que no hay forma
  // de medir (un entorno sin maquetación, o un panel todavía sin alto). Es la
  // válvula de seguridad: en ese caso se pinta la lista entera y no se
  // recorta nada. Al principio vale false, porque pintar las mil filas «por
  // si acaso» era justamente lo que se quería evitar.
  const cannotMeasure = ref(false)
  // Con medidas, los separadores ocupan lo que no se pinta; sin ellas, cero.
  const measured = ref(false)
  /**
   * La ventana pedida (por el desplazamiento o por «ir a»), en grupos y
   * líneas; null hasta el primer cálculo: entonces van las `initial` primeras.
   * @type {import('vue').ShallowRef<null|{gFirst:number, lineFirst:number, gLast:number, lineLast:number}>}
   */
  const requested = shallowRef(null)
  /** @type {HTMLElement|null} */
  let viewport = null
  let frame = 0
  let lastAnchorTop = 0 // dónde empezaban las filas la última vez que se midió

  /** Los grupos tal y como se pintan. Sin agrupar, uno solo y sin cabecera. */
  const layout = computed(() => {
    const groups = getGroups()
    if (groups && groups.length) return { heads: true, list: groups }
    return { heads: false, list: [{ start: 0, size: getCount() }] }
  })

  const all = computed(
    () =>
      getCount() + (layout.value.heads ? layout.value.list.length : 0) <= minimum ||
      cannotMeasure.value
  )

  /** Dónde empieza cada grupo (su cabecera) y el alto total, con lo medido. */
  const geometry = computed(() => {
    const { heads, list } = layout.value
    const cols = columns.value
    const rowH = rowHeight.value
    const headH = heads ? headHeight.value : 0
    const tops = new Float64Array(list.length + 1)
    let y = 0
    for (let g = 0; g < list.length; g++) {
      tops[g] = y
      y += headH + Math.ceil(list[g].size / cols) * rowH
    }
    tops[list.length] = y
    return { heads, list, cols, rowH, headH, tops, total: y }
  })

  const lines = (/** @type {VirtualGroup} */ g, /** @type {number} */ cols) =>
    Math.ceil(g.size / cols)

  /** En qué grupo y en qué línea de él cae la canción `index`. */
  function position(/** @type {number} */ index) {
    const { list } = layout.value
    const cols = columns.value
    let lo = 0
    let hi = list.length - 1
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1
      if (list[mid].start <= index) lo = mid
      else hi = mid - 1
    }
    return { g: lo, line: Math.floor(Math.max(0, index - list[lo].start) / cols) }
  }

  /** La ventana de canciones [a, b), en grupos y líneas. */
  function spanOfSongs(/** @type {number} */ a, /** @type {number} */ b) {
    const count = getCount()
    // desde el principio: con los grupos plegados de delante, que son solo su cabecera
    const first = a <= 0 ? { g: 0, line: 0 } : position(Math.min(a, count - 1))
    const lastIndex = Math.max(0, Math.min(b, count) - 1)
    const last = position(lastIndex)
    // un grupo plegado justo detrás de la última canción se ve entero (solo es su cabecera)
    const { list } = layout.value
    let gLast = last.g
    while (gLast + 1 < list.length && list[gLast + 1].size === 0 && b >= count) gLast++
    return {
      gFirst: first.g,
      lineFirst: first.line,
      gLast,
      lineLast: gLast === last.g ? last.line + 1 : 0
    }
  }

  /** La ventana entre las alturas `lo` y `hi` (ya con el margen), medida desde el principio. */
  function spanOfPixels(/** @type {number} */ lo, /** @type {number} */ hi) {
    const { list, tops, cols, rowH, headH } = geometry.value
    const n = list.length
    const gFirst = lastAtOrBefore(tops, n, Math.max(0, lo))
    const body1 = tops[gFirst] + headH
    const lineFirst =
      lo <= body1
        ? 0
        : Math.min(Math.max(0, lines(list[gFirst], cols) - 1), Math.floor((lo - body1) / rowH))
    const gLast = Math.max(gFirst, lastAtOrBefore(tops, n, Math.max(0, hi - 0.5)))
    const body2 = tops[gLast] + headH
    let lineLast =
      hi <= body2 ? 0 : Math.min(lines(list[gLast], cols), Math.ceil((hi - body2) / rowH))
    // al menos una línea, si el grupo tiene alguna
    if (gLast === gFirst && lineLast <= lineFirst) {
      lineLast = Math.min(lines(list[gLast], cols), lineFirst + 1)
    }
    return { gFirst, lineFirst, gLast, lineLast }
  }

  /** @type {import('vue').ComputedRef<VirtualWindow>} */
  const span = computed(() => {
    const { heads, list } = layout.value
    const count = getCount()
    const whole = () => ({
      gFirst: 0,
      lineFirst: 0,
      gLast: list.length - 1,
      lineLast: lines(list[list.length - 1], columns.value)
    })
    let s
    if (all.value) s = whole()
    else s = requested.value || spanOfSongs(0, Math.min(count, initial))
    // la lista pudo encogerse desde que se calculó
    const gFirst = Math.min(s.gFirst, list.length - 1)
    const gLast = Math.min(Math.max(s.gLast, gFirst), list.length - 1)
    return { list, heads, cols: columns.value, ...s, gFirst, gLast }
  })

  /** Las canciones que se pintan: [from, to). */
  const from = computed(() => {
    const w = span.value
    const g = w.list[w.gFirst]
    return Math.min(g.start + w.lineFirst * w.cols, getCount())
  })
  const to = computed(() => {
    const w = span.value
    const g = w.list[w.gLast]
    return Math.max(
      from.value,
      Math.min(g.start + Math.min(g.size, w.lineLast * w.cols), getCount())
    )
  })
  // Los separadores. La cabecera del primer grupo se pinta justo antes de su
  // primera línea pintada, así que el hueco de arriba llega hasta ella.
  const padTop = computed(() => {
    if (all.value || !measured.value) return 0
    const w = span.value
    const { tops, rowH } = geometry.value
    return tops[w.gFirst] + w.lineFirst * rowH
  })
  const padBottom = computed(() => {
    if (all.value || !measured.value) return 0
    const w = span.value
    const { tops, rowH, headH, total } = geometry.value
    return Math.max(0, total - (tops[w.gLast] + headH + w.lineLast * rowH))
  })

  /**
   * Mide de lo ya pintado: cuántos elementos hay por línea, cuánto baja de
   * una línea a la siguiente y cuánto ocupa una cabecera de grupo. Devuelve
   * false si no se pudo medir.
   */
  function measure() {
    const anchor = getAnchor()
    if (!anchor) return false
    const items = /** @type {HTMLElement[]} */ ([...anchor.querySelectorAll('[data-song-row]')])
    if (!items.length) return false
    const gap = rowGap(anchor)
    // Con decimales. Con los enteros de offsetTop una fila de 35,34 px
    // contaba 35, y en una lista de miles el error se comía cientos de
    // píxeles: al bajar del todo, las últimas no llegaban a verse. Sin
    // maquetación (las pruebas) todo mide cero y se usan los de siempre.
    const rects = items[0].getBoundingClientRect().height > 0
    const top = (/** @type {HTMLElement} */ el) =>
      rects ? el.getBoundingClientRect().top : el.offsetTop
    const height = (/** @type {HTMLElement} */ el) =>
      rects ? el.getBoundingClientRect().height : el.offsetHeight
    // Una línea son las filas seguidas (hermanas, sin una cabecera en medio)
    // que empiezan a la misma altura; el paso, lo que baja a la siguiente.
    let cols = 0
    let step = 0
    let run = 1
    for (let i = 1; i < items.length; i++) {
      const el = items[i]
      const prev = items[i - 1]
      if (el.previousElementSibling !== prev) {
        cols = Math.max(cols, run)
        run = 1
        continue
      }
      if (Math.abs(top(el) - top(prev)) < 0.5) {
        run++
        continue
      }
      // la primera línea entera (seguida de otra en su grupo) dice las columnas
      step = top(el) - top(prev)
      cols = run
      break
    }
    if (!step) cols = Math.max(cols, run)
    cols = gridTracks(anchor) || cols || 1
    if (!step) {
      // una sola línea pintada: sirve su propio alto
      if (items.length > 1 && cols === items.length && items.length >= minimum) {
        // muchos elementos y todos a la misma altura: no hay maquetación real
        return false
      }
      step = height(items[0]) + gap
    }
    if (step <= 0) return false
    rowHeight.value = step
    columns.value = Math.max(1, cols)
    const head = measureHead(anchor, step, Math.max(1, cols), top, height, gap)
    if (head > 0) headHeight.value = head
    return true
  }

  /**
   * Lo que ocupa de verdad una cabecera de grupo en la lista: de la primera
   * fila de un grupo a la del siguiente, menos sus líneas. Así entra también
   * lo que no es la cabecera en sí (en la tabla, el borde de la última fila
   * del grupo, que no es igual que el de las demás). Se mide entre grupos del
   * medio de lo pintado: el primero puede empezar a medias y su cabecera ir
   * pegada arriba (`sticky`, fuera de su sitio). Sin dos grupos así, su alto.
   * @returns {number}
   */
  function measureHead(
    /** @type {HTMLElement} */ anchor,
    /** @type {number} */ step,
    /** @type {number} */ cols,
    /** @type {(el: HTMLElement) => number} */ top,
    /** @type {(el: HTMLElement) => number} */ height,
    /** @type {number} */ gap
  ) {
    const heads = /** @type {HTMLElement[]} */ ([...anchor.querySelectorAll('[data-group-row]')])
    if (!heads.length) return 0
    /** Las filas de cada grupo pintado: las hermanas que siguen a su cabecera. */
    const runs = heads.map((h) => {
      const out = []
      for (let el = h.nextElementSibling; el && el.hasAttribute('data-song-row');) {
        out.push(/** @type {HTMLElement} */ (el))
        el = el.nextElementSibling
      }
      return out
    })
    const found = []
    for (let k = 1; k + 1 < runs.length; k++) {
      if (!runs[k].length || !runs[k + 1].length) continue
      const span = top(runs[k + 1][0]) - top(runs[k][0])
      const h = span - Math.ceil(runs[k].length / cols) * step
      if (h > 0) found.push(h)
    }
    if (found.length) return found.reduce((a, b) => a + b, 0) / found.length
    return height(heads[0]) + gap
  }

  function compute() {
    const anchor = getAnchor()
    if (!anchor) return
    if (!viewport) viewport = nearestScroller(anchor)
    if (getCount() + (layout.value.heads ? layout.value.list.length : 0) <= minimum) {
      cannotMeasure.value = false
      return
    }
    const ok = measure()
    const alto = viewport ? viewport.clientHeight : 0
    if (!viewport || !ok || alto <= 0) {
      cannotMeasure.value = true // sin medidas fiables: se pinta todo
      return
    }
    cannotMeasure.value = false
    measured.value = true
    // Dónde empiezan las filas, dentro del contenido desplazable. Si el ancla
    // es el propio panel que se desplaza, empiezan al principio: su
    // rectángulo no se mueve al desplazarse y la resta daría un valor que no es.
    const anchorTop =
      anchor === viewport
        ? 0
        : anchor.getBoundingClientRect().top -
          viewport.getBoundingClientRect().top +
          viewport.scrollTop
    lastAnchorTop = anchorTop
    windowAt(viewport.scrollTop - anchorTop, alto)
  }

  /** La ventana para lo que se ve desde `top` (en px, desde la primera fila) con alto `alto`. */
  function windowAt(/** @type {number} */ top, /** @type {number} */ alto) {
    const margin = overscan * rowHeight.value
    requested.value = spanOfPixels(top - margin, top + alto + margin)
  }

  /** Se agrupan los avisos de desplazamiento en un solo recálculo por cuadro. */
  function onScroll() {
    if (frame) return
    frame = requestAnimationFrame(() => {
      frame = 0
      compute()
    })
  }

  async function recompute() {
    await nextTick()
    compute()
  }

  /**
   * Asegura que ese índice está pintado y devuelve dónde cae, para poder
   * desplazarse hasta él aunque no estuviera en la ventana.
   * @param {number} index
   */
  async function reveal(index) {
    if (index < 0) return null
    if (!all.value) {
      const cols = columns.value
      requested.value = spanOfSongs(index - overscan * cols, index + (overscan + 1) * cols)
      await nextTick()
    }
    return { viewport, rowHeight: rowHeight.value }
  }

  /** @type {ResizeObserver|null} */
  let observer = null
  onMounted(() => {
    const anchor = getAnchor()
    viewport = anchor ? nearestScroller(anchor) : null
    if (viewport) viewport.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onScroll, { passive: true })
    if (typeof ResizeObserver === 'function' && viewport) {
      observer = new ResizeObserver(onScroll)
      observer.observe(viewport)
    }
    recompute()
  })

  onUnmounted(() => {
    if (viewport) viewport.removeEventListener('scroll', onScroll)
    window.removeEventListener('resize', onScroll)
    if (observer) observer.disconnect()
    if (frame) cancelAnimationFrame(frame)
    viewport = null
  })

  // Otra lista (otra búsqueda, otra vista, un grupo que se pliega): la
  // ventana se recalcula al momento para donde esté el scroll, con lo ya
  // medido. Volver a la inicial (las primeras) dejaba en blanco lo que se
  // estaba viendo hasta el cuadro siguiente, y con el scroll abajo del todo
  // plegar un grupo parpadeaba.
  watch(
    () => [getCount(), layout.value],
    () => {
      if (measured.value && viewport && !all.value)
        windowAt(viewport.scrollTop - lastAnchorTop, viewport.clientHeight)
      else requested.value = null
      recompute()
    }
  )

  return {
    from,
    to,
    padTop,
    padBottom,
    all,
    rowHeight,
    columns,
    span,
    recompute,
    reveal
  }
}
