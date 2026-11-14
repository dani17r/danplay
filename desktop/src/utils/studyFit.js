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
/** Lo que mide la onda como poco: por debajo ya no se distingue nada. */
export const WAVE_MIN = 80
/** Lo que mide un carril como poco y como mucho, en px. */
export const LANE_MIN = 20
export const LANE_MAX = 60

/**
 * Por debajo de este alto de ventana (px) el panel de estudio aprieta sus
 * espacios. Desde 800 las tarjetas del tono y del clic caben enteras con los
 * de siempre; por debajo, no: a 1366×768 (un portátil sin escalar) el panel
 * mide 536 px, el cuerpo se quedaba en 238 y ellas piden 266 (a 700 faltan 44).
 * No se achica el texto, solo lo que hay entre las cosas (`.study.tight`, en
 * study.css): el relleno y las separaciones del panel y de las tarjetas.
 */
export const TIGHT_BELOW = 800

/**
 * Si con ese alto de ventana el panel va con los espacios apretados.
 * @param {number} windowHeight  el alto de la ventana, en px (0: sin medir)
 */
export const isTight = (windowHeight) => windowHeight > 0 && windowHeight < TIGHT_BELOW

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

/**
 * El alto que se le guarda al cuerpo: un tercio largo del panel o, si es
 * más, lo que piden sus columnas (`wanted`), entre sus topes. Sin eso, en
 * una ventana de portátil las tarjetas del tono y del clic quedaban cortadas
 * por abajo aunque hubiera sitio.
 * @param {number} inner  el alto del panel
 * @param {number} [wanted]  lo que miden por dentro las columnas de los mandos
 */
export function bodyHeight(inner, wanted = 0) {
  return clamp(Math.max(inner * 0.34, wanted), BODY_MIN, BODY_MAX)
}

/**
 * El sitio que le queda a la onda (o a todos los carriles juntos).
 * @param {{inner: number, pad?: number, others?: number, gap?: number, rows?: number, rest?: number, wanted?: number}} m
 *   `inner`, el alto del panel; `pad`, su relleno; `others`, lo que miden la
 *   cabecera y la barra; `gap` y `rows`, la separación entre filas y cuántas
 *   son; `rest`, lo demás de la línea de tiempo (el carril del clic, los de
 *   las guías, la regla); `wanted`, lo que piden las columnas del cuerpo
 */
export function waveRoom({ inner, pad = 0, others = 0, gap = 0, rows = 0, rest = 0, wanted = 0 }) {
  const left = inner - pad - others - gap * Math.max(0, rows - 1) - rest
  let body = bodyHeight(inner, wanted)
  // lo que el cuerpo pide de más lo cede él antes que dejar la onda en nada
  const short = WAVE_MIN - (left - body)
  if (short > 0) body = Math.max(bodyHeight(inner), body - short)
  return Math.max(0, Math.round(left - body))
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
