// @ts-check
// Editar el clic manual de una canción, de muchos pulsos a la vez: lo que une
// la lógica pura (`utils/beatEdit.js`, `beatAlign.js`, `beatHistory.js`) con
// el modo estudio.
//
// Todo lo que cambia el clic manual pasa por UN camino, `commitOwn`: apunta
// cómo estaba (para deshacer), lo deja en el estudio de la canción, lo guarda
// y se lo da al motor. Por ahí van los arrastres de la onda, cada herramienta
// de la barra, el panel «Tempo y onda», «Re-analizar» en Manual y «Borrar el
// manual». Con eso, Ctrl+Z vale para todo y no hay dos formas de guardar.
//
// Aquí vive también lo que no es de nadie en concreto: los pulsos elegidos, los
// golpes del audio (que se piden cuando hacen falta), el automático de
// fondo, las preferencias de la barra y las teclas (deshacer, rehacer, borrar,
// mover con las flechas), que se escuchan en la fase de captura para ganarle
// al reproductor, que con las flechas adelanta o atrasa la canción.
import {
  computed,
  markRaw,
  onMounted,
  onUnmounted,
  reactive,
  ref,
  shallowRef,
  toValue,
  watch
} from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from './useNotices.js'
import { HOTKEY_BLOCKERS } from './useHotkeys.js'
import { BeatHistory } from '../utils/beatHistory.js'
import {
  addBeatAt,
  barsEvery,
  doubleBeats,
  filled,
  halveBeats,
  moveBlock,
  removeBeats,
  revertToAuto,
  selectBetween,
  selectRange,
  setTempoSpan,
  spanOf,
  spreadEven,
  tempoOf
} from '../utils/beatEdit.js'
import { alignBeats, estimateTempo, fitReport, REACH_DEFAULT } from '../utils/beatAlign.js'
import { formatBpm, gridToOwn, ownToGrid, ownTouched, touchedByEdit } from '../utils/beats.js'
import { readPref, savePref } from '../utils/prefs.js'

// ------------------------------------------------------------------ las palabras
/** «3 pulsos» / «1 pulso» */
export const beatsWord = (n) => `${n} ${n === 1 ? 'pulso' : 'pulsos'}`
/** Los bpm con coma, como se escriben aquí: «76,3». @param {number} bpm */
export const bpmText = (bpm) => formatBpm(bpm).replace('.', ',')
/** Un instante con la décima: «1:05,2». @param {number} t */
export function timeText(t) {
  const v = Math.max(0, Math.round((Number(t) || 0) * 10) / 10)
  const m = Math.floor(v / 60)
  const s = v - m * 60
  return `${m}:${s.toFixed(1).padStart(4, '0').replace('.', ',')}`
}
/** Cuánto va el clic con respecto al golpe: «14 ms tarde», «a tiempo». @param {number} offsetMs */
export function lagText(offsetMs) {
  const v = Number(offsetMs) || 0
  // negativo: el clic cae después del golpe (el mismo signo que el núcleo)
  return Math.abs(v) < 1 ? 'a tiempo' : `${Math.round(Math.abs(v))} ms ${v < 0 ? 'tarde' : 'antes'}`
}

// ------------------------------------------------------------ preferencias
const PREF_HITS = 'danplay.click.showHits'
const PREF_SNAP = 'danplay.click.snapHits'
const PREF_GHOST = 'danplay.click.ghostAuto'
const PREF_HEAR = 'danplay.click.hearTrial'

/** De cuántas canciones (y fuente) se guardan los golpes ya pedidos. */
export const HITS_KEEP = 4

/** Una preferencia de sí o no, recordada. */
function boolPref(key, fallback) {
  const saved = readPref(key, fallback)
  const r = ref(typeof saved === 'boolean' ? saved : fallback)
  watch(r, (v) => savePref(key, v))
  return r
}

// --------------------------------------------------------------- lo de dentro
const sameNumbers = (a, b) =>
  a.length === b.length && a.every((v, i) => Math.round(v * 1000) === Math.round(b[i] * 1000))
/** ¿Son lo mismo dos clics propios (pulsos, «1» y compás)? */
function sameOwn(a, b) {
  if (!a || !b) return !a && !b
  return (
    sameNumbers(a.beats || [], b.beats || []) &&
    (a.bars || []).join() === (b.bars || []).join() &&
    (a.meter ?? 4) === (b.meter ?? 4)
  )
}
const copyOwn = (own) =>
  own
    ? {
        beats: [...own.beats],
        bars: [...(own.bars || [])],
        meter: own.meter ?? 4,
        ...(Array.isArray(own.touched) ? { touched: own.touched.map((w) => [...w]) } : {})
      }
    : null
const ms = (t) => Math.round(t * 1000)
/** Que una promesa que nadie espera no deje un fallo suelto. */
const quiet = (p) => Promise.resolve(p).catch(() => {})

/**
 * Los pulsos que siguen elegidos cuando cambia la lista de pulsos (se añadió
 * o se quitó alguno): los mismos, por su tiempo, con los índices nuevos.
 */
function remapSelection(before, after, sel) {
  const chosen = new Set(sel.map((i) => ms(before[i])))
  const out = []
  after.forEach((t, i) => {
    if (chosen.has(ms(t))) out.push(i)
  })
  return out
}

