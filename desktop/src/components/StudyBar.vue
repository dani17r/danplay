<script setup>
/**
 * El modo estudio: para machacar un trozo de la canción que suena.
 *
 * Es una capa que sube desde el reproductor y ocupa media pantalla, sin
 * recolocar la lista de detrás. De arriba abajo:
 *
 * - la cabecera: la canción, «Salidas» (por dónde suena cada cosa), «Grande»,
 *   «Ocultar» (se esconde y todo sigue sonando igual) y, en ⋯, «Salir del
 *   modo estudio» (lo único que devuelve la canción a como viene);
 * - las pistas separadas (separarla, o que suenen ellas con su mezcla);
 * - la barra de los tramos, junto a la onda que los elige: el tramo (o los
 *   tramos), qué pasa al acabar (repetir, parar o seguir), sonar ya,
 *   guardarlo, y a la derecha el zoom de la onda;
 * - la onda con la regla, los tramos, los marcadores, las guías habladas y
 *   la rejilla del clic (con la rueda del ratón se acerca y se aleja);
 * - cuatro columnas: velocidad y tono · el clic · marcadores y guías · notas.
 *
 * Todo se guarda con la canción —en el índice y en una etiqueta del
 * archivo— y se va con ella si se borra. Al abrir se aplica lo guardado; al
 * ocultar no se toca nada; al salir del estudio se quita y la canción vuelve
 * a sonar normal.
 */
import { ref, computed, watch, onMounted, onUnmounted, shallowRef } from 'vue'
import { api, app as tauriApp, errorMessage, pickSavePath, JOBS } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { ask } from '../composables/useDialog.js'
import { usePlayback, MAX_VOLUME } from '../composables/usePlayback.js'
import { useSeparation } from '../composables/useSeparation.js'
import { useClick } from '../composables/useClick.js'
import { useRouting } from '../composables/useRouting.js'
import { takeToPhone } from '../composables/usePhone.js'
import { isMobileApp } from '../platform.js'
import { stemPlan, mixFileName } from '../utils/stems.js'
import { useHotkeys } from '../composables/useHotkeys.js'
import { formatTime } from '../utils/format.js'
import { transposeKey, toneWords, semitoneWords } from '../utils/theory.js'
import {
  effectiveGrid,
  formatBpm,
  parseBpm,
  multFactor,
  clickTimes,
  gridToOwn,
  METERS
} from '../utils/beats.js'
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
import StudyOutputs from './StudyOutputs.vue'
import StudyCue from './StudyCue.vue'

const props = defineProps({
  /** Se ve (la capa puede estar oculta y el estudio seguir puesto). */
  visible: { type: Boolean, default: true }
})
// `hide`: se esconde y todo sigue igual; `exit`: se sale del estudio (lo que
// sonaba distinto vuelve a como viene). `close` es `hide`, por las que lo
// cerraban antes (el celular)
const emit = defineEmits(['hide', 'exit', 'close'])
const player = usePlayback()
const { track, position, duration, speed, pitch, metronome, pitchPreserved, playing, volume } =
  player

const SPEEDS = [0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.25]
// `loops`: los tramos que se repiten, [[a, b], …] (uno solo es el bucle A-B);
// `mixer`: las pistas separadas, si suenan (`on`) y cómo va cada una;
// `cues`: las guías habladas, [{t, text?, voice?, file, gain?}]
const blank = () => ({
  loops: [],
  speed: 1,
  pitch: 0,
  metronome: {},
  markers: [],
  cues: [],
  notes: '',
  mixer: { on: false, tracks: {} }
})
const study = ref(blank())
// el clic: su rejilla, si carga, y el modo. Vive fuera del panel: con él
// cerrado, al pasar de canción, sigue igual
const click = useClick()
onUnmounted(click.watchGrid())
// para la plantilla (ahí solo se desenvuelven los refs de primer nivel)
const clickAuto = click.auto
const clickMode = click.mode
const clickWanted = click.wanted
// por dónde suena cada cosa
const routing = useRouting()
const outputsOpen = ref(false)
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
// al volver a automático, los de esta canción otra vez (a mano iba otro tempo)
watch(clickAuto, (on) => {
  if (on && loadedFor.value != null)
    click.songSettings(loadedFor.value, study.value.metronome, true)
})
const saving = ref(false)
/** El marcador elegido (uno de `study.markers`), o null. */
const selected = ref(null)
/** Los marcadores con la casilla puesta: para tocar varios seguidos. */
const checked = ref(/** @type {Set<any>} */ (new Set()))
/** La rejilla de pulsos de la cancion que suena, tal como la dio el analisis, o null. */
const grid = click.grid
/** La pestaña de notas: las de la cancion o las del marcador elegido. */
const notesTab = ref('song')
/** La línea de tiempo (su zoom y la edición de pulsos). */
const timeline = ref(null)
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

