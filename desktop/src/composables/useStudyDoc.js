// @ts-check
// Lo que el modo estudio guarda con cada canción (StudyBar.vue): los tramos,
// la velocidad y el tono, el clic, los marcadores, las guías, las notas y la
// mezcla de las pistas. Aquí está el estudio de la canción abierta, cómo se
// guarda (solo lo que no es lo de siempre, como lo espera el núcleo) y cuándo
// (600 ms después del último cambio). Leer lo guardado al cambiar de canción
// (`loadFor`) se queda en StudyBar: orquesta el clic, las guías y las pistas.
import { ref, markRaw } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from './useNotices.js'
import { round2 } from '../utils/num.js'

// `loops`: los tramos que se repiten, [[a, b], …] (uno solo es el bucle A-B);
// `mixer`: las pistas separadas, si suenan (`on`) y cómo va cada una;
// `cues`: las guías habladas, [{t, text?, voice?, file, gain?, lane?}];
// `cueLanes`: sus carriles (hasta 6), [{key, name, gain?, mute?, solo?, auto?}]
// (`auto`: el de la guía automática)
/** @returns {Record<string, any>} */
export const blank = () => ({
  loops: [],
  speed: 1,
  pitch: 0,
  metronome: {},
  markers: [],
  cues: [],
  cueLanes: [],
  notes: '',
  mixer: { on: false, tracks: {} }
})

/**
 * El clic manual (`metronome.own`) va sin hacer reactivo: son miles de
 * pulsos y nadie los cambia en su sitio (cada cambio es un `own` nuevo, que
 * sí avisa al sustituir el `metronome`). Hacerlo reactivo envolvía cada
 * número al leerlo.
 * @param {Record<string, any>} metronome
 */
export function withRawOwn(metronome) {
  const m = { ...metronome }
  if (m.own && typeof m.own === 'object') m.own = markRaw(m.own)
  return m
}

/** Un marcador que es un tramo (y no un instante suelto). @param {any} m */
export const isSpan = (m) => m.end > m.t