// ------------------------------------------------------- el plan de «Tempo y onda»
/**
 * Lo que sale de «Tempo y onda» con lo que hay puesto, sin tocar nada: el
 * tempo escrito (si se pide) y después pegarlo a los golpes (si se pide).
 *
 * @param {{beats: number[], bars: number[], meter: number}} own  el clic de ahora
 * @param {[number, number]} span  el tramo, en segundos
 * @param {{
 *   tempo?: {mode: 'keep'|'set', bpm?: number, anchor?: number|null, phaseMs?: number},
 *   fit?: {how: 'none'|'phase'|'follow'|'line', reach?: number, rigidity?: number, lead?: number, force?: boolean},
 *   duration?: number
 * }} settings
 * @param {Array<[number, number]>} hits  los golpes del audio (pueden no haber llegado)
 */
export function planAlign(own, span, settings = {}, hits = []) {
  const { tempo = { mode: 'keep' }, fit = { how: 'none' }, duration } = settings
  const [t0, t1] = span
  // los pulsos que salen de un tempo escrito pueden caer unos ms fuera del tramo
  const pad = 0.006
  /** @type {[number, number]} */
  const area = [t0 - pad, t1 + pad]
  const reach = fit.reach ?? REACH_DEFAULT
  const bpm = Number(tempo.bpm)
  const setTempo = tempo.mode === 'set' && Number.isFinite(bpm) && bpm > 0 && t1 > t0

  let current = { beats: own.beats, bars: own.bars, meter: own.meter }
  let selection = filled(selectBetween(own.beats, t0, t1))
  let added = 0
  let removed = 0
  if (setTempo) {
    const first = own.beats.find((t) => t >= t0 - 0.0005 && t <= t1) ?? t0
    const anchor = (tempo.anchor ?? first) + (tempo.phaseMs || 0) / 1000
    // el último pulso del tramo también se rehace: `t1` no cuenta en `setTempoSpan`
    const r = setTempoSpan(own, [t0 - 0.0005, t1 + 0.0005], bpm, {
      anchor,
      ...(duration ? { duration } : {})
    })
    current = { beats: r.beats, bars: r.bars, meter: r.meter }
    selection = r.selection
    added = r.added
    removed = r.removed
  }

  const how = fit.how && fit.how !== 'none' ? fit.how : 'none'
  let before = fitReport(own.beats, hits, area, { reach })
  /** @type {any} */
  let aligned = null
  if (how !== 'none') {
    aligned = alignBeats(current, area, hits, {
      how,
      reach,
      rigidity: fit.rigidity ?? 0.5,
      lead: fit.lead ?? 0,
      force: !!fit.force
    })
    if (aligned.applied) current = { ...current, beats: aligned.beats }
    // lo que se mide es lo que hace el ajuste: con un tempo escrito, el peine
    // tal como sale de él (comparar con el clic de antes, a otro tempo, no dice nada)
    before = aligned.report.before
  }
  const after = aligned?.applied
    ? aligned.report.after
    : fitReport(current.beats, hits, area, { reach })
  const n = selection.length || aligned?.report.n || before.n
  const changed = !sameOwn(current, own)
  const bits = []
  if (setTempo) bits.push('tempo')
  if (aligned?.applied) bits.push('onda')
  const label =
    bits.length === 2
      ? 'tempo y onda'
      : setTempo
        ? `tempo ${bpmText(bpm)} bpm`
        : aligned?.applied
          ? 'alinear a la onda'
          : 'tempo y onda'
  return {
    own: changed ? current : null,
    changed,
    selection,
    added,
    removed,
    label,
    n,
    setTempo,
    how,
    refused: aligned && !aligned.applied ? aligned.report.reason || 'no pega' : '',
    before,
    after,
    candidate: aligned?.report.candidate ?? null,
    // con «sigue la onda» cada pulso se pega a su golpe y la dispersión de
    // después sale casi 0 sin decir nada: no se enseña
    spreadReliable: aligned ? aligned.report.spreadReliable !== false : true
  }
}

/**
 * Lo que se dice del resultado en vivo: cuántos pulsos pegan con la onda y
 * cuánto cambió el desfase, o por qué no se movió nada.
 * @param {ReturnType<typeof planAlign>} plan
 * @param {{loading?: boolean, error?: string}} [hitsState]
 * @returns {{text: string, kind: 'ok'|'warn'|'info'}}
 */
