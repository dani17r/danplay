// @ts-check
// Deshacer y rehacer lo que se hace al clic de UNA canción.
//
// Cada paso guarda cómo estaba el clic propio ANTES del cambio: una copia de
// `own` (`{beats, bars, meter, touched?}`), o `null` si la canción aún no
// tenía clic manual (deshacer hasta ahí es «volver a usar el automático»).
// No sabe de Vue ni de la pantalla: quien la use le da el estado de ahora
// cuando deshace o rehace, para poder volver.

/** @typedef {{beats: number[], bars: number[], meter: number, touched?: Array<[number, number]>}} Own */
/** @typedef {{label: string, own: Own|null}} Step */

/**
 * Una copia que no comparte nada con el original (los clics son datos
 * planos: números, listas y objetos).
 * @template T @param {T} value @returns {T}
 */
function deepClone(value) {
  if (Array.isArray(value)) return /** @type {any} */ (value.map(deepClone))
  if (value && typeof value === 'object') {
    return /** @type {any} */ (
      Object.fromEntries(Object.entries(value).map(([k, v]) => [k, deepClone(v)]))
    )
  }
  return value
}

/** @param {any} own @returns {Own|null} */
const snapshot = (own) => (own == null ? null : deepClone(own))

export class BeatHistory {
  /**
   * @param {{limit?: number, mergeMs?: number, now?: () => number}} [options]
   *   `limit`: cuántos pasos se guardan (los más viejos se tiran);
   *   `mergeMs`: lo que pueden tardar dos cambios seguidos en juntarse;
   *   `now`: el reloj, para las pruebas.
   */
  constructor({ limit = 200, mergeMs = 600, now = () => Date.now() } = {}) {
    this.limit = Math.max(1, Math.floor(limit) || 1)
    this.mergeMs = mergeMs
    this.now = now
    /** @type {Array<{label: string, own: Own|null}>} */
    this.undoStack = []
    /** @type {Array<{label: string, own: Own|null}>} */
    this.redoStack = []
    /** @type {{label: string, key: string, at: number}|null} el último cambio, por si el siguiente se junta con él */
    this.last = null
  }

  /**
   * Apunta un cambio: `before` es cómo estaba el clic ANTES de hacerlo. Lo
   * rehecho se pierde (se ha tomado otro camino).
   *
   * Dos cambios seguidos con la misma `key` (y la misma `label`) a menos de
   * `mergeMs` uno de otro se juntan en un solo paso, que conserva el
   * `before` más viejo: mantener pulsada una flecha son muchos cambios y un
   * único «deshacer». Sin `key`, nunca se juntan.
   * @param {string} label  lo que se hizo, para decirlo («Mover 3 pulsos»)
   * @param {Own|null} before
   * @param {string} [key]
   */
  record(label, before, key) {
    const at = this.now()
    this.redoStack = []
    const last = this.last
    if (
      key != null &&
      last &&
      last.key === key &&
      last.label === label &&
      at - last.at <= this.mergeMs &&
      this.undoStack.length > 0
    ) {
      // el paso de antes sigue valiendo (con su `before` más viejo): solo se
      // alarga la ventana, para que seguir pulsando siga juntando
      last.at = at
      return
    }
    this.undoStack.push({ label, own: snapshot(before) })
    if (this.undoStack.length > this.limit)
      this.undoStack.splice(0, this.undoStack.length - this.limit)
    this.last = key == null ? null : { label, key, at }
  }

  /**
   * Deshace el último cambio. `current` es cómo está el clic ahora (se guarda
   * para poder rehacer). Devuelve cómo estaba y qué se deshizo; null si no
   * hay nada que deshacer.
   * @param {Own|null} current
   * @returns {Step|null}
   */
  undo(current) {
    const step = this.undoStack.pop()
    if (!step) return null
    this.redoStack.push({ label: step.label, own: snapshot(current) })
    this.last = null
    return { label: step.label, own: snapshot(step.own) }
  }

  /**
   * Rehace lo último que se deshizo. `current`: el clic de ahora.
   * @param {Own|null} current
   * @returns {Step|null}
   */
  redo(current) {
    const step = this.redoStack.pop()
    if (!step) return null
    // (no hace falta recortar: lo que hay entre deshacer y rehacer suma lo mismo
    // y `record` ya mantiene la pila de deshacer dentro del límite)
    this.undoStack.push({ label: step.label, own: snapshot(current) })
    this.last = null
    return { label: step.label, own: snapshot(step.own) }
  }

  get canUndo() {
    return this.undoStack.length > 0
  }

  get canRedo() {
    return this.redoStack.length > 0
  }

  /** Lo que se deshará («Mover 3 pulsos»), o '' si no hay nada. */
  get undoLabel() {
    return this.undoStack.at(-1)?.label ?? ''
  }

  /** Lo que se rehará, o '' si no hay nada. */
  get redoLabel() {
    return this.redoStack.at(-1)?.label ?? ''
  }

  /** Cuántos pasos hay para deshacer y para rehacer. @returns {[number, number]} */
  get size() {
    return [this.undoStack.length, this.redoStack.length]
  }

  /** Olvida todo (al cambiar de canción, o al volver al clic automático). */
  clear() {
    this.undoStack = []
    this.redoStack = []
    this.last = null
  }
}
