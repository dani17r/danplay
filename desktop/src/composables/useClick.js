/**
 * El clic del metrónomo, con el panel de estudio abierto o cerrado.
 *
 * Tres modos (en la interfaz se llaman «Automático», «Manual» y «Tempo
 * fijo»; lo guardado sigue siendo `'auto'`, `'own'` y `'manual'`):
 *
 * - **Automático** (`'auto'`, lo normal): lo que analizó DanPlay, cada
 *   canción con su clic. La rejilla la da
 *   Beat This! en el núcleo; la primera vez de cada canción tarda 15-20 s, y
 *   mientras tanto el clic NO suena (ni el de la canción anterior, que iría
 *   a otro tempo): se calla, el botón no deja ponerlo y la onda enseña una
 *   barra verde con lo que lleva. Al llegar, vuelve solo si estaba puesto.
 *   Con sus ajustes guardados (compás, «el 1», doble/mitad). Con el clic
 *   puesto, las siguientes de la cola se dejan preparadas: al pasar a ellas
 *   ya está. Si aquí no hay Beat This! (o no le sacó pulso), la de Rust, al
 *   momento. Siempre es la última que dio el núcleo, también tras un
 *   re-análisis hecho desde Manual.
 * - **Manual** (`'own'`): lo que mueve él en la onda, pulso a pulso, guardado
 *   con la canción (`metronome.own` de su estudio). Si una canción aún no lo
 *   tiene, suena la automática hasta que haga el suyo (y el panel lo dice).
 *   Pasar de un modo a otro, las veces que sea, no cambia nada de lo guardado.
 * - **Tempo fijo** (`'manual'`): un tempo para todas, el que se ponga. Al
 *   pasar de canción sigue sonando igual: ni se espera ni se cambia nada
 *   aunque llegue la rejilla de la otra.
 *
 * Antes la rejilla la pedía el panel de estudio y solo mientras estaba
 * abierto: con él cerrado, al pasar de canción el clic seguía suelto a 100
 * bpm (y con los ajustes de la anterior).
 */
import { effectScope, ref, shallowRef, watch } from 'vue'
import { api, ApiError } from '../api.js'
import { usePlayback } from './usePlayback.js'
import { mergeWindows, ownToGrid } from '../utils/beats.js'

const AUTO_KEY = 'danplay.click.auto'
const MODE_KEY = 'danplay.click.mode'
const MANUAL_KEY = 'danplay.click.manual'
/** Cada cuánto se pregunta por la rejilla que se está calculando. */
export const POLL_MS = 700
/**
 * Lo que se espera como mucho la rejilla de una canción. El núcleo corta al
 * detector a los diez minutos; si pasado eso sigue sin contestar, es que algo
 * se colgó, y el botón no se queda en «cargando» para siempre.
 */
export const LOAD_LIMIT_MS = 11 * 60 * 1000
/** Lo que se espera una respuesta del núcleo antes de volver a preguntar. */
export const ANSWER_MS = 20000
/** El núcleo no contestó a tiempo (`within`). */
const LATE = Symbol('tarde')

/** `promise`, o `LATE` si a los `ms` no ha contestado. @param {Promise<any>} promise @param {number} ms */
function within(promise, ms) {
  /** @type {any} */
  let timer = null
  const late = new Promise((resolve) => {
    timer = setTimeout(() => resolve(LATE), ms)
  })
  return Promise.race([promise, late]).finally(() => clearTimeout(timer))
}
/**
 * Cuántas de las siguientes de la lista se dejan preparadas: un repertorio o
 * un disco enteros. Eran 3 (y la cola del núcleo, de 6): a la cuarta canción
 * de un ensayo había que esperar otra vez los 15-20 s.
 */
export const AHEAD = 60

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
/** De dónde salió: 'beat-this', 'rust', 'own' (el manual) o ''. */
const source = ref('')
/**
 * Dónde el clic de una canción es una suposición (el análisis no encontró
 * pulso con apoyo en la música): `[[desde, hasta], …]` en segundos. Lo trae
 * el re-análisis (`report.weak`) y, si el núcleo la guarda con la rejilla,
 * esta misma. Sin ello, no hay nada que enseñar.
 */
