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
import { ref, computed, watch, onMounted, onUnmounted, provide } from 'vue'
// su CSS viene con él: es una isla y no se carga al arrancar (islands.js). Antes
// que lo demás; el de driver.js (useStudyTour.js) llega después, con el recorrido
import '../styles/study.css'
// y el de ajustar los pulsos de muchos en muchos, que es parte del estudio
import '../styles/beat-tools.css'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { usePlayback } from '../composables/usePlayback.js'
import {
  LANE_MIN,
  isTight,
  laneHeight as fittedLane,
  waveRoom as roomFor
} from '../utils/studyFit.js'
import { useViewport } from '../composables/useViewport.js'
import { useClick } from '../composables/useClick.js'
import { useRouting } from '../composables/useRouting.js'
import { vTips } from '../composables/useTooltips.js'
import { tourSeen } from '../composables/tourSeen.js'
import { usePreferences } from '../composables/usePreferences.js'
import { WAVE_STYLES } from '../utils/wavePaint.js'
import { isMobileApp } from '../platform.js'
import { useHotkeys } from '../composables/useHotkeys.js'
import { formatTime } from '../utils/format.js'
import {
  cleanSegments,
  segmentAt,
  nextSegment,
  segmentsLength,
  snapToBeats
} from '../utils/segments.js'
import { openMenu } from '../composables/useContextMenu.js'
import { useStudyDoc, blank, withRawOwn, parse } from '../composables/useStudyDoc.js'
import Icon from './Icon.vue'
import StudyTimeline from './StudyTimeline.vue'
import BeatTools from './BeatTools.vue'
import StudyClock from './StudyClock.vue'
import StudyOutputs from './StudyOutputs.vue'
import StudyCue from './StudyCue.vue'
import StudyExport from './StudyExport.vue'
import VoicePicker from './VoicePicker.vue'
import StudyNotes from './StudyNotes.vue'
import StudyTempo from './StudyTempo.vue'
import StudyMarkers from './StudyMarkers.vue'
import { useStudyMarkers } from '../composables/useStudyMarkers.js'
import { useStudyCues } from '../composables/useStudyCues.js'
import { useStudyStems } from '../composables/useStudyStems.js'
import StudyStems from './StudyStems.vue'
import { useStudyClick } from '../composables/useStudyClick.js'
import StudyClick from './StudyClick.vue'
import { STUDY } from '../composables/studyContext.js'
import { clamp, round2 } from '../utils/num.js'
import { readPref, savePref } from '../utils/prefs.js'
import { MAX_CUE_LANES, markerParts as partsOf } from '../utils/cues.js'

const props = defineProps({
  /** Se ve (la capa puede estar oculta y el estudio seguir puesto). */
  visible: { type: Boolean, default: true }
})
// `hide`: se esconde y todo sigue igual; `exit`: se sale del estudio (lo que
// sonaba distinto vuelve a como viene). `close` es `hide`, por las que lo
// cerraban antes (el celular)
const emit = defineEmits(['hide', 'exit', 'close'])
const player = usePlayback()
const { track, position, duration, speed, pitch, metronome, playing } = player

// el estudio de la canción y su guardado (useStudyDoc.js)
const doc = useStudyDoc()
const { study, loadedFor, studyFor, saving, scheduleSave, flushSave, flushFile } = doc
/** Sin golpes a la vista: siempre la misma lista (una nueva repintaba la onda). */
const NO_HITS = Object.freeze([])
// el clic: su rejilla, si carga, y el modo. Vive fuera del panel: con él
// cerrado, al pasar de canción, sigue igual
const click = useClick()
onUnmounted(click.watchGrid())
// para la plantilla (ahí solo se desenvuelven los refs de primer nivel)
const clickAuto = click.auto
const clickWanted = click.wanted
// por dónde suena cada cosa
const routing = useRouting()
/** El alto de la ventana: con poco, el panel aprieta sus espacios (`isTight`). */
const { height: windowHeight } = useViewport()
// cómo se pinta la onda (de quien toca, no de cada canción)
const { waveStyle } = usePreferences()
/** El submenú «Estilo de la onda». */
const waveStyleItem = () => ({
  label: 'Estilo de la onda',
  icon: 'palette',
  note: WAVE_STYLES.find((w) => w.v === waveStyle.value)?.n.toLowerCase() || '',
  children: WAVE_STYLES.map((w) => ({
    label: w.n,
    hint: w.note,
    active: w.v === waveStyle.value,
    action: () => (waveStyle.value = w.v)
  }))
})
const outputsOpen = ref(false)
// al volver a automático, los de esta canción otra vez (a mano iba otro tempo)
watch(clickAuto, (on) => {
  if (on && loadedFor.value != null)
    click.songSettings(loadedFor.value, study.value.metronome, true)
})
/** La pestaña de notas: las de la cancion o las del marcador elegido. */
const notesTab = ref('song')
/** La línea de tiempo (su zoom y la edición de pulsos). */
const timeline = ref(null)
/**
 * Lo que comparten las partes del estudio (StudyNotes, StudyTempo…): un solo
 * objeto, que se va llenando según se arma cada una (studyContext.js).
 * @type {import('../composables/studyContext.js').StudyContext}
 */
