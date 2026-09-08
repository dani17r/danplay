// Avisos flotantes («Añadida a la lista», «No se pudo…»).
//
// Es un singleton: antes cada página reemitía `@notice` hacia arriba hasta
// llegar a App, que era la única que podía pintarlos, y bastaba con olvidar
// un eslabón para que un aviso se perdiera en silencio. Ahora quien tenga
// algo que decir llama a `notify` y la pila la pinta App.
import { ref } from 'vue'

/** @typedef {{ id: number, message: string, kind: 'info'|'ok' }} Notice */

const notices = ref(/** @type {Notice[]} */ ([]))
let counter = 0
const timers = new Map()

/**
 * @param {string} message
 * @param {'info'|'ok'} [kind]
 * @param {number} [seconds]  cuánto se queda a la vista
 */
export function notify(message, kind = 'info', seconds = 6) {
  const id = ++counter
  notices.value = [...notices.value, { id, message, kind }]
  timers.set(
    id,
    setTimeout(() => dismiss(id), seconds * 1000)
  )
  return id
}

/** @param {number} id */
export function dismiss(id) {
  clearTimeout(timers.get(id))
  timers.delete(id)
  notices.value = notices.value.filter((n) => n.id !== id)
}

/** Vacía la pila. Para pruebas. */
export function clearNotices() {
  for (const t of timers.values()) clearTimeout(t)
  timers.clear()
  notices.value = []
}

export function useNotices() {
  return { notices, notify, dismiss, clearNotices }
}
