// @ts-check
// Añadir una carpeta a la biblioteca, con lo que hay que decirle al usuario.
//
// Estaba escrito dos veces —en la bienvenida y en Ajustes— y la copia de
// Ajustes comparaba con los nombres en castellano que la API dejó de usar
// («confirmar», «ya_estaba», «reemplaza»), así que allí no salía ningún aviso
// y el caso «parece una copia» vaciaba el campo sin añadir nada ni explicar
// por qué. Ahora los nombres viven en un solo sitio.
// @ts-check
import { api, errorMessage, JOBS } from '../api.js'

/** Lo que la API responde en `action` (docs/CONTRATO-INTERNO.md §4). */
export const FOLDER_ACTIONS = ['added', 'already_there', 'replaced', 'confirm', 'relocated']

/**
 * @typedef {Object} FolderOutcome
 * @property {'added'|'already_there'|'replaced'|'confirm'|'relocated'|'error'} action
 * @property {string} message      qué contarle al usuario
 * @property {'ok'|'info'} [kind]  cómo pintarlo
 * @property {boolean} confirmable si hace falta insistir para añadirla
 * @property {boolean} scanned     si se llegó a analizar
 * @property {string} [scanError]  si el análisis falló, por qué
 */

/**
 * Añade la carpeta y devuelve qué pasó, ya traducido.
 *
 * Si hace falta, la analiza entera: es una tarea larga del núcleo (puede
 * tardar minutos con una biblioteca grande) y `onProgress` va contando cómo
 * va. Antes se esperaba dentro de una sola petición y, pasado un minuto, el
 * puente de Rust la cortaba y la bienvenida daba error aunque el análisis
 * siguiera.
 * @param {string} path
 * @param {{ force?: boolean, label?: string, scan?: boolean,
 *           onProgress?: (job: import('../api.js').JobSnapshot) => void }} [options]
 * @returns {Promise<FolderOutcome>}
 */
export async function addFolder(path, options = {}) {
  const { force = false, label = '', scan = true, onProgress } = options
  const clean = (path || '').trim()
  if (!clean)
    return { action: 'error', message: 'Escribe una ruta', confirmable: false, scanned: false }
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
  /** @type {Record<string, [string, 'ok'|'info']>} */
  const messages = {
    already_there: [`${notice.message} — no hace falta añadirla otra vez`, 'info'],
    replaced: [`${notice.message} — se sustituyó por esta`, 'info'],
    // era la carpeta de siempre, movida: vuelve todo, con sus listas y notas
    relocated: ['Encontrada: es tu carpeta de antes. Todo vuelve con sus listas y sus notas', 'ok'],
    added: ['Carpeta añadida. Analizando…', 'ok']
  }
  const [message, kind] = messages[r.action] || ['Carpeta añadida', 'ok']
  if (!scan || r.action === 'already_there') {
    return { action: r.action, message, kind, confirmable: false, scanned: false }
  }
  try {
    await api.runJob(() => api.scan(), JOBS.scan, { onProgress })
  } catch (e) {
    // la carpeta ya está añadida; lo que falló es analizarla
    return {
      action: r.action,
      message: 'Carpeta añadida, pero no se pudo analizar: ' + errorMessage(e),
      kind: 'info',
      confirmable: false,
      scanned: false,
      scanError: errorMessage(e)
    }
  }
  return { action: r.action, message, kind, confirmable: false, scanned: true }
}
