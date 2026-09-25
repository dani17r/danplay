// Mantiene el foco dentro de un panel mientras está abierto.
//
// Un diálogo o un panel lateral que no atrapa el foco deja que Tab se vaya
// a lo que hay detrás (invisible, tapado por el fondo) y al cerrarse el foco
// se pierde en el <body>: quien navega con teclado no sabe dónde está. Aquí
// se hace lo de siempre: foco inicial dentro, Tab que da la vuelta, y al
// cerrar se devuelve el foco a donde estaba.
import { watch, nextTick, onUnmounted, unref } from 'vue'

export const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), ' +
  'select:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * Los elementos enfocables de dentro, en orden de tabulación.
 * @param {Element | null} root
 * @returns {HTMLElement[]}
 */
export function focusables(root) {
  if (!root) return []
  return [...root.querySelectorAll(FOCUSABLE)].filter(
    (el) => el instanceof HTMLElement && !el.hidden && el.getAttribute('aria-hidden') !== 'true'
  )
}

/**
 * @param {import('vue').Ref<Element|null>} container
 * @param {{
 *   active: import('vue').Ref<boolean> | (() => boolean),
 *   initial?: string,                 selector de lo que se enfoca al abrir
 *   returnFocus?: boolean
 * }} options
 */
export function useFocusTrap(container, options) {
  const isActive = () =>
    typeof options.active === 'function' ? !!options.active() : !!unref(options.active)
  let previous = null

  function focusInitial() {
    const root = unref(container)
    if (!root) return
    const wanted = options.initial ? root.querySelector(options.initial) : null
    const target = wanted || focusables(root)[0] || root
    if (target instanceof HTMLElement) {
      if (target === root && !root.hasAttribute('tabindex')) root.setAttribute('tabindex', '-1')
      target.focus()
      if (typeof target.select === 'function' && target.tagName === 'INPUT') target.select()
    }
  }

  function onKey(e) {
    if (e.key !== 'Tab' || !isActive()) return
    const root = unref(container)
    if (!root) return
    const list = focusables(root)
    if (!list.length) {
      e.preventDefault()
      return
    }
    const first = list[0]
    const last = list[list.length - 1]
    const current = document.activeElement
    const inside = root.contains(current)
    if (e.shiftKey && (current === first || !inside)) {
      e.preventDefault()
      last.focus()
    } else if (!e.shiftKey && (current === last || !inside)) {
      e.preventDefault()
      first.focus()
    }
  }

  async function activate() {
    previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    document.addEventListener('keydown', onKey, true)
    await nextTick()
    focusInitial()
  }
  function deactivate() {
    document.removeEventListener('keydown', onKey, true)
    if (options.returnFocus !== false && previous && previous.isConnected) {
      try {
        previous.focus()
      } catch {
        /* ya no se puede enfocar */
      }
    }
    previous = null
  }

  watch(isActive, (v) => (v ? activate() : deactivate()), { immediate: true })
  onUnmounted(deactivate)

  return { focusInitial }
}
