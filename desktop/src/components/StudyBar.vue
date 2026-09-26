<script setup>
/**
 * El modo estudio: para machacar un trozo de la canción que suena.
 *
 * Es una capa que sube desde el reproductor y ocupa media pantalla, sin
 * recolocar la lista de detrás. Arriba, la forma de onda con la regla de
 * minutos: ahí se elige el tramo que se repite (arrastrando, cogiendo los
 * bordes, o con las teclas A y B mientras suena). Con el candado de la onda
 * puesto, que es como empieza, mientras suena un clic no mueve la canción y
 * elegir un tramo no salta a él: entra cuando la canción llega. Debajo,
 * cuatro columnas:
 *
 * - Reproducción: el tramo, la velocidad (sin cambiar el tono) y el tono
 *   corrido, contado en tonos: de cuarto, de medio o de uno entero.
 * - Metrónomo: detecta solo el pulso y el compás de la canción y entra en
 *   el «1». Se puede doblar (una lenta a corcheas), poner el tempo a mano
 *   con decimales, cambiar el compás (o dejar todos los clics iguales) y
 *   subir el clic y la canción por encima de como vienen.
 * - Marcadores: tramos guardados con nombre (inicio Y final) y sus notas;
 *   pulsar uno vuelve a poner ese bucle y coloca la canción al principio.
 * - Notas: un solo cuadro con dos pestañas, las de la canción y las del
 *   marcador elegido.
 *
 * Encima de la onda, las pistas separadas de la canción (batería, voces,
 * bajo…): si no las tiene, se separa desde aquí; si las tiene, suenan en su
 * lugar con un carril cada una, y cada una se calla, se deja sola, se sube
 * o se lleva a un lado. La mezcla se guarda en un archivo (sin batería, para
 * el móvil). Con el bucle, la velocidad, el tono y el metrónomo, igual que
 * sobre la canción.
 *
 * Todo se guarda con la canción —en el índice y en una etiqueta del
 * archivo— y se va con ella si se borra. Al abrir se aplica lo guardado;
 * al cerrar se quita y la canción vuelve a sonar normal.
 */
import { ref, computed, watch, onUnmounted, shallowRef } from 'vue'
import { api, app as tauriApp, errorMessage, pickSavePath, JOBS } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { ask } from '../composables/useDialog.js'
import { usePlayback, MAX_VOLUME } from '../composables/usePlayback.js'
import { useSeparation } from '../composables/useSeparation.js'
import { stemPlan, mixFileName } from '../utils/stems.js'
import { useHotkeys } from '../composables/useHotkeys.js'
import { formatTime } from '../utils/format.js'
import { transposeKey, toneLabel, toneUnit } from '../utils/theory.js'
import { effectiveGrid, formatBpm, parseBpm, multFactor, METERS } from '../utils/beats.js'
import {
  cleanSegments,
  sameSegments,
  segmentAt,
  nextSegment,
  segmentsLength,
  snapToBeats
} from '../utils/segments.js'
import { openMenu } from '../composables/useContextMenu.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SliderField from './ui/SliderField.vue'
import StudyTimeline from './StudyTimeline.vue'

const emit = defineEmits(['close'])
const player = usePlayback()
const {
  track,
  position,
  duration,
  speed,
  pitch,
  metronome,
  loopDefer,
  pitchPreserved,
  playing,
  volume
} = player

const SPEEDS = [0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.25]
// `loops`: los tramos que se repiten, [[a, b], …] (uno solo es el bucle A-B);
// `mixer`: las pistas separadas, si suenan (`on`) y cómo va cada una
const blank = () => ({
  loops: [],
  speed: 1,
  pitch: 0,
  metronome: {},
  markers: [],
  notes: '',
  mixer: { on: false, tracks: {} }
})
const study = ref(blank())
// las pistas separadas de la canción (ver más abajo): aquí arriba porque las
// toca `loadFor`, que corre nada más montar
const separation = useSeparation()
/** Lo que dice el núcleo de las pistas de la canción, o null si no tiene. */
const stems = shallowRef(null)
/** El archivo de la canción, para el nombre de la mezcla que se guarda. */
const songFile = ref('')
/** Se pidió separar desde aquí esta canción: al acabar, suenan sus pistas. */
let wantStems = null
const loadedFor = ref(null)
const saving = ref(false)
/** El marcador elegido (uno de `study.markers`), o null. */
const selected = ref(null)
/** La rejilla de pulsos de la cancion que suena, tal como la dio el analisis, o null. */
const grid = shallowRef(null)
const analyzing = ref(false)
const gridError = ref('')
let gridFor = null
/** La pestaña de notas: las de la cancion o las del marcador elegido. */
const notesTab = ref('song')
let saveTimer = null
/**
 * Lo que falta por guardar: de que cancion es y como estaba al pedirlo. Se
 * apunta al programar el guardado, no al hacerlo: si la cancion cambiaba en
 * esos 600 ms, las notas de la primera acababan guardadas en la segunda (en
 * el indice y en la etiqueta del archivo) y la primera las perdia.
 */
let pending = null

const round2 = (v) => Math.round(v * 100) / 100
const isSpan = (m) => m.end > m.t
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

/** Lo que esta barra recuerda en el navegador (el candado, el paso del tono). */
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