const weak = shallowRef(
  /** @type {{id: number|null, regions: Array<[number, number]>}} */ ({ id: null, regions: [] })
)
/** Las zonas dudosas de la canción `id` (lo que no valga se descarta). @param {number|null} id @param {any} list */
function setWeak(id, list) {
  weak.value = { id, regions: mergeWindows(list) }
}
/**
 * Cómo encajaba el clic con los golpes antes y después del último
 * re-análisis (`report.fit`: desfase mediano y dispersión, en ms), si el
 * núcleo lo midió.
 */
const fit = shallowRef(
  /** @type {{id: number|null, before: {median: number, spread: number}|null, after: {median: number, spread: number}|null}} */ ({
    id: null,
    before: null,
    after: null
  })
)
/** @param {any} f */
const fitSide = (f) =>
  Number.isFinite(f?.median_ms) && Number.isFinite(f?.spread_ms)
    ? { median: Number(f.median_ms), spread: Number(f.spread_ms) }
    : null
/** @param {number|null} id @param {any} f  `{before, after}` o nada */
function setFit(id, f) {
  const before = fitSide(f?.before)
  const after = fitSide(f?.after)
  fit.value = before && after ? { id, before, after } : { id, before: null, after: null }
}
/** La rejilla de la que suena, en camino (en automático). */
const loading = ref(false)
/** Lo que lleva (0..1) mientras carga. */
const progress = ref(0)
/** Por qué no hay rejilla, si falló todo. */
const error = ref('')
/**
 * Por qué el clic es el aproximado (el de Rust) habiendo detector: falló con
 * esta canción, o tarda demasiado. Vacío si aquí no hay detector, que eso no
 * es un fallo. Antes no se decía, y el «1» a contrapié parecía cosa del clic.
 */
const fallbackWhy = ref('')
/**
 * Re-analizando con la batería separada: lo que lleva la red con ella
 * (0..1), o null si no se está en eso.
 */
const reanalyzing = ref(/** @type {number|null} */ (null))
/** Cuándo se empezó a pedir la rejilla de la que suena. */
let askedAt = 0
/**
 * Cuál es la última rejilla que se pidió: cada vez que se pide otra (cambiar
 * de modo, de canción, re-analizar) pasa a ser la siguiente, y lo que
 * llegue de una anterior se descarta. Sin esto, la rejilla automática que
 * tardaba en llegar se ponía encima del clic manual al que se había pasado
 * mientras tanto.
 */
let epoch = 0

/** De qué canción es lo de arriba. */
let forSong = { id: /** @type {number|null} */ (null), path: '' }
/** De qué canción se pusieron ya sus ajustes (una vez por canción). */
let settingsFor = /** @type {number|null} */ (null)
/** Cuántos quieren la rejilla aunque el clic esté quitado (el panel de estudio). */
let viewers = 0
/** Tras qué canción se prepararon ya las siguientes (una vez por canción). */
let aheadOf = /** @type {number|null} */ (null)
/** Las que ya se dejaron pedidas al núcleo: por ellas no se vuelve a preguntar. */
const prepared = new Set()
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
  // la de esta ya está: no hay que dejarla preparada desde otra canción
  if (g && forSong.id != null) prepared.add(forSong.id)
  await player().setMetronome(wanted.value ? { on: true } : {})
  if (wanted.value) prepareAhead()
}

/** La de Rust (medio segundo): cuando aquí no hay Beat This!. */
async function fromRust(id, path, my = epoch) {
  let g = null
  try {
    g = await player().analyzeBeats(player().track.value?.bpm || null, path)
  } catch (e) {
    if (my === epoch && sameSong(id, path)) error.value = e instanceof Error ? e.message : String(e)
  }
  if (my === epoch && sameSong(id, path)) await settle(g, 'rust')
}

/** La de Beat This! (o la manual), a Rust; si no vale, la de Rust. @param {any} found */
async function adopt(id, path, found, from = 'beat-this', my = epoch) {
  let g = null
  try {
    g = await player().setBeatGrid(found, path)
  } catch {
    /* no pasó la revisión de Rust: la suya */
  }
  if (my !== epoch || !sameSong(id, path)) return
  if (from === 'beat-this' && Array.isArray(found?.report?.weak)) setWeak(id, found.report.weak)
  if (g) await settle(g, from)
  else await fromRust(id, path, my)
}

