// @ts-check
// Formateadores de texto compartidos.
//
// La duración estaba copiada nueve veces por los componentes, cada una con su
// propio valor para «sin dato» («—», «0:00» o cadena vacía), y el «hace
// cuánto» dos. Aquí vive una sola vez; el valor de reserva lo decide quien
// la llama.

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
 * Segundos → «4 min» (minutos enteros, hacia abajo): lo que dura un grupo o
 * una lista entera, donde los segundos sobran.
 * @param {number|null|undefined} seconds
 * @returns {string}
 */
export function formatMinutes(seconds) {
  const s = Number(seconds)
  return `${Number.isFinite(s) && s > 0 ? Math.floor(s / 60) : 0} min`
}

/**
 * Hace cuánto, desde una marca de tiempo en segundos: «ahora mismo»,
 * «hace 5 min», «hace 3 h», «hace 2 días». Sin marca, «nunca».
 * @param {number|null|undefined} ts
 * @param {number} [now] ahora, en milisegundos (para las pruebas)
 * @returns {string}
 */
export function formatAgo(ts, now = Date.now()) {
  if (!ts) return 'nunca'
  const m = Math.round((now / 1000 - ts) / 60)
  if (m < 1) return 'ahora mismo'
  if (m < 60) return `hace ${m} min`
  const h = Math.round(m / 60)
  return h < 48 ? `hace ${h} h` : `hace ${Math.round(h / 24)} días`
}

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

/**
 * El nombre completo de una canción, como el de su archivo pero sin la
 * extensión: «Barak - Mi Gozo (feat. …)». Es lo que se copia para buscarla;
 * sin archivo, «Artista - Título».
 * @param {{ file?: string, title?: string, artist?: string }|null|undefined} song
 * @returns {string}
 */
export function fullName(song) {
  const file = String(song?.file || '')
    .replace(/\.[a-z0-9]{2,5}$/i, '')
    .trim()
  if (file) return file
  const title = String(song?.title || '').trim()
  return song?.artist ? `${song.artist} - ${title}` : title
}
