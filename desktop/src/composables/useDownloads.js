// Lo que se está bajando, compartido por toda la interfaz.
//
// Es un singleton, como usePlayback: la página de Descargas, el chat y la
// barra lateral leen el MISMO estado (el que publica el núcleo en
// `/api/youtube`), y hay un solo sitio que lo consulta. Antes cada página
// tenía su propio temporizador y la barra lateral no se enteraba de nada.
//
// No hay sondeo de fondo: las descargas solo las arranca esta interfaz, así
// que quien arranca una llama a `wake()` y a partir de ahí se consulta cada
// poco hasta que termina. Al terminar se avisa a quien se haya apuntado con
// `onFinished` y se deja de preguntar.
import { reactive, computed } from 'vue'
import { api } from '../api.js'

const IDLE = {
  available: true,
  reason: '',
  quality: 'high',
  active: false,
  phase: '',
  name: '',
  percent: 0,
  index: 0,
  total: 0,
  results: [],
  error: ''
}

const state = reactive({ ...IDLE, known: false })
const finished = new Set()
let timer = null
let wasActive = false
let armed = false // alguien acaba de arrancar una y aún no la hemos visto
let failures = 0

/** Cada cuánto se mira mientras baja algo: hay una barra que mover. */
const EVERY = 700
/** Cuántos fallos seguidos del núcleo antes de dejar de insistir. */
const GIVE_UP = 5

function snapshot() {
  return { ...state, results: (state.results || []).slice() }
}

function fire() {
  const s = snapshot()
  for (const fn of finished) {
    try {
      fn(s)
    } catch (e) {
      console.warn('[danplay] al terminar la descarga:', e)
    }
  }
}

/** Una consulta al núcleo. Devuelve si contestó. */
async function refresh() {
  let e
  try {
    e = await api.youtube()
  } catch {
    return false // el núcleo aún no responde
  }
  Object.assign(state, IDLE, e, { known: true })
  if (wasActive && !state.active) {
    armed = false
    fire()
  }
  wasActive = !!state.active
  // si hay algo en marcha (una consulta suelta al arrancar), se sigue solo
  if (state.active && !timer) schedule()
  return true
}

async function tick() {
  timer = null
  const answered = await refresh()
  if (!answered) {
    if (++failures < GIVE_UP) schedule()
    return
  }
  failures = 0
  if (state.active) {
    armed = false
    schedule()
    return
  }
  if (armed) {
    // arrancó y terminó entre dos miradas (o falló al momento): se cuenta igual
    armed = false
    fire()
  }
}

function schedule(ms = EVERY) {
  clearTimeout(timer)
  timer = setTimeout(tick, ms)
}

/** Alguien acaba de arrancar una descarga: se sigue hasta que acabe. */
function wake() {
  armed = true
  failures = 0
  schedule(0)
}

/**
 * Qué hacer cuando termina una descarga (la que sea). Devuelve cómo
 * desapuntarse. Recibe una foto del estado con los `results`.
 * @param {(s: any) => void} fn
 */
function onFinished(fn) {
  finished.add(fn)
  return () => finished.delete(fn)
}

/** Vuelve al estado inicial. Solo para pruebas. */
export function resetDownloads() {
  clearTimeout(timer)
  timer = null
  wasActive = false
  armed = false
  failures = 0
  finished.clear()
  Object.assign(state, IDLE, { known: false, results: [] })
}

export function useDownloads() {
  return {
    state,
    active: computed(() => !!state.active),
    wake,
    refresh,
    onFinished,
    snapshot
  }
}
