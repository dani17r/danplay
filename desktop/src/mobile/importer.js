// @ts-check
// Traer al celular un `.danpista` (el archivo que hace «Exportar…» › «Para el
// celular» en la PC). Se elige con el selector del sistema —en Android llega como
// `content://`, de Descargas, de WhatsApp o de Drive— y Rust lo copia y lo
// abre (`import_bundle`, `engine/src/bundle.rs`).
//
// De una en una, y diciéndolo mientras dura. Copiar y abrir una pista son
// unos segundos en los que no se veía nada: se volvía a tocar «Importar» (o
// llegaba otro archivo desde WhatsApp) y las dos importaciones iban a la vez.
import { ref } from 'vue'
import { inTauri, errorMessage } from '../api.js'
import { notify, dismiss } from '../composables/useNotices.js'

/**
 * @typedef {Object} Imported  Lo que queda de una pista importada.
 * @property {string} id
 * @property {string} title
 * @property {string} artist
 * @property {string[]} parts
 * @property {boolean} click
 */

/** Cuántas importaciones hay en marcha o esperando su turno. */
export const importing = ref(0)
/** La cola: cada importación empieza cuando acaba la anterior. */
let chain = Promise.resolve()
/** Lo que se queda el aviso de «Importando…» si nadie lo quita (no debería). */
const WORKING_SECONDS = 600

/**
 * Importa una ruta o un `content://`, cuando le toque. Devuelve si se trajo.
 * @param {string} uri
 * @param {string} [which]  cuál es de las que se eligieron («2 de 5»)
 * @returns {Promise<boolean>}
 */
export function importUri(uri, which = '') {
  importing.value++
  const job = chain.then(async () => {
    const working = notify(`Importando${which ? ' ' + which : ''}…`, 'info', WORKING_SECONDS)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
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
    } finally {
      dismiss(working)
      importing.value--
    }
  })
  // la siguiente espera a esta, haya ido bien o mal
  chain = job.then(
    () => {},
    () => {}
  )
  return job
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
  const done = await Promise.all(
    list.map((uri, i) =>
      importUri(String(uri), list.length > 1 ? `${i + 1} de ${list.length}` : '')
    )
  )
  return done.filter(Boolean).length
}
