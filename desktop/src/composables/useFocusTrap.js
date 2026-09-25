// @ts-check
// Mantiene el foco dentro de un panel mientras está abierto.
//
// Un diálogo o un panel lateral que no atrapa el foco deja que Tab se vaya
// a lo que hay detrás (invisible, tapado por el fondo) y al cerrarse el foco
// se pierde en el <body>: quien navega con teclado no sabe dónde está. Aquí
// se hace lo de siempre: foco inicial dentro, Tab que da la vuelta, y al
// cerrar se devuelve el foco a donde estaba.
//
// Puede haber uno encima de otro (la confirmación de «quitar proveedor»
// sobre la ventana de la IA): manda solo el de arriba.
import { watch, nextTick, onUnmounted, unref } from 'vue'

export const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), ' +
  'select:not([disabled]), [tabindex]:not([tabindex="-1"])'

/** Las trampas activas, la de arriba al final. @type {object[]} */
const stack = []

/**
 * Los elementos enfocables de dentro, en orden de tabulación.
 * @param {Element | null | undefined} root
 * @returns {HTMLElement[]}
 */
export function focusables(root) {
  if (!root) return []
  return /** @type {HTMLElement[]} */ ([...root.querySelectorAll(FOCUSABLE)]).filter(
    (el) => el instanceof HTMLElement && !el.hidden && el.getAttribute('aria-hidden') !== 'true'
  )
}

/**
 * @param {import('vue').Ref<HTMLElement|null> | import('vue').ShallowRef<HTMLElement|null>} container
 * @param {{
 *   active: import('vue').Ref<boolean> | (() => boolean),
 *   initial?: string,
 *   returnFocus?: boolean | (() => boolean)
 * }} options
 *   active: cuándo está abierto
 *   initial: selector de lo que se enfoca al abrir (si no, lo primero enfocable)
 *   returnFocus: devolver el foco al cerrar (por defecto, sí); si es una
 *   función, se le pregunta al cerrar
 */
export function useFocusTrap(container, options) {
  const isActive = () =>
    typeof options.active === 'function' ? !!options.active() : !!unref(options.active)
  const me = {}
  /** @type {HTMLElement | null} */
  let previous = null
  let on = false

  function focusInitial() {
    const root = unref(container)
    if (!root) return
    const wanted = options.initial ? root.querySelector(options.initial) : null
    const target = wanted instanceof HTMLElement ? wanted : focusables(root)[0] || root
    if (target === root && !root.hasAttribute('tabindex')) root.setAttribute('tabindex', '-1')
    target.focus()
    if (target instanceof HTMLInputElement) target.select()
  }

  /** @param {KeyboardEvent} e */
  function onKey(e) {
    if (e.key !== 'Tab' || !on || stack[stack.length - 1] !== me) return
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
    if (on) return
    on = true
    previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    stack.push(me)
    document.addEventListener('keydown', onKey, true)
    await nextTick()
    if (on) focusInitial()
  }
  function deactivate() {
    if (!on) return
    on = false
    document.removeEventListener('keydown', onKey, true)
    const i = stack.indexOf(me)
    if (i >= 0) stack.splice(i, 1)
    const back =
      typeof options.returnFocus === 'function'
        ? options.returnFocus()
        : options.returnFocus !== false
    if (back && previous && previous.isConnected) {
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
