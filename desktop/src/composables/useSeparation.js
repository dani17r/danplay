// @ts-check
// Separar canciones en pistas (batería, voces, bajo…), compartido por toda
// la interfaz.
//
// Es un singleton, como useDownloads: el modo estudio, el menú de una
// canción y Ajustes leen el MISMO estado, el que publica el núcleo en
// `/api/separate` (si se puede separar, lo que pesa el separador y la cola).
// Separar tarda, así que el núcleo lleva una cola y aquí solo se mira cómo
// va: cada poco mientras haya algo, y nada cuando no.
//
// Cada canción pasa dos veces: la rápida (en unos minutos ya hay pistas) y
// la buena (las mejora). Al acabar cada pasada se avisa a quien se haya
// apuntado (`onSeparated`): el modo estudio carga las pistas en cuanto están,
// y las cambia por las buenas cuando llegan.
import { reactive, computed } from 'vue'
import { api, errorMessage, JOBS } from '../api.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'

/**
 * @typedef {Object} QueueItem
 * @property {number} id
 * @property {string} title
 * @property {'separate'|'refine'} stage  la pasada rápida (y luego la buena), o solo mejorarlas
 * @property {number} [done]
 * @property {number} [total]
 * @property {string} [step]
 */
/**
 * @typedef {Object} SeparationEvent  Una que acabó, bien o mal.
 * @property {number} seq
 * @property {number} id
 * @property {string} title
 * @property {'separate'|'refine'} stage
 * @property {boolean} ok
 * @property {string} [error]
 */

const IDLE = {
  known: false,
  ok: false,
  reason: '',
  /** lo que pesa el separador entero, y lo que falta por bajar */
  bytes: 0,
  pending: 0,
  installed: false,
  folder: 'Separadas',
  /** en qué se guardan las pistas, y si este ffmpeg sabe hacer Opus */
  format: 'flac',
  opus: false,
  /** @type {QueueItem|null} */
  current: null,
  /** @type {QueueItem[]} */
  queue: [],
  /** lo que dice la tarea: «Separando «X» (1 de 3)» */
  message: '',
  /** cuándo empezó la pasada que va, para estimar lo que queda */
  since: 0
}

const state = reactive({ ...IDLE })
const listeners = new Set()
let timer = null
let failures = 0
/** El último aviso del núcleo que ya se contó (-1: aún ninguno). */
let lastSeq = -1

/** Cada cuánto se mira mientras se separa algo: hay una barra que mover. */
const EVERY = 800
/** Cuántos fallos seguidos del núcleo antes de dejar de insistir. */
const GIVE_UP = 5

const busy = () => !!state.current || state.queue.length > 0
/** La pasada que va ahora, para saber cuándo empieza otra. */
const running = () => (state.current ? `${state.current.id}:${state.current.stage}` : '')