/** Lo guardado con la canción, si lo hay. */
function parse(raw) {
  try {
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}
async function loadFor(id) {
  // lo pendiente es de la cancion que se deja: se guarda en ELLA ya, sin
  // esperar (su copia y su id ya van apuntados)
  flushSave()
  loadedFor.value = id
  selected.value = null
  grid.value = null
  stems.value = null
  if (!id) {
    study.value = blank()
    return
  }
  try {
    const song = await api.song(id)
    const s = parse(song?.study)
    if (loadedFor.value !== id) return
    songFile.value = song?.file || ''
    // varios tramos van en `loops`; uno solo, en `loop` (como siempre)
    const loops = cleanSegments(
      Array.isArray(s.loops) && s.loops.length ? s.loops : Array.isArray(s.loop) ? [s.loop] : []
    )
    study.value = {
      loops,
      speed: s.speed || 1,
      // en semitonos, con fracciones (un cuarto de tono es medio semitono)
      pitch: Number.isFinite(s.pitch) ? round2(clamp(s.pitch, -12, 12)) : 0,
      metronome: typeof s.metronome === 'object' && s.metronome ? { ...s.metronome } : {},
      markers: (s.markers || []).map((m) => ({ ...m })),
      notes: s.notes || '',
      mixer: {
        on: !!s.mixer?.on,
        tracks: Object.fromEntries(
          Object.entries(s.mixer?.tracks || {}).map(([k, v]) => [k, { ...v }])
        )
      }
    }
    multi.value = loops.length > 1
    // lo guardado se aplica al entrar: para eso se guardo
    await player.setLoops(loops)
    if (study.value.speed !== speed.value) await player.setSpeed(study.value.speed)
    if (study.value.pitch !== pitch.value) await player.setPitch(study.value.pitch)
    await player.resetMetronomeOverrides(study.value.metronome)
    selected.value = markerOf(study.value.loops)
    // el compas se analiza ya, para que el clic entre al momento al pedirlo
    ensureGrid()
    // y las pistas: si las tiene y se dejaron sonando, vuelven a sonar
    await loadStems(id)
  } catch (e) {
    notify(errorMessage(e))
  }
}
watch(
  () => track.value?.id ?? null,
  (id) => loadFor(id),
  { immediate: true }
)

/**
 * Lo que vale cada ajuste del metrónomo cuando no se ha tocado. El compás
 * 0 (sin acento) SÍ es un ajuste: lo que no se ha tocado es null.
 */
const METRO_DEFAULTS = { bpm: null, meter: null, shift: 0, mult: 0 }
const isDefault = (k, v) => v == null || v === METRO_DEFAULTS[k]

/** Lo que se guarda: solo lo que no es lo de siempre, como lo espera el nucleo. */
function payload(s) {
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
    ...(s.notes.trim() ? { notes: s.notes.trim() } : {})
  }
}
/** Lo que se guarda del mezclador: si suena y lo que no está como viene. */
function mixerPayload(m) {
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
function scheduleSave() {
  const id = loadedFor.value
  if (!id) return
  pending = { id, study: payload(study.value) }
  clearTimeout(saveTimer)
  saveTimer = setTimeout(flushSave, 600)
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
    await api.setStudy(job.id, job.study)
  } catch (e) {
    notify('No se pudo guardar el estudio: ' + errorMessage(e))
  } finally {
    saving.value = false
  }
}

// ---- el candado de la onda
// Puesto (lo normal), mientras suena un clic en la onda no mueve la cancion
// y elegir un tramo no salta a el: el tramo entra cuando la cancion llega
// (Rust lo espera; ver `ab_loop`). En pausa, la onda coloca como siempre.
const LOCK_KEY = 'danplay.study.lock'
const locked = ref(readPref(LOCK_KEY, true) !== false)
watch(locked, (v) => savePref(LOCK_KEY, v))
/** La onda no puede mover la cancion ahora. */
const guarded = computed(() => locked.value && playing.value)

// ---- los tramos que se repiten: uno (el bucle A-B) o varios seguidos
/** Varios tramos a la vez: cada arrastre en la onda añade otro. */
const multi = ref(false)
const pendingA = ref(null)
const hasLoop = computed(() => study.value.loops.length > 0)
const loopLabel = computed(() => {
  const list = study.value.loops
  if (list.length === 1) return `${formatTime(list[0][0])} – ${formatTime(list[0][1])}`
  if (list.length > 1) return `${list.length} tramos · ${formatTime(segmentsLength(list))}`
  if (pendingA.value != null) return `A en ${formatTime(pendingA.value)} · pulsa B donde acabe`
  return 'sin tramo'
})

/**
 * Deja los tramos. `jump` dice si la cancion va a ellos: 'outside' si va
 * por fuera de todos (lo de siempre: al siguiente que venga), 'never' para
 * dejarla donde va (la onda con candado), o un segundo concreto (la tecla
 * B vuelve a su A). Con `defer`, esperan a que acabe la cancion; si no se
 * dice, sigue como estaba: mover un borde o ajustarlo a los pulsos no quita
 * el «al acabar» que uno puso (saltar a un tramo si lo quita, en Rust).
 */
async function applyLoops(list, { jump = 'outside', defer = loopDefer.value } = {}) {
  const clean = cleanSegments(list)
  if (clean.some(([a, b]) => b - a < 0.5)) {
    notify('El bucle tiene que durar al menos medio segundo')
    return
  }
  pendingA.value = null
  study.value.loops = clean
  await player.setLoops(clean, { defer })
  if (typeof jump === 'number') await player.seek(jump)
  else if (jump === 'outside' && clean.length && segmentAt(clean, position.value) < 0) {
    await player.seek(clean[nextSegment(clean, position.value)][0])
  }
  scheduleSave()
}
async function clearLoop() {
  pendingA.value = null
  study.value.loops = []
  await player.clearLoop()
  scheduleSave()
}
/** Los tramos de un marcador: sus partes, su tramo, o nada (un instante). */
const partsOf = (m) => (m.parts?.length > 1 ? m.parts : isSpan(m) ? [[m.t, m.end]] : [])
/** Un marcador pasa a ser esos tramos (uno solo, o varios como partes). */
function setMarkerParts(m, list) {
  m.t = list[0][0]
  m.end = list.at(-1)[1]
  if (list.length > 1) m.parts = list.map((x) => [...x])
  else delete m.parts
  study.value.markers = [...study.value.markers].sort((x, y) => x.t - y.t)
}
/**
 * Lo que manda la linea de tiempo. Un tramo nuevo dibujado de cero (o uno
 * que se añade o se quita) deja de apuntar al marcador que hubiera elegido;
 * mover un borde con un marcador elegido cambia el tramo DEL marcador, que es
 * lo que uno espera al estirarlo.
 */
function onLoops(list, { mode } = {}) {
  if (!list.length) return clearLoop()
  const m = selected.value
  if (mode === 'edit' && m && partsOf(m).length) setMarkerParts(m, cleanSegments(list))
  else if (mode === 'select' || mode === 'add' || mode === 'remove') selected.value = null
  return applyLoops(list, { jump: guarded.value || mode === 'remove' ? 'never' : 'outside' })
}
function markA() {
  if (!track.value) return
  const at = round2(position.value)
  selected.value = null
  const list = study.value.loops
  // con un solo tramo, A lo acorta por delante
  if (!multi.value && list.length === 1 && at < list[0][1] - 0.5) {
    return applyLoops([[at, list[0][1]]])
  }
  if (!multi.value && list.length) clearLoop()
  pendingA.value = at
}
function markB() {
  if (!track.value) return
  const at = round2(position.value)
  const list = study.value.loops
  const single = !multi.value && list.length === 1
  const from = pendingA.value ?? (single ? list[0][0] : 0)
  if (at - from < 0.5) {
    notify('El bucle tiene que durar al menos medio segundo')
    return
  }
  selected.value = null
  // B cierra el tramo donde va la cancion y vuelve a A: es la repeticion
  // A-B. Con «varios», el tramo se añade a los que hubiera.
  return applyLoops(multi.value ? [...list, [from, at]] : [[from, at]], { jump: from })
}

