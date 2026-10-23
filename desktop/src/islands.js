// @ts-check
// Las islas de la ventana principal: las partes que no hacen falta para el
// primer pintado. Cada una es su propio trozo del paquete (con su CSS) y se
// baja, se analiza y se ejecuta cuando hace falta, no al arrancar:
//
//   - al abrirla (el modo estudio, Transformar, la búsqueda avanzada…);
//   - antes, si es probable que venga: en tiempo libre tras el primer pintado
//     (`preloadWhenIdle`), o al pasar el ratón o el foco por el botón que la
//     abre (`v-preload`), que suele ir unas décimas por delante del clic.
//
// Es la idea de Qwik llevada a una app de escritorio sin servidor: no hay
// HTML hecho de antemano que «reanudar», pero sí se puede no cargar lo que no
// se ve y adelantar lo que viene en los ratos libres.
//
// Una isla es un `shallowRef` con el componente (null hasta que llega) y su
// `load()`. La plantilla la pinta con `v-if="X && …"`: así, cuando se pinta,
// el componente ya está y se monta en el acto, con sus transiciones, sus
// `ref` y sus atributos como si se hubiera importado de golpe.
import { shallowRef } from 'vue'

/**
 * @template T
 * @typedef {{ component: import('vue').ShallowRef<T | null>, load: () => Promise<T> }} Island
 */

/**
 * @template T
 * @param {() => Promise<{ default: T }>} loader
 * @returns {Island<T>}
 */
export function island(loader) {
  const component = shallowRef(/** @type {T | null} */ (null))
  /** @type {Promise<T> | null} */
  let loading = null
  function load() {
    loading ??= loader().then(
      (m) => (component.value = m.default),
      (e) => {
        // la próxima vez se vuelve a intentar
        loading = null
        throw e
      }
    )
    return loading
  }
  return { component, load }
}

// ------------------------------------------------------------- las islas
export const details = island(() => import('./components/DetailsPanel.vue'))
export const study = island(() => import('./components/StudyBar.vue'))
export const transform = island(() => import('./components/TransformPanel.vue'))
export const terms = island(() => import('./components/TermsModal.vue'))
export const welcome = island(() => import('./components/WelcomePage.vue'))
export const searchPanel = island(() => import('./components/SearchPanel.vue'))
export const searchResults = island(() => import('./components/SearchResults.vue'))
export const selectionBar = island(() => import('./components/SelectionBar.vue'))
export const compactPlayer = island(() => import('./components/CompactPlayer.vue'))

/**
 * Las vistas de la lista, menos la tabla (la de fábrica, que va con la app):
 * se carga la que se ve, y las otras al pasar por sus botones.
 */
export const listViews = {
  rows: island(() => import('./components/SongRows.vue')),
  cards: island(() => import('./components/SongCards.vue')),
  grid: island(() => import('./components/SongGrid.vue'))
}

/** Las páginas del enrutador (router.js), por su nombre. */
export const pages = {
  settings: island(() => import('./components/SettingsPage.vue')),
  chat: island(() => import('./components/ChatPage.vue')),
  downloads: island(() => import('./components/DownloadsPage.vue')),
  inbox: island(() => import('./components/InboxPage.vue')),
  duplicates: island(() => import('./components/DuplicatesPage.vue')),
  about: island(() => import('./components/AboutPage.vue'))
}

// ------------------------------------------------------- en tiempo libre
/**
 * Cuando el hilo no tiene nada que hacer. WebKit (el de la app en Linux y
 * macOS) no tiene `requestIdleCallback`: ahí, un rato después del siguiente
 * fotograma.
 * @param {() => void} fn
 */
export function whenIdle(fn) {
  if (typeof requestIdleCallback === 'function') requestIdleCallback(fn, { timeout: 2000 })
  else requestAnimationFrame(() => setTimeout(fn, 120))
}

/**
 * Carga esas islas de una en una, cada una en su rato libre: cada trozo se
 * analiza y se ejecuta en su propia tarea, sin juntar una larga.
 * @param {Island<any>[]} list
 */
export function preloadWhenIdle(list) {
  const queue = [...list]
  const next = () => {
    const one = queue.shift()
    if (!one) return
    one
      .load()
      .catch(() => {
        /* se reintentará al abrirla */
      })
      .finally(() => whenIdle(next))
  }
  whenIdle(next)
}

/** Carga sin esperar ni quejarse: si falla, se reintentará al abrirla. @param {Island<any>} one */
export function warm(one) {
  one.load().catch(() => {})
}

/**
 * `v-preload="isla"` (o una lista): la carga al pasar el ratón o el foco por
 * el elemento, que es lo que pasa justo antes de pulsarlo.
 * @type {import('vue').ObjectDirective<HTMLElement & { _preload?: () => void, _islands?: any }, Island<any> | Island<any>[] | null | undefined>}
 */
export const vPreload = {
  mounted(el, { value }) {
    el._islands = value
    el._preload = () => {
      const v = el._islands
      for (const one of Array.isArray(v) ? v : v ? [v] : []) warm(one)
    }
    el.addEventListener('pointerenter', el._preload, { passive: true })
    el.addEventListener('focusin', el._preload)
  },
  updated(el, { value }) {
    el._islands = value
  },
  beforeUnmount(el) {
    if (!el._preload) return
    el.removeEventListener('pointerenter', el._preload)
    el.removeEventListener('focusin', el._preload)
  }
}
