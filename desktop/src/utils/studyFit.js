/**
 * El reparto del alto del modo estudio en el escritorio: cuánto le queda a
 * la onda y cuánto mide cada carril.
 *
 * El panel tiene, de arriba abajo: la cabecera y la barra (`others`), la
 * línea de tiempo y el cuerpo (las columnas de velocidad, clic, marcadores y
 * notas). Antes el alto de los carriles salía de un número fijo, y con una
 * secuencia de veintitantas pistas la onda se comía el panel: de las
 * columnas solo quedaba el título.
 */

/** Lo que se le guarda al cuerpo como poco y como mucho, en px. */
export const BODY_MIN = 200
export const BODY_MAX = 300
/** Lo que mide un carril como poco y como mucho, en px. */
export const LANE_MIN = 20
export const LANE_MAX = 60

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

/**
 * El alto que se le guarda al cuerpo: un tercio largo del panel, entre sus
 * topes.
 * @param {number} inner  el alto del panel
 */
export function bodyHeight(inner) {
  return clamp(inner * 0.34, BODY_MIN, BODY_MAX)
}

/**
 * El sitio que le queda a la onda (o a todos los carriles juntos).
 * @param {{inner: number, pad?: number, others?: number, gap?: number, rows?: number, rest?: number}} m
 *   `inner`, el alto del panel; `pad`, su relleno; `others`, lo que miden la
 *   cabecera y la barra; `gap` y `rows`, la separación entre filas y cuántas
 *   son; `rest`, lo demás de la línea de tiempo (el carril del clic, los de
 *   las guías, la regla)
 */
export function waveRoom({ inner, pad = 0, others = 0, gap = 0, rows = 0, rest = 0 }) {
  const room = inner - pad - others - gap * Math.max(0, rows - 1) - bodyHeight(inner) - rest
  return Math.max(0, Math.round(room))
}

/**
 * Lo que mide cada carril: se reparten el sitio, sin pasar de `LANE_MAX`
 * (con pocas pistas sobraría) ni bajar de `LANE_MIN` (lo que piden sus
 * botones: si no caben, la línea de tiempo se desplaza).
 * @param {number} count  cuántos carriles
 * @param {number} room   el sitio (`waveRoom`)
 */
export function laneHeight(count, room) {
  if (!(count > 0)) return LANE_MAX
  return clamp(Math.floor(room / count), LANE_MIN, LANE_MAX)
}