// ---- las opciones de un tramo (el boton ⋯ de la onda, o el clic derecho)
/** Ir ya a ese tramo y que suene. */
async function playNow(index = 0) {
  const seg = study.value.loops[index]
  if (!seg) return
  if (!playing.value) await player.toggle()
  await player.seek(seg[0])
}
/** Que la cancion siga hasta el final y entonces se repitan los tramos (o no). */
const deferLoops = (on) => player.setLoops(study.value.loops, { defer: on })
/** Los bordes de los tramos, al pulso mas cercano: para que entren a tiempo. */
function snapLoops() {
  const beats = shownGrid.value?.beats
  if (!beats?.length || !hasLoop.value) return
  const snapped = snapToBeats(study.value.loops, beats)
  const m = selected.value
  if (m && partsOf(m).length) setMarkerParts(m, snapped)
  return applyLoops(snapped, { jump: 'never' })
}
function removeSegment(index) {
  return onLoops(
    study.value.loops.filter((_, i) => i !== index),
    { mode: 'remove' }
  )
}
/**
 * Lo que se puede hacer con un tramo de la onda (o, con `index` -1, en ese
 * punto de ella). Las que mueven la cancion son a proposito, asi que valen
 * tambien con el candado puesto.
 */
function showOptions({ index, t, x, y, event }) {
  const list = study.value.loops
  const seg = list[index]
  const items = []
  if (seg) {
    items.push({
      label: 'Reproducir ahora',
      icon: 'play',
      note: formatTime(seg[0]),
      action: () => playNow(index)
    })
    items.push(
      loopDefer.value
        ? {
            label: 'Repetir ya, sin esperar al final',
            icon: 'repeat',
            action: () => deferLoops(false)
          }
        : {
            label: 'Repetir cuando acabe la canción',
            icon: 'repeat',
            action: () => deferLoops(true)
          }
    )
  }
  items.push({ label: 'Ir aquí', icon: 'right', note: formatTime(t), action: () => player.seek(t) })
  if (seg) {
    items.push({
      label: 'Ajustar a los pulsos',
      icon: 'note',
      disabled: !shownGrid.value?.beats?.length,
      action: snapLoops
    })
    if (!loopSaved.value)
      items.push({ label: 'Guardar como marcador', icon: 'save', action: saveMarker })
  }
  items.push(
    multi.value
      ? { label: 'Volver a un solo tramo', icon: 'repeat', action: () => (multi.value = false) }
      : { label: 'Añadir más tramos', icon: 'plus', action: () => (multi.value = true) }
  )
  if (list.length) {
    items.push({ separator: true })
    if (seg && list.length > 1) {
      items.push({ label: 'Quitar este tramo', icon: 'close', action: () => removeSegment(index) })
    }
    items.push({
      label: list.length > 1 ? 'Quitar todos los tramos' : 'Quitar la selección',
      icon: 'trash',
      danger: true,
      action: clearLoop
    })
  }
  const title = seg ? (list.length > 1 ? `Tramo ${index + 1}` : 'El tramo') : 'La onda'
  openMenu(event ?? { clientX: x, clientY: y }, items, title)
}
useHotkeys({ a: markA, b: markB })

// ---- velocidad
async function setSpeed(v) {
  const s = Math.round(Math.max(0.25, Math.min(3, Number(v) || 1)) * 100) / 100
  study.value.speed = s
  await player.setSpeed(s)
  scheduleSave()
}
const speedPercent = computed(() => Math.round(speed.value * 100))

// ---- tono, contado como lo cuenta un musico: medio tono es un semitono.
// El cuarto de tono (medio semitono) sirve para ponerse a la par de una
// grabacion que no esta afinada a 440.
const PITCH_STEPS = [
  { v: 0.5, n: '¼', name: 'un cuarto de tono', title: 'De cuarto en cuarto de tono' },
  { v: 1, n: '½', name: 'medio tono', title: 'De medio en medio tono (un semitono)' },
  { v: 2, n: '1', name: 'un tono', title: 'De tono en tono' }
]
const PITCH_STEP_KEY = 'danplay.study.pitchStep'
const savedStep = readPref(PITCH_STEP_KEY, 1)
/** De cuánto en cuánto se mueve el tono, en semitonos. */
const pitchStep = ref(PITCH_STEPS.some((s) => s.v === savedStep) ? savedStep : 1)
watch(pitchStep, (v) => savePref(PITCH_STEP_KEY, v))
const stepName = computed(() => PITCH_STEPS.find((s) => s.v === pitchStep.value)?.name || '')
const pitchLabel = computed(() => toneLabel(pitch.value))
const pitchTitle = computed(() =>
  pitch.value
    ? `${toneLabel(pitch.value)} ${toneUnit(pitch.value)} (${round2(pitch.value)} semitonos)`
    : 'Como está grabada'
)
const keyNow = computed(() => (track.value?.key ? transposeKey(track.value.key, pitch.value) : ''))
async function setPitch(n) {
  const p = round2(clamp(Number(n) || 0, -12, 12))
  study.value.pitch = p
  await player.setPitch(p)
  scheduleSave()
}

// ---- metronomo
// El pulso y el compas los detecta Rust (`analyzeBeats`); la rejilla se
// pinta sobre la onda y el clic la sigue. Lo que uno ajuste a mano (tempo,
// compas, «1», doble/mitad) se guarda con la cancion en `study.metronome`.
async function ensureGrid() {
  const path = player.state.path
  const id = loadedFor.value
  if (!path || !id || gridFor === path) return grid.value
  gridFor = path
  analyzing.value = true
  gridError.value = ''
  try {
    const g = await player.analyzeBeats(track.value?.bpm || null)
    if (loadedFor.value !== id) return null
    grid.value = g
    // con la rejilla ya en Rust, se vuelven a mandar los ajustes para que
    // el clic la coja (si esta sonando, entra en el siguiente pulso)
    await player.setMetronome({})
    return g
  } catch (e) {
    if (loadedFor.value === id) gridError.value = errorMessage(e)
    return null
  } finally {
    if (loadedFor.value === id) analyzing.value = false
  }
}
// la ruta que suena puede llegar despues de la ficha: se vuelve a intentar
watch(
  () => player.state.path,
  () => {
    if (loadedFor.value && !grid.value) {
      gridFor = null
      ensureGrid()
    }
  }
)
/**
 * La rejilla tal como suena, para la onda: con el doble o la mitad, el
 * compas y el «1» corrido. Antes se pintaba la del analisis tal cual, y al
 * pulsar ×2 no se veia cambiar nada.
 */
const shownGrid = computed(() => {
  const m = study.value.metronome || {}
  return effectiveGrid(grid.value, {
    mult: m.mult || 0,
    meter: m.meter ?? null,
    shift: m.shift || 0
  })
})

