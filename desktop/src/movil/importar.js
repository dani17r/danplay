// @ts-check
// Traer al celular un `.danpista` (el archivo que hace «Llevar al celular» en
// la PC). Se elige con el selector del sistema —en Android llega como
// `content://`, de Descargas, de WhatsApp o de Drive— y Rust lo copia y lo
// abre (`import_pista`, `motor/src/bundle.rs`).
import { inTauri } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { errorMessage } from '../api.js'

/**
 * @typedef {Object} Imported  Lo que queda de una pista importada.
 * @property {string} id
 * @property {string} title
 * @property {string} artist
 * @property {string[]} parts
 * @property {boolean} click
 */

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
  const { invoke } = await import('@tauri-apps/api/core')
  // sin filtro de tipo: Android no conoce «.danpista» y escondería el archivo
  const picked = await open({ multiple: true, title: 'Elige las pistas (.danpista)' })
  const list = picked == null ? [] : Array.isArray(picked) ? picked : [picked]
  let done = 0
  for (const uri of list) {
    try {
      /** @type {Imported} */
      const s = await invoke('import_pista', { uri: String(uri) })
      done++
      notify(
        `Lista: «${s.title}»${s.artist ? ' · ' + s.artist : ''} (${s.parts.length} partes${s.click ? ', con clic' : ''})`,
        'ok',
        5
      )
    } catch (e) {
      notify('No se pudo importar: ' + errorMessage(e))
    }
  }
  return done
}
