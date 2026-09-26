// @ts-check
// Separar canciones en pistas (batería, voces, bajo…), compartido por toda
// la interfaz.
//
// Es un singleton, como useDownloads: el modo estudio, el menú de una
// canción y Ajustes leen el MISMO estado, el que publica el núcleo en
// `/api/separate` (si se puede separar, los modelos y la cola). Separar tarda
// en torno a lo que dura la canción, así que el núcleo lleva una cola y aquí
// solo se mira cómo va: cada poco mientras haya algo, y nada cuando no.
//
// Al acabar cada canción se avisa a quien se haya apuntado (`onSeparated`):
// el modo estudio carga sus pistas en cuanto están.
import { reactive, computed } from 'vue'
import { api, errorMessage, JOBS } from '../api.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'

/**
 * @typedef {Object} SeparationModel
 * @property {'6'|'4'} id
 * @property {string} label
 * @property {string} detail
 * @property {string[]} [sources]
 * @property {number} [bytes]
 * @property {boolean} [installed]
 */
/**
 * @typedef {Object} QueueItem
 * @property {number} id
 * @property {string} title
 * @property {string} model
 * @property {number} [done]
 * @property {number} [total]
 * @property {string} [step]
 */

const IDLE = {
  known: false,
  ok: false,
  reason: '',
  /** @type {SeparationModel[]} */
  models: [],
  default: '6',
  folder: 'Separadas',
  /** @type {QueueItem|null} */
  current: null,
  /** @type {QueueItem[]} */
  queue: [],
  /** lo que dice la tarea: «Separando «X» (1 de 3)» */
  message: '',
  /** cuándo empezó la canción que se separa, para estimar lo que queda */
  since: 0
}

const state = reactive({ ...IDLE })
const listeners = new Set()
let timer = null
let failures = 0

/** Cada cuánto se mira mientras se separa algo: hay una barra que mover. */
const EVERY = 800
/** Cuántos fallos seguidos del núcleo antes de dejar de insistir. */
const GIVE_UP = 5

const busy = () => !!state.current || state.queue.length > 0

/** Lo que cuenta el núcleo (`/api/separate`), sin traerse nada más. */
function adopt(r) {
  if (!r || typeof r !== 'object') return
  for (const k of ['ok', 'reason', 'models', 'default', 'folder', 'current', 'queue']) {
    if (k in r) state[k] = r[k]
  }
  state.known = true
}

/** Una consulta al núcleo. Devuelve si contestó. */
async function refresh() {
  let r
  try {
    r = await api.separation()
  } catch {
    return false
  }
  const before = state.current?.id ?? null
  const waiting = new Set([before, ...state.queue.map((q) => q.id)].filter((x) => x != null))
  adopt({ ...IDLE, ...r })
  const now = state.current?.id ?? null
  if (now !== before) state.since = now == null ? 0 : Date.now()
  // las que estaban y ya no: se acabaron (bien o mal); la tarea dice cuáles
  const still = new Set([now, ...state.queue.map((q) => q.id)].filter((x) => x != null))
  const left = [...waiting].filter((id) => !still.has(id))
  if (left.length) await finished(left)
  if (busy()) {
    try {
      const job = await api.job(JOBS.separate)
      state.message = job?.message || ''
    } catch {
      /* sin mensaje: se enseña el título */
    }
  } else {
    state.message = ''
  }
  return true
}

/** Lo que salió de las que terminaron: se dice y se avisa a quien escuche. */
async function finished(ids) {
  let result = null
  try {
    result = (await api.job(JOBS.separate))?.result
  } catch {
    /* la tarea aún no cerró: se avisa sin detalles */
  }
  const separated = (result?.separated || []).filter((s) => ids.includes(s.id))
  const failed = (result?.failed || []).filter((s) => ids.includes(s.id))
  for (const s of separated) notify(`«${s.title}» ya está separada en pistas`, 'ok', 6)
  for (const f of failed) notify(`No se pudo separar «${f.title}»: ${f.error}`)
  for (const fn of listeners) {
    try {
      fn(ids, { separated, failed })
    } catch (e) {
      console.warn('[danplay] al terminar de separar:', e)
    }
  }
}

async function tick() {
  timer = null
  const answered = await refresh()
  if (!answered) {
    if (++failures < GIVE_UP) schedule()
    return
  }
  failures = 0
  if (busy()) schedule()
}

