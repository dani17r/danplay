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
 *
 * Cada parte va en su componente (StudyStems, StudyWaveBar, StudyTempo,
 * StudyClick, StudyMarkers con StudyCueList, StudyNotes) y lo que hace, en su
 * composable (useStudyDoc, useStudyStems, useStudyMarkers, useStudyLoops,
 * useStudyClick, useStudyCues): aquí se arman y se dan a las partes con un
 * solo contexto (studyContext.js). Aquí quedan la cabecera, leer lo guardado
 * al cambiar de canción (`loadFor`), ocultar y salir, las pestañas, el
 * recorrido, el encaje de la onda en el panel, los diálogos y la línea de
 * tiempo.
 */
import { ref, computed, watch, nextTick, onMounted, onUnmounted, provide } from 'vue'
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
  TABS_BODY,
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
import { cleanSegments } from '../utils/segments.js'
import { openMenu } from '../composables/useContextMenu.js'
import { useStudyDoc, blank, withRawOwn, parse } from '../composables/useStudyDoc.js'
import Icon from './Icon.vue'
import StudyTimeline from './StudyTimeline.vue'
import BeatTools from './BeatTools.vue'
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
import { useStudyLoops } from '../composables/useStudyLoops.js'
import StudyWaveBar from './StudyWaveBar.vue'
import StudyClick from './StudyClick.vue'
import { STUDY } from '../composables/studyContext.js'
import { clamp, round2 } from '../utils/num.js'
import { readPref, savePref } from '../utils/prefs.js'
import { MAX_CUE_LANES } from '../utils/cues.js'

const props = defineProps({
  /** Se ve (la capa puede estar oculta y el estudio seguir puesto). */
  visible: { type: Boolean, default: true }
})
// `hide`: se esconde y todo sigue igual; `exit`: se sale del estudio (lo que
// sonaba distinto vuelve a como viene). `close` es `hide`, por las que lo
// cerraban antes (el celular)
const emit = defineEmits(['hide', 'exit', 'close'])
const player = usePlayback()
const { track, duration, speed, pitch, metronome, playing } = player

// el estudio de la canción y su guardado (useStudyDoc.js)
const doc = useStudyDoc()
const { study, loadedFor, studyFor, saving, flushSave, flushFile } = doc
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
 * objeto, que se va llenando según se arma cada una (studyContext.js). Cada
 * composable se arma donde estaba su código (el orden de los `watch` cuenta).
 */