const ctx = /** @type {import('../composables/studyContext.js').StudyContext} */ ({
  doc,
  notesTab,
  timeline,
  click,
  routing,
  visible: () => props.visible
})
provide(STUDY, ctx)
// las pistas separadas: aquí arriba porque `loadFor` las quita nada más empezar
const stemsPart = useStudyStems(ctx)
ctx.stems = stemsPart
const {
  stems,
  songFile,
  hasStems,
  mixerOn,
  plan,
  lanes,
  loadStems,
  sendMix,
  onLane,
  exportOpen,
  exportInitial,
  openExport,
  saveMix,
  toPhone,
  heardNote
} = stemsPart
// los marcadores: aquí arriba porque `loadFor` quita el elegido nada más empezar
const markers = useStudyMarkers(ctx)
ctx.markers = markers
const { selected, checked, markerOf, loopSaved, saveMarker, selectMarker, jump } = markers

async function loadFor(id) {
  // lo pendiente es de la cancion que se deja: se guarda en ELLA ya, sin
  // esperar (su copia y su id ya van apuntados)
  const leaving = loadedFor.value
  flushSave().then(() => leaving && flushFile())
  loadedFor.value = id
  studyFor.value = null
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
      metronome: typeof s.metronome === 'object' && s.metronome ? withRawOwn(s.metronome) : {},
      markers: (s.markers || []).map((m) => ({ ...m })),
      cues: (Array.isArray(s.cues) ? s.cues : []).map((c) => ({ ...c })),
      cueLanes: (Array.isArray(s.cueLanes) ? s.cueLanes : [])
        .slice(0, MAX_CUE_LANES)
        .filter((l) => l && typeof l.key === 'string')
        .map((l) => ({
          key: l.key,
          name: l.name || '',
          gain: Number.isFinite(l.gain) ? l.gain : 1,
          mute: !!l.mute,
          solo: !!l.solo,
          ...(l.auto ? { auto: true } : {})
        })),
      notes: s.notes || '',
      mixer: {
        on: !!s.mixer?.on,
        tracks: Object.fromEntries(
          Object.entries(s.mixer?.tracks || {}).map(([k, v]) => [k, { ...v }])
        )
      }
    }
    studyFor.value = id
    multi.value = loops.length > 1
    lastTouched.value = loops.length - 1
    // Lo guardado se aplica al entrar: para eso se guardó. Tras cada espera
    // se mira que siga siendo esta canción: saltando deprisa de una a otra,
    // la velocidad, el tono o el clic de esta caían sobre la siguiente.
    const still = () => loadedFor.value === id
    await player.setLoops(loops, { end: loopEndWanted.value })
    if (!still()) return
    if (study.value.speed !== speed.value) await player.setSpeed(study.value.speed)
    if (!still()) return
    if (study.value.pitch !== pitch.value) await player.setPitch(study.value.pitch)
    if (!still()) return
    // los ajustes del clic de esta canción (su rejilla la pide `useClick`)
    await click.songSettings(id, study.value.metronome)
    if (!still()) return
    selected.value = markerOf(study.value.loops)
    await sendCues()
    if (!still()) return
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
const END_KEY = 'danplay.study.atEnd'
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
ctx.loops = {
  LOOP_ENDS,
  loopEndWanted,
  setLoopEnd,
  multi,
  lastTouched,
  pendingA,
  hasLoop,
  onLoops,
  playNow
}
// ---- los mandos de la canción, encima de la onda (sin bajar al reproductor)
/** Al principio: del tramo, si hay, o de la canción. */
function toStart() {
  player.seek(hasLoop.value ? study.value.loops[0][0] : 0)
}
/** Atrás o adelante unos segundos. @param {number} secs */
function nudgeTime(secs) {
  const d = duration.value || 0
  player.seek(clamp(position.value + secs, 0, Math.max(0, d - 0.05)))
}
/** Detener: se para y vuelve al principio (del tramo, si hay). */
async function stopHere() {
  if (playing.value) await player.toggle()
  toStart()
}
/**
 * «Al acabar: parar» (lo de fábrica) para la canción al final del tramo y la
 * deja al principio de él. Sin verlo, parece que se pausa sola: se dice.
 */
let soundingAt = 0
watch(position, (p) => {
  if (playing.value) soundingAt = p
})
watch(playing, (now, before) => {
  if (now || !before || !hasLoop.value || loopEndWanted.value !== 'stop') return
  const list = study.value.loops
  if (soundingAt < list[list.length - 1][1] - 1.5) return
  notify(
    'Se paró al acabar el tramo («Al acabar: parar»). Para que vuelva a empezar solo, elige «Repetir».',
    'info',
    6
  )
})
/**
 * Lo que se puede hacer con el ratón sobre la onda: en el botón de ayuda de
 * la barra (antes, una línea de texto siempre a la vista).
 */
