// @ts-check
// Los términos y condiciones: si se aceptaron y la ventana para aceptarlos.
//
// Se enseñan solos la primera vez que se abre DanPlay (y otra vez si
// cambian). Sin aceptarlos se puede escuchar y ordenar la música, pero no
// separar en pistas, usar el asistente de IA ni «Transformar»: esas
// funciones llaman a `requireTerms()` antes de hacer nada, y si faltan, la
// ventana sale sola y espera a que se acepten (o no).
//
// Lo aceptado lo guarda el núcleo (`/api/terms`, un archivo de sus datos): así
// no se pierde aunque el WebView olvide lo suyo.
import { reactive, computed } from 'vue'
import { api } from '../api.js'
import { GATED, TERMS_VERSION } from '../data/terminos.js'

/** Ya se enseñaron al abrir (esa versión): no se vuelven a sacar solos. */
const SEEN_KEY = 'danplay.terminosVistos'

const state = reactive({
  /** ya se sabe lo que dice el núcleo */
  known: false,
  /** la versión aceptada ('' si ninguna) y cuándo (segundos) */
  accepted: '',
  at: /** @type {number|null} */ (null),
  /** la ventana de aceptarlos, abierta */
  open: false,
  /** lo que se quería hacer cuando salió («separar canciones en pistas») */
  reason: ''
})

/** @type {((ok: boolean) => void)[]} */
let waiting = []

const isAccepted = () => state.accepted === TERMS_VERSION

/** Lee del núcleo si se aceptaron. Devuelve si están aceptados. */
async function load() {
  try {
    const r = await api.terms()
    state.accepted = typeof r?.accepted === 'string' ? r.accepted : ''
    state.at = typeof r?.at === 'number' ? r.at : null
    state.known = true
  } catch {
    /* el núcleo aún no contesta: se vuelve a mirar cuando haga falta */
  }
  return isAccepted()
}

/** Abre la ventana. @param {string} [reason] lo que se quería hacer */
function show(reason = '') {
  state.reason = reason
  state.open = true
  try {
    localStorage.setItem(SEEN_KEY, TERMS_VERSION)
  } catch {
    /* sin almacenamiento: se volverán a enseñar al abrir, nada más */
  }
}

/** Cierra la ventana y contesta a quien esperaba. */
function close() {
  state.open = false
  state.reason = ''
  const ok = isAccepted()
  const w = waiting
  waiting = []
  for (const fn of w) fn(ok)
}

/** Aceptarlos (la casilla marcada tras leerlos hasta el final). */
async function accept() {
  const r = await api.acceptTerms(TERMS_VERSION)
  state.accepted = typeof r?.accepted === 'string' ? r.accepted : TERMS_VERSION
  state.at = typeof r?.at === 'number' ? r.at : Math.round(Date.now() / 1000)
  state.known = true
  close()
}

/**
 * Al abrir la app: si no se aceptaron y aún no se enseñaron (esta versión),
 * salen solos. Devuelve si se abrió la ventana.
 */
async function showIfFirstTime() {
  if (await load()) return false
  if (!state.known) return false
  let seen = ''
  try {
    seen = localStorage.getItem(SEEN_KEY) || ''
  } catch {
    /* sin almacenamiento, como si fuera la primera vez */
  }
  if (seen === TERMS_VERSION) return false
  show()
  return true
}

/**
 * Antes de una función que los pide: si ya se aceptaron, `true` sin más; si
 * no, se abre la ventana y se espera a que se acepten (`true`) o se cierre
 * sin aceptar (`false`).
 * @param {keyof typeof GATED} [feature]
 * @returns {Promise<boolean>}
 */
async function requireTerms(feature) {
  if (isAccepted()) return true
  if (await load()) return true
  show(feature ? GATED[feature] : '')
  return new Promise((resolve) => waiting.push(resolve))
}

/** Solo para pruebas: vuelve al estado del arranque. */
export function resetTerms() {
  Object.assign(state, { known: false, accepted: '', at: null, open: false, reason: '' })
  waiting = []
}

export { requireTerms }

export function useTerms() {
  return {
    state,
    accepted: computed(isAccepted),
    load,
    show,
    close,
    accept,
    showIfFirstTime,
    requireTerms
  }
}
