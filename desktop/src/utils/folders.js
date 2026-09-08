// Añadir una carpeta a la biblioteca, con lo que hay que decirle al usuario.
//
// Estaba escrito dos veces —en la bienvenida y en Ajustes— y la copia de
// Ajustes comparaba con los nombres en castellano que la API dejó de usar
// («confirmar», «ya_estaba», «reemplaza»), así que allí no salía ningún aviso
// y el caso «parece una copia» vaciaba el campo sin añadir nada ni explicar
// por qué. Ahora los nombres viven en un solo sitio.
import { api, errorMessage } from '../api.js'

/** Lo que la API responde en `action` (docs/CONTRATO-INTERNO.md §4). */
export const FOLDER_ACTIONS = ['added', 'already_there', 'replaced', 'confirm']

/**
 * @typedef {Object} FolderOutcome
 * @property {'added'|'already_there'|'replaced'|'confirm'|'error'} action
 * @property {string} message      qué contarle al usuario
 * @property {'ok'|'info'} [kind]  cómo pintarlo
 * @property {boolean} confirmable si hace falta insistir para añadirla
 * @property {boolean} scanned     si se llegó a analizar
 */

/**
 * Añade la carpeta y devuelve qué pasó, ya traducido.
 * @param {string} path
 * @param {{ force?: boolean, label?: string, scan?: boolean }} [options]
 * @returns {Promise<FolderOutcome>}
 */
export async function addFolder(path, options = {}) {
  const { force = false, label = '', scan = true } = options
  const clean = (path || '').trim()
  if (!clean) return { action: 'error', message: 'Escribe una ruta', confirmable: false, scanned: false }
  let r
  try {
    r = await api.addFolder(clean, label, force)
  } catch (e) {
    return {
      action: 'error',
      message: 'No se pudo añadir: ' + errorMessage(e),
      confirmable: false,
      scanned: false
    }
  }
  const notice = r.notice || {}
  if (r.action === 'confirm') {
    return {
      action: 'confirm',
      message: notice.message || 'Parece una copia de una carpeta que ya tienes',
      confirmable: true,
      scanned: false
    }
  }
  const messages = {
    already_there: [`${notice.message} — no hace falta añadirla otra vez`, 'info'],
    replaced: [`${notice.message} — se sustituyó por esta`, 'info'],
    added: ['Carpeta añadida. Analizando…', 'ok']
  }
  const [message, kind] = messages[r.action] || ['Carpeta añadida', 'ok']
  let scanned = false
  if (scan && r.action !== 'already_there') {
    await api.scan()
    scanned = true
  }
  return { action: r.action, message, kind, confirmable: false, scanned }
}