/** El tempo que se oye: siguiendo la cancion, el suyo por la velocidad; a mano, el puesto. */
const bpmHeard = computed(() => {
  const m = metronome.value
  const base = m.bpm || 0
  return m.free ? base : base * speed.value
})
const bpmShown = computed(() => (bpmHeard.value ? formatBpm(bpmHeard.value) : '—'))
/** Lo que se esta escribiendo en el tempo (o null: se enseña el que va). */
const bpmDraft = ref(null)
const metroStatus = computed(() => {
  const m = metronome.value
  if (analyzing.value) return 'buscando el pulso y el compás…'
  if (gridError.value) return 'sin compás detectado: va libre'
  if (!m.has_grid) return 'sin compás: va libre'
  if (m.free) return 'tempo a mano: entra con el pulso de la canción y va a su aire'
  const sure = m.confidence >= 0.6 ? 'seguro' : m.confidence >= 0.3 ? 'probable' : 'dudoso'
  return `sigue la canción · el «1» ${sure}`
})
function metroPatch(patch) {
  const m = { ...(study.value.metronome || {}) }
  for (const [k, v] of Object.entries(patch)) {
    if (isDefault(k, v)) delete m[k]
    else m[k] = v
  }
  study.value.metronome = m
  scheduleSave()
  return player.setMetronome(patch)
}
async function toggleMetronome() {
  if (!track.value) return
  const on = !metronome.value.on
  if (on && !grid.value && !gridError.value) await ensureGrid()
  await player.setMetronome({ on })
}
/**
 * Pone a mano el tempo que se oye, con decimales. Con el doble o la mitad
 * puestos se guarda el tempo sin ellos: al quitarlos, vuelve al que era.
 */
function setBpm(heard) {
  const base = heard / multFactor(metronome.value.mult)
  return metroPatch({ bpm: round2(clamp(base, 20, 300)) })
}
function bumpBpm(delta) {
  const now = Math.round((bpmHeard.value || 100) * 10) / 10
  return setBpm(now + delta)
}
/** El campo del tempo, para saber si lo que llega se esta escribiendo en el. */
let bpmInput = null
function onBpmFocus(e) {
  bpmInput = e.target
  bpmDraft.value = bpmHeard.value ? bpmShown.value : ''
  e.target.select?.()
}
/**
 * Lo que llega del campo del tempo. Vacio sin estar escribiendo en el es la
 * «x» del campo, que solo sale con tempo a mano: vuelve al de la cancion.
 */
function onBpmInput(v) {
  if (v === '' && document.activeElement !== bpmInput) {
    bpmDraft.value = null
    followSong()
    return
  }
  bpmDraft.value = v
}
function commitBpm() {
  const v = parseBpm(bpmDraft.value)
  bpmDraft.value = null
  if (v != null && Math.abs(v - bpmHeard.value) >= 0.05) setBpm(v)
}
/** En el tempo: Enter lo pone, Escape lo deja; las flechas lo mueven de décima en décima. */
function onBpmKey(e) {
  if (e.key === 'Enter') {
    e.preventDefault()
    e.target.blur()
  } else if (e.key === 'Escape') {
    bpmDraft.value = null
    e.target.blur()
  } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
    e.preventDefault()
    const step = (e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 1 : 0.1)
    const from = parseBpm(bpmDraft.value) ?? Math.round(bpmHeard.value * 10) / 10
    const to = Math.round(clamp(from + step, 10, 600) * 10) / 10
    bpmDraft.value = formatBpm(to)
    setBpm(to)
  }
}
const followSong = () => metroPatch({ bpm: null })
/**
 * Otro compas. El que detecto el analisis no se guarda como ajuste: si otro
 * dia se analiza mejor, que mande el analisis. El «1» corrido se olvida: el
 * de otro compas se saca otra vez de donde caia.
 */
const setMeter = (v) => metroPatch({ meter: grid.value?.meter === v ? null : v, shift: 0 })
const shiftOne = () =>
  metroPatch({ shift: ((metronome.value.shift || 0) + 1) % (metronome.value.meter || 4) })
const setMult = (mult) => metroPatch({ mult: metronome.value.mult === mult ? 0 : mult })
/** Lo que dice ×2 o ÷2, con los bpm de verdad: «68 → 136». */
function multTitle(mult) {
  const plain = bpmHeard.value / multFactor(metronome.value.mult)
  if (metronome.value.mult === mult) return `Quitar: volver a ${formatBpm(plain)} bpm`
  const to = `${formatBpm(plain)} → ${formatBpm(plain * multFactor(mult))} bpm`
  return mult === 1
    ? `El doble de clics (${to}): para ir a corcheas en una lenta, o si el tempo salió a la mitad`
    : `La mitad de clics (${to})`
}
const setMetroVolume = (v) => player.setMetronome({ volume: v })
const setSongVolume = (v) => player.setVolume(v)
const pctText = (v) => Math.round((Number(v) || 0) * 100) + ' %'

// ---- marcadores: tramos con nombre y notas (uno, o varios seguidos), o un
// instante suelto
/** El marcador que son exactamente esos tramos, si lo hay. */
function markerOf(list) {
  if (!list?.length) return null
  return (
    study.value.markers.find((m) => {
      const parts = partsOf(m)
      return parts.length > 0 && sameSegments(parts, list)
    }) || null
  )
}
function markerLabel(m) {
  const parts = partsOf(m)
  if (parts.length > 1) return `${parts.length} tramos · ${formatTime(parts[0][0])}…`
  return isSpan(m) ? `${formatTime(m.t)} – ${formatTime(m.end)}` : formatTime(m.t)
}
const loopSaved = computed(() => hasLoop.value && !!markerOf(study.value.loops))

function saveMarker() {
  if (!track.value) return
  let m
  const list = study.value.loops
  if (list.length) {
    const dup = markerOf(list)
    if (dup) {
      selected.value = dup
      return notify(`Ese tramo ya es «${dup.label}»`, 'info')
    }
    const n = study.value.markers.filter(isSpan).length + 1
    m = { t: list[0][0], end: list.at(-1)[1], label: `Tramo ${n}`, notes: '' }
    // varios tramos se guardan juntos: un solo marcador que los repite seguidos
    if (list.length > 1) m.parts = list.map((x) => [...x])
  } else {
    const n = study.value.markers.filter((x) => !isSpan(x)).length + 1
    m = { t: round2(position.value), label: `Marca ${n}`, notes: '' }
  }
  study.value.markers = [...study.value.markers, m].sort((a, b) => a.t - b.t)
  selected.value = m
  notesTab.value = 'marker'
  scheduleSave()
}
/**
 * Elige un marcador: pone sus tramos y, si `jump`, coloca la cancion al
 * principio. Desde la onda con candado, sonando, no salta: el tramo entra
 * cuando la cancion llega.
 */
