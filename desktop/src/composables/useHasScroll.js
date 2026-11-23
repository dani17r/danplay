// @ts-check
// ¿El contenido de este panel se sale y hay que desplazarse para verlo?
//
// Se usa para no enseñar atajos que no hacen falta: el boton de «ir a lo que
// suena» solo tiene sentido si la lista es larga de verdad. Con veinte
// canciones que caben en pantalla, ese boton es ruido.
import { ref, onMounted, onUnmounted, nextTick } from 'vue'

/**
 * @param getEl  funcion que devuelve el elemento con scroll (puede ser null)
 * @param margin cuanto tiene que sobrar para considerarlo «largo», en px
 */
export function useHasScroll(getEl, margin = 240) {
  const hasScroll = ref(false)
  let observer = null
  let watched = null

  function check() {
    const el = getEl()
    hasScroll.value = !!el && el.scrollHeight > el.clientHeight + margin
    // el elemento con scroll cambia al cambiar de vista (tabla, rejilla...)
    if (el !== watched) {
      if (watched && observer) observer.unobserve(watched)
      watched = el
      if (el && observer) observer.observe(el)
    }
  }

  /** Vuelve a medir en el siguiente pintado, cuando el DOM ya esta puesto. */
  async function recheck() {
    await nextTick()
    check()
  }

  onMounted(() => {
    if (typeof ResizeObserver === 'function') {
      observer = new ResizeObserver(check)
    }
    window.addEventListener('resize', check)
    recheck()
  })

  onUnmounted(() => {
    window.removeEventListener('resize', check)
    if (observer) observer.disconnect()
    observer = null
    watched = null
  })

  return { hasScroll, recheck }
}

/**
 * ¿El elemento objetivo esta fuera de la vista del contenedor?
 *
 * Sirve para esconder el atajo cuando ya tienes delante lo que buscabas. Si
 * el objetivo no esta ni en el DOM (suena algo de otra lista) cuenta como
 * fuera: ahi el atajo es justamente lo que hace falta.
 */
/** Lo que se espera tras el último desplazamiento para volver a mirar, en ms. */
const SETTLE_MS = 150

export function useIsOffscreen(getContainer, getTarget) {
  const offscreen = ref(false)
  let io = null
  /** Lo que se está mirando, y el panel en el que se escucha el desplazamiento. */
  let observed = null
  let scroller = null
  let timer = null

  // La lista virtual recicla las filas: al bajar, la que suena se desmonta (o
  // la misma fila pasa a ser otra canción), y al volver es OTRO elemento. El
  // observador seguía mirando el de antes y el atajo se quedaba puesto con la
  // canción a la vista. Al dejar de desplazarse, si la que suena ya no es la
  // que se mira, se vuelve a mirar. Un rato después y no en el fotograma
  // siguiente: la lista pinta las filas nuevas por su cuenta, y mirando
  // enseguida aún no estaba la que suena.
  function onScroll() {
    clearTimeout(timer)
    timer = setTimeout(() => {
      if (getTarget() !== observed) attach()
    }, SETTLE_MS)
  }
  function listenTo(root) {
    if (root === scroller) return
    scroller?.removeEventListener('scroll', onScroll)
    scroller = root || null
    scroller?.addEventListener('scroll', onScroll, { passive: true })
  }

  function attach() {
    if (io) {
      io.disconnect()
      io = null
    }
    const root = getContainer()
    const el = getTarget()
    observed = el || null
    listenTo(root)
    if (!el) {
      offscreen.value = !!root // no esta pintado: hay que ir a buscarlo
      return
    }
    if (typeof IntersectionObserver !== 'function') {
      offscreen.value = false
      return
    }
    io = new IntersectionObserver(
      ([entry]) => {
        offscreen.value = !entry.isIntersecting
      },
      { root: root || null, threshold: 0.6 }
    )
    io.observe(el)
  }

  async function recheck() {
    await nextTick()
    attach()
  }

  onMounted(recheck)
  onUnmounted(() => {
    if (io) io.disconnect()
    io = null
    listenTo(null)
    clearTimeout(timer)
  })

  return { offscreen, recheck }
}
