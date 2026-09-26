// @ts-check
// Donde se guarda lo que se descarga: se pregunta la primera vez, y a partir
// de ahí todo va a esa carpeta (docs/ARQUITECTURA.md, «La carpeta de
// descargas»).
//
// Antes no se preguntaba: iba a la carpeta de la biblioteca, que si nadie la
// había elegido era una suposición (~/Musica, la de Música del sistema), y lo
// descargado podía acabar en otro disco, fuera de las carpetas que la app
// mira, sin salir en ningún sitio. Lo usan Descargas, el asistente y Ajustes.
import { api, errorMessage, inTauri, pickFolder } from '../api.js'
import { ask } from '../composables/useDialog.js'
import { notify } from '../composables/useNotices.js'
import { addFolder } from './folders.js'

/** Las subcarpetas que usa lo descargado (las de `youtube.SUBFOLDERS` y Entrada/). */
export const SUBFOLDERS = ['Artistas', 'Pistas', 'Secuencias', 'Tutoriales y Play Along', 'Entrada']

/** @type {Record<string, string>} por qué hay que (volver a) elegirla */
const WHY = {
  unset: 'La primera vez hay que decirlo: a partir de ahí, todo lo que bajes irá a esa carpeta.',
  gone: 'La carpeta de descargas de antes ya no está (¿un disco sin conectar?).',
  unmanaged:
    'La carpeta de descargas de antes no es de tus carpetas de música: lo bajado no se vería.'
}

/**
 * Antes de bajar nada: si ya hay carpeta, sigue; si no, la pregunta.
 * @returns {Promise<boolean>} si se puede descargar
 */
export async function ensureDownloadFolder() {
  let status
  try {
    status = await api.downloadFolder()
  } catch {
    return true // sin respuesta del núcleo: que la descarga diga ella el error
  }
  return status.ready ? true : chooseDownloadFolder(status)
}

/**
 * Pregunta la carpeta de descargas y la guarda. Si no es de tus carpetas de
 * música, se añade (y se analiza) antes: si no, lo bajado no saldría.
 * @param {{ path?: string, reason?: string, suggested?: string }} [status]
 * @returns {Promise<boolean>} si quedó elegida
 */
export async function chooseDownloadFolder(status = {}) {
  const path = await ask({
    kind: 'prompt',
    title: '¿Dónde guardo lo que descargues?',
    message:
      (WHY[status.reason || ''] || 'Todo lo que bajes irá a esta carpeta.') +
      `\nDentro van ${SUBFOLDERS.slice(0, -1).join(', ')} y ${SUBFOLDERS.at(-1)}: ` +
      'se usan las que ya haya y se crean las que falten.',
    value: status.suggested || status.path || '',
    placeholder: '/ruta/a/tu/musica',
    okLabel: 'Guardar aquí',
    browse: inTauri ? () => pickFolder('¿Dónde guardo lo que descargues?') : undefined
  })
  if (!path) return false

  let added = await addFolder(path)
  if (added.action === 'confirm') {
    const again = await ask({
      kind: 'confirm',
      title: 'Parece una copia',
      message: `${added.message}. ¿Guardar las descargas ahí igualmente?`,
      okLabel: 'Sí, ahí'
    })
    if (!again) return false
    added = await addFolder(path, { force: true })
  }
  if (added.action === 'error') {
    notify(added.message)
    return false
  }
  try {
    const r = await api.setDownloadFolder(path)
    notify(`Lo que descargues irá a ${r.path}`, 'ok')
    return true
  } catch (e) {
    notify('No se pudo usar esa carpeta: ' + errorMessage(e))
    return false
  }
}
