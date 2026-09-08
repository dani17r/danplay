// La máquina de estados de la cola, sin efectos.
//
// En la app de escritorio la cola vive en Rust y esto no se usa: Rust aplica
// exactamente estas reglas (ver docs/CONTRATO-INTERNO.md §1). Fuera de Tauri
// —`npm run dev` en un navegador y las pruebas— el reproductor web necesita
// la misma lógica, y tenerla en un módulo puro permite probarla sin audio.
//
// El estado que manejan estas funciones es un subconjunto de PlaybackState:
//   { length, index, repeat, shuffle }
// y todas devuelven el índice al que ir, o -1 cuando hay que pararse.

/** @typedef {'list'|'one'|'once'|'queue'} Repeat */

export const REPEAT_MODES = ['list', 'one', 'once', 'queue']

/**
 * Normaliza un modo de repetición: lo que no se conozca vale «list», que es
 * lo que espera casi todo el mundo al poner música.
 * @param {any} mode
 * @returns {Repeat}
 */
export function normalizeRepeat(mode) {
  return REPEAT_MODES.includes(mode) ? mode : 'list'
}

/**
 * Un índice al azar distinto del actual. Se inyecta `random` para poder
 * probarlo con un valor fijo.
 * @param {number} length
 * @param {number} current
 * @param {() => number} [random]
 * @returns {number} -1 si no hay otro al que ir
 */
export function randomOther(length, current, random = Math.random) {
  if (length <= 1) return length === 1 && current !== 0 ? 0 : -1
  // se elige entre los demás y se salta el actual, para que la distribución
  // sea uniforme sin repetir tiradas
  let i = Math.floor(random() * (length - 1))
  if (i >= current) i++
  return i
}

/**
 * A dónde ir cuando el usuario pulsa «siguiente» (delta 1) o «anterior»
 * (delta -1). A mano siempre se mueve: al final se da la vuelta, y con
 * `shuffle` «siguiente» elige otra al azar.
 * @param {{length:number,index:number,shuffle?:boolean}} state
 * @param {number} delta
 * @param {() => number} [random]
 * @returns {number} índice, o -1 si la cola está vacía
 */
export function nextIndex(state, delta, random = Math.random) {
  const n = state.length
  if (!n) return -1
  if (state.index < 0) return 0
  if (state.shuffle && delta > 0) {
    const other = randomOther(n, state.index, random)
    return other < 0 ? state.index : other
  }
  return (((state.index + delta) % n) + n) % n
}

/**
 * Qué hacer cuando una pista se acaba sola. Aplica el modo de repetición:
 *   list  → sigue, y al final da la vuelta
 *   one   → repite la misma
 *   once  → se para
 *   queue → sigue, pero al llegar al final se para
 * Con `shuffle` (en list/queue) elige otra al azar distinta de la actual.
 * @param {{length:number,index:number,repeat?:string,shuffle?:boolean}} state
 * @param {() => number} [random]
 * @returns {number} índice a reproducir, o -1 para pararse
 */
export function afterEnd(state, random = Math.random) {
  const n = state.length
  if (!n || state.index < 0) return -1
  const repeat = normalizeRepeat(state.repeat)
  if (repeat === 'once') return -1
  if (repeat === 'one') return state.index
  if (state.shuffle) {
    // «queue» con aleatorio: se para cuando ya no queda ninguna otra, que
    // con una sola canción es al acabar la primera
    const other = randomOther(n, state.index, random)
    if (other < 0) return repeat === 'list' ? state.index : -1
    return other
  }
  const last = state.index >= n - 1
  if (last) return repeat === 'queue' ? -1 : 0
  return state.index + 1
}

/**
 * ¿Hay anterior / siguiente? Es lo que la interfaz usa para poner en gris
 * los botones. A mano siempre se puede dar la vuelta, así que basta con que
 * haya más de una canción.
 * @param {{length:number,index:number}} state
 */
export function neighbours(state) {
  const has = state.length > 1 && state.index >= 0
  return { has_previous: has, has_next: has }
}
