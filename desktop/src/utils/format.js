// Formateadores de texto compartidos.
//
// La duración estaba copiada nueve veces por los componentes, cada una con su
// propio valor para «sin dato» («—», «0:00» o cadena vacía). Aquí vive una
// sola vez; el valor de reserva lo decide quien la llama.

/**
 * Segundos → «m:ss». Sin dato (0, null, NaN) devuelve `empty`.
 * @param {number|null|undefined} seconds
 * @param {string} [empty='—'] Qué enseñar cuando no hay duración.
 * @returns {string}
 */
export function formatDuration(seconds, empty = '—') {
  const s = Number(seconds)
  if (!s || s < 0 || !Number.isFinite(s)) return empty
  const m = Math.floor(s / 60)
  const r = Math.floor(s % 60)
  return `${m}:${String(r).padStart(2, '0')}`
}

/**
 * Como `formatDuration`, pero para posiciones de reproducción: ahí «nada»
 * es «0:00», no un guion.
 * @param {number|null|undefined} seconds
 * @returns {string}
 */
export const formatTime = (seconds) => formatDuration(seconds, '0:00')

/**
 * Bytes → «1.5 MB».
 * @param {number|null|undefined} bytes
 * @returns {string}
 */
export function formatMegabytes(bytes) {
  return bytes ? (bytes / 1048576).toFixed(1) + ' MB' : '—'
}

/**
 * Bytes → «12.30» (gigabytes con dos decimales, sin unidad).
 * @param {number} bytes
 * @returns {string}
 */
export const formatGigabytes = (bytes) => (bytes / 1073741824).toFixed(2)