const ctx = /** @type {import('../composables/studyContext.js').StudyContext} */ ({
  doc,
  notesTab,
  timeline,
  click,
  routing,
  visible: () => props.visible,
  waveStyleItem
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
const { selected, checked, markerOf, selectMarker, jump } = markers

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

// ---- los tramos, el candado y los mandos de encima de la onda (useStudyLoops.js)
const loopsPart = useStudyLoops(ctx)
ctx.loops = loopsPart
const { locked, guarded, loopEndWanted, multi, lastTouched, hasLoop, onLoops, showOptions } =
  loopsPart

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
ctx.view = { showMix, showRoutes, tour }

// ---- el clic (useStudyClick.js, StudyClick.vue)
const metro = useStudyClick(ctx)
ctx.metro = metro
const { clickLoading, clickLane, weakRegions, onClickLane, editingBeats, edit, onOwnGrid } = metro

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
  snapBeats,
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
    ...(isMobileApp
      ? []
      : [
          waveStyleItem(),
          layout.value === 'tabs'
            ? {
                label: 'Ver las partes en columnas',
                icon: 'column',
                hint: 'La velocidad, el clic, los marcadores y las notas, las cuatro a la vez una al lado de otra (con la ventana ancha)',
                action: () => (layout.value = 'columns')
              }
            : {
                label: 'Ver las partes en pestañas',
                icon: 'viewList',
                hint: 'Una parte a la vez y a todo lo ancho, con más sitio para la onda',
                action: () => (layout.value = 'tabs')
              },
          { label: 'Cómo se usa', icon: 'help', action: tour }
        ]),
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
// ---- las partes de abajo (velocidad y tono, el clic, los marcadores y las
// notas): en pestañas, una a la vez y a todo lo ancho, o en cuatro columnas.
// En pestañas por defecto (lo pidió Daniel: cada parte tiene sitio de verdad
// y la onda se queda con más alto); en columnas, si se elige en «⋯». En una
// ventana de ancho medio, siempre pestañas: en una rejilla de dos por dos
// cada una se quedaba con cuatro dedos de alto.
const root = ref(null)
/** El ancho del panel, en px (0: sin medir, como en las pruebas). */
const bodyWidth = ref(0)
/** Lo que mide como poco para que quepan las cuatro columnas. */
const COLUMNS_FROM = 1100
const LAYOUT_KEY = 'danplay.study.layout'
const layout = ref(readPref(LAYOUT_KEY, 'tabs') === 'columns' ? 'columns' : 'tabs')
watch(layout, (v) => savePref(LAYOUT_KEY, v))
const tabbed = computed(
  () =>
    !isMobileApp &&
    bodyWidth.value >= 680 &&
    (bodyWidth.value < COLUMNS_FROM || layout.value === 'tabs')
)
// Fijado, el panel va debajo de la onda; sin fijar, solo quedan las
// pestañas (la onda gana todo el alto) y al pulsar una el panel se abre
// encima de la onda, sin moverla, hasta que se pulsa fuera, Esc o la misma
// pestaña.
const PINNED_KEY = 'danplay.study.pinned'
const pinned = ref(readPref(PINNED_KEY, true) !== false)
watch(pinned, (v) => savePref(PINNED_KEY, v))
const floating = computed(() => tabbed.value && !pinned.value)
// las notas, en pestañas, ponen las de la canción y las del marcador lado a lado
ctx.view.tabbed = tabbed
/** Sin fijar, si el panel está abierto encima de la onda. */
const peek = ref(false)
const bodyShown = computed(() => !floating.value || peek.value)
const strip = ref(null)
const body = ref(null)
/** Dónde se abre el panel sin fijar: justo encima de las pestañas. */
const floatStyle = ref({})
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
/** Una pestaña: sin fijar, la abre encima de la onda (o la cierra si ya estaba). */
function pickPanel(/** @type {string} */ v) {
  if (floating.value) {
    const open = !(peek.value && panel.value === v)
    if (open) placeFloat()
    peek.value = open
  }
  panel.value = v
}
function placeFloat() {
  const el = root.value
  const tabs = strip.value
  if (!el || !tabs) return
  const cs = getComputedStyle(el)
  floatStyle.value = {
    bottom: el.clientHeight - tabs.offsetTop + 'px',
    left: cs.paddingLeft,
    right: cs.paddingRight
  }
}
function togglePinned() {
  pinned.value = !pinned.value
  peek.value = false
}
// sin fijar, se cierra al pulsar fuera (no en un menú o un diálogo que se
// abrió desde dentro) o con Esc
function closeFloatOutside(/** @type {PointerEvent} */ e) {
  if (!peek.value) return
  const t = /** @type {Element | null} */ (e.target instanceof Element ? e.target : null)
  if (!t || body.value?.contains(t) || strip.value?.contains(t)) return
  if (t.closest('[role="menu"], [role="listbox"], [role="dialog"], .ctx-menu, .modal')) return
  peek.value = false
}
function closeFloatOnEscape(/** @type {KeyboardEvent} */ e) {
  if (peek.value && e.key === 'Escape') peek.value = false
}
onMounted(() => {
  document.addEventListener('pointerdown', closeFloatOutside, true)
  document.addEventListener('keydown', closeFloatOnEscape)
})
onUnmounted(() => {
  document.removeEventListener('pointerdown', closeFloatOutside, true)
  document.removeEventListener('keydown', closeFloatOnEscape)
})
// al fijarlo, o al pasar a columnas, el panel ya no flota
watch(floating, (on) => {
  if (!on) peek.value = false
})
// lo que cambia el sitio de la onda: volver a repartir
watch([tabbed, floating], () => nextTick(() => fitTimeline()))
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
    const style = getComputedStyle(child)
    // las ventanas que se abren encima, y el panel sin fijar
    if (style.position === 'fixed' || style.position === 'absolute' || style.display === 'none')
      continue
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
  // en pestañas el cuerpo tiene un alto conocido (o ninguno, sin fijar: se
  // abre encima de la onda)
  const fixed = tabbed.value ? (floating.value ? 0 : TABS_BODY) : null
  const next = roomFor({ inner, pad, others, gap, rows, rest, wanted, body: fixed })
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
      bodyWidth.value = w
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
    <StudyWaveBar />

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

    <div
      v-if="tabbed"
      ref="strip"
      class="study-panels"
      :class="{ floating }"
      role="tablist"
      aria-label="Partes del estudio"
    >
      <button
        v-for="p in PANELS"
        :key="p.v"
        type="button"
        role="tab"
        class="study-panel-tab"
        :class="{ on: panel === p.v && bodyShown }"
        :aria-selected="panel === p.v && bodyShown"
        @click="pickPanel(p.v)"
      >
        <Icon :n="p.icon" :t="12" /> {{ p.n }}
        <span
          v-if="p.v === 'click' && clickWanted"
          class="study-panel-dot"
          aria-hidden="true"
        ></span>
      </button>
      <button
        type="button"
        class="btn mini icon-btn study-pin"
        :class="{ on: pinned }"
        :aria-pressed="pinned ? 'true' : 'false'"
        aria-label="Fijar el panel"
        :title="
          pinned
            ? 'Fijado: el panel siempre a la vista, debajo de la onda. Quítalo para que la onda tenga todo el alto y el panel se abra encima al pulsar una pestaña'
            : 'Sin fijar: la onda tiene todo el alto y el panel se abre encima al pulsar una pestaña. Fíjalo para tenerlo siempre a la vista'
        "
        @click="togglePinned"
      >
        <Icon n="pin" :t="13" />
      </button>
    </div>
    <div
      ref="body"
      class="study-body"
      :class="{ tabbed, floating, shown: bodyShown }"
      :style="floating && peek ? floatStyle : null"
    >
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
