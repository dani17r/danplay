// @ts-check
// Lo que el diario recuperó al abrir (ver danplay/journal.py), dicho para la
// persona: canciones que se movieron y volvieron a sus listas, listas
// rehechas, lo que ya no está.

/**
 * @typedef {Object} JournalReport
 * @property {number} [songs]    canciones que recuperaron lo suyo (estrellas, estudio…)
 * @property {number} [lists]    listas rehechas o puestas en orden
 * @property {number} [entries]  canciones que volvieron a sus listas
 * @property {number} [ghosts]   canciones que ya no están y quedan como «No está»
 * @property {boolean} [fresh]   la base era nueva (se perdió, otro equipo)
 * @property {number} [at]       cuándo (segundos)
 */

const n = (/** @type {number} */ k, /** @type {string} */ one, /** @type {string} */ many) =>
  k === 1 ? one : `${k} ${many}`

/**
 * El aviso, o '' si no hay nada que contar.
 * @param {JournalReport|null|undefined} r
 */
export function journalMessage(r) {
  if (!r) return ''
  const parts = []
  if (r.entries) {
    parts.push(
      r.entries === 1
        ? 'una canción que se movió volvió a su lista'
        : `${r.entries} canciones que se movieron volvieron a sus listas`
    )
  }
  if (r.fresh && r.lists) parts.push(`${n(r.lists, 'una lista rehecha', 'listas rehechas')}`)
  if (r.songs) {
    parts.push(
      `${n(r.songs, 'una canción recuperó', 'canciones recuperaron')} sus estrellas y su estudio`
    )
  }
  if (r.ghosts) {
    parts.push(
      r.ghosts === 1
        ? 'una que ya no está quedó en su lista como «No está»'
        : `${r.ghosts} que ya no están quedaron en sus listas como «No está»`
    )
  }
  if (!parts.length) return ''
  const text = parts.join('; ')
  return 'Tus listas siguen intactas: ' + text + '.'
}
