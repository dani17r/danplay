/**
 * El clic del metrónomo, con el panel de estudio abierto o cerrado.
 *
 * Tres modos:
 *
 * - **Automático** (lo normal): cada canción con su clic. La rejilla la da
 *   Beat This! en el núcleo; la primera vez de cada canción tarda 15-20 s, y
 *   mientras tanto el clic NO suena (ni el de la canción anterior, que iría
 *   a otro tempo): se calla, el botón no deja ponerlo y la onda enseña una
 *   barra verde con lo que lleva. Al llegar, vuelve solo si estaba puesto.
 *   Con sus ajustes guardados (compás, «el 1», doble/mitad). Con el clic
 *   puesto, las siguientes de la cola se dejan preparadas: al pasar a ellas
 *   ya está. Si aquí no hay Beat This! (o no le sacó pulso), la de Rust, al
 *   momento.
 * - **Propio**: el de la canción ajustado a mano, pulso a pulso, sobre la
 *   onda, y guardado con ella (`metronome.own` de su estudio). Si una canción
 *   aún no tiene el suyo, la automática (y al tocar un pulso pasa a ser suyo).
 * - **A mano**: un tempo para todas, el que se ponga. Al pasar de canción
 *   sigue sonando igual: ni se espera ni se cambia nada aunque llegue la
 *   rejilla de la otra.
 *
 * Antes la rejilla la pedía el panel de estudio y solo mientras estaba
 * abierto: con él cerrado, al pasar de canción el clic seguía suelto a 100
 * bpm (y con los ajustes de la anterior).
 */
import { effectScope, ref, shallowRef, watch } from 'vue'
import { api } from '../api.js'
import { usePlayback } from './usePlayback.js'
import { ownToGrid } from '../utils/beats.js'

const AUTO_KEY = 'danplay.click.auto'
const MODE_KEY = 'danplay.click.mode'
const MANUAL_KEY = 'danplay.click.manual'
/** Cada cuánto se pregunta por la rejilla que se está calculando. */
export const POLL_MS = 700
/** Cuántas de las siguientes de la cola se dejan preparadas. */
export const AHEAD = 3

function readPref(key, fallback) {
  try {
    const v = localStorage.getItem(key)
    return v == null ? fallback : JSON.parse(v)
  } catch {
    return fallback
  }
}
function savePref(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* modo privado */
  }
}
/** El tempo y el compás del modo a mano, con sus topes. @param {any} m */
function cleanManual(m) {
  const bpm = Number(m?.bpm)
  const meter = Number(m?.meter)
  return {
    bpm: Number.isFinite(bpm) && bpm >= 20 && bpm <= 300 ? Math.round(bpm * 100) / 100 : 100,
    meter: Number.isInteger(meter) && meter >= 0 && meter <= 12 ? meter : 4
  }
}
/** Lo guardado con la canción (`study`, en JSON), o nada. @param {any} raw */
function songMetronome(raw) {
  try {
    const s = raw ? JSON.parse(raw) : {}
    return s?.metronome && typeof s.metronome === 'object' ? { ...s.metronome } : {}
  } catch {
    return {}
  }
}

/** El modo guardado (o el de antes: «Clic automático» sí o no). */
function savedMode() {
  const m = readPref(MODE_KEY, null)
  if (m === 'auto' || m === 'own' || m === 'manual') return m
  return readPref(AUTO_KEY, true) === false ? 'manual' : 'auto'
}
/** 'auto' (el de cada canción), 'own' (el ajustado a mano) o 'manual' (uno para todas). */
const mode = ref(savedMode())
/** Automático o propio (cada canción con el suyo) frente a a mano (uno para todas). */
const auto = ref(mode.value !== 'manual')
/** El tempo y el compás del modo a mano. */
const manual = ref(cleanManual(readPref(MANUAL_KEY, {})))
/** El clic pedido. Puede no sonar aún: en automático, mientras carga. */
const wanted = ref(false)
/** La rejilla de la canción que suena, ya en Rust (para pintarla). */
const grid = shallowRef(/** @type {import('../api.js').BeatGrid|null} */ (null))
/** De dónde salió: 'beat-this', 'rust', 'own' (el propio) o ''. */
const source = ref('')
/** La rejilla de la que suena, en camino (en automático). */
const loading = ref(false)
/** Lo que lleva (0..1) mientras carga. */
const progress = ref(0)
/** Por qué no hay rejilla, si falló todo. */
const error = ref('')

