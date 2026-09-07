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
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'

/** El antepasado que tiene la barra de desplazamiento. */
function nearestScroller (el) {
  let n = el?.parentElement
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
  const rowHeight = ref(0)
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
  const padTop = computed(() => all.value ? 0 : from.value * rowHeight.value)
  const padBottom = computed(() =>
    all.value ? 0 : Math.max(0, (getCount() - to.value) * rowHeight.value))

  /** Alto real de una fila, medido de la primera que haya pintada. */
  function measureRow () {
    const anchor = getAnchor()
    const row = anchor?.firstElementChild
    // el primer hijo puede ser el separador de arriba: se busca uno con datos
    let el = row
    while (el && el.dataset && el.dataset.spacer !== undefined) el = el.nextElementSibling
    const h = el ? el.offsetHeight : 0
    if (h > 0 && h !== rowHeight.value) rowHeight.value = h
    return rowHeight.value
  }

  function compute () {
    const anchor = getAnchor()
    if (!anchor) return
    if (!viewport) viewport = nearestScroller(anchor)
    const total = getCount()
    const h = measureRow()
    const alto = viewport ? viewport.clientHeight : 0
    if (total <= minimum) {
      cannotMeasure.value = false
      first.value = 0
      last.value = total
      return
    }
    if (!viewport || h <= 0 || alto <= 0) {
      cannotMeasure.value = true      // sin medidas fiables: se pinta todo
      first.value = 0
      last.value = total
      return
    }
    cannotMeasure.value = false
    // donde empiezan las filas, dentro del contenido desplazable
    const anchorTop = anchor.getBoundingClientRect().top
      - viewport.getBoundingClientRect().top + viewport.scrollTop
    const desde = Math.floor((viewport.scrollTop - anchorTop) / h) - overscan
    const hasta = Math.ceil((viewport.scrollTop + alto - anchorTop) / h) + overscan
    first.value = Math.max(0, Math.min(desde, Math.max(0, total - 1)))
    last.value = Math.max(first.value, Math.min(total, hasta))
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
      first.value = Math.max(0, index - overscan)
      last.value = Math.min(getCount(), index + overscan + 1)
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

  return { from, to, padTop, padBottom, all, rowHeight, recompute, reveal }
}