export function describePlan(plan, { loading = false, error = '' } = {}) {
  if (plan.how === 'none' && !plan.setTempo) {
    return { text: 'Elige un tempo o cómo ajustarlo a la onda para ver el resultado', kind: 'info' }
  }
  const count = (fit) => Math.round((fit?.supported || 0) * plan.n)
  // lo que pegarían esos pulsos corridos a sitios que no son suyos: con tanto golpe
  // como hay en la música, un peine cualquiera ya cae sobre la mitad
  const byChance = (fit) => Math.round((fit?.chance || 0) * plan.n)
  if (plan.how !== 'none' && loading) {
    return { text: 'Oyendo la canción…', kind: 'info' }
  }
  if (plan.how !== 'none' && error) {
    return { text: `No se pudo oír la canción: ${error}`, kind: 'warn' }
  }
  if (plan.refused === 'sin golpes') {
    return {
      text:
        'Sin golpes claros en este tramo' +
        (plan.setTempo ? ': se queda con el tempo exacto, sin ajustar' : ''),
      kind: 'warn'
    }
  }
  if (plan.refused === 'sin pulsos') {
    return { text: 'No hay pulsos en este tramo para ajustar', kind: 'warn' }
  }
  if (plan.refused === 'no cabe') {
    return { text: 'No cabe entre los pulsos de los lados', kind: 'warn' }
  }
  if (plan.refused === 'empeora') {
    // lo que ya pega es mejor que lo que saldría: se avisa antes de que alguien lo estropee
    return {
      text:
        `Empeoraría: pegan ${count(plan.before)} de ${plan.n} pulsos y pasarían a ` +
        `${count(plan.candidate)}, y al azar pegarían unos ${byChance(plan.candidate)}; ` +
        (plan.setTempo ? 'se queda con el tempo exacto, sin ajustar' : 'se queda como está') +
        ' (puedes forzar el ajuste)',
      kind: 'warn'
    }
  }
  if (plan.refused) {
    const c = count(plan.candidate)
    return {
      text:
        `No pega con la onda en este tramo: ${c} de ${plan.n} pulsos, ` +
        `y al azar pegarían unos ${byChance(plan.candidate)}; ` +
        (plan.setTempo
          ? 'se queda con el tempo exacto, sin ajustar'
          : 'se queda como está, sin ajustar'),
      kind: 'warn'
    }
  }
  const { before, after } = plan
  if (!after || (!before.supported && !after.supported && !after.offsetMs)) {
    return plan.how === 'none'
      ? { text: 'Sin golpes que comparar: se pone el tempo exacto', kind: 'info' }
      : { text: 'Sin golpes claros en este tramo', kind: 'warn' }
  }
  return {
    text:
      `Pega: ${count(after)} de ${plan.n} pulsos (al azar pegarían unos ${byChance(after)})` +
      ` · el clic iba ${lagText(before.offsetMs)} → ${lagText(after.offsetMs)}` +
      (plan.spreadReliable
        ? ` · dispersión ${Math.round(before.spreadMs)} → ${Math.round(after.spreadMs)} ms`
        : ''),
    kind: 'ok'
  }
}

/**
 * Cuántos puntos del peine caben en un tramo a un tempo (lo que se dice en
 * «pegan 31 de 38»).
 * @param {[number, number]} span @param {number} bpm
 */
function combPoints(span, bpm) {
  return Math.max(1, Math.floor(((span[1] - span[0]) * bpm) / 60 + 1e-9) + 1)
}
/**
 * Busca en la onda el tempo del tramo. Con `around` (lo que se espera: el que
 * se ha escrito o el que ya tiene), solo cerca de él; sin él, a ciegas. Cada
 * resultado trae cuántos puntos del peine caben (`points`) y cuántos pegan
 * (`pegan`), también cada alternativa (el doble y la mitad: se ofrecen, no se
 * eligen solas).
 * @param {Array<[number, number]>} hits @param {[number, number]} span @param {number} [around]
 */
export function findTempo(hits, span, around = 0) {
  const r = estimateTempo(hits, span, { around })
  if (!r) return null
  const counted = (c) => {
    const points = combPoints(span, c.bpm)
    return { ...c, points, pegan: Math.round(c.support * points) }
  }
  return { ...counted(r), alternatives: (r.alternatives || []).map(counted) }
}

/** Menos de esto de `excess` (lo que pega de más sobre el azar) es que ahí no hay nada. */
export const NOTHING_EXCESS = 0.4
/** Una alternativa solo merece enseñarse si pega casi tanto sobre el azar como lo elegido. */
const ALTERNATIVE_KEEP = 0.85
/**
 * Lo que se ofrece tras buscar: el elegido y sus alternativas que pegan sobre
 * el azar al menos el 85 % de lo que pega lo elegido, para saltar de uno a
 * otro. (El apoyo a secas no vale: un peine al azar ya apoya la mitad.)
 * @param {ReturnType<typeof findTempo>} found
 */
export function tempoChoices(found) {
  if (!found) return []
  const keep = (found.alternatives || []).filter(
    (a) => a.excess >= ALTERNATIVE_KEEP * found.excess - 1e-9
  )
  return [found, ...keep]
}
/** «×2», «÷2», «×4»…: cómo se ve el tempo `to` desde `from` (el doble, la mitad…). */
export function ratioLabel(from, to) {
  const r = to / from
  const n = r >= 1 ? r : 1 / r
  const shown = Math.abs(n - Math.round(n)) < 0.06 ? String(Math.round(n)) : n.toFixed(1)
  return `${r >= 1 ? '×' : '÷'}${shown}`
}

// -------------------------------------------------------------------- el compuesto
const EDITABLE =
  'input:not([type="checkbox"]):not([type="radio"]):not([type="range"]):not([type="button"]), textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"]'
const ARROW_OWNERS = `${EDITABLE}, input[type="range"], input[type="radio"], [role="slider"], [role="radio"], [role="listbox"], [role="menu"]`
const inside = (el, selector) => el instanceof Element && !!el.closest(selector)

/**
 * @typedef {Object} BeatEditDeps
 * @property {any} songId    la canción abierta (ref o función): la historia es de cada una
 * @property {any} study     el estudio de la canción (ref): `metronome.own`, `loops`…
 * @property {any} grid      la rejilla que se ve y suena (`soundingGrid`)
 * @property {any} duration
 * @property {any} editing   ref: se están ajustando los pulsos
 * @property {any} [visible] el estudio se ve (si no, las teclas no son suyas)
 * @property {any} click     `useClick()`
 * @property {any} player    `usePlayback()`
 * @property {() => void} [scheduleSave]
 * @property {() => Promise<void>} [flushSave]
 * @property {any} [hasDrums] la canción tiene la batería separada
 * @property {(a: number, b: number) => Promise<any>} [hear]  poner ese tramo en bucle y que suene
 * @property {() => number} [now]  el reloj de la historia (pruebas)
 */