const waveHelp = computed(() => {
  if (editingBeats.value) {
    return 'Ajustando tu clic: clic en un pulso lo elige (Ctrl suma, Mayús elige hasta ahí) · arrastra sobre la franja de arriba para elegir una zona · arrastrar un pulso elegido los lleva todos · doble clic añade uno · clic derecho lo quita o lo hace el «1» · flechas: 5 ms · Ctrl + Z deshace · Ctrl + rueda acerca'
  }
  const pick = multi.value
    ? 'Arrastra para añadir tramos (suenan seguidos, saltándose lo de en medio) · clic sobre uno para quitarlo'
    : 'Arrastra sobre la onda para elegir un tramo · ' +
      (locked.value
        ? 'con el candado, sonando, un clic no mueve la canción'
        : 'clic para ir a un punto')
  return (
    pick +
    ' · Ctrl + rueda: acercar · Mayús + rueda: recorrer · clic derecho: opciones · teclas A y B · en las guías: doble clic pone una, arrástralas para moverlas'
  )
})

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
  // B sin A (y sin un tramo que alargar) hacía el tramo desde 0:00 y mandaba
  // la canción al principio: un dedo en la tecla de al lado, a media
  // práctica, y se perdía por dónde se iba
  if (pendingA.value == null && !single) {
    notify('Marca primero dónde empieza el tramo, con A')
    return
  }
  const from = pendingA.value ?? list[0][0]
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
  const beats = soundingGrid.value?.beats
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
        active: loopEndWanted.value === e.v,
        action: () => setLoopEnd(e.v)
      })
    }
  }
  items.push({ label: 'Ir aquí', icon: 'right', note: formatTime(t), action: () => player.seek(t) })
  if (!isMobileApp) {
    items.push({
      label: 'Guía aquí…',
      icon: 'mic',
      note: playing.value ? 'para la canción' : formatTime(t),
      disabled: playing.value,
      action: () => addCue(t)
    })
  }
  if (seg) {
    items.push({
      label: 'Ajustar a los pulsos',
      icon: 'note',
      disabled: !soundingGrid.value?.beats?.length,
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
  if (!isMobileApp) items.push({ separator: true }, waveStyleItem())
  openMenu(event ?? { clientX: x, clientY: y }, items, title)
}
useHotkeys({ a: markA, b: markB })

// ---- velocidad y tono (StudyTempo.vue)

// «Grande»: todo el alto que deja el reproductor. Se recuerda: si se cierra
// grande, se vuelve a abrir grande
const BIG_KEY = 'danplay.study.large'
const big = ref(readPref(BIG_KEY, false) === true)
watch(big, (v) => savePref(BIG_KEY, v))
/** A la vista en los carriles: los mandos de la mezcla y los canales (se recuerda). */
const MIX_KEY = 'danplay.study.showMix'
const ROUTES_KEY = 'danplay.study.showRoutes'
const showMix = ref(readPref(MIX_KEY, true) !== false)
const showRoutes = ref(readPref(ROUTES_KEY, true) !== false)
watch(showMix, (v) => savePref(MIX_KEY, v))
watch(showRoutes, (v) => savePref(ROUTES_KEY, v))
/** Con el candado, que un clic en la onda lleve igual la canción ahí (se recuerda). */
const SEEK_KEY = 'danplay.study.seekClick'
const seekClick = ref(readPref(SEEK_KEY, false) === true)
watch(seekClick, (v) => savePref(SEEK_KEY, v))

// ---- el clic (useStudyClick.js, StudyClick.vue)
const metro = useStudyClick(ctx)
ctx.metro = metro
const {
  soundingGrid,
  clickLoading,
  clickLane,
  weakRegions,
  onClickLane,
  editingBeats,
  edit,
  onOwnGrid
} = metro
// ---- marcadores: tramos con nombre y notas (useStudyMarkers.js, StudyMarkers.vue)

// ---- las guías habladas (useStudyCues.js, StudyCueList.vue)
const cues = useStudyCues(ctx)
ctx.cues = cues
const {
  cueVolume,
  guideLanes,
  laneRoute,
  audibleCues,
  sendCues,
  cueEdit,
  STOP_FIRST,
  snapCues,
  snapBeats,
  magnetTitle,
  cueWaves,
  cueDraft,
  addCue,
  editCue,
  moveCue,
  setCueLane,
  addCueLane,
  renameCueLane,
  cueLaneMenu,
  cueMenu,
  voicePick,
  onVoicePicked,
  onVoiceClosed,
  autoBusy,
  autoStage,
  autoGuide,
  goToCue,
  onCueMade,
  removeCue
} = cues
// Rust anuncia la canción nueva un instante antes de que su ruta sea la del
// motor (el estado lleva `track` de la cola y `path` del audio). Lo que se
// mandó en ese instante iba con la ruta de la anterior y el motor lo
// descartó: la siguiente sonaba sin sus guías, o entera con «Pistas»
// encendido. Al llegar su ruta, se manda otra vez.
watch(
  () => player.state.path,
  (path, before) => {
    const id = track.value?.id ?? null
    if (!path || path === before || id == null) return
    if (loadedFor.value !== id || studyFor.value !== id) return
    sendCues()
    if (mixerOn.value) sendMix()
  }
)
// ---- notas: un solo cuadro, dos pestañas (StudyNotes.vue)

// ---- las pistas separadas (useStudyStems.js, StudyStems.vue). Aquí, el
// encaje de sus carriles en el panel, y volver a mandar la mezcla al cambiar
// el reparto (detrás del `watch` de las guías, que mira lo mismo)
/** Con más pistas que estas (una secuencia), el estudio va a todo el alto. */
const MANY_LANES = 10
/** El sitio que le queda a la onda en el panel, en px (null: sin medir). */
const waveRoom = ref(null)
/**
 * Lo alto de cada carril: más bajos cuantas más pistas (una secuencia trae
 * veintitantas y la onda pasaba de 700 px), sin bajar de lo que piden sus
 * botones.
 */
const laneHeight = computed(() => {
  const base = isMobileApp ? 42 : big.value ? 46 : 34
  const n = lanes.value.length
  if (!n) return base
  // en el escritorio, lo que mide de verdad el sitio de la onda (`waveRoom`):
  // los carriles llenan lo que deja el panel, ni más (y el resto del estudio
  // se quedaba sin sitio) ni menos (y sobraba media pantalla)
  if (!isMobileApp && waveRoom.value != null) return fittedLane(n, waveRoom.value)
  const room = isMobileApp ? 460 : big.value ? 520 : 340
  return Math.max(isMobileApp ? 26 : LANE_MIN, Math.min(base, Math.floor(room / n)))
})
/** El alto de la onda de la canción (sin pistas): en grande, lo que deje el panel. */
const waveHeight = computed(() => {
  if (isMobileApp) return 120
  const base = big.value ? 150 : 80
  if (waveRoom.value == null) return base
  return Math.round(clamp(waveRoom.value, 80, big.value ? 280 : 130))
})
// el reparto cambió (otra pista a otra salida): las pistas, con su bus nuevo
watch(
  () => JSON.stringify(routing.routing.value.stems),
  () => {
    if (mixerOn.value) sendMix()
  }
)
// ---- ocultar o salir
/**
 * Ocultar: la capa se esconde y TODO sigue igual (el clic, los tramos, la
 * velocidad, el tono, las pistas). Antes «Cerrar» lo quitaba todo, y quien
 * solo quería ver la lista se quedaba sin clic.
 */
async function hide() {
  stopTour()
  await flushSave()
  flushFile()
  emit('hide')
  emit('close')
}
/** Salir del modo estudio: la canción vuelve a sonar como viene. */
async function exit() {
  await flushSave()
  flushFile()
  if (player.stems.value) await player.setStems(null)
  await player.clearLoop()
  if (speed.value !== 1) await player.setSpeed(1)
  if (pitch.value !== 0) await player.setPitch(0)
  // la cuenta y las guías son cosa del estudio: fuera de el, play es play.
  // Y el clic se quita de verdad (`click.stop`): callado solo en Rust,
  // volvía él solo al pasar de canción.
  await click.stop()
  await player.setMetronome({ count_in: 0 })
  await player.setCues([], cueVolume.value)
  emit('exit')
}
function headMenu(ev) {
  const items = [
    ...(isMobileApp
      ? []
      : [
          {
            label: 'Exportar…',
            icon: 'download',
            hint: 'La canción, la mezcla de las pistas o un .danpista para el celular',
            action: () => openExport(mixerOn.value ? 'mix' : 'song')
          },
          {
            label: 'Guía automática',
            icon: 'sparkles',
            hint: 'Las partes de la canción (intro, estrofas, coros, puente, solos…), dichas justo antes de cada una',
            disabled: !cueEdit.value || autoBusy.value,
            action: autoGuide
          },
          { separator: true }
        ]),
    // también en el celular: con un cable en Y, el clic y las guías a un lado
    // (a los cascos) y la música al otro
    {
      label: 'Salidas…',
      icon: 'outputs',
      hint: 'La salida de audio y los repartos de siempre (el clic y las guías a un lado, la música al otro)',
      action: () => (outputsOpen.value = true)
    },
    ...(isMobileApp ? [] : [waveStyleItem(), { label: 'Cómo se usa', icon: 'help', action: tour }]),
    { separator: true },
    {
      label: 'Salir del modo estudio',
      icon: 'exit',
      hint: 'Quita el clic, los tramos, la velocidad y el tono: la canción vuelve a sonar como viene',
      action: exit
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
/**
 * El recorrido (useStudyTour.js), cargado la primera vez que se pide: trae
 * driver.js y su CSS, y sale una vez en la vida. `null` hasta entonces.
 * @type {typeof import('../composables/useStudyTour.js') | null}
 */
let tourModule = null
/** El recorrido por el modo estudio (con «?» o desde el menú). */
async function tour() {
  tourModule ??= await import('../composables/useStudyTour.js')
  // mientras llegaba su trozo se pudo ocultar el estudio
  if (root.value && props.visible) tourModule.startStudyTour(root.value)
}
const stopTour = () => tourModule?.stopStudyTour()
// la primera vez que se abre en el escritorio, solo (y ya no más: se recuerda)
let tourTimer = null
watch(
  () => !isMobileApp && props.visible && !!track.value,
  (on) => {
    clearTimeout(tourTimer)
    if (!on || tourSeen()) return
    tourTimer = setTimeout(() => {
      // a la vista de verdad (en las pruebas no hay medidas: no sale)
      if (root.value?.getBoundingClientRect().height > 0 && !tourSeen()) tour()
    }, 900)
  },
  { immediate: true }
)
onUnmounted(() => {
  clearTimeout(tourTimer)
  stopTour()
})
let sizeWatch = null
/**
 * Mide el sitio que le queda a la onda dentro del panel: el alto del panel
 * menos lo de arriba (la cabecera y la barra), menos lo que se le guarda al
 * cuerpo (las columnas de velocidad, clic, marcadores y notas) y menos lo
 * demás de la línea de tiempo (el carril del clic, los de las guías, la
 * regla). Con eso se reparten los carriles.
 *
 * Antes el alto de los carriles salía de un número fijo, y con una secuencia
 * de veintitantas pistas la onda se comía el panel entero: las columnas de
 * abajo se quedaban en su título y no se podía llegar a la velocidad, al
 * clic ni a los marcadores.
 */
function fitTimeline() {
  const el = root.value
  const tl = timeline.value?.$el
  if (isMobileApp || !el || !tl?.querySelector) return
  const box = tl.querySelector('.tl-box')
  const inner = el.clientHeight
  // sin medidas (la capa aún no se ve, o un entorno sin maquetación): lo de siempre
  if (!box || !(inner > 0) || !(box.offsetHeight > 0)) return
  const cs = getComputedStyle(el)
  const pad = (parseFloat(cs.paddingTop) || 0) + (parseFloat(cs.paddingBottom) || 0)
  const gap = parseFloat(cs.rowGap) || 0
  let others = 0
  let rows = 0
  for (const child of el.children) {
    const pos = getComputedStyle(child).position
    if (pos === 'fixed' || pos === 'absolute') continue // las ventanas que se abren encima
    rows++
    if (child !== tl && !child.classList.contains('study-body')) others += child.offsetHeight
  }
  const rest = tl.scrollHeight - box.offsetHeight
  // lo que piden las columnas de los mandos (velocidad y tono, y el clic):
  // la suma de lo suyo, que no depende del alto que tengan ahora. Las de
  // los marcadores y las notas se estiran o se desplazan: no cuentan.
  let wanted = 0
  for (const col of [...el.querySelectorAll('.study-body > .study-col')].slice(0, 2)) {
    const between = parseFloat(getComputedStyle(col).rowGap) || 0
    const items = [...col.children]
    const sum = items.reduce((total, item) => total + item.offsetHeight, 0)
    wanted = Math.max(wanted, sum + between * Math.max(0, items.length - 1))
  }
  const next = roomFor({ inner, pad, others, gap, rows, rest, wanted })
  // de dos en dos píxeles: que un redondeo no la tenga midiendo sin parar
  if (waveRoom.value == null || Math.abs(next - waveRoom.value) >= 2) waveRoom.value = next
}
/**
 * Mientras el panel cambia de alto con su animación («Grande», 0,3 s) no se
 * reparte el sitio: el observador avisaba en cada fotograma, y cada vez se
 * medía todo, cambiaban los carriles y se repintaba la onda (unas 18 veces
 * en 300 ms). Se reparte una vez, al acabar. Por si el final no llega (el
 * panel se oculta a medias), un tope.
 */
let growing = false
let growTimer = null
function onGrowStart(e) {
  if (e.target !== root.value || e.propertyName !== 'height') return
  growing = true
  clearTimeout(growTimer)
  growTimer = setTimeout(onGrowEnd, 600)
}
function onGrowEnd(e) {
  if (e && (e.target !== root.value || e.propertyName !== 'height')) return
  clearTimeout(growTimer)
  if (!growing) return
  growing = false
  fitTimeline()
}
onMounted(() => {
  // cuántos canales tiene la salida: los botones de cada carril son L y R, o
  // 1, 2, 3… con una interfaz de más salidas
  if (!isMobileApp) routing.loadDevices?.().catch?.(() => {})
  if (isMobileApp || typeof ResizeObserver !== 'function' || !root.value) return
  sizeWatch = new ResizeObserver((entries) => {
    for (const e of entries) {
      if (e.target !== root.value) continue
      const w = e.contentRect.width
      tabbed.value = w >= 680 && w < 1100
    }
    if (!growing) fitTimeline()
  })
  sizeWatch.observe(root.value)
  // y lo de dentro de la línea de tiempo: otro carril de guías, el del clic
  const tl = timeline.value?.$el
  if (tl?.nodeType === 1) sizeWatch.observe(tl)
})
onUnmounted(() => {
  sizeWatch?.disconnect()
  clearTimeout(growTimer)
})

// el celular lo oculta también con «atrás» y deslizando hacia abajo
defineExpose({ close: hide, hide, exit })
</script>

<template>
  <div
    ref="root"
    v-tips
    class="study"
    :class="{
      'with-lanes': lanes.length,
      'many-lanes': lanes.length > MANY_LANES,
      big,
      // ajustando los pulsos: todo el alto, y con sitio para sus herramientas
      'editing-beats': editingBeats && !isMobileApp,
      desk: !isMobileApp,
      // en una ventana baja, los espacios más apretados para que las
      // tarjetas quepan (el texto no cambia): ver `isTight`
      tight: !isMobileApp && !big && isTight(windowHeight)
    }"
    role="region"
    aria-label="Modo estudio"
    @transitionrun="onGrowStart"
    @transitionend="onGrowEnd"
    @transitioncancel="onGrowEnd"
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
      <!-- «Salidas», a la vista: el clic y las guías a un lado y la música al
           otro, como una secuencia, en estéreo o en mono -->
      <button
        class="btn mini study-head-icon study-outputs"
        :class="{ warn: routing.short.value }"
        type="button"
        aria-label="Salidas"
        @click="outputsOpen = true"
      >
        <Icon n="outputs" :t="20" />
      </button>
      <button
        class="btn mini study-head-icon"
        type="button"
        aria-label="Más del modo estudio"
        @click="headMenu"
      >
        <Icon n="more" :t="20" />
      </button>
      <button
        class="btn mini study-head-icon"
        type="button"
        aria-label="Ocultar el modo estudio"
        @click="hide"
      >
        <Icon n="down" :t="20" />
      </button>
    </div>
    <div v-else class="study-head">
      <Icon n="academic" :t="15" />
      <strong class="study-head-name">Modo estudio</strong>
      <span
        v-if="track"
        class="study-song"
        :title="track.artist + ' — ' + track.title + (track.key ? ' · ' + track.key : '')"
        >{{ track.artist }} — {{ track.title }}
        <span v-if="track.key" class="mono study-key">· {{ track.key }}</span></span
      >
      <span v-else class="study-song">Pon una canción para estudiarla</span>
      <Transition name="fade">
        <span v-if="saving" class="study-saving" title="Guardando lo que cambiaste"
          ><Icon n="save" :t="12"
        /></span>
      </Transition>
      <span class="study-head-tools">
        <!-- las pistas separadas: separarla, o que suenen ellas -->
        <StudyStems head />
        <span class="study-head-sep" aria-hidden="true"></span>
        <button
          class="btn mini icon-btn study-outputs"
          :class="{ warn: routing.short.value }"
          type="button"
          aria-label="Salidas"
          :title="
            routing.short.value
              ? 'Salidas: a esta salida le faltan canales para su reparto y suena todo junto'
              : 'Salidas: por dónde suena la música, cada pista, el clic y las guías (izquierda, derecha, una interfaz…)'
          "
          @click="outputsOpen = true"
        >
          <Icon n="outputs" :t="14" />
        </button>
        <button
          class="btn mini icon-btn study-size"
          type="button"
          :class="{ on: big }"
          :aria-pressed="big ? 'true' : 'false'"
          aria-label="Grande"
          :title="
            big
              ? 'Volver al tamaño normal (media pantalla)'
              : 'Grande: todo el alto, sin tapar el reproductor (se recuerda)'
          "
          @click="big = !big"
        >
          <Icon n="maximize" :t="14" />
        </button>
        <button
          class="btn mini icon-btn study-hide"
          type="button"
          aria-label="Ocultar"
          title="Ocultar el modo estudio: todo sigue sonando igual (el clic, los tramos, la velocidad…)"
          @click="hide"
        >
          <Icon n="down" :t="14" />
        </button>
        <button
          class="btn mini icon-btn study-more"
          type="button"
          aria-label="Más del modo estudio"
          title="Exportar, la guía automática, las salidas, el estilo de la onda, salir del modo estudio…"
          @click="headMenu"
        >
          <Icon n="more" :t="14" />
        </button>
      </span>
    </div>

    <!-- las pistas separadas: separarla, o que suenen ellas con su mezcla -->
    <StudyStems />

    <!-- encima de la onda: la canción (sin bajar al reproductor), los tramos y
         la vista -->
    <div
      v-if="!isMobileApp || hasLoop"
      class="study-bar"
      role="toolbar"
      aria-label="La canción, los tramos y la vista de la onda"
    >
      <span v-if="!isMobileApp" class="study-bar-group study-transport">
        <button
          class="btn mini icon-btn"
          type="button"
          aria-label="Al principio"
          :title="hasLoop ? 'Al principio del tramo' : 'Al principio de la canción'"
          :disabled="!track"
          @click="toStart"
        >
          <Icon n="previous" :t="13" />
        </button>
        <button
          class="btn mini icon-btn"
          type="button"
          aria-label="5 segundos atrás"
          title="5 segundos atrás"
          :disabled="!track"
          @click="nudgeTime(-5)"
        >
          <Icon n="back10" :t="13" />
        </button>
        <button
          class="btn mini icon-btn study-play"
          type="button"
          :class="{ on: playing }"
          :aria-label="playing ? 'Pausar' : 'Reproducir'"
          :title="playing ? 'Pausar (espacio)' : 'Reproducir (espacio)'"
          :disabled="!track"
          @click="player.toggle()"
        >
          <!-- en un span: Icon lleva comentarios y la transición no lo animaría -->
          <Transition name="icon-swap" mode="out-in">
            <span :key="playing ? 'pause' : 'play'" class="icon-swap"
              ><Icon :n="playing ? 'pause' : 'play'" :t="14"
            /></span>
          </Transition>
        </button>
        <button
          class="btn mini icon-btn"
          type="button"
          aria-label="Detener"
          :title="
            hasLoop ? 'Detener y volver al principio del tramo' : 'Detener y volver al principio'
          "
          :disabled="!track"
          @click="stopHere"
        >
          <Icon n="stop" :t="13" />
        </button>
        <button
          class="btn mini icon-btn"
          type="button"
          aria-label="5 segundos adelante"
          title="5 segundos adelante"
          :disabled="!track"
          @click="nudgeTime(5)"
        >
          <Icon n="forward10" :t="13" />
        </button>
        <StudyClock />
      </span>
      <span class="study-bar-group study-loop-group">
        <span
          class="study-loop mono"
          :class="{ off: !hasLoop && pendingA == null }"
          :title="hasLoop || pendingA != null ? '' : 'Arrastra sobre la onda para elegir un tramo'"
          ><Icon v-if="!isMobileApp" n="repeat" :t="11" /> {{ loopLabel }}</span
        >
        <Transition name="fade">
          <span v-if="hasLoop" class="study-loop-tools">
            <span class="study-ends" role="radiogroup" aria-label="Al acabar el tramo">
              <span v-if="isMobileApp" class="study-ends-label">Al acabar el tramo</span>
              <button
                v-for="e in LOOP_ENDS"
                :key="e.v"
                type="button"
                role="radio"
                class="btn mini study-end"
                :class="{ on: loopEndWanted === e.v, 'icon-btn': !isMobileApp }"
                :aria-checked="loopEndWanted === e.v"
                :aria-label="e.n"
                :title="'Al acabar: ' + e.n.toLowerCase() + ' · ' + e.title"
                @click="setLoopEnd(e.v)"
              >
                <Icon :n="e.icon" :t="12" /><template v-if="isMobileApp"> {{ e.n }}</template>
              </button>
            </span>
            <button
              class="btn mini"
              :class="{ 'icon-btn': !isMobileApp }"
              type="button"
              aria-label="Ahora"
              title="Ir ya al tramo (al primero, si hay varios) y ponerlo a sonar"
              @click="playNow(0)"
            >
              <Icon n="play" :t="12" /><template v-if="isMobileApp"> Ahora</template>
            </button>
            <button
              v-if="!loopSaved"
              class="btn mini icon-btn study-save-loop"
              type="button"
              aria-label="Guardar"
              title="Guardar como marcador, con nombre y notas (varios tramos, juntos en uno)"
              @click="saveMarker"
            >
              <Icon n="save" :t="12" />
            </button>
            <button
              class="btn mini icon-btn study-clear"
              type="button"
              aria-label="Quitar"
              title="Quitar el tramo (la canción sigue de largo)"
              @click="clearLoop"
            >
              <Icon n="close" :t="12" />
            </button>
          </span>
        </Transition>
      </span>
      <span v-if="!isMobileApp" class="study-bar-group study-view">
        <button
          class="btn mini study-add-cue"
          type="button"
          :disabled="!cueEdit"
          aria-label="Guía"
          :title="
            !track
              ? ''
              : cueEdit
                ? 'Una guía aquí: escríbela (la dice una voz), grábala o importa un audio, y sonará al pasar'
                : STOP_FIRST
          "
          @click="addCue()"
        >
          <Icon n="mic" :t="13" /> Guía
        </button>
        <button
          class="btn mini icon-btn study-auto-guide"
          type="button"
          :class="{ busy: autoBusy }"
          :disabled="!cueEdit || autoBusy"
          aria-label="Guía automática"
          :title="
            !track
              ? ''
              : autoBusy
                ? autoStage || 'Buscando las partes de la canción…'
                : cueEdit
                  ? 'Guía automática: DanPlay busca las partes (intro, estrofas, coros, puente, solos…) y las dice justo antes de cada una'
                  : STOP_FIRST
          "
          @click="autoGuide"
        >
          <Icon n="sparkles" :t="13" />
        </button>
        <button
          class="btn mini icon-btn study-magnet"
          type="button"
          :class="{ on: snapCues }"
          :aria-pressed="snapCues"
          aria-label="Imán"
          :title="magnetTitle"
          @click="snapCues = !snapCues"
        >
          <Icon n="magnet" :t="14" />
        </button>
        <span class="study-head-sep" aria-hidden="true"></span>
        <button
          class="btn mini icon-btn study-show-mix"
          type="button"
          :class="{ on: showMix }"
          :aria-pressed="showMix"
          aria-label="Mezcla"
          :title="
            showMix
              ? 'Ocultar la mezcla de cada carril (M, S, volumen): más sitio para la onda'
              : 'Ver la mezcla de cada carril: callar, dejar solo y el volumen'
          "
          @click="showMix = !showMix"
        >
          <Icon n="adjust" :t="13" />
        </button>
        <button
          class="btn mini icon-btn study-show-routes"
          type="button"
          :class="{ on: showRoutes }"
          :aria-pressed="showRoutes"
          aria-label="Canales"
          :title="
            showRoutes
              ? 'Ocultar los canales de cada carril (L, R, estéreo o mono)'
              : 'Ver los canales de cada carril: por qué lado sale cada cosa, en estéreo o en mono'
          "
          @click="showRoutes = !showRoutes"
        >
          <Icon n="outputs" :t="13" />
        </button>
        <span class="study-head-sep" aria-hidden="true"></span>
        <button
          class="btn mini icon-btn study-zoom"
          type="button"
          title="Alejar (o Ctrl + rueda sobre la onda)"
          aria-label="Alejar"
          @click="timeline?.zoomBy?.(0.5)"
        >
          <Icon n="zoomOut" :t="13" />
        </button>
        <button
          class="btn mini icon-btn study-zoom"
          type="button"
          title="Acercar (o Ctrl + rueda sobre la onda)"
          aria-label="Acercar"
          @click="timeline?.zoomBy?.(2)"
        >
          <Icon n="zoomIn" :t="13" />
        </button>
        <button
          class="btn mini icon-btn"
          type="button"
          title="Toda la canción a la vista"
          aria-label="Ver toda la canción"
          @click="timeline?.zoomFit?.()"
        >
          <Icon n="fit" :t="13" />
        </button>
        <button
          class="btn mini icon-btn study-help"
          type="button"
          aria-label="Cómo se usa"
          :title="waveHelp + ' · Pulsa para un recorrido por el modo estudio'"
          @click="tour"
        >
          <Icon n="help" :t="14" />
        </button>
      </span>
    </div>

    <!-- la onda, la regla, los tramos, los marcadores, las guías y la rejilla -->
    <StudyTimeline
      ref="timeline"
      :song-id="track?.id ?? null"
      :duration="duration"
      :visible="visible"
      :loops="study.loops"
      :multi="multi"
      :markers="study.markers"
      :cues="study.cues"
      :selected="selected"
      :grid="edit.viewGrid"
      :weak="weakRegions"
      :edit-beats="editingBeats"
      :selection="edit.selection"
      :preview="edit.preview"
      :ghost-beats="edit.ghostBeats"
      :hits="edit.showHits || edit.snapHits ? edit.hits : NO_HITS"
      :hits-ref="edit.hitsRef"
      :show-hits="edit.showHits"
      :snap-hits="edit.snapHits"
      :show-mix="showMix"
      :show-routes="showRoutes"
      :click-lane="clickLane"
      :seek-click="seekClick"
      :click-loading="clickLoading ? click.progress.value : click.reanalyzing.value"
      :height="waveHeight"
      :lane-height="laneHeight"
      :locked="locked"
      :playing="playing"
      :lanes="lanes"
      guides
      :cue-lanes="guideLanes"
      :cue-edit="cueEdit"
      :cue-waves="cueWaves"
      :snap-beats="snapBeats"
      @lane="onLane"
      @add-cue="(t, lane) => addCue(t, lane)"
      @move-cue="moveCue"
      @cue-lane="setCueLane"
      @add-cue-lane="addCueLane"
      @rename-cue-lane="renameCueLane"
      @cue-lane-menu="cueLaneMenu"
      @click-lane="onClickLane"
      @update:seek-click="(v) => (seekClick = v)"
      @cue-menu="cueMenu"
      @update:locked="(v) => (locked = v)"
      @update:multi="(v) => (multi = v)"
      @update:loops="onLoops"
      @update:grid="onOwnGrid"
      @update:selection="edit.setSelection"
      @seek="jump"
      @marker="(m) => selectMarker(m, { jump: !guarded })"
      @cue="(c) => (isMobileApp ? goToCue(c) : editCue(c))"
      @options="showOptions"
    />
    <!-- ajustando los pulsos: sus herramientas, y «Tempo y onda» debajo -->
    <BeatTools v-if="editingBeats && !isMobileApp" :edit="edit" :has-loop="hasLoop" />
    <!-- en el celular se toca, no hay ratón ni teclas (en el escritorio, lo
         mismo va en el título de la onda) -->
    <p v-if="isMobileApp" class="study-hint">
      <template v-if="multi">
        Desliza el dedo sobre la onda para añadir tramos · toca uno para quitarlo ·
        <b>⋯</b> opciones
      </template>
      <template v-else>
        Desliza el dedo sobre la onda para elegir el tramo ·
        {{ locked ? 'con el candado, tocar no mueve la canción' : 'toca para ir a un punto' }}
        · <b>⋯</b> opciones
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
      <StudyTempo :class="{ active: panel === 'tempo' }" />

      <!-- ===================================================== el clic -->
      <StudyClick :class="{ active: panel === 'click' }" />

      <!-- ======================================= marcadores y guías -->
      <StudyMarkers :class="{ active: panel === 'markers' }" />

      <!-- ========================================================= notas -->
      <StudyNotes :class="{ active: panel === 'notes' }" />
    </div>

    <StudyOutputs
      :open="outputsOpen"
      :stems="[
        ...plan.map((t) => ({ key: t.key, name: t.name, role: t.role })),
        ...(guideLanes.length > 1
          ? guideLanes.map((l) => ({
              key: laneRoute(l.key),
              name: 'Guías: ' + (l.name || 'Guías'),
              role: 'guide',
              lane: true
            }))
          : [])
      ]"
      @close="outputsOpen = false"
    />
    <VoicePicker
      v-if="!isMobileApp"
      :open="!!voicePick"
      :title="voicePick?.title"
      :ok-label="voicePick?.okLabel"
      :current="voicePick?.current || ''"
      @pick="onVoicePicked"
      @close="onVoiceClosed"
    />
    <StudyExport
      v-if="!isMobileApp"
      :open="exportOpen"
      :initial="exportInitial"
      :has-stems="hasStems"
      :guides="audibleCues().length"
      :click-on="!!metronome.on"
      :heard="heardNote"
      :routed="routing.routed.value"
      @close="exportOpen = false"
      @export="saveMix"
      @phone="
        () => {
          exportOpen = false
          toPhone()
        }
      "
    />
    <StudyCue
      v-if="cueDraft"
      :draft="cueDraft"
      :song-id="loadedFor"
      :song-length="duration || 0"
      :beats="snapBeats"
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
