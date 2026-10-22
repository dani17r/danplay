// @ts-check
// Traer al celular un `.danpista` (el archivo que hace «Convertir a DanPista» en
// la PC). Se elige con el selector del sistema —en Android llega como
// `content://`, de Descargas, de WhatsApp o de Drive— y Rust lo copia y lo
// abre (`import_bundle`, `engine/src/bundle.rs`).
import { inTauri, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'

/**
 * @typedef {Object} Imported  Lo que queda de una pista importada.
 * @property {string} id
 * @property {string} title
 * @property {string} artist
 * @property {string[]} parts
 * @property {boolean} click
 */

/**
 * Importa una ruta o un `content://`. Devuelve si se trajo.
 * @param {string} uri
 */
export async function importUri(uri) {
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    /** @type {Imported} */
    const s = await invoke('import_bundle', { uri: String(uri) })
    notify(
      `Lista: «${s.title}»${s.artist ? ' · ' + s.artist : ''} (${s.parts.length} partes${s.click ? ', con clic' : ''})`,
      'ok',
      5
    )
    return true
  } catch (e) {
    notify('No se pudo importar: ' + errorMessage(e))
    return false
  }
}

/**
 * Pide el archivo y lo importa. Devuelve cuántas se trajeron.
 * @returns {Promise<number>}
 */
export async function importBundles() {
  if (!inTauri) {
    notify('Importar pistas solo se puede en la app del celular')
    return 0
  }
  const { open } = await import('@tauri-apps/plugin-dialog')
  // sin filtro de tipo: Android no conoce «.danpista» y escondería el archivo
  const picked = await open({ multiple: true, title: 'Elige las pistas (.danpista)' })
  const list = picked == null ? [] : Array.isArray(picked) ? picked : [picked]
  let done = 0
  for (const uri of list) {
    if (await importUri(String(uri))) done++
  }
  return done
}