/** De qué canción es lo de arriba. */
let forSong = { id: /** @type {number|null} */ (null), path: '' }
/** De qué canción se pusieron ya sus ajustes (una vez por canción). */
let settingsFor = /** @type {number|null} */ (null)
/** Cuántos quieren la rejilla aunque el clic esté quitado (el panel de estudio). */
let viewers = 0
/** Tras qué canción se prepararon ya las siguientes (una vez por canción). */
let aheadOf = /** @type {number|null} */ (null)
let pollTimer = /** @type {ReturnType<typeof setTimeout>|null} */ (null)
let started = false

const player = () => usePlayback()
const sameSong = (id, path) => forSong.id === id && forSong.path === path

function stopPolling() {
  if (pollTimer) clearTimeout(pollTimer)
  pollTimer = null
}

/**
 * Pone los ajustes guardados con la canción (tempo, compás, «1», doble o
 * mitad): una vez al llegar a ella. Después manda lo que se toque.
 * @param {number} id
 */
async function applySongSettings(id) {
  // con el panel de estudio abierto los pone él (`songSettings`): ya ha
  // leído la canción, y así no se lee dos veces
  if (settingsFor === id || viewers > 0) return
  settingsFor = id
  let saved = {}
  try {
    saved = songMetronome((await api.song(id))?.study)
  } catch {
    /* sin ficha (abierta desde fuera): lo de por defecto */
  }
  if (settingsFor !== id || !auto.value) return
  await player().resetMetronomeOverrides(saved)
}

/**
 * Los ajustes de clic de una canción, ya leídos por quien la tiene abierta
 * (el panel de estudio). Una vez por canción; con `again`, otra vez (al
 * volver a automático, que a mano se usaba otro tempo).
 * @param {number} id @param {any} saved @param {boolean} [again]
 */
function songSettings(id, saved, again = false) {
  if (!again && settingsFor === id) return Promise.resolve()
  settingsFor = id
  if (!auto.value) return Promise.resolve()
  return player().resetMetronomeOverrides({ ...(saved || {}) })
}

/** Lo guardado con una canción (su estudio), leído hace poco. */
const studies = new Map()
async function songStudy(id) {
  const hit = studies.get(id)
  if (hit && Date.now() - hit.at < 4000) return hit.study
  let study
  try {
    const raw = (await api.song(id))?.study
    study = raw ? JSON.parse(raw) : {}
  } catch {
    study = {}
  }
  studies.set(id, { study, at: Date.now() })
  if (studies.size > 20) studies.delete(studies.keys().next().value)
  return study
}

/**
 * Ya hay rejilla: se queda, y si el clic estaba pedido, suena (entra en el
 * siguiente pulso de la canción).
 * @param {any} g @param {string} from
 */
async function settle(g, from) {
  stopPolling()
  grid.value = g
  source.value = g ? from : ''
  loading.value = false
  progress.value = g ? 1 : 0
  if (!g && !error.value) error.value = 'no se encontró el pulso'
  await player().setMetronome(wanted.value ? { on: true } : {})
  if (wanted.value) prepareAhead()
}

/** La de Rust (medio segundo): cuando aquí no hay Beat This!. */
async function fromRust(id, path) {
  let g = null
  try {
    g = await player().analyzeBeats(player().track.value?.bpm || null, path)
  } catch (e) {
    if (sameSong(id, path)) error.value = e instanceof Error ? e.message : String(e)
  }
  if (sameSong(id, path)) await settle(g, 'rust')
}