/** Lo guardado con la canción, si lo hay. @param {string | null | undefined} raw */
export function parse(raw) {
  try {
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

/**
 * Lo que vale cada ajuste del metrónomo cuando no se ha tocado. El compás
 * 0 (sin acento) SÍ es un ajuste: lo que no se ha tocado es null.
 * @type {Record<string, any>}
 */
const METRO_DEFAULTS = { bpm: null, meter: null, shift: 0, mult: 0, own: null }
/** @param {string} k @param {any} v */
export const isDefault = (k, v) => v == null || v === METRO_DEFAULTS[k]

/**
 * Lo que se guarda: solo lo que no es lo de siempre, como lo espera el nucleo.
 * @param {Record<string, any>} s
 */
export function payload(s) {
  const markers = s.markers.map((m) => ({
    t: m.t,
    ...(isSpan(m) ? { end: m.end } : {}),
    ...(m.parts?.length > 1 ? { parts: m.parts.map((p) => [...p]) } : {}),
    label: m.label,
    ...(m.notes?.trim() ? { notes: m.notes.trim() } : {})
  }))
  const metro = Object.fromEntries(
    Object.entries(s.metronome || {}).filter(([k, v]) => !isDefault(k, v))
  )
  const mixer = mixerPayload(s.mixer)
  const cues = (s.cues || []).map((c) => ({
    t: round2(c.t),
    file: c.file,
    ...(c.text ? { text: c.text } : {}),
    ...(c.voice ? { voice: c.voice } : {}),
    ...(c.gain != null && c.gain !== 1 ? { gain: c.gain } : {}),
    ...(c.lane ? { lane: c.lane } : {})
  }))
  const cueLanes = (s.cueLanes || []).map((l) => ({
    key: l.key,
    name: l.name || '',
    ...(l.gain != null && l.gain !== 1 ? { gain: l.gain } : {}),
    ...(l.mute ? { mute: true } : {}),
    ...(l.solo ? { solo: true } : {}),
    // el de la guía automática (se rehace con ella)
    ...(l.auto ? { auto: true } : {})
  }))
  return {
    ...(mixer ? { mixer } : {}),
    ...(s.loops.length === 1
      ? { loop: [...s.loops[0]] }
      : s.loops.length > 1
        ? { loops: s.loops.map((x) => [...x]) }
        : {}),
    ...(s.speed && s.speed !== 1 ? { speed: s.speed } : {}),
    ...(s.pitch ? { pitch: s.pitch } : {}),
    ...(Object.keys(metro).length ? { metronome: metro } : {}),
    ...(markers.length ? { markers } : {}),
    ...(cues.length ? { cues } : {}),
    ...(cueLanes.length ? { cueLanes } : {}),
    ...(s.notes.trim() ? { notes: s.notes.trim() } : {})
  }
}
/**
 * Lo que se guarda del mezclador: si suena y lo que no está como viene.
 * @param {Record<string, any> | undefined} m
 */
export function mixerPayload(m) {
  /** @type {Record<string, any>} */
  const tracks = {}
  for (const [key, t] of Object.entries(m?.tracks || {})) {
    const kept = {
      ...(t.gain != null && t.gain !== 1 ? { gain: t.gain } : {}),
      ...(t.pan ? { pan: t.pan } : {}),
      ...(t.mute ? { mute: true } : {}),
      ...(t.solo ? { solo: true } : {})
    }
    if (Object.keys(kept).length) tracks[key] = kept
  }
  const out = {
    ...(m?.on ? { on: true } : {}),
    ...(Object.keys(tracks).length ? { tracks } : {})
  }
  return Object.keys(out).length ? out : null
}

/** El estudio de la canción abierta y su guardado. Uno por StudyBar. */
export function useStudyDoc() {
  const study = ref(blank())
  /** La canción cuyo estudio se pidió (la que suena). */
  const loadedFor = ref(null)
  const saving = ref(false)
  /** De qué canción es ya `study` (mientras se lee la nueva, aún es el de la anterior). */
  const studyFor = ref(null)
  /** @type {ReturnType<typeof setTimeout> | null} */
  let saveTimer = null
  /**
   * Lo que falta por guardar: de que cancion es y como estaba al pedirlo. Se
   * apunta al programar el guardado, no al hacerlo: si la cancion cambiaba en
   * esos 600 ms, las notas de la primera acababan guardadas en la segunda (en
   * el indice y en la etiqueta del archivo) y la primera las perdia.
   * @type {{ id: any, study: Record<string, any> } | null}
   */
  let pending = null

  function scheduleSave() {
    const id = loadedFor.value
    if (!id) return
    pending = { id, study: payload(study.value) }
    clearTimeout(saveTimer)
    saveTimer = setTimeout(flushSave, 600)
  }
  /** Las canciones de las que ya se avisó de que su estudio vive solo en DanPlay. */
  const warnedOnlyHere = new Set()
  /**
   * El núcleo dice si la etiqueta del archivo se escribió. Si no se pudo (el
   * archivo no admite etiquetas, o no se le puede escribir), el estudio queda
   * solo en la base de DanPlay: se avisa una vez por canción, no en cada
   * guardado (que se hace solo, a cada cambio). Con `tags_pending` la etiqueta
   * aún no se escribió (el núcleo la aplaza): no es un fallo.
   * @param {any} id
   * @param {any} saved
   */
  function warnIfOnlyHere(id, saved) {
    if (!saved || saved.tags_pending || saved.tags_written !== false || warnedOnlyHere.has(id))
      return
    if (saved.tags_skipped === 'unsupported') {
      notify(
        'Este archivo no admite etiquetas: el estudio de esta canción se guarda solo en DanPlay'
      )
    } else if (saved.tags_skipped === 'failed') {
      notify(
        'No se pudo escribir en el archivo (¿es de solo lectura?): el estudio de esta canción se guarda solo en DanPlay'
      )
    } else return
    warnedOnlyHere.add(id)
  }
  /** Lo guardado, ya también en el archivo: el núcleo aplaza la etiqueta hasta que se deja la canción o el estudio. */
  function flushFile() {
    return api.flushStudy().catch(() => {})
  }
  /** Guarda ya lo pendiente, si hay algo. */
  async function flushSave() {
    clearTimeout(saveTimer)
    saveTimer = null
    const job = pending
    pending = null
    if (!job) return
    saving.value = true
    try {
      warnIfOnlyHere(job.id, await api.setStudy(job.id, job.study))
    } catch (e) {
      notify('No se pudo guardar el estudio: ' + errorMessage(e))
    } finally {
      saving.value = false
    }
  }

  return {
    study,
    loadedFor,
    studyFor,
    saving,
    scheduleSave,
    flushSave,
    flushFile,
    warnIfOnlyHere
  }
}
