// Pintar solo las filas que se ven.
//
// Una lista de mil canciones son casi treinta mil nodos en la pagina: la
// tabla tiene una veintena larga por fila (los iconos de play, el corazon,
// las cinco estrellas...). Montar eso tarda, y se monta ENTERO cada vez que
// cambias de busqueda, de orden o de vista. En un equipo modesto se nota como
// un tiron cada vez que escribes una letra.
//
// Aqui solo se pintan las filas de la ventana visible mas un margen; el resto
// del alto lo ocupan dos separadores, uno arriba y otro abajo. La barra de
// desplazamiento mide lo mismo que antes y todo se comporta igual.
//
// Las filas miden todas lo mismo (el css las deja en una sola linea, sin
// partir), asi que basta con medir una para saber donde cae cada indice. Si
// por lo que sea no se puede medir —el contenedor aun no tiene alto, o
// estamos en un entorno sin maquetacion como las pruebas— se pinta la lista
// entera: mas vale de mas que de menos.
//
// Sirve igual para la tabla y para la cuadricula. La diferencia es cuantos
// elementos caben en una linea, y eso no se deduce del css sino que se mira
// en lo ya pintado: los que empiezan a la misma altura son una linea. Asi
// funciona sin saber nada de `auto-fill`, del ancho de ficha ni del hueco.
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'

/**
 * El elemento que tiene la barra de desplazamiento.
 *
 * Empieza por el propio: en la tabla el ancla es el <tbody> y quien se
 * desplaza es un antepasado, pero en la cuadricula el ancla ES la rejilla y
 * ella misma es la que se desplaza.
 */
function nearestScroller (el) {
  let n = el
  while (n && n !== document.body) {
    const o = getComputedStyle(n).overflowY
    if (o === 'auto' || o === 'scroll') return n
    n = n.parentElement
  }
  return null
}

/**
 * @param getAnchor  funcion que devuelve el elemento que contiene las filas
 * @param getCount   funcion que devuelve cuantas filas hay en total
 * @param options    minimum: por debajo de esto se pinta todo sin mas
 *                   overscan: filas de margen por arriba y por abajo
 */
export function useVirtualRows (getAnchor, getCount, options = {}) {
  const minimum = options.minimum ?? 80
  const overscan = options.overscan ?? 10
  // Cuantas filas se pintan ANTES de haber medido nada. Tiene que dar para
  // llenar una pantalla grande y para poder medir una fila; en cuanto se mide
  // (el mismo cuadro) la ventana se ajusta a lo que de verdad se ve.
  const initial = options.initial ?? 120

  const first = ref(0)
  const last = ref(initial)
  const rowHeight = ref(0)      // lo que baja de una linea a la siguiente
  const columns = ref(1)        // cuantos elementos caben en una linea
  // Solo se pone a true si, DESPUES de intentarlo, resulta que no hay forma
  // de medir (un entorno sin maquetacion, o un panel todavia sin alto). Es la
  // valvula de seguridad: en ese caso se pinta la lista entera y no se
  // recorta nada. Al principio vale false, porque pintar las mil filas «por
  // si acaso» era justamente lo que se queria evitar.
  const cannotMeasure = ref(false)
  let viewport = null
  let frame = 0

  const all = computed(() => getCount() <= minimum || cannotMeasure.value)
  const from = computed(() => all.value ? 0 : Math.min(first.value, getCount()))
  const to = computed(() => all.value ? getCount() : Math.min(last.value, getCount()))
  const padTop = computed(() =>
    all.value ? 0 : Math.floor(from.value / columns.value) * rowHeight.value)
  const padBottom = computed(() => all.value ? 0 : Math.max(0,
    Math.ceil((getCount() - to.value) / columns.value) * rowHeight.value))

  /**
   * Mide de lo ya pintado: cuantos elementos hay por linea y cuanto baja de
   * una linea a la siguiente. Devuelve false si no se pudo medir.
   */
  function measure () {
    const anchor = getAnchor()
    if (!anchor) return false
    const items = []
    for (const el of anchor.children) {
      if (!el.dataset || el.dataset.spacer === undefined) items.push(el)
    }
    if (!items.length) return false
    const top = items[0].offsetTop
    let cols = 0
    let step = 0
    for (const el of items) {
      if (el.offsetTop === top) cols++
      else { step = el.offsetTop - top; break }
    }
    if (!step) {
      // solo hay una linea pintada: sirve su propio alto
      if (items.length > 1 && cols === items.length && items.length >= minimum) {
        // muchos elementos y todos a la misma altura: no hay maquetacion real
        return false
      }
      step = items[0].offsetHeight
    }
    if (step <= 0) return false
    rowHeight.value = step
    columns.value = Math.max(1, cols)
    return true
  }

  function compute () {
    const anchor = getAnchor()
    if (!anchor) return
    if (!viewport) viewport = nearestScroller(anchor)
    const total = getCount()
    if (total <= minimum) {
      cannotMeasure.value = false
      first.value = 0
      last.value = total
      return
    }
    const ok = measure()
    const alto = viewport ? viewport.clientHeight : 0
    if (!viewport || !ok || alto <= 0) {
      cannotMeasure.value = true      // sin medidas fiables: se pinta todo
      first.value = 0
      last.value = total
      return
    }
    cannotMeasure.value = false
    const h = rowHeight.value
    const cols = columns.value
    // Donde empiezan las filas, dentro del contenido desplazable. Si el ancla
    // es el propio panel que se desplaza, empiezan al principio: su rectangulo
    // no se mueve al desplazarse y la resta daria un valor que no es.
    const anchorTop = anchor === viewport ? 0
      : anchor.getBoundingClientRect().top
        - viewport.getBoundingClientRect().top + viewport.scrollTop
    const lineas = Math.ceil(total / cols)
    const desde = Math.max(0,
      Math.floor((viewport.scrollTop - anchorTop) / h) - overscan)
    const hasta = Math.min(lineas,
      Math.ceil((viewport.scrollTop + alto - anchorTop) / h) + overscan)
    first.value = Math.min(desde * cols, Math.max(0, total - 1))
    last.value = Math.max(first.value, Math.min(total, hasta * cols))
  }

  /** Se agrupan los avisos de desplazamiento en un solo recalculo por cuadro. */
  function onScroll () {
    if (frame) return
    frame = requestAnimationFrame(() => { frame = 0; compute() })
  }

  async function recompute () {
    await nextTick()
    compute()
  }

  /**
   * Asegura que ese indice esta pintado y devuelve donde cae, para poder
   * desplazarse hasta el aunque no estuviera en la ventana.
   */
  async function reveal (index) {
    if (index < 0) return null
    if (!all.value) {
      const cols = columns.value
      const linea = Math.floor(index / cols)
      first.value = Math.max(0, (linea - overscan) * cols)
      last.value = Math.min(getCount(), (linea + overscan + 1) * cols)
      await nextTick()
    }
    return { viewport, rowHeight: rowHeight.value }
  }

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

  // al cambiar la lista se vuelve arriba: es otra busqueda, otra vista
  watch(() => getCount(), () => { first.value = 0; last.value = initial; recompute() })

  return { from, to, padTop, padBottom, all, rowHeight, columns, recompute, reveal }
}