/** Lo que cuenta el núcleo (`/api/separate`), sin traerse nada más. */
function adopt(r) {
  if (!r || typeof r !== 'object') return
  for (const k of [
    'ok',
    'reason',
    'bytes',
    'pending',
    'installed',
    'folder',
    'format',
    'opus',
    'current',
    'queue'
  ]) {
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
  const before = running()
  adopt({ ...IDLE, ...r })
  if (running() !== before) state.since = state.current ? Date.now() : 0
  await told(/** @type {SeparationEvent[]} */ (r?.events || []))
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

/**
 * Lo que acabó desde la última vez: se dice y se avisa a quien escuche. Lo
 * que ya había al abrir la app no se cuenta (era de antes).
 * @param {SeparationEvent[]} events
 */
async function told(events) {
  const top = events.reduce((m, e) => Math.max(m, e.seq), 0)
  if (lastSeq < 0) {
    lastSeq = top
    return
  }
  // el núcleo volvió a arrancar: empieza a contar de cero
  if (top < lastSeq) lastSeq = 0
  const fresh = events.filter((e) => e.seq > lastSeq)
  if (top > lastSeq) lastSeq = top
  if (!fresh.length) return
  for (const e of fresh) {
    if (!e.ok) {
      const what = e.stage === 'refine' ? 'mejorar las pistas de' : 'separar'
      notify(`No se pudo ${what} «${e.title}»: ${e.error}`)
    } else if (e.stage === 'refine') {
      notify(`Las pistas de «${e.title}» ya tienen la mejor calidad`, 'ok', 4)
    } else {
      notify(`«${e.title}» ya está separada en pistas`, 'ok', 6)
    }
  }
  const ids = [...new Set(fresh.map((e) => e.id))]
  for (const fn of listeners) {
    try {
      fn(ids, fresh)
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

/** Lo que pesa algo, para decirlo antes de bajarlo. @param {number} [bytes] */
function megas(bytes) {
  return bytes ? `${Math.round(bytes / 1e6)} MB` : ''
}

/**
 * Separa esas canciones (a la cola), con la mejor calidad: las que ya tienen
 * las pistas rápidas, solo se mejoran. La primera vez se pregunta antes:
 * hay que bajar el separador, y se dice cuánto pesa.
 * @param {Array<{id: number, title?: string, file?: string}>} songs
 * @returns {Promise<boolean>} si se pidió
 */
async function request(songs) {
  if (!state.known) await refresh()
  if (!state.ok) {
    notify('No se puede separar en pistas: ' + (state.reason || 'el núcleo no responde'))
    return false
  }
  if (state.pending > 0) {
    const ok = await ask({
      kind: 'confirm',
      title: 'Separar pistas',
      message:
        `La primera vez hay que bajar el separador (${megas(state.pending)}, una sola vez). ` +
        'Luego todo se hace en tu equipo, sin internet.',
      detail:
        'En unos minutos tienes las pistas para usarlas; después se siguen mejorando la ' +
        'batería y el bajo, sin cortar lo que suena. Puedes seguir usando DanPlay ' +
        'mientras tanto: las pistas quedan en una carpeta junto a tu música.',
      okLabel: 'Bajar y separar'
    })
    if (!ok) return false
  }
  let asked = 0
  for (const song of songs) {
    try {
      adopt(await api.separate(song.id))
      asked++
    } catch (e) {
      notify(`No se pudo pedir «${song.title || song.file}»: ${errorMessage(e)}`)
    }
  }
  if (asked) {
    if (!state.since && state.current) state.since = Date.now()
    notify(
      asked > 1
        ? `${asked} canciones a la cola: primero todas se separan, luego se mejoran`
        : 'Separando en pistas: en unos minutos las tienes, y luego se mejoran',
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
 * Cómo va una canción: null si no está en la cola; si no, si espera (y en
 * qué puesto) o la parte hecha (0..1) de la pasada que va, y cuál es:
 * `refine` es mejorar unas pistas que ya se pueden usar.
 * @param {number|null|undefined} id
 */
function progressOf(id) {
  if (id == null) return null
  const c = state.current
  if (c && c.id === id) {
    const fraction = c.total ? Math.min(1, (c.done || 0) / c.total) : 0
    return { waiting: false, stage: c.stage, step: c.step || 'separate', fraction }
  }
  const at = state.queue.findIndex((q) => q.id === id)
  if (at < 0) return null
  return { waiting: true, stage: state.queue[at].stage, place: at + 1, step: 'queue', fraction: 0 }
}

/**
 * Lo que queda de la pasada que va, en segundos, sacado de lo que lleva: o
 * null si aún no se puede saber.
 */
function remaining() {
  const c = state.current
  if (!c || !c.total || !c.done || !state.since || c.step === 'download') return null
  const took = (Date.now() - state.since) / 1000
  return Math.max(0, (took / c.done) * (c.total - c.done))
}

/**
 * Qué hacer cuando acaba una pasada de alguna (bien o mal): recibe sus ids
 * y lo que pasó. Devuelve cómo desapuntarse.
 * @param {(ids: number[], events: SeparationEvent[]) => void} fn
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
  lastSeq = -1
  listeners.clear()
  Object.assign(state, IDLE, { queue: [], current: null })
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