async function selectMarker(m, { jump = true } = {}) {
  selected.value = m
  notesTab.value = 'marker'
  const parts = partsOf(m)
  if (parts.length) {
    pendingA.value = null
    if (parts.length > 1) multi.value = true
    study.value.loops = cleanSegments(parts)
    await player.setLoops(study.value.loops, { defer: loopDefer.value })
    scheduleSave()
  }
  if (jump) await player.seek(m.t)
}
function removeMarker(m) {
  if (selected.value === m) {
    selected.value = null
    notesTab.value = 'song'
  }
  study.value.markers = study.value.markers.filter((x) => x !== m)
  scheduleSave()
}
async function renameMarker(m) {
  const label = await ask({
    kind: 'prompt',
    title: 'Nombre del marcador',
    value: m.label,
    okLabel: 'Guardar'
  })
  if (!label) return
  m.label = label.slice(0, 60)
  scheduleSave()
}
const jump = (t) => player.seek(t)

// ---- notas: un solo cuadro, dos pestañas
watch(selected, (m) => {
  if (!m) notesTab.value = 'song'
})

// ---- las pistas separadas: batería, voces, bajo… cada una con su volumen
const hasStems = computed(() => !!stems.value?.complete)
const mixerOn = computed(() => hasStems.value && !!study.value.mixer?.on)
/** Cómo va cada pista (lo guardado, o como viene) y si suena. */
const plan = computed(() => stemPlan(stems.value?.tracks || [], study.value.mixer?.tracks || {}))
/** Los carriles de la onda: solo con las pistas sonando. */
const lanes = computed(() => (mixerOn.value ? plan.value : []))
const progress = computed(() => separation.progressOf(track.value?.id))
const canSeparate = computed(() => separation.state.ok)

async function loadStems(id) {
  // sin pistas (404), o el núcleo no contesta: null
  const info = await api.stems(id).catch(() => null)
  if (loadedFor.value !== id) return
  stems.value = info
  if (wantStems === id && info?.complete) {
    wantStems = null
    study.value.mixer.on = true
    scheduleSave()
  }
  await sendMix()
}
// al acabar cada pasada de la canción que se estudia, sus pistas: las
// rápidas en cuanto están, y luego las buenas (con los mismos nombres: el
// reproductor ve que cambiaron y las reabre donde iba)
const stopListening = separation.onSeparated((ids) => {
  const id = loadedFor.value
  if (id != null && ids.includes(id)) loadStems(id)
})
onUnmounted(stopListening)

/** Manda a Rust lo que tiene que sonar: las pistas con su mezcla, o la canción. */
async function sendMix() {
  if (mixerOn.value) {
    await player.setStems(
      plan.value.map((t) => ({ path: t.path, gain: t.gain, pan: t.pan, on: t.on }))
    )
  } else if (player.stems.value) {
    await player.setStems(null)
  }
}
async function toggleStems() {
  if (!hasStems.value) return
  study.value.mixer.on = !study.value.mixer.on
  scheduleSave()
  await sendMix()
}
/** Un carril cambió: callar, dejar sola, volumen o panorama. */
async function onLane(key, patch) {
  const all = study.value.mixer.tracks
  all[key] = { ...(all[key] || {}), ...patch }
  study.value.mixer.tracks = { ...all }
  scheduleSave()
  await sendMix()
}
/** Que vuelvan a sonar todas, como vienen. */
async function resetMix() {
  study.value.mixer.tracks = {}
  scheduleSave()
  await sendMix()
}
/** Separa la canción que se estudia, con la mejor calidad. */
async function separateThis() {
  const t = track.value
  if (!t) return
  if (await separation.request([{ id: t.id, title: t.title, file: songFile.value }])) {
    wantStems = t.id
  }
}
const progressText = computed(() => {
  const p = progress.value
  if (!p) return ''
  const verb = p.stage === 'refine' ? 'mejorando' : 'separando'
  if (p.waiting) {
    return p.stage === 'refine'
      ? `en la cola para mejorar (${p.place}.º)`
      : `en la cola (${p.place}.º)`
  }
  if (p.step === 'download') return `bajando el separador · ${Math.round(p.fraction * 100)} %`
  if (!p.fraction) return p.stage === 'refine' ? 'mejorando…' : 'preparando…'
  const left = separation.remaining()
  const eta = left == null ? '' : ` · quedan ~${formatTime(left)}`
  return `${verb} · ${Math.round(p.fraction * 100)} %${eta}`
})

/** Guarda en un archivo lo que suena ahora de las pistas. */
async function saveMix({ asHeard = false } = {}) {
  const info = stems.value
  const id = loadedFor.value
  if (!info || !id) return
  const on = plan.value.filter((t) => t.on)
  if (!on.length) return notify('Todas las pistas están calladas: no hay nada que guardar')
  const name = mixFileName(songFile.value || track.value?.title || 'Cancion', plan.value)
  const path = await pickSavePath({
    title: 'Guardar la mezcla',
    defaultPath: `${info.folder}/${name}.mp3`,
    filters: [
      { name: 'MP3', extensions: ['mp3'] },
      { name: 'FLAC', extensions: ['flac'] },
      { name: 'WAV', extensions: ['wav'] }
    ]
  })
  if (!path) return
  notify('Guardando la mezcla…', 'info', 3)
  try {
    const r = await api.runJob(
      () =>
        api.exportMix(id, {
          tracks: on.map((t) => ({ source: t.key, gain: t.gain, pan: t.pan })),
          path,
          ...(asHeard ? { speed: speed.value, pitch: pitch.value } : {})
        }),
      JOBS.mix
    )
    notify(`Guardada «${r.name}»${r.id ? ': ya está en tu biblioteca' : ''}`, 'ok', 6)
  } catch (e) {
    notify('No se pudo guardar la mezcla: ' + errorMessage(e))
  }
}
async function revealStems() {
  const first = stems.value?.tracks?.[0]?.path
  if (!first) return
  try {
    await tauriApp.revealInFolder(first)
  } catch (e) {
    notify(errorMessage(e))
  }
}
async function deleteStems() {
  const id = loadedFor.value
  if (!id || !stems.value) return
  const ok = await ask({
    kind: 'confirm',
    title: 'Borrar las pistas separadas',
    danger: true,
    message: 'Las pistas van a la papelera del sistema. La canción no se toca.',
    detail: stems.value.folder,
    okLabel: 'A la papelera'
  })
  if (!ok) return
  try {
    if (player.stems.value) await player.setStems(null)
    await api.deleteStems(id)
    stems.value = null
    study.value.mixer = { on: false, tracks: {} }
    scheduleSave()
    notify('Pistas en la papelera', 'ok')
  } catch (e) {
    notify('No se pudieron borrar: ' + errorMessage(e))
  }
}
const heardDiffers = computed(() => speed.value !== 1 || pitch.value !== 0)
function stemsMenu(ev) {
  const items = [
    { label: 'Guardar esta mezcla…', icon: 'download', action: () => saveMix() },
    ...(heardDiffers.value
      ? [
          {
            label: 'Guardarla como suena…',
            icon: 'download',
            note: `${Math.round(speed.value * 100)} %${pitch.value ? ' · ' + pitchLabel.value : ''}`,
            action: () => saveMix({ asHeard: true })
          }
        ]
      : []),
    { label: 'Que suenen todas, como vienen', icon: 'refresh', action: resetMix },
    { label: 'Abrir la carpeta de las pistas', icon: 'folderOpen', action: revealStems },
    { separator: true },
    ...(progress.value
      ? []
      : [
          {
            label: stems.value?.best ? 'Separar otra vez' : 'Separar otra vez con la mejor calidad',
            icon: 'mixer',
            note: stems.value?.best
              ? ''
              : stems.value?.quality === 'rapida'
                ? 'solo se mejoran'
                : 'se hicieron con una versión anterior',
            action: separateThis
          }
        ]),
    { separator: true },
    { label: 'Borrar las pistas…', icon: 'trash', danger: true, action: deleteStems }
  ]
  openMenu(ev, items, 'Las pistas')
}

