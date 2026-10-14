// @ts-check
// Llevar canciones separadas a la app del celular: cada una en su
// `.danpista`, dentro de `Celular/` en su biblioteca (`danplay/bundle.py`).
// Al acabar se abre la carpeta con el archivo marcado: de ahí se arrastra a
// WhatsApp, Drive o al teléfono por cable.
import { api, app as tauriApp, errorMessage, JOBS } from '../api.js'
import { notify } from './useNotices.js'

/** Mientras se prepara una tanda, otra petición se suma a la misma espera. */
let running = /** @type {Promise<any>|null} */ (null)

/**
 * @param {{id: number, title?: string, has_stems?: boolean}[]} list
 *   las canciones; las que se sabe que no tienen pistas no se piden
 */
export async function takeToPhone(list) {
  const songs = list.filter((s) => s.has_stems !== false)
  if (!songs.length) {
    return notify(
      list.length > 1
        ? 'Ninguna de esas canciones tiene sus pistas separadas'
        : 'Esta canción no tiene sus pistas separadas'
    )
  }
  if (running) return notify('Ya se está preparando una tanda para el celular', 'info', 3)
  const n = songs.length
  notify(
    n === 1
      ? 'Preparando para el celular… (unos segundos por canción)'
      : `Preparando ${n} canciones para el celular…`,
    'info',
    4
  )
  running = api.runJob(() => api.toPhone(songs.map((s) => s.id)), JOBS.phone)
  try {
    const r = await running
    const done = r?.done || []
    const failed = r?.failed || []
    if (done.length) {
      const first = done[0]
      const noClick = done.filter((/** @type {any} */ d) => !d.click).length
      notify(
        (done.length === 1
          ? `Lista para el celular: «${first.name}» (${megas(first.size)})`
          : `${done.length} listas para el celular, en la carpeta «Celular»`) +
          (noClick ? (done.length === 1 ? ', sin clic automático' : `; ${noClick} sin clic`) : ''),
        'ok',
        8
      )
      tauriApp.revealInFolder(first.path).catch(() => {})
    }
    for (const f of failed) notify(`«${f.title}»: ${f.error}`)
  } catch (e) {
    notify('No se pudo preparar para el celular: ' + errorMessage(e))
  } finally {
    running = null
  }
}

/** @param {number} bytes */
function megas(bytes) {
  const mb = (bytes || 0) / 1_000_000
  return mb >= 10 ? `${Math.round(mb)} MB` : `${mb.toFixed(1).replace('.', ',')} MB`
}