/** Pregunta al núcleo: la rejilla, o por dónde va. */
async function ask(id, path, my = epoch) {
  pollTimer = null
  /** @type {any} sin Beat This! aquí, o la canción no le dio pulso: null */
  let r = null
  let why = ''
  try {
    r = await within(api.beatGridNow(id), ANSWER_MS)
  } catch (e) {
    // 422: el detector no pudo con esta canción (503: aquí no lo hay)
    if (e instanceof ApiError && e.status === 422) why = e.message
  }
  if (my !== epoch || !sameSong(id, path) || !auto.value) return
  if (r === LATE || r?.pending) {
    if (Date.now() - askedAt < LOAD_LIMIT_MS) {
      if (r !== LATE) progress.value = Math.max(progress.value, Number(r.progress) || 0)
      pollTimer = setTimeout(() => ask(id, path, my), POLL_MS)
      return
    }
    why = 'tarda demasiado'
    r = null
  }
  fallbackWhy.value = why
  if (r) await adopt(id, path, r, 'beat-this', my)
  else await fromRust(id, path, my)
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
  const my = ++epoch
  forSong = { id, path }
  grid.value = null
  source.value = ''
  error.value = ''
  fallbackWhy.value = ''
  progress.value = 0
  loading.value = true
  askedAt = Date.now()
  if (weak.value.id !== id) setWeak(id, [])
  // el de la anterior no puede seguir sonando sobre esta: iría a su tempo
  if (p.metronomeSent().on) await p.setMetronome({ on: false })
  await applySongSettings(id)
  if (my !== epoch || !sameSong(id, path)) return
  // el manual, si la canción lo tiene
  if (mode.value === 'own') {
    const own = ownToGrid((await songStudy(id))?.metronome?.own)
    if (my !== epoch || !sameSong(id, path)) return
    if (own) {
      await adopt(id, path, own, 'own', my)
      if (wanted.value) prepareAhead()
      return
    }
  }
  await ask(id, path, my)
  if (wanted.value) prepareAhead()
}

/**
 * La rejilla de la canción que suena, otra vez: la guardada cambió (se
 * re-analizó). `load()` no vale para esto: con la misma canción y su rejilla
 * ya puesta no hace nada, que es justo como está al re-analizar. Con `fresh`
 * (la que devolvió el núcleo) no se vuelve a preguntar. Devuelve si la que
 * suena cambió: en Manual (con su clic hecho) manda el suyo, y la
 * re-analizada queda debajo, en Automático, que es donde se ve.
 * @param {import('../api.js').BeatGrid|null} [fresh]
 */
async function reload(fresh = null) {
  const p = player()
  const id = p.track.value?.id ?? null
  const path = p.state.path
  if (!auto.value || id == null || !path) return false
  if (mode.value === 'own') {
    studies.delete(id)
    const own = ownToGrid((await songStudy(id))?.metronome?.own)
    if (own) return false
  }
  stopPolling()
  const my = ++epoch
  forSong = { id, path }
  error.value = ''
  fallbackWhy.value = ''
  askedAt = Date.now()
  if (fresh?.beats?.length) {
    await adopt(id, path, fresh, 'beat-this', my)
    return true
  }
  grid.value = null
  source.value = ''
  progress.value = 0
  loading.value = true
  await ask(id, path, my)
  return true
}

/**
 * «Re-analizar el clic» de una canción. Con la batería separada, la red
 * tiene que oírla antes (15-20 s la primera vez): antes se esperaba dentro
 * de la petición, sin avance a la vista. Ahora el núcleo la pone en su cola
 * y contesta por dónde va (`pending`); aquí se pregunta cada poco, se
 * enseña (`reanalyzing`) y se devuelve la respuesta de verdad cuando llega.
 * @param {number} id
 * @param {{mode: string, start?: number, end?: number}} opts
 */
async function reanalyze(id, opts) {
  if (opts.mode !== 'drums') return api.reanalyzeBeats(id, opts)
  const since = Date.now()
  try {
    for (;;) {
      const r = await api.reanalyzeBeats(id, { ...opts, wait: false })
      if (!r?.pending) return r
      // el mismo plazo que la carga del clic: el núcleo corta al detector
      if (Date.now() - since > LOAD_LIMIT_MS) throw new Error('tarda demasiado')
      reanalyzing.value = Math.max(reanalyzing.value ?? 0, Number(r.progress) || 0)
      await new Promise((resolve) => setTimeout(resolve, POLL_MS))
    }
  } finally {
    reanalyzing.value = null
  }
}