/** Al cerrar, la cancion vuelve a sonar normal. */
async function close() {
  await flushSave()
  if (player.stems.value) await player.setStems(null)
  await player.clearLoop()
  if (speed.value !== 1) await player.setSpeed(1)
  if (pitch.value !== 0) await player.setPitch(0)
  if (metronome.value.on) await player.setMetronome({ on: false })
  emit('close')
}
// Lo pendiente no se tira al desmontar: se guarda. (Cargar lo guardado ya lo
// hace el `watch` inmediato de arriba; aqui se volvia a pedir al montar y la
// ficha se leia dos veces.)
onUnmounted(flushSave)
</script>

<template>
  <div
    class="study"
    :class="{ 'with-lanes': lanes.length }"
    role="region"
    aria-label="Modo estudio"
  >
    <div class="study-head">
      <Icon n="academic" :t="15" /> <strong>Modo estudio</strong>
      <span v-if="track" class="study-song"
        >{{ track.artist }} — {{ track.title }}
        <span v-if="track.key" class="mono study-key">· {{ track.key }}</span></span
      >
      <span v-else class="study-song">Pon una canción para estudiarla</span>
      <span v-if="saving" class="study-saving">guardando…</span>
      <button
        class="btn mini"
        type="button"
        title="Cerrar el modo estudio (vuelve a sonar normal)"
        @click="close"
      >
        <Icon n="close" :t="13" /> Cerrar
      </button>
    </div>

    <!-- las pistas separadas: separarla, o que suenen ellas con su mezcla -->
    <div v-if="track && (hasStems || canSeparate || progress)" class="study-stems">
      <template v-if="hasStems">
        <button
          type="button"
          class="btn mini study-stems-toggle"
          :class="{ on: mixerOn }"
          :aria-pressed="mixerOn"
          :title="
            mixerOn
              ? 'Suenan las pistas separadas: pulsa para volver a la canción tal cual'
              : 'Que suenen las pistas separadas, cada una con su volumen'
          "
          @click="toggleStems"
        >
          <Icon n="mixer" :t="12" /> Pistas
        </button>
        <span class="study-stems-note">
          <template v-if="mixerOn">
            {{ plan.filter((t) => t.on).length }} de {{ plan.length }} suenan · M calla, S deja sola
          </template>
          <template v-else>{{ plan.length }} pistas separadas</template>
        </span>
        <span v-if="progress" class="study-stems-progress mono">{{ progressText }}</span>
        <button
          type="button"
          class="btn mini study-stems-more"
          title="Guardar la mezcla, abrir la carpeta, separar otra vez…"
          aria-label="Opciones de las pistas"
          @click="stemsMenu"
        >
          ⋯
        </button>
      </template>
      <template v-else-if="progress">
        <Icon n="mixer" :t="12" />
        <span class="study-stems-progress mono">{{ progressText }}</span>
        <span
          class="study-stems-bar"
          role="progressbar"
          :aria-valuenow="Math.round(progress.fraction * 100)"
          aria-valuemin="0"
          aria-valuemax="100"
          ><span :style="{ width: progress.fraction * 100 + '%' }"></span
        ></span>
        <button
          type="button"
          class="btn mini"
          :title="progress.waiting ? 'Quitarla de la cola' : 'Parar la separación'"
          @click="progress.waiting ? separation.unqueue(track.id) : separation.cancel()"
        >
          <Icon n="close" :t="11" /> {{ progress.waiting ? 'Quitar' : 'Parar' }}
        </button>
      </template>
      <template v-else>
        <button
          type="button"
          class="btn mini study-stems-separate"
          title="Separar la canción en pistas (batería, voces, bajo…) para callar o dejar sola cada una"
          @click="separateThis"
        >
          <Icon n="mixer" :t="12" /> Separar pistas
        </button>
        <span class="study-stems-note">
          batería, voces, bajo… cada una aparte · en unos minutos, y luego se mejoran
        </span>
      </template>
    </div>

    <!-- la onda, la regla, los tramos, los marcadores y la rejilla del compas -->
    <StudyTimeline
      :song-id="track?.id ?? null"
      :duration="duration"
      :position="position"
      :loops="study.loops"
      :multi="multi"
      :markers="study.markers"
      :selected="selected"
      :grid="shownGrid"
      :height="80"
      :locked="locked"
      :playing="playing"
      :lanes="lanes"
      @lane="onLane"
      @update:locked="(v) => (locked = v)"
      @update:multi="(v) => (multi = v)"
      @update:loops="onLoops"
      @seek="jump"
      @marker="(m) => selectMarker(m, { jump: !guarded })"
      @options="showOptions"
    />
    <p class="study-hint">
      <template v-if="multi">
        Arrastra para añadir tramos: se repiten seguidos, saltándose lo de en medio · clic sobre uno
        para quitarlo · <b>⋯</b> o clic derecho: opciones
      </template>
      <template v-else-if="locked">
        Arrastra sobre la onda para elegir el tramo · con el candado, mientras suena, ni un clic ni
        el tramo mueven la canción · clic para quitarlo · <b>⋯</b> o clic derecho: opciones
      </template>
      <template v-else>
        Arrastra sobre la onda para elegir el tramo que se repite · clic para ir a un punto (y
        quitar el tramo) · <b>⋯</b> o clic derecho: opciones · teclas <b>A</b> y <b>B</b>
      </template>
    </p>

    <div class="study-body">
      <!-- ================================================== reproduccion -->
      <section class="study-col">
        <h5 class="study-col-title"><Icon n="play" :t="12" /> Reproducción</h5>

        <div class="study-block">
          <div class="study-block-head">
            <span class="study-block-name"
              ><Icon n="repeat" :t="12" />
              {{ study.loops.length > 1 ? 'Tramos que se repiten' : 'Tramo que se repite' }}</span
            >
            <span class="study-loop mono" :class="{ off: !hasLoop && pendingA == null }"
              >{{ loopLabel }}<em v-if="hasLoop && loopDefer"> · al acabar</em></span
            >
          </div>
          <div v-if="hasLoop" class="btn-row">
            <button
              class="btn mini"
              type="button"
              title="Ir ya al tramo (al primero, si hay varios) y ponerlo a sonar"
              @click="playNow(0)"
            >
              <Icon n="play" :t="11" /> Ahora
            </button>
            <button
              class="btn mini"
              type="button"
              :class="{ on: loopDefer }"
              :aria-pressed="loopDefer"
              :title="
                loopDefer
                  ? 'La canción sigue hasta el final y entonces se repite. Pulsa para repetir ya'
                  : 'Que la canción siga hasta el final y entonces se repita el tramo'
              "
              @click="deferLoops(!loopDefer)"
            >
              <Icon n="repeat" :t="11" /> Al acabar
            </button>
            <button
              v-if="!loopSaved"
              class="btn mini"
              type="button"
              title="Guardar como marcador, con nombre y notas (varios tramos, juntos en uno)"
              @click="saveMarker"
            >
              <Icon n="save" :t="12" /> Guardar como marcador
            </button>
            <button
              class="btn mini study-clear"
              type="button"
              title="Quitar el bucle"
              @click="clearLoop"
            >
              <Icon n="close" :t="12" /> Quitar
            </button>
          </div>
        </div>

        <div class="study-block">
          <div class="study-block-head">
            <span class="study-block-name"><Icon n="forward10" :t="12" /> Velocidad</span>
            <span class="mono study-value"
              >{{ speedPercent }} %
              <em v-if="speed !== 1 && !pitchPreserved" class="model-warn"
                >cambia el tono: falta ffmpeg</em
              >
              <em v-else-if="speed !== 1">mismo tono</em></span
            >
          </div>
          <div class="btn-row study-speeds">
            <button
              v-for="v in SPEEDS"
              :key="v"
              class="btn mini"
              type="button"
              :class="{ on: Math.abs(speed - v) < 0.005 }"
              :aria-pressed="Math.abs(speed - v) < 0.005"
              :disabled="!track"
              @click="setSpeed(v)"
            >
              {{ v }}×
            </button>
          </div>
          <SliderField
            :model-value="speed"
            :min="0.5"
            :max="1.25"
            :step="0.01"
            width="100%"
            @update:model-value="setSpeed"
          />
        </div>

        <div class="study-block">
          <div class="study-block-head">
            <span class="study-block-name"><Icon n="chords" :t="12" /> Tono</span>
            <span class="mono study-value">
              <template v-if="track?.key"
                >{{ track.key
                }}<template v-if="pitch">
                  → <b>{{ keyNow }}</b></template
                ></template
              >
              <template v-else-if="pitch">{{ pitchLabel }} {{ toneUnit(pitch) }}</template>
              <template v-else>como está grabada</template>
            </span>
          </div>
          <div class="btn-row study-pitch">
            <button
              class="btn mini study-pitch-down"
              type="button"
              :title="'Bajar ' + stepName"
              :disabled="!track || pitch <= -12"
              @click="setPitch(pitch - pitchStep)"
            >
              −
            </button>
            <span class="study-pitch-n mono" :class="{ on: pitch !== 0 }" :title="pitchTitle">{{
              pitchLabel
            }}</span>
            <button
              class="btn mini study-pitch-up"
              type="button"
              :title="'Subir ' + stepName"
              :disabled="!track || pitch >= 12"
              @click="setPitch(pitch + pitchStep)"
            >
              +
            </button>
            <span
              class="study-steps"
              role="group"
              aria-label="De cuánto en cuánto se corre el tono"
            >
              <button
                v-for="s in PITCH_STEPS"
                :key="s.v"
                type="button"
                class="btn mini study-step"
                :class="{ on: pitchStep === s.v }"
                :aria-pressed="pitchStep === s.v"
                :title="s.title"
                @click="pitchStep = s.v"
              >
                {{ s.n }}
              </button>
            </span>
            <button
              v-if="pitch !== 0"
              class="btn mini"
              type="button"
              title="Volver al tono original"
              @click="setPitch(0)"
            >
              original
            </button>
            <em v-if="!pitchPreserved && speed === 1" class="model-warn">hace falta ffmpeg</em>
          </div>
        </div>
      </section>

      <!-- ===================================================== metronomo -->
      <section class="study-col">
        <h5 class="study-col-title"><Icon n="note" :t="12" /> Metrónomo</h5>
        <div class="study-block study-metro" :class="{ on: metronome.on }">
          <p class="study-metro-status">{{ metroStatus }}</p>
          <div class="study-metro-row">
            <button
              class="btn mini study-metro-toggle"
              type="button"
              :class="{ on: metronome.on }"
              :disabled="!track"
              :title="
                metronome.on
                  ? 'Parar el metrónomo (la canción sigue)'
                  : 'Arrancar el metrónomo, al compás de la canción'
              "
              @click="toggleMetronome"
            >
              <Icon :n="metronome.on ? 'pause' : 'play'" :t="12" />
              {{ metronome.on ? 'Parar' : 'Clic' }}
            </button>
            <span class="study-bpm">
              <button
                class="btn mini study-bpm-step"
                type="button"
                title="Un pulso menos por minuto (el clic pasa a tempo a mano)"
                :disabled="!track"
                @click="bumpBpm(-1)"
              >
                −
              </button>
              <!-- la «x» (con tempo a mano) lo vacia: vuelve al de la cancion -->
              <TextField
                class="study-bpm-input mono"
                compact
                :clearable="metronome.free && metronome.has_grid"
                :model-value="bpmDraft ?? bpmShown"
                :disabled="!track"
                aria-label="Tempo del clic, en pulsos por minuto"
                title="Escribe el tempo (admite decimales: 90.7) · ↑ ↓ lo mueven de décima en décima, con Mayús de uno en uno"
                @update:model-value="onBpmInput"
                @focus="onBpmFocus"
                @keydown="onBpmKey"
                @blur="commitBpm"
              />
              <small>bpm</small>
              <button
                class="btn mini study-bpm-step"
                type="button"
                title="Un pulso más por minuto (el clic pasa a tempo a mano)"
                :disabled="!track"
                @click="bumpBpm(1)"
              >
                +
              </button>
            </span>
          </div>
          <div class="study-metro-row">
            <button
              class="btn mini study-mult"
              type="button"
              :class="{ on: metronome.mult === 1 }"
              :aria-pressed="metronome.mult === 1"
              :title="multTitle(1)"
              :disabled="!track"
              @click="setMult(1)"
            >
              ×2
            </button>
            <button
              class="btn mini study-mult"
              type="button"
              :class="{ on: metronome.mult === -1 }"
              :aria-pressed="metronome.mult === -1"
              :title="multTitle(-1)"
              :disabled="!track"
              @click="setMult(-1)"
            >
              ÷2
            </button>
            <button
              v-if="metronome.free && metronome.has_grid"
              class="btn mini"
              type="button"
              title="Volver al tempo de la canción y seguirla"
              @click="followSong"
            >
              <Icon n="refresh" :t="11" /> el de la canción
            </button>
          </div>
          <div class="study-metro-row study-meters" role="group" aria-label="Compás">
            <span class="study-metro-label">Compás</span>
            <button
              v-for="m in METERS"
              :key="m.v"
              class="btn mini"
              type="button"
              :class="{ on: metronome.meter === m.v }"
              :aria-pressed="metronome.meter === m.v"
              :title="m.title + (grid && grid.meter === m.v ? ' · el que detectó DanPlay' : '')"
              :disabled="!track"
              @click="setMeter(m.v)"
            >
              {{ m.n }}
            </button>
          </div>
          <div class="study-metro-row">
            <button
              class="btn mini"
              type="button"
              title="Si el acento no cae en el 1: el siguiente pulso pasa a ser el 1"
              :disabled="!metronome.has_grid || metronome.meter === 0"
              @click="shiftOne"
            >
              el 1 es el siguiente
            </button>
          </div>
          <div class="study-metro-row study-metro-vol">
            <span class="study-metro-label">Clic</span>
            <SliderField
              :model-value="metronome.volume"
              :min="0"
              :max="2"
              :step="0.05"
              :mark="1"
              width="100%"
              aria-label="Volumen del clic"
              :value-text="pctText(metronome.volume)"
              title="El volumen del clic: hasta el doble de fuerte"
              @update:model-value="setMetroVolume"
            />
            <span class="study-vol-n mono">{{ pctText(metronome.volume) }}</span>
          </div>
          <div class="study-metro-row study-song-vol">
            <span class="study-metro-label">Canción</span>
            <SliderField
              :model-value="volume"
              :min="0"
              :max="MAX_VOLUME"
              :step="0.01"
              :mark="1"
              width="100%"
              aria-label="Volumen de la canción"
              :value-text="pctText(volume)"
              title="El volumen de la canción: hasta un 50 % más de como viene"
              @update:model-value="setSongVolume"
            />
            <span class="study-vol-n mono">{{ pctText(volume) }}</span>
          </div>
        </div>
        <p class="study-empty">
          El pulso y el «1» los detecta DanPlay al abrir la canción; el clic entra en el compás y
          sigue la velocidad del estudio. Se puede parar y arrancar sin tocar la canción.
        </p>
      </section>

      <!-- ==================================================== marcadores -->
      <section class="study-col">
        <h5 class="study-col-title">
          <Icon n="list" :t="12" /> Marcadores
          <small v-if="study.markers.length">pulsa uno para repetir su tramo</small>
        </h5>
        <div class="study-markers">
          <div
            v-for="m in study.markers"
            :key="m.t + ':' + (m.end || 0)"
            class="study-marker"
            :class="{ on: m === selected, span: isSpan(m) }"
          >
            <button
              type="button"
              class="study-pick"
              :title="
                isSpan(m) ? 'Repetir este tramo (' + markerLabel(m) + ')' : 'Ir a ' + markerLabel(m)
              "
              @click="selectMarker(m)"
            >
              <Icon :n="isSpan(m) ? 'repeat' : 'right'" :t="11" class="study-pick-ico" />
              <span class="mono study-pick-time">{{ markerLabel(m) }}</span>
              <span class="study-pick-name">{{ m.label }}</span>
            </button>
            <button
              type="button"
              class="field-btn"
              :title="'Renombrar «' + m.label + '»'"
              @click="renameMarker(m)"
            >
              <Icon n="pencil" :t="11" />
            </button>
            <button type="button" class="field-btn" title="Quitar" @click="removeMarker(m)">
              <Icon n="close" :t="11" />
            </button>
          </div>
          <p v-if="!study.markers.length" class="study-empty">
            Elige un tramo en la onda y guárdalo: aquí quedan tus partes con nombre y notas.
          </p>
        </div>
        <div class="btn-row study-col-foot">
          <button
            v-if="hasLoop && !loopSaved"
            class="btn mini"
            type="button"
            title="Guardar el tramo que se repite como marcador"
            @click="saveMarker"
          >
            <Icon n="save" :t="12" /> Guardar tramo
          </button>
          <button
            v-else-if="!hasLoop"
            class="btn mini"
            type="button"
            :disabled="!track"
            title="Marcar este instante (sin tramo elegido)"
            @click="saveMarker"
          >
            <Icon n="plus" :t="12" /> aquí
          </button>
        </div>
      </section>

      <!-- ========================================================= notas -->
      <section class="study-col study-notes">
        <h5 class="study-col-title"><Icon n="pencil" :t="12" /> Notas</h5>
        <div class="study-tabs" role="tablist">
          <button
            type="button"
            role="tab"
            class="study-tab"
            :class="{ on: notesTab === 'song' }"
            :aria-selected="notesTab === 'song'"
            @click="notesTab = 'song'"
          >
            La canción
          </button>
          <button
            v-if="selected"
            type="button"
            role="tab"
            class="study-tab"
            :class="{ on: notesTab === 'marker' }"
            :aria-selected="notesTab === 'marker'"
            @click="notesTab = 'marker'"
          >
            «{{ selected.label }}»
          </button>
          <span v-else class="study-tab off" title="Elige un marcador para escribirle notas"
            >un marcador</span
          >
        </div>
        <!-- primero se apunta lo escrito y luego se programa el guardado, que
             copia la ficha en ese momento: con v-model y @update a la vez el
             orden dependia de como los juntara el compilador -->
        <TextField
          v-if="notesTab === 'marker' && selected"
          :key="'m' + selected.t"
          :model-value="selected.notes"
          multiline
          :rows="6"
          width="100%"
          :placeholder="'qué trabajar en «' + selected.label + '»…'"
          class="study-notes-field"
          @update:model-value="
            (v) => {
              selected.notes = v
              scheduleSave()
            }
          "
        />
        <TextField
          v-else
          :model-value="study.notes"
          multiline
          :rows="6"
          width="100%"
          placeholder="cejilla en 2, entrar tras el redoble…"
          :disabled="!track"
          class="study-notes-field"
          @update:model-value="
            (v) => {
              study.notes = v
              scheduleSave()
            }
          "
        />
      </section>
    </div>
  </div>
</template>