/** La de Beat This! (o la propia), a Rust; si no vale, la de Rust. @param {any} found */
async function adopt(id, path, found, from = 'beat-this') {
  let g = null
  try {
    g = await player().setBeatGrid(found, path)
  } catch {
    /* no pasó la revisión de Rust: la suya */
  }
  if (!sameSong(id, path)) return
  if (g) await settle(g, from)
  else await fromRust(id, path)
}

/** Pregunta al núcleo: la rejilla, o por dónde va. */
async function ask(id, path) {
  pollTimer = null
  /** @type {any} sin Beat This! aquí, o la canción no le dio pulso: null */
  let r = null
  try {
    r = await api.beatGridNow(id)
  } catch {
    /* se queda en null: la de Rust */
  }
  if (!sameSong(id, path) || !auto.value) return
  if (r && r.pending) {
    progress.value = Math.max(progress.value, Number(r.progress) || 0)
    pollTimer = setTimeout(() => ask(id, path), POLL_MS)
    return
  }
  if (r) await adopt(id, path, r)
  else await fromRust(id, path)
}

/**
 * La rejilla de la canción que suena, en automático. Mientras llega, el
 * clic se calla (lo pedido sigue pedido: vuelve solo al llegar).
 */
async function load() {
  const p = player()
  const id = p.track.value?.id ?? null
  const path = p.state.path
  if (!auto.value || id == null || !path) return
  if (sameSong(id, path) && (grid.value || loading.value)) return
  stopPolling()
  forSong = { id, path }
  grid.value = null
  source.value = ''
  error.value = ''
  progress.value = 0
  loading.value = true
  // el de la anterior no puede seguir sonando sobre esta: iría a su tempo
  if (p.metronomeSent().on) await p.setMetronome({ on: false })
  await applySongSettings(id)
  if (!sameSong(id, path)) return
  // el propio, si la canción lo tiene
  if (mode.value === 'own') {
    const own = ownToGrid((await songStudy(id))?.metronome?.own)
    if (!sameSong(id, path)) return
    if (own) {
      await adopt(id, path, own, 'own')
      if (wanted.value) prepareAhead()
      return
    }
  }
  await ask(id, path)
  if (wanted.value) prepareAhead()
}

/** Las siguientes de la cola, preparadas (sin aleatorio: ahí no se sabe cuál). */
function prepareAhead() {
  const p = player()
  if (!auto.value || p.shuffle.value || aheadOf === forSong.id) return
  aheadOf = forSong.id
  const list = p.queue.value
  const at = p.state.index
  if (!Array.isArray(list) || at < 0) return
  for (let k = 1; k <= AHEAD; k++) {
    let i = at + k
    if (i >= list.length) {
      if (p.repeat.value !== 'list') break
      i -= list.length
    }
    const next = list[i]
    if (next?.id != null && next.id !== forSong.id) {
      api.beatGridPrepare(next.id).catch(() => {})
    }
  }
}

/** Pedir o quitar el clic. En automático, mientras carga, no hace nada. */
async function toggle() {
  const p = player()
  if (auto.value && loading.value) return
  const on = !wanted.value
  wanted.value = on
  if (!on) return p.setMetronome({ on: false })
  if (!auto.value) return p.setMetronome({ on: true })
  if (grid.value && sameSong(p.track.value?.id ?? null, p.state.path)) {
    await p.setMetronome({ on: true })
    prepareAhead()
    return
  }
  await load()
}

/**
 * Automático o a mano. Al pasar a mano se queda el tempo que se oía, para
 * que el clic no cambie de golpe; al volver a automático, cada canción con
 * el suyo otra vez.
 * @param {boolean} on
 */
