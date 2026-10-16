// @ts-check
// Abrir una secuencia descargada (la carpeta de MultiTracks.com, Loop
// Community…: las pistas, el «Click Track», la «Guide» y el proyecto de
// Ableton). El núcleo la deja lista para tocar: la canción en Secuencias/,
// sus pistas como pistas separadas (el clic y la guía, a la salida del clic),
// sus partes como marcadores y su clic exacto. Aquí: elegir la carpeta,
// enseñar lo que trae, abrirla con su avance y ponerla en el estudio.
import { reactive } from 'vue'
import { api, errorMessage, JOBS, pickFolder } from '../api.js'
import { ask } from './useDialog.js'
import { notify } from './useNotices.js'

/** Cómo va la que se está abriendo (para enseñarlo). */
const state = reactive({ busy: false, message: '', done: 0, total: 0 })

/**
 * Lo que se dice antes de abrirla.
 * @param {Awaited<ReturnType<typeof api.sequenceInfo>>} s
 */
function describe(s) {
  const roles = s.tracks.filter((t) => t.role).map((t) => t.name)
  const parts = [
    `${s.tracks.length} pistas${roles.length ? ` (${roles.join(' y ')} van a tus cascos)` : ''}`,
    s.bpm ? `${Math.round(s.bpm * 100) / 100} bpm` : '',
    s.key ? `en ${s.key}` : ''
  ].filter(Boolean)
  return parts.join(' · ')
}

/**
 * Abre una secuencia: la de `path` (una carpeta o un archivo de ella) o la
 * que se elija. Devuelve la canción hecha ({id, title…}) o null.
 * @param {string} [path]
 * @returns {Promise<{id: number, title: string, artist: string, tracks: number} | null>}
 */
async function open(path) {
  if (state.busy) {
    notify('Ya se está abriendo una secuencia', 'info')
    return null
  }
  const folder = path || (await pickFolder('Elige la carpeta de la secuencia'))
  if (!folder) return null
  let info
  try {
    info = await api.sequenceInfo(folder)
  } catch (e) {
    notify('Esa carpeta no parece una secuencia: ' + errorMessage(e))
    return null
  }
  const ok = await ask({
    kind: 'confirm',
    title: 'Abrir secuencia',
    message: `«${info.title}»${info.artist ? ' de ' + info.artist : ''}: ${describe(info)}.`,
    detail:
      (info.sections.length
        ? `Sus partes (${info.sections.slice(0, 8).join(', ')}${info.sections.length > 8 ? '…' : ''}) quedan como marcadores y el clic, exacto. `
        : '') +
      'La canción queda en tu biblioteca, en «Secuencias», con cada pista lista para el estudio: ' +
      'el clic y la guía salen por la salida del clic (en «Salidas» eliges cuál).',
    okLabel: 'Abrir'
  })
  if (!ok) return null
  state.busy = true
  state.message = `Abriendo «${info.title}»…`
  try {
    const result = await api.runJob(() => api.importSequence(info.folder), JOBS.sequence, {
      onProgress: (job) => {
        state.message = job.message || state.message
        state.done = job.done || 0
        state.total = job.total || 0
      }
    })
    notify(`«${result.title}» lista: ${result.tracks} pistas, con sus partes y su clic`, 'ok', 6)
    return result
  } catch (e) {
    notify('No se pudo abrir la secuencia: ' + errorMessage(e))
    return null
  } finally {
    state.busy = false
  }
}

export function useSequence() {
  return { state, open }
}