function schedule(ms = EVERY) {
  clearTimeout(timer)
  timer = setTimeout(tick, ms)
}

/** Lo que pesa un modelo, para decirlo antes de bajarlo. @param {number} [bytes] */
function megas(bytes) {
  return bytes ? `${Math.round(bytes / 1e6)} MB` : ''
}

/**
 * Separa esas canciones (a la cola). La primera vez que se usa un modelo,
 * antes se pregunta: hay que bajarlo, y se dice cuánto pesa.
 * @param {Array<{id: number, title?: string, file?: string}>} songs
 * @param {'6'|'4'} [model]
 * @returns {Promise<boolean>} si se pidió
 */
async function request(songs, model) {
  if (!state.known) await refresh()
  if (!state.ok) {
    notify('No se puede separar en pistas: ' + (state.reason || 'el núcleo no responde'))
    return false
  }
  const chosen = /** @type {'6'|'4'} */ (model || state.default || '6')
  const info = state.models.find((m) => m.id === chosen)
  if (info && info.installed === false) {
    const ok = await ask({
      kind: 'confirm',
      title: 'Separar en pistas',
      message:
        `La primera vez hay que bajar el separador de ${info.label} ` +
        `(${megas(info.bytes)}, una sola vez). Luego todo se hace en tu equipo, sin internet.`,
      detail:
        'Separar tarda en torno a lo que dura la canción. Puedes seguir usando DanPlay ' +
        'mientras tanto: las pistas quedan en una carpeta junto a tu música.',
      okLabel: 'Bajar y separar'
    })
    if (!ok) return false
  }
  let asked = 0
  for (const song of songs) {
    try {
      adopt(await api.separate(song.id, chosen))
      asked++
    } catch (e) {
      notify(`No se pudo pedir «${song.title || song.file}»: ${errorMessage(e)}`)
    }
  }
  if (asked) {
    if (!state.since && state.current) state.since = Date.now()
    notify(
      asked > 1
        ? `${asked} canciones a la cola: se separan una detrás de otra`
        : 'Separando en pistas: tardará en torno a lo que dura la canción',
      'info',
      5
    )
    failures = 0
    schedule(200)
  }
  return asked > 0
}

/** Para lo que se esté separando y vacía la cola. */
async function cancel() {
  try {
    adopt(await api.cancelSeparation())
  } catch (e) {
    notify('No se pudo parar: ' + errorMessage(e))
  }
  schedule(300)
}

/** Quita de la cola una que aún espera. @param {number} id */
async function unqueue(id) {
  try {
    adopt(await api.unqueueSeparation(id))
  } catch (e) {
    notify(errorMessage(e))
  }
}

/**
 * Cómo va una canción: null si no está en la cola; si no, `waiting` (en la
 * cola), o la parte hecha (0..1) si es la que se separa.
 * @param {number|null|undefined} id
 */
function progressOf(id) {
  if (id == null) return null
  const c = state.current
  if (c && c.id === id) {
    const fraction = c.total ? Math.min(1, (c.done || 0) / c.total) : 0
    return { waiting: false, step: c.step || 'separate', fraction }
  }
  const at = state.queue.findIndex((q) => q.id === id)
  return at >= 0 ? { waiting: true, place: at + 1, step: 'queue', fraction: 0 } : null
}

/**
 * Lo que queda de la que se separa, en segundos, sacado de lo que lleva: o
 * null si aún no se puede saber.
 */
function remaining() {
  const c = state.current
  if (!c || !c.total || !c.done || !state.since || c.step === 'download') return null
  const took = (Date.now() - state.since) / 1000
  return Math.max(0, (took / c.done) * (c.total - c.done))
}

/**
 * Qué hacer cuando termina alguna (bien o mal): recibe sus ids y lo que salió.
 * Devuelve cómo desapuntarse.
 * @param {(ids: number[], outcome: {separated: any[], failed: any[]}) => void} fn
 */
function onSeparated(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

/** Vuelve al estado inicial. Solo para pruebas. */
export function resetSeparation() {
  clearTimeout(timer)
  timer = null
  failures = 0
  listeners.clear()
  Object.assign(state, IDLE, { models: [], queue: [], current: null })
}

export function useSeparation() {
  if (!state.known && !timer) schedule(0)
  return {
    state,
    busy: computed(busy),
    available: computed(() => state.ok),
    refresh,
    request,
    cancel,
    unqueue,
    progressOf,
    remaining,
    onSeparated,
    megas
  }
}