async function setAuto(on) {
  if (on === auto.value) return
  auto.value = on
  savePref(AUTO_KEY, on)
  if (!on) {
    mode.value = 'manual'
    savePref(MODE_KEY, 'manual')
  } else if (mode.value === 'manual') {
    mode.value = 'auto'
    savePref(MODE_KEY, 'auto')
  }
  const p = player()
  if (!on) {
    stopPolling()
    loading.value = false
    const heard = p.metronome.value
    manual.value = cleanManual({
      bpm: heard?.bpm || manual.value.bpm,
      meter: heard?.meter ?? manual.value.meter
    })
    savePref(MANUAL_KEY, manual.value)
    await p.resetMetronomeOverrides({ ...manual.value, on: wanted.value })
    return
  }
  settingsFor = null
  forSong = { id: null, path: '' }
  grid.value = null
  await load()
}

/**
 * El tempo o el compás del modo a mano: para todas las canciones.
 * @param {{bpm?: number, meter?: number}} patch
 */
function setManual(patch) {
  manual.value = cleanManual({ ...manual.value, ...patch })
  savePref(MANUAL_KEY, manual.value)
  if (auto.value) return Promise.resolve()
  return player().setMetronome({ bpm: manual.value.bpm, meter: manual.value.meter })
}

/**
 * Automático, propio o a mano.
 * @param {'auto'|'own'|'manual'} m
 */
async function setMode(m) {
  if (!['auto', 'own', 'manual'].includes(m) || m === mode.value) return
  const was = mode.value
  mode.value = m
  savePref(MODE_KEY, m)
  if (m === 'manual') return setAuto(false)
  if (was === 'manual') return setAuto(true)
  // de automático a propio (o al revés): otra rejilla para la misma canción
  forSong = { id: null, path: '' }
  grid.value = null
  await load()
}

/**
 * El clic propio de la canción que suena, recién ajustado (o null: se
 * quitó, y vuelve el automático). Lo guarda quien lo edita, con la canción.
 * @param {number} id @param {any} own  `{beats, bars, meter}` o null
 */
async function applyOwn(id, own) {
  const p = player()
  studies.delete(id)
  if (mode.value !== 'own' || (p.track.value?.id ?? null) !== id) return
  const g = ownToGrid(own)
  const path = p.state.path
  forSong = { id, path }
  if (g) await adopt(id, path, g, 'own')
  else {
    grid.value = null
    loading.value = true
    await ask(id, path)
  }
}

/** Al cambiar de canción: en automático, la suya; a mano, nada (sigue igual). */
function onSongChange() {
  if (!auto.value) return
  if (wanted.value || viewers > 0) load()
}

function start() {
  if (started) return
  started = true
  const p = player()
  // en un ámbito propio: si lo llama primero un componente (el panel de
  // estudio), el watch no se puede ir con él al cerrarlo
  effectScope(true).run(() => {
    watch(() => [p.track.value?.id ?? null, p.state.path], onSongChange)
  })
  // a mano desde el principio: su tempo, ya
  if (!auto.value) p.resetMetronomeOverrides({ ...manual.value })
}

/**
 * El panel de estudio se abre (quiere ver la rejilla aunque el clic esté
 * quitado) o se cierra. Devuelve con qué soltarlo.
 */
function watchGrid() {
  viewers++
  if (auto.value) load()
  let done = false
  return () => {
    if (!done) viewers--
    done = true
  }
}

export function useClick() {
  start()
  return {
    auto,
    mode,
    setMode,
    applyOwn,
    manual,
    wanted,
    grid,
    source,
    loading,
    progress,
    error,
    toggle,
    setAuto,
    setManual,
    watchGrid,
    songSettings,
    /** vuelve a pedir la rejilla de la que suena (la «x» del tempo a mano, etc.) */
    reload: load
  }
}

/** Vuelve al estado inicial. Solo para pruebas. */
export function resetClick() {
  stopPolling()
  mode.value = savedMode()
  auto.value = mode.value !== 'manual'
  studies.clear()
  manual.value = cleanManual(readPref(MANUAL_KEY, {}))
  wanted.value = false
  grid.value = null
  source.value = ''
  loading.value = false
  progress.value = 0
  error.value = ''
  forSong = { id: null, path: '' }
  settingsFor = null
  viewers = 0
  aheadOf = null
}