/** Lo que esta barra recuerda en el navegador (el candado, el paso del tono…). */
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
  checked.value = new Set()
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
      cues: (Array.isArray(s.cues) ? s.cues : []).map((c) => ({ ...c })),
      notes: s.notes || '',
      mixer: {
        on: !!s.mixer?.on,
        tracks: Object.fromEntries(
          Object.entries(s.mixer?.tracks || {}).map(([k, v]) => [k, { ...v }])
        )
      }
    }
    multi.value = loops.length > 1
    lastTouched.value = loops.length - 1
    // lo guardado se aplica al entrar: para eso se guardo
    await player.setLoops(loops, { end: loopEndWanted.value })
    if (study.value.speed !== speed.value) await player.setSpeed(study.value.speed)
    if (study.value.pitch !== pitch.value) await player.setPitch(study.value.pitch)
    // los ajustes del clic de esta canción (su rejilla la pide `useClick`)
    await click.songSettings(id, study.value.metronome)
    selected.value = markerOf(study.value.loops)
    await sendCues()
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
const METRO_DEFAULTS = { bpm: null, meter: null, shift: 0, mult: 0, own: null }
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
  const cues = (s.cues || []).map((c) => ({
    t: round2(c.t),
    file: c.file,
    ...(c.text ? { text: c.text } : {}),
    ...(c.voice ? { voice: c.voice } : {}),
    ...(c.gain != null && c.gain !== 1 ? { gain: c.gain } : {})
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
// (Rust lo espera). En pausa, la onda coloca como siempre.
const LOCK_KEY = 'danplay.study.lock'
const locked = ref(readPref(LOCK_KEY, true) !== false)
watch(locked, (v) => savePref(LOCK_KEY, v))
/** La onda no puede mover la cancion ahora. */
const guarded = computed(() => locked.value && playing.value)

// ---- los tramos: uno (el bucle A-B) o varios seguidos
/**
 * Qué pasa al acabar el último tramo: 'repeat' (vuelta al primero),
 * 'stop' (se para al principio del primero: play lo vuelve a tocar) o
 * 'continue' (suenan una vez y la canción sigue). Es de quien toca: se
 * recuerda. Antes se repetían siempre, y el botón «Al acabar» quería decir
 * otra cosa (esperar al final de la canción): parecía no funcionar.
 */
const END_KEY = 'danplay.study.alAcabar'
const LOOP_ENDS = [
  {
    v: 'repeat',
    n: 'Repetir',
    icon: 'repeat',
    title: 'Al acabar el último tramo, vuelve al primero: en bucle hasta que lo quites'
  },
  {
    v: 'stop',
    n: 'Parar',
    icon: 'stop',
    title: 'Suena cada tramo una vez y se para al principio del primero (play, otra vez)'
  },
  {
    v: 'continue',
    n: 'Seguir',
    icon: 'right',
    title: 'Suena cada tramo una vez y la canción sigue desde ahí'
  }
]
const savedEnd = readPref(END_KEY, 'stop')
const loopEndWanted = ref(LOOP_ENDS.some((e) => e.v === savedEnd) ? savedEnd : 'stop')
watch(loopEndWanted, (v) => savePref(END_KEY, v))
async function setLoopEnd(v) {
  loopEndWanted.value = v
  await player.setLoops(study.value.loops, { end: v })
}

/** Varios tramos a la vez: cada arrastre en la onda añade otro. */
const multi = ref(false)
/** El último tramo que se tocó (al dejar «varios», se queda ese). */
const lastTouched = ref(-1)
const pendingA = ref(null)
const hasLoop = computed(() => study.value.loops.length > 0)
const loopLabel = computed(() => {
  const list = study.value.loops
  if (list.length === 1) return `${formatTime(list[0][0])} – ${formatTime(list[0][1])}`
  if (list.length > 1) return `${list.length} tramos · ${formatTime(segmentsLength(list))}`
  if (pendingA.value != null) return `A en ${formatTime(pendingA.value)} · pulsa B donde acabe`
  return 'sin tramo: arrastra sobre la onda'
})
// al dejar «varios» se queda un solo tramo: el último que se tocó
watch(multi, (on) => {
  const list = study.value.loops
  if (on || list.length <= 1) return
  const keep = list[clamp(lastTouched.value, 0, list.length - 1)] || list.at(-1)
  lastTouched.value = 0
  applyLoops([keep], { jump: 'never' })
})

/**
 * Deja los tramos. `jump` dice si la cancion va a ellos: 'outside' si va
 * por fuera de todos (lo de siempre: al siguiente que venga), 'never' para
 * dejarla donde va (la onda con candado), o un segundo concreto (la tecla
 * B vuelve a su A).
 */
async function applyLoops(list, { jump = 'outside' } = {}) {
  const clean = cleanSegments(list)
  if (clean.some(([a, b]) => b - a < 0.5)) {
    notify('El bucle tiene que durar al menos medio segundo')
    return
  }
  pendingA.value = null
  study.value.loops = clean
  await player.setLoops(clean, { end: loopEndWanted.value })
  if (typeof jump === 'number') await player.seek(jump)
  else if (jump === 'outside' && clean.length && segmentAt(clean, position.value) < 0) {
    await player.seek(clean[nextSegment(clean, position.value)][0])
  }
  scheduleSave()
}
async function clearLoop() {
  pendingA.value = null
  study.value.loops = []
  lastTouched.value = -1
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
/** El índice en `after` del tramo que no estaba en `before` (el que se añadió). */
function addedIndex(before, after) {
  const i = after.findIndex(
    ([a, b]) => !before.some(([x, y]) => Math.abs(x - a) < 0.011 && Math.abs(y - b) < 0.011)
  )
  return i < 0 ? after.length - 1 : i
}
/**
 * Lo que manda la linea de tiempo. Un tramo nuevo dibujado de cero (o uno
 * que se añade o se quita) deja de apuntar al marcador que hubiera elegido;
 * mover un borde con un marcador elegido cambia el tramo DEL marcador, que es
 * lo que uno espera al estirarlo.
 */
function onLoops(list, { mode, index } = {}) {
  if (!list.length) return clearLoop()
  const m = selected.value
  const clean = cleanSegments(list)
  if (mode === 'edit' && m && partsOf(m).length) setMarkerParts(m, clean)
  else if (mode === 'select' || mode === 'add' || mode === 'remove') selected.value = null
  if (mode === 'add') lastTouched.value = addedIndex(study.value.loops, clean)
  else if (mode === 'edit' && Number.isInteger(index)) lastTouched.value = index
  else if (mode === 'select') lastTouched.value = 0
  else lastTouched.value = Math.min(lastTouched.value, clean.length - 1)
  return applyLoops(list, { jump: guarded.value || mode === 'remove' ? 'never' : 'outside' })
}
function markA() {
  if (!track.value || !props.visible) return
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
  if (!track.value || !props.visible) return
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
  const next = multi.value ? [...list, [from, at]] : [[from, at]]
  if (multi.value) lastTouched.value = addedIndex(list, cleanSegments(next))
  return applyLoops(next, { jump: from })
}

// ---- las opciones de un tramo (el boton ⋯ de la onda, o el clic derecho)
/** Ir ya a ese tramo y que suene. */
async function playNow(index = 0) {
  const seg = study.value.loops[index]
  if (!seg) return
  await player.seek(seg[0])
  if (!playing.value) await player.toggle()
}
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
    for (const e of LOOP_ENDS) {
      items.push({
        label: `Al acabar: ${e.n.toLowerCase()}`,
        icon: e.icon,
        note: loopEndWanted.value === e.v ? 'puesto' : '',
        action: () => setLoopEnd(e.v)
      })
    }
  }
  items.push({ label: 'Ir aquí', icon: 'right', note: formatTime(t), action: () => player.seek(t) })
  if (!isMobileApp) {
    items.push({
      label: 'Guía hablada aquí…',
      icon: 'mic',
      note: formatTime(t),
      action: () => addCue(t)
    })
  }
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
  { v: 0.5, n: '¼ de tono', name: 'un cuarto de tono', title: 'De cuarto en cuarto de tono' },
  { v: 1, n: '½ tono', name: 'medio tono', title: 'De medio en medio tono (un semitono)' },
  { v: 2, n: '1 tono', name: 'un tono', title: 'De tono en tono (dos semitonos)' }
]
const PITCH_STEP_KEY = 'danplay.study.pitchStep'
// «Grande»: todo el alto que deja el reproductor. Se recuerda: si se cierra
// grande, se vuelve a abrir grande
const BIG_KEY = 'danplay.study.grande'
const big = ref(readPref(BIG_KEY, false) === true)
watch(big, (v) => savePref(BIG_KEY, v))
const savedStep = readPref(PITCH_STEP_KEY, 1)
/** De cuánto en cuánto se mueve el tono, en semitonos. */
const pitchStep = ref(PITCH_STEPS.some((s) => s.v === savedStep) ? savedStep : 1)
watch(pitchStep, (v) => savePref(PITCH_STEP_KEY, v))
const stepName = computed(() => PITCH_STEPS.find((s) => s.v === pitchStep.value)?.name || '')
/** «medio tono arriba», «1 tono abajo», «original». */
const pitchWords = computed(() => toneWords(pitch.value))
const pitchTitle = computed(() =>
  pitch.value ? `${toneWords(pitch.value)} (${semitoneWords(pitch.value)})` : 'Como está grabada'
)
const keyNow = computed(() => (track.value?.key ? transposeKey(track.value.key, pitch.value) : ''))
async function setPitch(n) {
  const p = round2(clamp(Number(n) || 0, -12, 12))
  study.value.pitch = p
  await player.setPitch(p)
  scheduleSave()
}

// ---- metronomo
// El pulso y el compas los da `useClick`: con el automático, los de cada
// cancion (Beat This! en el nucleo, o el analisis de Rust si aqui no se
// puede); con el propio, el ajustado a mano y guardado con ella; a mano, un
// tempo para todas. Lo que uno ajuste a una cancion (tempo, compas, «1»,
// doble/mitad, sus pulsos) se guarda con ella en `study.metronome`; lo de a
// mano, en el navegador (es de quien toca).
const CLICK_MODES = [
  {
    v: 'auto',
    n: 'Automático',
    title: 'Cada canción con su clic, detectado solo (el pulso y el «1»)'
  },
  {
    v: 'own',
    n: 'Propio',
    title:
      'El clic de cada canción ajustado por ti: mueve los pulsos sobre la onda y se guarda con ella'
  },
  {
    v: 'manual',
    n: 'A mano',
    title: 'Un tempo que pones tú, el mismo en todas las canciones'
  }
]
/**
 * La rejilla tal como suena, para la onda: con el doble o la mitad, el
 * compas y el «1» corrido.
 */
const shownGrid = computed(() => {
  // a mano el clic no sigue la cancion: su rejilla no pinta nada
  if (!click.auto.value) return null
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
/** Lo que lleva la rejilla que carga, en %. */
const clickPercent = computed(() => Math.round(click.progress.value * 100))
/** Cargando el clic de esta canción (en automático): no se puede poner aún. */
const clickLoading = computed(() => click.auto.value && click.loading.value)
/** Esta canción tiene su clic propio guardado. */
const hasOwn = computed(() => !!study.value.metronome?.own?.beats?.length)
const metroStatus = computed(() => {
  const m = metronome.value
  if (player.state.counting_in) return 'contando…'
  if (!click.auto.value) return 'a mano: el mismo tempo en todas las canciones'
  if (clickLoading.value) return `cargando el clic de esta canción… ${clickPercent.value} %`
  if (click.error.value) return 'sin compás detectado: va libre'
  if (!m.has_grid) return 'sin compás: va libre'
  if (m.free) return 'tempo a mano: entra con el pulso de la canción y va a su aire'
  if (clickMode.value === 'own') {
    return hasOwn.value
      ? 'tu clic de esta canción: los pulsos que ajustaste'
      : 'aún sin clic propio: suena el automático; ajusta los pulsos para hacer el tuyo'
  }
  const sure = m.confidence >= 0.6 ? 'seguro' : m.confidence >= 0.3 ? 'probable' : 'dudoso'
  return `sigue la canción · el «1» ${sure}`
})
function metroPatch(patch) {
  // a mano, el tempo y el compas son de todas las canciones, no de esta
  if (!click.auto.value) return click.setManual(patch)
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
  await click.toggle()
}
/**
 * Pone a mano el tempo que se oye, con decimales. Con el doble o la mitad
 * puestos se guarda el tempo sin ellos: al quitarlos, vuelve al que era.
 */
function setBpm(heard) {
  // a mano no hay doble ni mitad: el tempo es el que se oye
  if (!click.auto.value) return click.setManual({ bpm: round2(clamp(heard, 20, 300)) })
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
const setMeter = (v) =>
  clickAuto.value
    ? metroPatch({ meter: grid.value?.meter === v ? null : v, shift: 0 })
    : click.setManual({ meter: v })
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

// ---- el clic propio: los pulsos ajustados a mano sobre la onda
/** Editando los pulsos en la onda. */
const editingBeats = ref(false)
watch(clickMode, (m) => {
  if (m !== 'own') editingBeats.value = false
})
/**
 * Los pulsos que se ajustaron en la onda: pasan a ser el clic propio de la
 * canción (se guarda con ella) y suenan ya.
 * @param {import('../api.js').BeatGrid} edited
 */
async function onOwnGrid(edited) {
  const id = loadedFor.value
  if (!id) return
  const own = gridToOwn(edited)
  study.value.metronome = { ...(study.value.metronome || {}), own, shift: 0, mult: 0 }
  delete study.value.metronome.shift
  delete study.value.metronome.mult
  scheduleSave()
  await flushSave()
  await click.applyOwn(id, own)
}
/** Empezar el propio desde el automático, o quitarlo (vuelve el automático). */
async function resetOwn() {
  const id = loadedFor.value
  if (!id || !hasOwn.value) return
  const ok = await ask({
    kind: 'confirm',
    title: 'Quitar tu clic de esta canción',
    message: 'Se olvidan los pulsos que ajustaste a mano y vuelve el automático.',
    okLabel: 'Quitarlo',
    danger: true
  })
  if (!ok) return
  const m = { ...(study.value.metronome || {}) }
  delete m.own
  study.value.metronome = m
  scheduleSave()
  await flushSave()
  await click.applyOwn(id, null)
}
async function startEditing() {
  if (clickMode.value !== 'own') await click.setMode('own')
  editingBeats.value = !editingBeats.value
  if (editingBeats.value) {
    timeline.value?.zoomToBeats?.()
    notify(
      'Acerca la onda con la rueda del ratón: arrastra un pulso para moverlo, doble clic para añadir uno, clic derecho para quitarlo o hacerlo el «1»',
      'info',
      8
    )
  }
}

// La cuenta y el sonido del clic son de quien toca, no de cada cancion: se
// recuerdan en el navegador.
const COUNT_INS = [
  { v: 0, n: 'no', title: 'Al darle a play, la canción entra sin cuenta' },
  { v: 1, n: '1 compás', title: 'Al darle a play en pausa, un compás de clic antes de que entre' },
  {
    v: 2,
    n: '2 compases',
    title: 'Al darle a play en pausa, dos compases de clic antes de que entre'
  }
]
const CLICK_SOUNDS = [
  { v: 'clasico', n: 'clásico', title: 'Un pitido corto' },
  { v: 'madera', n: 'madera', title: 'Un bloque de madera: seco' },
  {
    v: 'baqueta',
    n: 'baqueta',
    title: 'Baqueta contra el aro: corto y brillante, se oye entre platos'
  },
  { v: 'cencerro', n: 'cencerro', title: 'Cencerro: se oye aunque la banda suene fuerte' },
  { v: 'claves', n: 'claves', title: 'Claves: agudo y con cuerpo, atraviesa cualquier mezcla' },
  { v: 'hihat', n: 'charles', title: 'Un charles cerrado: como el que marca un baterista' }
]
const PREF_COUNT = 'danplay.clic.cuenta'
const PREF_SOUND = 'danplay.clic.sonido'
function setCountIn(v) {
  savePref(PREF_COUNT, v)
  return player.setMetronome({ count_in: v })
}
function setClickSound(v) {
  savePref(PREF_SOUND, v)
  return player.setMetronome({ sound: v })
}
// al abrir el estudio, lo que quedo elegido
{
  const count = Math.min(2, Math.max(0, Math.round(Number(readPref(PREF_COUNT, 0)) || 0)))
  const sound = readPref(PREF_SOUND, 'clasico')
  player.setMetronome({
    count_in: count,
    sound: CLICK_SOUNDS.some((c) => c.v === sound) ? sound : 'clasico'
  })
}
/** Más ajustes del clic (compás, cuenta, sonido): plegados, que son muchos. */
const CLICK_MORE_KEY = 'danplay.study.clicMas'
const clickMore = ref(readPref(CLICK_MORE_KEY, false) === true)
watch(clickMore, (v) => savePref(CLICK_MORE_KEY, v))
const setSongVolume = (v) => player.setVolume(v)
const pctText = (v) => Math.round((Number(v) || 0) * 100) + ' %'

/** Volver a analizar el clic: de toda la canción o del tramo elegido. */
async function reanalyze(ev) {
  const id = loadedFor.value
  if (!id) return
  const seg = study.value.loops.length
    ? [study.value.loops[0][0], study.value.loops.at(-1)[1]]
    : null
  const run = async (mode, range) => {
    notify('Analizando el clic otra vez…', 'info', 4)
    try {
      const r = await api.reanalyzeBeats(id, {
        mode,
        ...(range ? { start: range[0], end: range[1] } : {})
      })
      await click.reload()
      const fixed = r?.report?.fixed || 0
      notify(
        fixed
          ? `Clic arreglado: ${fixed} ${fixed === 1 ? 'tropiezo' : 'tropiezos'} corregidos`
          : 'Clic analizado otra vez',
        'ok',
        5
      )
    } catch (e) {
      notify('No se pudo analizar: ' + errorMessage(e))
    }
  }
  const items = [
    {
      label: 'Arreglar huecos y saltos',
      icon: 'sparkles',
      note: 'toda la canción',
      action: () => run('repair', null)
    },
    {
      label: 'Tempo fijo',
      icon: 'note',
      note: 'para las grabadas a clic',
      action: () => run('steady', null)
    },
    {
      label: 'Con la batería separada',
      icon: 'mixer',
      note: hasStems.value ? 'la más precisa' : 'sepárala antes',
      disabled: !hasStems.value,
      action: () => run('drums', null)
    },
    {
      label: 'Rellenar silencios',
      icon: 'plus',
      note: 'que no se calle en las partes suaves',
      action: () => run('fill', null)
    }
  ]
  if (seg) {
    items.push({ separator: true })
    items.push({
      label: 'Solo el tramo elegido',
      icon: 'repeat',
      note: `${formatTime(seg[0])} – ${formatTime(seg[1])}`,
      action: () => run('repair', seg)
    })
    items.push({
      label: 'El tramo, a tempo fijo',
      icon: 'note',
      note: `${formatTime(seg[0])} – ${formatTime(seg[1])}`,
      action: () => run('steady', seg)
    })
  }
  openMenu(ev, items, 'Volver a analizar el clic')
}

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
    lastTouched.value = study.value.loops.length - 1
    await player.setLoops(study.value.loops, { end: loopEndWanted.value })
    scheduleSave()
  }
  if (jump) await player.seek(m.t)
}
/** Un marcador, a sonar ya (desde su principio). */
async function playMarker(m) {
  await selectMarker(m)
  if (!playing.value) await player.toggle()
}
function toggleChecked(m) {
  const next = new Set(checked.value)
  if (next.has(m)) next.delete(m)
  else next.add(m)
  checked.value = next
}
const checkedList = computed(() =>
  study.value.markers.filter((m) => checked.value.has(m) && partsOf(m).length)
)
/**
 * Los marcadores con la casilla puesta, seguidos (saltándose lo de entre
 * medias), desde el primero: una vez o en bucle, según «Al acabar».
 */
async function playChecked() {
  const list = checkedList.value
  if (!list.length) return
  const parts = cleanSegments(list.flatMap((m) => partsOf(m)))
  selected.value = list.length === 1 ? list[0] : null
  pendingA.value = null
  multi.value = parts.length > 1
  study.value.loops = parts
  lastTouched.value = parts.length - 1
  await player.setLoops(parts, { end: loopEndWanted.value })
  scheduleSave()
  await player.seek(parts[0][0])
  if (!playing.value) await player.toggle()
}
function removeMarker(m) {
  if (selected.value === m) {
    selected.value = null
    notesTab.value = 'song'
  }
  if (checked.value.has(m)) toggleChecked(m)
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

// ---- las guías habladas: una frase que suena en un punto de la canción
// (escrita, y dicha con una voz, o grabada con el micrófono). Las hace el
// núcleo (`/api/cues`) y suenan en el motor en su sitio, también a otra
// velocidad y en la vuelta de un tramo, por la salida de las guías.
const CUE_VOLUME_KEY = 'danplay.guias.volumen'
const cueVolume = ref(clamp(Number(readPref(CUE_VOLUME_KEY, 1)) || 1, 0, 3))
watch(cueVolume, (v) => {
  savePref(CUE_VOLUME_KEY, v)
  sendCues()
})
function sendCues() {
  const path = player.state.path
  if (!path || !loadedFor.value) return Promise.resolve()
  const cues = (study.value.cues || [])
    .filter((c) => c.file)
    .map((c) => ({ at: c.t, path: c.file, gain: c.gain ?? 1 }))
  return player.setCues(cues, cueVolume.value, path)
}
/** El diálogo de una guía: el instante, el texto y la voz (o grabar). */
const cueDraft = ref(null)
function addCue(t = position.value) {
  if (!track.value) return
  cueDraft.value = { t: round2(t), text: '', voice: readPref('danplay.guias.voz', ''), cue: null }
}
function editCue(c) {
  cueDraft.value = { t: c.t, text: c.text || '', voice: c.voice || '', cue: c }
}
/** La guía ya hecha (por el diálogo): a la canción, y a sonar. */
async function onCueMade({ t, text, voice, file, cue }) {
  const list = (study.value.cues || []).filter((c) => c !== cue)
  list.push({ t: round2(t), text, voice, file })
  study.value.cues = list.sort((a, b) => a.t - b.t)
  cueDraft.value = null
  if (voice) savePref('danplay.guias.voz', voice)
  scheduleSave()
  await sendCues()
}
function removeCue(c) {
  study.value.cues = (study.value.cues || []).filter((x) => x !== c)
  scheduleSave()
  sendCues()
}
async function previewCue(c) {
  try {
    await api.previewCue(c.file)
  } catch (e) {
    notify(errorMessage(e))
  }
}
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
/**
 * Lo alto de cada carril: más bajos cuantas más pistas (una secuencia trae
 * veintitantas y la onda pasaba de 700 px), sin bajar de lo que piden sus
 * botones.
 */
const laneHeight = computed(() => {
  const base = isMobileApp ? 42 : big.value ? 46 : 34
  const n = lanes.value.length
  if (!n) return base
  const room = isMobileApp ? 460 : big.value ? 520 : 340
  return Math.max(isMobileApp ? 26 : 20, Math.min(base, Math.floor(room / n)))
})
const progress = computed(() => separation.progressOf(track.value?.id))
const canSeparate = computed(() => separation.state.ok)
/**
 * Se está separando OTRA canción: esta irá a la cola y empezará cuando
 * acabe. Antes el botón decía «Separar pistas» como si empezara ya.
 */
const busyWith = computed(() => {
  const cur = separation.state.current
  if (!cur || cur.id === track.value?.id) return null
  const pct = cur.total ? Math.round(((cur.done || 0) / cur.total) * 100) : 0
  return { title: cur.title || 'otra canción', pct, waiting: separation.state.queue.length }
})

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
  await routing.setStems((info?.tracks || []).map((t) => ({ key: t.source, role: t.role || '' })))
  await sendMix()
}
// al acabar cada pasada de la canción que se estudia, sus pistas: las
// rápidas en cuanto están, y luego las buenas (con los mismos nombres: el
// motor ve que cambiaron y entran a la par, sin cortar)
const stopListening = separation.onSeparated((ids) => {
  const id = loadedFor.value
  if (id != null && ids.includes(id)) loadStems(id)
})
onUnmounted(stopListening)

/** Manda a Rust lo que tiene que sonar: las pistas con su mezcla, o la canción. */
async function sendMix() {
  if (mixerOn.value) {
    await player.setStems(
      plan.value.map((t) => ({
        path: t.path,
        gain: t.gain,
        pan: t.pan,
        on: t.on,
        bus: routing.busOfStem(t.key)
      }))
    )
  } else if (player.stems.value) {
    await player.setStems(null)
  }
}
// el reparto cambió (otra pista a otra salida): las pistas, con su bus nuevo
watch(
  () => JSON.stringify(routing.routing.value.stems),
  () => {
    if (mixerOn.value) sendMix()
  }
)
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
/** Separa la canción que se estudia, con la mejor calidad (o a la cola). */
/** La máxima calidad: más instrumentos, sobre las pistas buenas. */
async function separateMore() {
  const t = track.value
  if (!t) return
  if (await separation.request([{ id: t.id, title: t.title, file: songFile.value }], { max: true }))
    wantStems = t.id
}
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
      : `en la cola: empieza cuando acabe la que se está separando (${p.place}.º)`
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
  // como suena: con el clic, si va (los golpes que se oyen, con su sonido)
  const m = metronome.value
  const clickTrack =
    asHeard && m.on
      ? {
          ...clickTimes(shownGrid.value, m, duration.value || 0),
          sound: m.sound || 'clasico',
          volume: m.volume
        }
      : null
  const name = mixFileName(songFile.value || track.value?.title || 'Cancion', plan.value, {
    click: !!clickTrack
  })
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
          ...(asHeard ? { speed: speed.value, pitch: pitch.value } : {}),
          ...(clickTrack ? { click: clickTrack } : {})
        }),
      JOBS.mix
    )
    notify(`Guardada «${r.name}»${r.id ? ': ya está en tu biblioteca' : ''}`, 'ok', 6)
  } catch (e) {
    notify('No se pudo guardar la mezcla: ' + errorMessage(e))
  }
}
/** Al celular, con lo último que se cambió del estudio ya guardado. */
async function toPhone() {
  const id = loadedFor.value
  if (!id) return
  await flushSave()
  await takeToPhone([{ id, title: track.value?.title }])
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
/** Lo que se oye no es la mezcla tal cual: otra velocidad, otro tono, o el clic. */
const heardDiffers = computed(() => speed.value !== 1 || pitch.value !== 0 || metronome.value.on)
const heardNote = computed(() => {
  const bits = []
  if (speed.value !== 1 || pitch.value !== 0) {
    bits.push(`${Math.round(speed.value * 100)} %${pitch.value ? ' · ' + pitchWords.value : ''}`)
  }
  if (metronome.value.on) bits.push('con clic')
  return bits.join(' · ')
})
function stemsMenu(ev) {
  // en el celular las pistas vienen hechas: ni se guardan mezclas, ni se
  // separan otra vez, ni hay carpeta que abrir
  if (isMobileApp) {
    return openMenu(
      ev,
      [{ label: 'Que suenen todas, como vienen', icon: 'refresh', action: resetMix }],
      'Las pistas'
    )
  }
  const items = [
    { label: 'Guardar esta mezcla…', icon: 'download', action: () => saveMix() },
    ...(heardDiffers.value
      ? [
          {
            label: 'Guardarla como suena…',
            icon: 'download',
            note: heardNote.value,
            action: () => saveMix({ asHeard: true })
          }
        ]
      : []),
    {
      label: 'Convertir a DanPista (para el celular)',
      icon: 'send',
      note: 'un archivo .danpista con las pistas, el clic, los tramos, la letra y los acordes',
      action: toPhone
    },
    { label: 'Que suenen todas, como vienen', icon: 'refresh', action: resetMix },
    {
      label: 'Por dónde suena cada pista…',
      icon: 'volume',
      action: () => (outputsOpen.value = true)
    },
    { label: 'Abrir la carpeta de las pistas', icon: 'folderOpen', action: revealStems },
    { separator: true },
    ...(progress.value
      ? []
      : [
          ...(stems.value?.best && !stems.value?.max && separation.state.max
            ? [
                {
                  label: 'Más instrumentos (máxima calidad)',
                  icon: 'sparkles',
                  note: 'percusión aparte, guitarras, teclados, cuerdas… · tarda',
                  action: separateMore
                }
              ]
            : []),
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

// ---- ocultar o salir
/**
 * Ocultar: la capa se esconde y TODO sigue igual (el clic, los tramos, la
 * velocidad, el tono, las pistas). Antes «Cerrar» lo quitaba todo, y quien
 * solo quería ver la lista se quedaba sin clic.
 */
async function hide() {
  await flushSave()
  emit('hide')
  emit('close')
}
/** Salir del modo estudio: la canción vuelve a sonar como viene. */
async function exit() {
  await flushSave()
  if (player.stems.value) await player.setStems(null)
  await player.clearLoop()
  if (speed.value !== 1) await player.setSpeed(1)
  if (pitch.value !== 0) await player.setPitch(0)
  // la cuenta y las guías son cosa del estudio: fuera de el, play es play
  await player.setMetronome({ on: false, count_in: 0 })
  await player.setCues([], cueVolume.value)
  emit('exit')
}
function headMenu(ev) {
  const items = [
    {
      label: 'Salir del modo estudio',
      icon: 'exit',
      note: 'quita el clic, los tramos, la velocidad y el tono',
      action: exit
    },
    ...(isMobileApp
      ? []
      : [
          {
            label: 'Convertir a DanPista (para el celular)',
            icon: 'send',
            note: hasStems.value ? '' : 'hace falta separarla en pistas',
            disabled: !hasStems.value,
            action: toPhone
          }
        ]),
    // también en el celular: con un cable en Y, el clic y las guías a un lado
    // (a los cascos) y la música al otro
    {
      label: 'Salidas: por dónde suena cada cosa…',
      icon: 'volume',
      action: () => (outputsOpen.value = true)
    }
  ]
  openMenu(ev, items, 'Modo estudio')
}
// Lo pendiente no se tira al desmontar: se guarda. (Cargar lo guardado ya lo
// hace el `watch` inmediato de arriba.)
onUnmounted(() => {
  flushSave()
  if (metronome.value.count_in) player.setMetronome({ count_in: 0 })
})
// ---- en una ventana de ancho medio, las columnas son pestañas: en una
// rejilla de dos por dos cada una se quedaba con cuatro dedos de alto
const root = ref(null)
const tabbed = ref(false)
const PANEL_KEY = 'danplay.study.panel'
const PANELS = [
  { v: 'tempo', n: 'Velocidad y tono', icon: 'forward10' },
  { v: 'click', n: 'Clic', icon: 'note' },
  { v: 'markers', n: 'Marcadores', icon: 'list' },
  { v: 'notes', n: 'Notas', icon: 'pencil' }
]
const savedPanel = readPref(PANEL_KEY, 'click')
const panel = ref(PANELS.some((p) => p.v === savedPanel) ? savedPanel : 'click')
watch(panel, (v) => savePref(PANEL_KEY, v))
let sizeWatch = null
onMounted(() => {
  if (isMobileApp || typeof ResizeObserver !== 'function' || !root.value) return
  sizeWatch = new ResizeObserver(([e]) => {
    const w = e.contentRect.width
    tabbed.value = w >= 680 && w < 1100
  })
  sizeWatch.observe(root.value)
})
onUnmounted(() => sizeWatch?.disconnect())

// el celular lo oculta también con «atrás» y deslizando hacia abajo
defineExpose({ close: hide, hide, exit })
</script>

<template>
  <div
    ref="root"
    class="study"
    :class="{ 'with-lanes': lanes.length, big }"
    role="region"
    aria-label="Modo estudio"
  >
    <!-- en el celular: «Modo estudio» pequeño encima, el título en grande y
         el artista debajo, cada uno en su línea -->
    <div v-if="isMobileApp" class="study-head">
      <div class="study-head-text">
        <span class="study-kicker"
          >Modo estudio<template v-if="saving"> · guardando…</template></span
        >
        <strong class="study-head-title">{{
          track ? track.title : 'Pon una canción para estudiarla'
        }}</strong>
        <span v-if="track" class="study-head-artist"
          >{{ track.artist || '—'
          }}<span v-if="track.key" class="mono study-key"> · {{ track.key }}</span></span
        >
      </div>
      <button class="btn mini" type="button" aria-label="Más del modo estudio" @click="headMenu">
        ⋯
      </button>
      <button
        class="btn mini"
        type="button"
        title="Ocultar el modo estudio (todo sigue sonando igual)"
        @click="hide"
      >
        <Icon n="down" :t="13" /> Ocultar
      </button>
    </div>
    <div v-else class="study-head">
      <Icon n="academic" :t="15" /> <strong>Modo estudio</strong>
      <span v-if="track" class="study-song"
        >{{ track.artist }} — {{ track.title }}
        <span v-if="track.key" class="mono study-key">· {{ track.key }}</span></span
      >
      <span v-else class="study-song">Pon una canción para estudiarla</span>
      <span v-if="saving" class="study-saving">guardando…</span>
      <button
        class="btn mini study-outputs"
        type="button"
        title="Salidas: por dónde suena la música, cada pista, el clic y las guías (izquierda, derecha, una interfaz…)"
        @click="outputsOpen = true"
      >
        <Icon n="volume" :t="12" /> Salidas
      </button>
      <button
        class="btn mini study-size"
        type="button"
        :class="{ on: big }"
        :aria-pressed="big ? 'true' : 'false'"
        :title="
          big
            ? 'Volver al tamaño normal (media pantalla)'
            : 'Grande: todo el alto, sin tapar el reproductor (se recuerda al volver a abrirlo)'
        "
        @click="big = !big"
      >
        <Icon n="maximize" :t="12" /> Grande
      </button>
      <button
        class="btn mini study-hide"
        type="button"
        title="Ocultar el modo estudio: todo sigue sonando igual (el clic, los tramos, la velocidad…)"
        @click="hide"
      >
        <Icon n="down" :t="13" /> Ocultar
      </button>
      <button
        class="btn mini study-more"
        type="button"
        aria-label="Más del modo estudio"
        title="Salir del modo estudio, convertir a DanPista…"
        @click="headMenu"
      >
        ⋯
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
          :title="
            isMobileApp
              ? 'Que suenen todas, como vienen'
              : 'Guardar la mezcla, convertir a DanPista, abrir la carpeta, separar otra vez…'
          "
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
          :title="
            busyWith
              ? 'Ponerla en la cola: se separa cuando acabe la que se está separando ahora'
              : 'Separar la canción en pistas (batería, voces, bajo…) para callar o dejar sola cada una'
          "
          @click="separateThis"
        >
          <Icon n="mixer" :t="12" /> {{ busyWith ? 'Poner en la cola' : 'Separar en pistas' }}
        </button>
        <span class="study-stems-note">
          <template v-if="busyWith">
            Ahora se separa «{{ busyWith.title }}»<template v-if="busyWith.pct">
              ({{ busyWith.pct }} %)</template
            >: esta empieza cuando acabe<template v-if="busyWith.waiting">
              (y las {{ busyWith.waiting }} que esperan)</template
            >
          </template>
          <template v-else>
            batería, voces, bajo… cada una aparte · en unos minutos, y luego se mejoran
          </template>
        </span>
      </template>
    </div>

    <!-- los tramos, junto a la onda en que se eligen; a la derecha, la vista -->
    <div class="study-bar" role="toolbar" aria-label="Los tramos y la vista de la onda">
      <span class="study-bar-group">
        <span class="study-loop mono" :class="{ off: !hasLoop && pendingA == null }">{{
          loopLabel
        }}</span>
        <template v-if="hasLoop">
          <span class="study-ends" role="radiogroup" aria-label="Al acabar el tramo">
            <span class="study-ends-label">Al acabar:</span>
            <button
              v-for="e in LOOP_ENDS"
              :key="e.v"
              type="button"
              role="radio"
              class="btn mini study-end"
              :class="{ on: loopEndWanted === e.v }"
              :aria-checked="loopEndWanted === e.v"
              :title="e.title"
              @click="setLoopEnd(e.v)"
            >
              <Icon :n="e.icon" :t="11" /> {{ e.n }}
            </button>
          </span>
          <button
            class="btn mini"
            type="button"
            title="Ir ya al tramo (al primero, si hay varios) y ponerlo a sonar"
            @click="playNow(0)"
          >
            <Icon n="play" :t="11" /> Ahora
          </button>
          <button
            v-if="!loopSaved"
            class="btn mini"
            type="button"
            title="Guardar como marcador, con nombre y notas (varios tramos, juntos en uno)"
            @click="saveMarker"
          >
            <Icon n="save" :t="12" /> Guardar
          </button>
          <button
            class="btn mini study-clear"
            type="button"
            title="Quitar el tramo (la canción sigue de largo)"
            @click="clearLoop"
          >
            <Icon n="close" :t="12" /> Quitar
          </button>
        </template>
      </span>
      <span v-if="!isMobileApp" class="study-bar-group study-view">
        <button
          class="btn mini"
          type="button"
          :disabled="!track"
          title="Una guía hablada aquí: escribe lo que se dirá (o grábalo) y sonará al pasar"
          @click="addCue()"
        >
          <Icon n="mic" :t="12" /> Guía
        </button>
        <button
          class="btn mini study-zoom"
          type="button"
          title="Alejar la onda (o la rueda del ratón sobre ella)"
          aria-label="Alejar"
          @click="timeline?.zoomBy?.(0.5)"
        >
          <Icon n="zoomOut" :t="12" />
        </button>
        <button
          class="btn mini study-zoom"
          type="button"
          title="Acercar la onda (o la rueda del ratón sobre ella)"
          aria-label="Acercar"
          @click="timeline?.zoomBy?.(2)"
        >
          <Icon n="zoomIn" :t="12" />
        </button>
        <button
          class="btn mini"
          type="button"
          title="Toda la canción a la vista"
          aria-label="Ver toda la canción"
          @click="timeline?.zoomFit?.()"
        >
          <Icon n="fit" :t="12" />
        </button>
      </span>
    </div>

    <!-- la onda, la regla, los tramos, los marcadores, las guías y la rejilla -->
    <StudyTimeline
      ref="timeline"
      :song-id="track?.id ?? null"
      :duration="duration"
      :position="position"
      :loops="study.loops"
      :multi="multi"
      :markers="study.markers"
      :cues="study.cues"
      :selected="selected"
      :grid="shownGrid"
      :edit-beats="editingBeats"
      :click-loading="clickLoading ? click.progress.value : null"
      :height="isMobileApp ? 120 : big ? 150 : 80"
      :lane-height="laneHeight"
      :locked="locked"
      :playing="playing"
      :lanes="lanes"
      @lane="onLane"
      @update:locked="(v) => (locked = v)"
      @update:multi="(v) => (multi = v)"
      @update:loops="onLoops"
      @update:grid="onOwnGrid"
      @seek="jump"
      @marker="(m) => selectMarker(m, { jump: !guarded })"
      @cue="editCue"
      @options="showOptions"
    />
    <p class="study-hint">
      <!-- en el celular se toca, no hay ratón ni teclas -->
      <template v-if="isMobileApp">
        <template v-if="multi">
          Desliza el dedo sobre la onda para añadir tramos · toca uno para quitarlo ·
          <b>⋯</b> opciones
        </template>
        <template v-else>
          Desliza el dedo sobre la onda para elegir el tramo ·
          {{ locked ? 'con el candado, tocar no mueve la canción' : 'toca para ir a un punto' }}
          · <b>⋯</b> opciones
        </template>
      </template>
      <template v-else-if="editingBeats">
        Ajustando tu clic: <b>arrastra</b> un pulso para moverlo · <b>doble clic</b> añade uno ·
        <b>clic derecho</b> lo quita o lo hace el «1» · la <b>rueda</b> acerca
      </template>
      <template v-else-if="multi">
        Arrastra para añadir tramos: suenan seguidos, saltándose lo de en medio · clic sobre uno
        para quitarlo · <b>rueda</b>: zoom · <b>⋯</b> o clic derecho: opciones
      </template>
      <template v-else>
        Arrastra sobre la onda para elegir el tramo ·
        {{
          locked
            ? 'con el candado, sonando, un clic no mueve la canción'
            : 'clic para ir a un punto'
        }}
        · <b>rueda</b>: zoom · <b>⋯</b> o clic derecho: opciones · teclas <b>A</b> y <b>B</b>
      </template>
    </p>

    <div v-if="tabbed" class="study-panels" role="tablist" aria-label="Partes del estudio">
      <button
        v-for="p in PANELS"
        :key="p.v"
        type="button"
        role="tab"
        class="study-panel-tab"
        :class="{ on: panel === p.v }"
        :aria-selected="panel === p.v"
        @click="panel = p.v"
      >
        <Icon :n="p.icon" :t="12" /> {{ p.n }}
        <span
          v-if="p.v === 'click' && clickWanted"
          class="study-panel-dot"
          aria-hidden="true"
        ></span>
      </button>
    </div>
    <div class="study-body" :class="{ tabbed }">
      <!-- ========================================== velocidad y tono -->
      <section class="study-col" :class="{ active: panel === 'tempo' }">
        <h5 class="study-col-title"><Icon n="forward10" :t="12" /> Velocidad y tono</h5>

        <div class="study-block">
          <div class="study-block-head">
            <span class="study-block-name"><Icon n="forward10" :t="12" /> Velocidad</span>
            <span class="mono study-value"
              >{{ speedPercent }} %
              <em v-if="speed !== 1 && !pitchPreserved" class="model-warn">cambia el tono</em>
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
            </span>
          </div>
          <div class="study-pitch-now" :class="{ on: pitch !== 0 }" :title="pitchTitle">
            <button
              class="btn mini study-pitch-down"
              type="button"
              :title="'Bajar ' + stepName"
              :aria-label="'Bajar ' + stepName"
              :disabled="!track || pitch <= -12"
              @click="setPitch(pitch - pitchStep)"
            >
              −
            </button>
            <span class="study-pitch-words">
              <span class="study-pitch-n">{{ pitch ? pitchWords : 'Como está grabada' }}</span>
              <small v-if="pitch" class="mono">{{ semitoneWords(pitch) }}</small>
            </span>
            <button
              class="btn mini study-pitch-up"
              type="button"
              :title="'Subir ' + stepName"
              :aria-label="'Subir ' + stepName"
              :disabled="!track || pitch >= 12"
              @click="setPitch(pitch + pitchStep)"
            >
              +
            </button>
          </div>
          <div class="btn-row study-pitch">
            <span
              class="study-steps"
              role="group"
              aria-label="De cuánto en cuánto se corre el tono"
            >
              <span class="study-metro-label">Pasos de</span>
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
            <em v-if="!pitchPreserved && speed === 1" class="model-warn">no se puede aquí</em>
          </div>
        </div>
      </section>

      <!-- ===================================================== el clic -->
      <section class="study-col" :class="{ active: panel === 'click' }">
        <h5 class="study-col-title"><Icon n="note" :t="12" /> Clic</h5>
        <div class="study-block study-metro" :class="{ on: clickWanted }">
          <div class="study-metro-row">
            <button
              class="btn study-metro-toggle"
              type="button"
              :class="{ on: clickWanted, loading: clickLoading }"
              :aria-pressed="clickWanted ? 'true' : 'false'"
              :disabled="!track || clickLoading"
              :title="
                clickLoading
                  ? 'Cargando el clic de esta canción: en cuanto esté, se puede poner'
                  : clickWanted
                    ? 'Parar el metrónomo (la canción sigue)'
                    : clickAuto
                      ? 'Arrancar el metrónomo, al compás de la canción'
                      : 'Arrancar el metrónomo, al tempo puesto'
              "
              @click="toggleMetronome"
            >
              <Icon :n="clickWanted ? 'pause' : 'play'" :t="12" />
              {{ clickLoading ? `Cargando… ${clickPercent} %` : clickWanted ? 'Parar' : 'Clic' }}
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
                :clearable="clickAuto && metronome.free && metronome.has_grid"
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
          <p class="study-metro-status" aria-live="polite">{{ metroStatus }}</p>
          <div
            class="study-metro-row study-modes"
            role="radiogroup"
            aria-label="De dónde sale el clic"
          >
            <button
              v-for="m in CLICK_MODES"
              :key="m.v"
              type="button"
              role="radio"
              class="btn mini study-mode"
              :class="{ on: clickMode === m.v }"
              :aria-checked="clickMode === m.v"
              :title="m.title"
              @click="click.setMode(m.v)"
            >
              {{ m.n }}
            </button>
          </div>
          <div v-if="clickAuto" class="study-metro-row">
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
              class="btn mini"
              type="button"
              title="Si el acento no cae en el 1: el siguiente pulso pasa a ser el 1"
              :disabled="!metronome.has_grid || metronome.meter === 0"
              @click="shiftOne"
            >
              el 1 es el siguiente
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
          <div v-if="clickAuto && !isMobileApp" class="study-metro-row">
            <button
              class="btn mini study-edit-beats"
              type="button"
              :class="{ on: editingBeats }"
              :aria-pressed="editingBeats"
              :disabled="!track || !grid"
              title="Ajustar los pulsos a mano sobre la onda: el clic propio de esta canción"
              @click="startEditing"
            >
              <Icon n="pencil" :t="11" /> {{ editingBeats ? 'Listo' : 'Ajustar pulsos' }}
            </button>
            <button
              class="btn mini"
              type="button"
              :disabled="!track"
              title="Volver a analizar el clic: arreglar huecos y saltos, tempo fijo, con la batería separada, solo el tramo…"
              @click="reanalyze"
            >
              <Icon n="sparkles" :t="11" /> Re-analizar…
            </button>
            <button
              v-if="hasOwn"
              class="btn mini"
              type="button"
              title="Olvidar los pulsos ajustados a mano (vuelve el automático)"
              @click="resetOwn"
            >
              <Icon n="close" :t="11" /> quitar el mío
            </button>
          </div>
          <div class="study-metro-row study-metro-vol">
            <span class="study-metro-label">Clic</span>
            <SliderField
              :model-value="metronome.volume"
              :min="0"
              :max="3"
              :step="0.05"
              :mark="1"
              width="100%"
              aria-label="Volumen del clic"
              :value-text="pctText(metronome.volume)"
              title="El volumen del clic: hasta el triple de fuerte"
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
          <button
            type="button"
            class="study-more-toggle"
            :aria-expanded="clickMore"
            @click="clickMore = !clickMore"
          >
            <Icon :n="clickMore ? 'up' : 'down'" :t="11" />
            {{ clickMore ? 'Menos' : 'Compás, cuenta y sonido' }}
          </button>
          <template v-if="clickMore">
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
            <div class="study-metro-row study-meters" role="group" aria-label="Cuenta al empezar">
              <span class="study-metro-label">Cuenta</span>
              <button
                v-for="c in COUNT_INS"
                :key="c.v"
                class="btn mini"
                type="button"
                :class="{ on: (metronome.count_in || 0) === c.v }"
                :aria-pressed="(metronome.count_in || 0) === c.v"
                :title="c.title"
                @click="setCountIn(c.v)"
              >
                {{ c.n }}
              </button>
            </div>
            <div class="study-metro-row study-meters" role="group" aria-label="Sonido del clic">
              <span class="study-metro-label">Sonido</span>
              <button
                v-for="c in CLICK_SOUNDS"
                :key="c.v"
                class="btn mini"
                type="button"
                :class="{ on: (metronome.sound || 'clasico') === c.v }"
                :aria-pressed="(metronome.sound || 'clasico') === c.v"
                :title="c.title"
                @click="setClickSound(c.v)"
              >
                {{ c.n }}
              </button>
            </div>
          </template>
        </div>
      </section>

      <!-- ======================================= marcadores y guías -->
      <section class="study-col" :class="{ active: panel === 'markers' }">
        <h5 class="study-col-title">
          <Icon n="list" :t="12" /> Marcadores
          <small v-if="study.markers.length">marca varios y tócalos seguidos</small>
        </h5>
        <div class="study-markers">
          <div
            v-for="m in study.markers"
            :key="m.t + ':' + (m.end || 0)"
            class="study-marker"
            :class="{ on: m === selected, span: isSpan(m), checked: checked.has(m) }"
          >
            <button
              v-if="isSpan(m)"
              type="button"
              role="checkbox"
              class="study-check"
              :class="{ on: checked.has(m) }"
              :aria-checked="checked.has(m) ? 'true' : 'false'"
              :aria-label="'Elegir «' + m.label + '» para tocarlo con otros'"
              :title="'Elegir «' + m.label + '» para tocarlo seguido con otros'"
              @click="toggleChecked(m)"
            >
              <Icon v-if="checked.has(m)" n="check" :t="10" />
            </button>
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
              v-if="isSpan(m)"
              type="button"
              class="field-btn"
              :title="'Tocar «' + m.label + '» ya'"
              @click="playMarker(m)"
            >
              <Icon n="play" :t="11" />
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
          <template v-if="study.cues?.length">
            <h6 class="study-sub"><Icon n="mic" :t="11" /> Guías habladas</h6>
            <div v-for="c in study.cues" :key="'c' + c.t + c.file" class="study-marker study-cue">
              <button
                type="button"
                class="study-pick"
                :title="'Ir a ' + formatTime(c.t)"
                @click="jump(c.t)"
              >
                <Icon n="mic" :t="11" class="study-pick-ico" />
                <span class="mono study-pick-time">{{ formatTime(c.t) }}</span>
                <span class="study-pick-name">{{ c.text || 'tu voz' }}</span>
              </button>
              <button type="button" class="field-btn" title="Escucharla" @click="previewCue(c)">
                <Icon n="play" :t="11" />
              </button>
              <button type="button" class="field-btn" title="Cambiarla" @click="editCue(c)">
                <Icon n="pencil" :t="11" />
              </button>
              <button type="button" class="field-btn" title="Quitarla" @click="removeCue(c)">
                <Icon n="close" :t="11" />
              </button>
            </div>
            <div class="study-metro-row study-metro-vol">
              <span class="study-metro-label">Guías</span>
              <SliderField
                v-model="cueVolume"
                :min="0"
                :max="3"
                :step="0.05"
                :mark="1"
                width="100%"
                aria-label="Volumen de las guías"
                :value-text="pctText(cueVolume)"
              />
              <span class="study-vol-n mono">{{ pctText(cueVolume) }}</span>
            </div>
          </template>
        </div>
        <div class="btn-row study-col-foot">
          <template v-if="checkedList.length">
            <button
              class="btn mini study-play-checked"
              type="button"
              :title="
                'Tocar seguidos los ' +
                checkedList.length +
                ' elegidos, saltándose lo de en medio (al acabar: ' +
                LOOP_ENDS.find((e) => e.v === loopEndWanted).n.toLowerCase() +
                ')'
              "
              @click="playChecked"
            >
              <Icon n="play" :t="11" /> Tocar {{ checkedList.length }}
              {{ checkedList.length === 1 ? 'elegido' : 'elegidos' }}
            </button>
            <span class="study-ends" role="radiogroup" aria-label="Los elegidos, al acabar">
              <button
                v-for="e in LOOP_ENDS"
                :key="e.v"
                type="button"
                role="radio"
                class="btn mini study-end"
                :class="{ on: loopEndWanted === e.v }"
                :aria-checked="loopEndWanted === e.v"
                :title="e.title"
                @click="setLoopEnd(e.v)"
              >
                <Icon :n="e.icon" :t="11" />
              </button>
            </span>
          </template>
          <button
            v-else-if="hasLoop && !loopSaved"
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
      <section class="study-col study-notes" :class="{ active: panel === 'notes' }">
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
             copia la ficha en ese momento -->
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

    <StudyOutputs
      :open="outputsOpen"
      :stems="plan.map((t) => ({ key: t.key, name: t.name, role: t.role }))"
      @close="outputsOpen = false"
    />
    <StudyCue
      v-if="cueDraft"
      :draft="cueDraft"
      :song-id="loadedFor"
      @made="onCueMade"
      @remove="
        (c) => {
          removeCue(c)
          cueDraft = null
        }
      "
      @close="cueDraft = null"
    />
  </div>
</template>