/**
 * @param {BeatEditDeps} deps
 */
export function useBeatEdit(deps) {
  const { study, editing, click, player } = deps
  const songId = computed(() => toValue(deps.songId) ?? null)
  const durationNow = () => Number(toValue(deps.duration)) || 0
  const baseGrid = computed(() => toValue(deps.grid) ?? null)

  // ------------------------------------------------------------- la historia
  const history = new BeatHistory(deps.now ? { now: deps.now } : {})
  /** Para que lo que depende de la historia se entere de que cambió. */
  const rev = ref(0)
  const bump = () => rev.value++
  const tracked = (read) =>
    computed(() => {
      void rev.value
      return read()
    })
  const canUndo = tracked(() => history.canUndo)
  const canRedo = tracked(() => history.canRedo)
  const undoLabel = tracked(() => history.undoLabel)
  const redoLabel = tracked(() => history.redoLabel)
  /** La velocidad y el «1» corrido que tenía el automático al empezar el manual (para deshacer hasta ahí). */
  let autoSettings = null

  // ---------------------------------------------------------------- la selección
  const selection = ref(/** @type {number[]} */ ([]))
  function setSelection(list) {
    const n = beatsNow.value.length
    const clean = [
      ...new Set((list || []).map(Number).filter((i) => Number.isInteger(i) && i >= 0 && i < n))
    ]
    clean.sort((a, b) => a - b)
    const same =
      clean.length === selection.value.length && clean.every((v, i) => v === selection.value[i])
    if (!same) selection.value = clean
  }

  // ----------------------------------------------- lo que se ve y lo que está guardado
  /** La rejilla que se enseña en la onda: la guardada, aunque suene una prueba. */
  const frozen = shallowRef(null)
  const viewGrid = computed(() => frozen.value ?? baseGrid.value)
  const savedOwn = () => study.value.metronome?.own ?? null
  /** Los pulsos de ahora: los del clic manual si lo hay, y si no, los que se ven. */
  const beatsNow = computed(() => {
    const own = study.value.metronome?.own
    if (own?.beats?.length) return own.beats
    return viewGrid.value?.beats || []
  })
  /** El clic de ahora, tal como lo editan las herramientas. */
  function currentOwn() {
    const own = savedOwn()
    if (own?.beats?.length >= 2) {
      return { beats: [...own.beats], bars: [...(own.bars || [])], meter: own.meter ?? 4 }
    }
    const g = viewGrid.value
    if (!g?.beats?.length) return null
    const { beats, bars, meter } = gridToOwn(g)
    return { beats, bars, meter }
  }
  const hasOwn = () => !!savedOwn()?.beats?.length

  // ---------------------------------------------------- guardar el manual (el camino)
  let syncing = null
  let dirty = false
  /** Lo que hay que poner en el motor al acabar de deshacer hasta «sin manual». */
  let pendingMetro = null
  /**
   * Lo guardado y puesto en el motor, una y otra vez si cambia mientras tanto:
   * mantener una flecha son muchos cambios y solo hace falta que el último
   * llegue al final.
   */
  function syncEngine() {
    dirty = true
    if (syncing) return syncing
    const id = songId.value
    syncing = (async () => {
      try {
        while (dirty && songId.value === id) {
          dirty = false
          const own = savedOwn()
          const metro = pendingMetro
          pendingMetro = null
          deps.scheduleSave()
          await deps.flushSave()
          if (songId.value !== id) break
          // Lo ajustado ya lleva el doble, la mitad y el «1» corrido que se veían:
          // a Rust también hay que quitárselos, o los aplica otra vez encima.
          if (own) await player.setMetronome({ mult: 0, shift: 0 })
          else if (metro) await player.setMetronome(metro)
          await click.applyOwn(id, own)
        }
      } finally {
        syncing = null
      }
    })()
    return syncing
  }

  /**
   * Deja `own` en el estudio de la canción (null: se quita el manual). Sin
   * hacerlo reactivo (miles de pulsos que nadie cambia en su sitio: cada
   * cambio es otro `own`).
   */
  function writeOwn(own) {
    const m = { ...(study.value.metronome || {}) }
    if (own) {
      m.own = markRaw(own)
      delete m.shift
      delete m.mult
    } else {
      delete m.own
    }
    study.value.metronome = m
  }

  /**
   * Lo que se guarda de unos pulsos editados: con ellos, las zonas que tocó
   * (lo que cambió respecto a lo que se veía antes). Un clic de antes de que se
   * guardaran las zonas sigue sin ellas: el núcleo las deduce.
   */
  function finalize(next) {
    // una rejilla (lo que manda la onda) ya sabe dónde cae su «1», aunque no
    // lleve la lista de compases; unos pulsos sueltos, por su lista
    const after = Number.isFinite(next?.first_downbeat) ? next : ownToGrid(next)
    if (!after) return null
    const prev = savedOwn()
    const saved = ownTouched(prev)
    const legacy = !!prev?.beats?.length && saved === null
    const was = prev?.beats?.length ? ownToGrid(prev) : viewGrid.value
    const touched = legacy
      ? undefined
      : was
        ? touchedByEdit(was, after, saved ?? [])
        : (saved ?? [])
    return gridToOwn(after, touched)
  }

  /**
   * Lo único que cambia el clic manual. `next` son los pulsos nuevos
   * (`{beats, bars, meter}`) o null para quitar el manual y volver al
   * automático. Con `final`, `next` ya lleva sus zonas tocadas (el re-análisis).
   * `key`: los cambios seguidos con la misma clave se deshacen de una vez.
   * @param {any} next @param {string} label
   * @param {{key?: string, final?: boolean}} [opts]
   * @returns {Promise<boolean>} si cambió algo (y ya está guardado y sonando)
   */
  function commitOwn(next, label, { key, final = false } = {}) {
    const id = songId.value
    if (id == null) return Promise.resolve(false)
    const before = savedOwn()
    const owned = next ? (final ? copyOwn(next) : finalize(next)) : null
    if (next && !owned) return Promise.resolve(false)
    if (!next && !before) return Promise.resolve(false)
    if (next && before && sameOwn(before, owned) && !final) return Promise.resolve(false)
    history.record(label, before, key)
    if (!before && owned) {
      const m = study.value.metronome || {}
      autoSettings = { mult: m.mult || 0, shift: m.shift || 0 }
    }
    bump()
    writeOwn(owned)
    return syncEngine().then(() => true)
  }

  // ------------------------------------------------------------ deshacer y rehacer
  function restore(step, verb) {
    const was = savedOwn()
    writeOwn(step.own)
    if (!step.own && autoSettings) {
      // el automático vuelve con el doble o el «1» corrido que tenía
      const m = { ...(study.value.metronome || {}) }
      if (autoSettings.mult) m.mult = autoSettings.mult
      if (autoSettings.shift) m.shift = autoSettings.shift
      study.value.metronome = m
      pendingMetro = { ...autoSettings }
    }
    // con otra cantidad de pulsos, los índices elegidos ya no valen
    if ((was?.beats?.length ?? 0) !== (step.own?.beats?.length ?? 0)) selection.value = []
    bump()
    notify(`${verb}: ${step.label}`, 'info', 3)
    return syncEngine().then(() => true)
  }
  function undo() {
    if (songId.value == null) return Promise.resolve(false)
    const step = history.undo(copyOwn(savedOwn()))
    if (!step) return Promise.resolve(false)
    endPreviewState()
    return restore(step, 'Deshecho')
  }
  function redo() {
    if (songId.value == null) return Promise.resolve(false)
    const step = history.redo(copyOwn(savedOwn()))
    if (!step) return Promise.resolve(false)
    endPreviewState()
    return restore(step, 'Rehecho')
  }

  // ------------------------------------------------- lo que manda la línea de tiempo
  /**
   * Los pulsos que se movieron en la onda (la rejilla entera): un arrastre,
   * un doble clic, el menú de un pulso.
   * @param {any} edited @param {string} [label]
   */
  function onTimelineGrid(edited, label = 'ajustar pulsos') {
    const before = [...beatsNow.value]
    const sel = [...selection.value]
    const done = commitOwn(edited, label)
    if (before.length !== (edited?.beats?.length ?? 0)) {
      selection.value = remapSelection(before, edited.beats, sel)
    }
    return done
  }

  // --------------------------------------------------------------- los golpes
  const showHits = boolPref(PREF_HITS, false)
  const snapHits = boolPref(PREF_SNAP, false)
  const ghostAuto = boolPref(PREF_GHOST, false)
  const hearTrial = boolPref(PREF_HEAR, true)
  const hitsSource = ref(/** @type {'mix'|'drums'} */ ('mix'))
  const hits = shallowRef(/** @type {Array<[number, number]>} */ ([]))
  const hitsRef = ref(0)
  const hitsStatus = ref(/** @type {'idle'|'loading'|'ready'|'error'} */ ('idle'))
  const hitsError = ref('')
  /**
   * Los golpes ya pedidos, por canción y fuente (también los que están
   * llegando): los de las últimas HITS_KEEP. Son miles de pares por canción y
   * el estudio vive toda la sesión: sin tope se quedaban los de cada canción
   * ajustada desde que se abrió la app.
   */
  const hitsCache = new Map()
  const hasDrums = computed(() => !!toValue(deps.hasDrums))
  watch(hasDrums, (has) => {
    if (!has && hitsSource.value === 'drums') hitsSource.value = 'mix'
  })

  /** Pide (una vez) los golpes de la canción por esa fuente; los deja a la vista. */
  async function loadHits(source = hitsSource.value) {
    const id = songId.value
    if (id == null) return null
    const key = `${id}:${source}`
    let pending = hitsCache.get(key)
    if (pending) {
      // la usada pasa al final: se tira la que lleva más sin usarse
      hitsCache.delete(key)
      hitsCache.set(key, pending)
    } else {
      pending = api.beatHits(id, source)
      hitsCache.set(key, pending)
      pending.catch(() => {
        if (hitsCache.get(key) === pending) hitsCache.delete(key)
      })
      while (hitsCache.size > HITS_KEEP) hitsCache.delete(hitsCache.keys().next().value)
    }
    const mine = source === hitsSource.value
    if (mine) {
      hitsStatus.value = 'loading'
      hitsError.value = ''
    }
    try {
      const r = await pending
      if (songId.value !== id || hitsSource.value !== source) return r
      hits.value = Array.isArray(r?.hits) ? r.hits : []
      hitsRef.value = Number(r?.ref) || 0
      hitsStatus.value = 'ready'
      return r
    } catch (e) {
      if (songId.value === id && hitsSource.value === source) {
        hits.value = []
        hitsRef.value = 0
        hitsStatus.value = 'error'
        hitsError.value = errorMessage(e)
      }
      return null
    }
  }
  const panelOpen = ref(false)
  const wantHits = computed(
    () => editing.value && (showHits.value || snapHits.value || panelOpen.value)
  )
  watch(
    [wantHits, songId, hitsSource],
    ([want]) => {
      if (want) loadHits()
    },
    { flush: 'post' }
  )

  // ---------------------------------------------------------- el automático de fondo
  /** El clic automático de la canción (en el formato del manual), para volver a él o enseñarlo. */
  const autoOwn = shallowRef(/** @type {{id: number, own: any}|null} */ (null))
  let autoAsk = null
  async function ensureAuto() {
    const id = songId.value
    if (id == null) throw new Error('no hay canción')
    if (autoOwn.value?.id === id) return autoOwn.value.own
    if (!autoAsk || autoAsk.id !== id) {
      const ask = api.beatGridNow(id).then((r) => {
        if (!r || r.pending || !r.beats?.length) {
          throw new Error('el clic automático de esta canción aún no está listo')
        }
        const own = gridToOwn(r)
        if (songId.value === id) autoOwn.value = { id, own }
        return own
      })
      autoAsk = { id, ask }
      ask.catch(() => {
        if (autoAsk?.ask === ask) autoAsk = null
      })
    }
    return autoAsk.ask
  }
  /** El automático cambió (se re-analizó): el de antes ya no vale. */
  function forgetAuto() {
    autoOwn.value = null
    autoAsk = null
    if (ghostAuto.value && editing.value) ensureAuto().catch(() => {})
  }
  const ghostBeats = computed(() =>
    ghostAuto.value && editing.value && autoOwn.value?.id === songId.value
      ? autoOwn.value.own.beats
      : []
  )
  watch(
    [ghostAuto, editing, songId],
    ([on, ed]) => {
      if (on && ed)
        ensureAuto().catch((e) => notify(`No se pudo traer el automático: ${errorMessage(e)}`))
    },
    { flush: 'post' }
  )

  // ------------------------------------------------------------ el objetivo de las herramientas
  /**
   * Sobre qué actúan las herramientas: los pulsos elegidos o, sin elegir, los
   * del tramo (A-B). Sin ninguno, no hay nada que tocar.
   */
  const target = computed(() => {
    const n = beatsNow.value.length
    const chosen = selection.value.filter((i) => i < n)
    if (chosen.length) return chosen
    const loops = study.value.loops || []
    if (!loops.length) return []
    return selectBetween(beatsNow.value, loops[0][0], loops.at(-1)[1])
  })
  const targetFromLoop = computed(
    () => !selection.value.some((i) => i < beatsNow.value.length) && target.value.length > 0
  )
  /** Su tramo en segundos [primer pulso, último pulso], o null. */
  const targetSpan = computed(() => {
    const sp = spanOf(target.value)
    if (!sp) return null
    const b = beatsNow.value
    return [b[sp[0]], b[sp[1]]]
  })
  /** Cómo se llama lo que se tiene entre manos (para el resumen). */
  const targetInfo = computed(() => {
    const sp = targetSpan.value
    if (!sp) return null
    const sel = filled(target.value)
    return {
      count: sel.length,
      from: sp[0],
      to: sp[1],
      bpm: tempoOf(beatsNow.value, sel),
      fromLoop: targetFromLoop.value
    }
  })
  /** El tramo para «Tempo y onda»: el de los pulsos elegidos o el del bucle. */
  const alignSpan = computed(() => {
    if (selection.value.length) return targetSpan.value
    const loops = study.value.loops || []
    return loops.length ? [loops[0][0], loops.at(-1)[1]] : null
  })

  // ---------------------------------------------------------------- las herramientas
  /**
   * Una herramienta: actúa sobre el objetivo y deja una sola entrada en la historia.
   * @param {any} label
   * @param {(own: any, sel: number[]) => any} make
   * @param {{ key?: string, needs?: number }} [opts]
   */
  async function tool(label, make, { key, needs = 1 } = {}) {
    const sel = target.value
    const own = currentOwn()
    if (!own || sel.length < needs) return false
    const r = make(own, sel)
    if (!r) return false
    const done = await commitOwn(r, label(sel, r), { key })
    if (done && Array.isArray(r.selection)) selection.value = [...r.selection]
    return done
  }
  const selectAll = () => setSelection(selectRange(0, beatsNow.value.length - 1))
  const selectNone = () => (selection.value = [])
  function selectLoop() {
    const loops = study.value.loops || []
    if (!loops.length) return
    setSelection(selectBetween(beatsNow.value, loops[0][0], loops.at(-1)[1]))
  }
  /** Del primer elegido (o del primero que viene tras la aguja) hasta el final. */
  function selectToEnd(from = 0) {
    const n = beatsNow.value.length
    const first = selection.value.length
      ? Math.min(...selection.value)
      : Math.max(
          0,
          beatsNow.value.findIndex((t) => t >= from)
        )
    if (n) setSelection(selectRange(first, n - 1))
  }
  /** Poner el tramo de lo elegido en bucle y que suene. */
  async function hearTarget() {
    const sp = targetSpan.value
    if (!sp || !deps.hear) return
    // un pulso de margen por cada lado, para oír cómo entra y cómo sale
    const bpm = tempoOf(beatsNow.value, filled(target.value))
    const period = bpm ? 60 / bpm : 0.5
    const from = Math.max(0, sp[0] - period)
    const to = Math.min(durationNow() || Infinity, sp[1] + period)
    await deps.hear(from, Math.max(to, from + 0.5))
  }
  const named = (what) => (sel) => `${what} ${beatsWord(filled(sel).length)}`
  const moveBy = (ms_) =>
    tool(
      (sel) => `desplazar ${beatsWord(sel.length)}`,
      (own, sel) => {
        const r = moveBlock(own, sel, ms_ / 1000, { duration: durationNow() || undefined })
        if (!r.applied) {
          notify('No se puede mover más: el pulso de al lado se quedaría pegado', 'info', 3)
          return null
        }
        return r
      },
      { key: 'shift' }
    )
  const nudge = (seconds) =>
    tool(
      (sel) => `mover ${beatsWord(sel.length)}`,
      (own, sel) => {
        const r = moveBlock(own, sel, seconds, { duration: durationNow() || undefined })
        return r.applied ? r : null
      },
      { key: 'nudge' }
    )
  const spread = () =>
    tool(named('repartir parejo'), (own, sel) => spreadEven(own, sel), { needs: 3 })
  const doubleUp = () =>
    tool(
      () => 'doble de pulsos',
      (own, sel) => doubleBeats(own, sel),
      { needs: 2 }
    )
  const halve = () =>
    tool(
      () => 'mitad de pulsos',
      (own, sel) => halveBeats(own, sel),
      { needs: 3 }
    )
  const everyN = (n) =>
    tool(
      () => `el «1» cada ${n}`,
      (own, sel) => barsEvery(own, sel, n, { setMeter: n !== own.meter })
    )
  const remove = () =>
    tool(
      (sel) => `quitar ${beatsWord(sel.length)}`,
      (own, sel) => {
        const r = removeBeats(own, sel)
        if (r.beats.length === own.beats.length) {
          notify('La canción no puede quedarse con menos de dos pulsos', 'info', 4)
          return null
        }
        return r
      }
    )
  async function revertTarget() {
    const sp = alignSpan.value
    const own = currentOwn()
    if (!sp || !own) return false
    let auto
    try {
      auto = await ensureAuto()
    } catch (e) {
      notify(`No se pudo traer el automático: ${errorMessage(e)}`)
      return false
    }
    const r = revertToAuto(currentOwn() ?? own, auto, /** @type {[number, number]} */ (sp))
    const done = await commitOwn(r, 'volver al automático en el tramo')
    if (done) selection.value = [...r.selection]
    return done
  }
  function addAt(t) {
    const own = currentOwn()
    if (!own) return false
    const r = addBeatAt(own, t, { duration: durationNow() || undefined })
    if (r.refused) return false
    return commitOwn(r, 'añadir un pulso')
  }

  // --------------------------------------------------------------- probar sin guardar
  /** Lo que se ve de la prueba en la onda (líneas fantasma), o null. */
  const preview = shallowRef(/** @type {{beats: number[], bars: number[]}|null} */ (null))
  /** Hay una prueba sonando en el motor: lo guardado es otra cosa. */
  let sounding = null
  let soundTimer = null
  function setPreview(own) {
    preview.value = own ? { beats: own.beats, bars: own.bars || [] } : null
  }
  /** Hace sonar `own` en el motor SIN guardarlo (poco después del último cambio). */
  function hearOwn(own) {
    clearTimeout(soundTimer)
    if (!own) return stopSound()
    soundTimer = setTimeout(() => quiet(playTrial(own)), 200)
    return Promise.resolve()
  }
  /** Dejar de oír la prueba (el motor vuelve a lo guardado) sin quitar lo que se ve de ella. */
  function stopSound() {
    const keep = preview.value
    const done = endPreview()
    preview.value = keep
    return done
  }
  async function playTrial(own) {
    const id = songId.value
    if (id == null || !editing.value) return
    if (!sounding) {
      // lo guardado, para devolverlo
      sounding = { id, own: copyOwn(savedOwn()), metro: { ...(study.value.metronome || {}) } }
      frozen.value = baseGrid.value
      // el automático lleva su doble y su «1» corrido: la prueba ya los trae vistos
      if (!sounding.own) await player.setMetronome({ mult: 0, shift: 0 })
    }
    await click.applyOwn(id, { beats: own.beats, bars: own.bars, meter: own.meter })
  }
  /**
   * Se acabó la prueba (sin guardarla): el estado, sin tocar el motor. Quien
   * lo llama decide si el motor vuelve a lo guardado (`endPreview`) o si va
   * a recibir otra cosa (un cambio guardado, o deshacer).
   */
  function endPreviewState({ keepView = false } = {}) {
    clearTimeout(soundTimer)
    soundTimer = null
    preview.value = null
    const was = sounding
    sounding = null
    // (la onda sigue enseñando lo guardado hasta que el motor lo recupere)
    if (!keepView) frozen.value = null
    return was
  }
  /**
   * La salida de TODA prueba (cancelar, Esc, cerrar el panel, salir de
   * Ajustar, cambiar de canción, desmontar): el motor vuelve a lo guardado.
   */
  async function endPreview() {
    const was = endPreviewState({ keepView: true })
    if (!was) {
      if (!sounding) frozen.value = null
      return
    }
    try {
      const m = was.metro || {}
      if (!was.own) await player.setMetronome({ mult: m.mult || 0, shift: m.shift || 0 })
      await click.applyOwn(was.id, was.own)
    } finally {
      // salvo que ya empezara otra prueba mientras tanto
      if (!sounding) frozen.value = null
    }
  }
  /**
   * Aplicar lo probado: se guarda y pasa a sonar (no se devuelve el motor a lo
   * de antes para volver a cambiarlo al instante).
   */
  function commitPreview(own, label, nextSelection) {
    clearTimeout(soundTimer)
    const done = commitOwn(own, label)
    endPreviewState()
    panelOpen.value = false
    if (nextSelection) selection.value = [...nextSelection]
    return done
  }
  function closePanel() {
    panelOpen.value = false
    return endPreview()
  }
  function togglePanel() {
    if (panelOpen.value) return closePanel()
    panelOpen.value = true
    return Promise.resolve()
  }
  /** Cambia una de las tres cosas que se ven en la onda (se recuerda). @param {'showHits'|'snapHits'|'ghostAuto'|'hearTrial'} name */
  function toggle(name) {
    const r = { showHits, snapHits, ghostAuto, hearTrial }[name]
    if (r) r.value = !r.value
  }
  /** De dónde salen los golpes: la mezcla o la batería separada. @param {'mix'|'drums'} source */
  function setHitsSource(source) {
    hitsSource.value = source === 'drums' && hasDrums.value ? 'drums' : 'mix'
  }

  // -------------------------------------------------------------- salir y cambiar
  watch(editing, (on) => {
    if (on) return
    panelOpen.value = false
    selection.value = []
    quiet(endPreview())
  })
  // el estudio se esconde (sigue sonando todo): una prueba sin guardar no se queda sonando a escondidas
  watch(
    () => !!toValue(deps.visible ?? true),
    (shown) => {
      if (shown) return
      panelOpen.value = false
      quiet(endPreview())
    }
  )
  watch(songId, () => {
    // la historia es de cada canción
    history.clear()
    autoSettings = null
    pendingMetro = null
    dirty = false
    selection.value = []
    panelOpen.value = false
    hits.value = []
    hitsRef.value = 0
    hitsStatus.value = 'idle'
    hitsError.value = ''
    hitsSource.value = 'mix'
    autoOwn.value = null
    autoAsk = null
    bump()
    quiet(endPreview())
  })
  onUnmounted(() => {
    clearTimeout(soundTimer)
    quiet(endPreview())
  })

  // ---------------------------------------------------------------------- las teclas
  /** ¿Ha pulsado una tecla de lo suyo? Si es así, la atiende y no la deja pasar. */
  function onKey(e) {
    if (!toValue(deps.visible ?? true)) return false
    if (typeof document !== 'undefined' && document.querySelector(HOTKEY_BLOCKERS)) return false
    const key = String(e.key || '')
    const lower = key.toLowerCase()
    const ctrl = e.ctrlKey || e.metaKey
    const typing = inside(e.target, EDITABLE)
    const take = () => {
      e.preventDefault()
      e.stopImmediatePropagation()
      return true
    }
    // deshacer y rehacer: en un campo de texto manda el del propio campo
    if (ctrl && !e.altKey && !typing) {
      if (lower === 'z' && !e.shiftKey) {
        if (!history.canUndo) return false
        undo()
        return take()
      }
      if ((lower === 'z' && e.shiftKey) || lower === 'y') {
        if (!history.canRedo) return false
        redo()
        return take()
      }
    }
    if (!editing.value) return false
    if (key === 'Escape') {
      // dentro del panel, Esc lo cierra aunque se esté escribiendo en él
      if (typing && !inside(e.target, '.beat-align')) return false
      if (panelOpen.value) {
        quiet(closePanel())
        return take()
      }
      if (selection.value.length && !typing) {
        selectNone()
        return take()
      }
      return false
    }
    if (typing) return false
    if (ctrl && !e.altKey && !e.shiftKey && lower === 'a') {
      if (!beatsNow.value.length) return false
      selectAll()
      return take()
    }
    if (key === 'Delete' && !ctrl && selection.value.length) {
      if (!e.repeat) remove()
      return take()
    }
    if ((key === 'ArrowLeft' || key === 'ArrowRight') && selection.value.length) {
      if (inside(e.target, ARROW_OWNERS) || e.altKey) return false
      const step = e.shiftKey ? 0.001 : ctrl ? 0.025 : 0.005
      nudge(key === 'ArrowLeft' ? -step : step)
      return take()
    }
    return false
  }
  onMounted(() => window.addEventListener('keydown', onKey, true))
  onUnmounted(() => window.removeEventListener('keydown', onKey, true))

  return reactive({
    // la historia
    canUndo,
    canRedo,
    undoLabel,
    redoLabel,
    undo,
    redo,
    clearHistory: () => {
      history.clear()
      bump()
    },
    // el camino
    commitOwn,
    commitPreview,
    onTimelineGrid,
    currentOwn,
    hasOwn,
    viewGrid,
    beats: beatsNow,
    // la selección
    selection,
    setSelection,
    selectAll,
    selectNone,
    selectLoop,
    selectToEnd,
    target,
    targetInfo,
    targetFromLoop,
    alignSpan,
    // las herramientas
    moveBy,
    nudge,
    spread,
    doubleUp,
    halve,
    everyN,
    remove,
    revertTarget,
    addAt,
    hearTarget,
    // lo que se ve en la onda
    showHits,
    snapHits,
    ghostAuto,
    ghostBeats,
    hits,
    hitsRef,
    hitsSource,
    hitsStatus,
    hitsError,
    hasDrums,
    loadHits,
    forgetAuto,
    ensureAuto,
    // el panel «Tempo y onda»
    panelOpen,
    togglePanel,
    closePanel,
    preview,
    setPreview,
    hearTrial,
    hearOwn,
    stopSound,
    endPreview,
    toggle,
    setHitsSource,
    // las teclas
    onKey,
    duration: computed(durationNow)
  })
}
