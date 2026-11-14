// @ts-check
// Abrir un `.danpista` en la PC: la canción con sus pistas separadas, su
// clic, lo del estudio, la letra y el cifrado, en un solo archivo (el que se
// manda al celular). Antes solo lo sabía abrir la app del celular: lo que un
// compañero de la banda preparaba en su PC no se podía abrir en otra.
import { reactive } from 'vue'
import { api, errorMessage, JOBS, pickBundle } from '../api.js'
import { notify } from './useNotices.js'

/** Cómo va el que se está abriendo (para enseñarlo). */
const state = reactive({ busy: false, message: '', done: 0, total: 0 })

/**
 * Abre un `.danpista`: el de `path` o el que se elija. Devuelve la canción
 * ({id, title…}; con `existing` si ya estaba en la biblioteca) o null.
 * @param {string} [path]
 * @returns {Promise<{id: number, title: string, artist: string, tracks?: number, existing?: boolean} | null>}
 */
async function open(path) {
  if (state.busy) {
    notify('Ya se está abriendo un .danpista: espera a que termine', 'info')
    return null
  }
  const file = path || (await pickBundle())
  if (!file) return null
  state.busy = true
  state.message = 'Abriendo el .danpista…'
  try {
    const result = await api.runJob(() => api.importBundle(file), JOBS.bundleImport, {
      busy: 'aún se está abriendo otro .danpista: espera a que termine',
      onProgress: (job) => {
        state.message = job.message || state.message
        state.done = job.done || 0
        state.total = job.total || 0
      }
    })
    if (result.existing) {
      notify(`«${result.title}» ya estaba en tu biblioteca: te la enseño`, 'info', 6)
    } else {
      notify(
        `«${result.title}» lista: ${result.tracks} pistas, con su clic y lo del estudio`,
        'ok',
        6
      )
    }
    return result
  } catch (e) {
    notify('No se pudo abrir el .danpista: ' + errorMessage(e))
    return null
  } finally {
    state.busy = false
  }
}

export function useBundle() {
  return { state, open }
}