/**
 * La lista que suena, preparada: las de detrás de esta primero (son las que
 * vienen) y después las del principio, hasta `AHEAD`, en una sola petición.
 * El núcleo las deja en su cola (la que ya está guardada no cuesta nada) y
 * dice cuáles no pudo (`left`): esas se piden otra vez más adelante.
 *
 * En aleatorio no se sabe cuál viene: una lista que cabe entera se prepara
 * igual (vendrán todas); de una más larga no se prepara nada, que sería
 * pasar la red por canciones al azar.
 */
function prepareAhead() {
  const p = player()
  if (!auto.value || aheadOf === forSong.id) return
  aheadOf = forSong.id
  const list = p.queue.value
  const at = p.state.index
  if (!Array.isArray(list) || at < 0) return
  if (p.shuffle.value && list.length > AHEAD + 1) return
  /** @type {number[]} */
  const ids = []
  for (let k = 1; k < list.length && k <= AHEAD; k++) {
    const id = list[(at + k) % list.length]?.id
    if (id != null && id !== forSong.id && !prepared.has(id) && !ids.includes(id)) ids.push(id)
  }
  if (!ids.length) return
  for (const id of ids) prepared.add(id)
  api
    .beatGridPrepare(ids[0], ids.slice(1))
    .then((r) => {
      if (r?.queued === false) prepared.delete(ids[0])
      for (const id of r?.left || []) prepared.delete(id)
    })
    .catch(() => {
      for (const id of ids) prepared.delete(id)
    })
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
 * Quitar el clic del todo (al salir del modo estudio): lo pedido deja de
 * estarlo. Callarlo solo en Rust no basta: seguía «pedido», y en automático
 * volvía él solo en la canción siguiente.
 */
function stop() {
  wanted.value = false
  return player().setMetronome({ on: false })
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
 * Automático, Manual o Tempo fijo. Cambiar de uno a otro no toca nada de lo
 * guardado: lo manual de cada canción y lo analizado siguen donde estaban, y
 * cada modo enseña lo suyo (el manual se vuelve a leer, por si cambió).
 * @param {'auto'|'own'|'manual'} m
 */
async function setMode(m) {
  if (!['auto', 'own', 'manual'].includes(m) || m === mode.value) return
  const was = mode.value
  mode.value = m
  savePref(MODE_KEY, m)
  if (forSong.id != null) studies.delete(forSong.id)
  if (m === 'manual') return setAuto(false)
  if (was === 'manual') return setAuto(true)
  // de automático a manual (o al revés): otra rejilla para la misma canción
  forSong = { id: null, path: '' }
  grid.value = null
  await load()
}

/**
 * El clic manual de la canción que suena, recién ajustado (o null: se
 * borró, y vuelve el automático). Lo guarda quien lo edita, con la canción.
 * @param {number} id @param {any} own  `{beats, bars, meter, touched}` o null
 */
async function applyOwn(id, own) {
  const p = player()
  studies.delete(id)
  if (mode.value !== 'own' || (p.track.value?.id ?? null) !== id) return
  const g = ownToGrid(own)
  const path = p.state.path
  const my = ++epoch
  forSong = { id, path }
  if (g) await adopt(id, path, g, 'own', my)
  else {
    grid.value = null
    loading.value = true
    await ask(id, path, my)
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
    fallbackWhy,
    toggle,
    stop,
    setAuto,
    setManual,
    watchGrid,
    songSettings,
    weak,
    setWeak,
    fit,
    setFit,
    /** la rejilla de la que suena, otra vez: se re-analizó y cambió */
    reload,
    reanalyze,
    reanalyzing
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
  fallbackWhy.value = ''
  reanalyzing.value = null
  weak.value = { id: null, regions: [] }
  fit.value = { id: null, before: null, after: null }
  askedAt = 0
  epoch++
  forSong = { id: null, path: '' }
  settingsFor = null
  viewers = 0
  aheadOf = null
  prepared.clear()
}
