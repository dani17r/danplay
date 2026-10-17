<script setup>
/**
 * La línea de tiempo del modo estudio: la forma de onda de la canción, una
 * regla con los minutos, los tramos, los marcadores, las guías habladas, la
 * rejilla del clic y la aguja.
 *
 * El tramo se elige sobre la propia onda: se arrastra de donde a donde, y
 * listo. Los bordes del tramo se cogen y se mueven; el tramo entero también,
 * sin cambiar lo que dura. Un clic sin arrastrar quita la selección y lleva
 * la canción a ese punto.
 *
 * Con «varios» puesto, cada arrastre añade otro tramo en vez de sustituir el
 * que hubiera, y un clic sobre un tramo lo quita: suenan seguidos,
 * saltándose lo de entre medias. Cada tramo lleva un botón de opciones (y el
 * clic derecho): quien las ofrece es StudyBar.
 *
 * Con el candado (arriba a la derecha, puesto de entrada), mientras suena un
 * clic no mueve la canción.
 *
 * **Zoom y desplazamiento, como en un editor de música.** Ctrl (o ⌘) y la
 * rueda, o pellizcar en el trackpad, acercan y alejan alrededor de donde está
 * el ratón; Mayús y la rueda, o el gesto lateral del trackpad, recorren la
 * canción; la rueda sola sube y baja el panel, como en cualquier sitio.
 * Acercada, la vista sigue a la aguja cuando sale de ella (salvo
 * que se acabe de mover a mano), y debajo de la regla una tira con toda la
 * canción enseña qué trozo se ve: se pincha o se arrastra para ir a otro.
 * Muy cerca, la onda se pide al núcleo con detalle de ese trozo (la de toda
 * la canción son 800 columnas: a diez segundos en pantalla se veía a
 * bloques).
 *
 * **Las guías habladas** (`guides`): debajo de la onda, sus carriles (hasta
 * seis, como los de las pistas separadas: nombre, callar, solo y volumen).
 * Cada guía es un bloque en su sitio. Con la canción parada (`cueEdit`),
 * doble clic en un carril pone una guía ahí, arrastrar un bloque la mueve y
 * pulsarlo la cambia; sonando, no se tocan.
 *
 * **Ajustar pulsos** (`editBeats`): cada pulso del clic es un asa que se
 * arrastra (con Mayús, se llevan también todos los de detrás); doble clic en
 * la onda añade uno; el clic derecho sobre uno lo quita o lo hace el «1».
 * Lo ajustado se manda entero (`update:grid`): es el clic propio de la
 * canción.
 *
 * La onda la pinta un canvas y solo se vuelve a pintar cuando cambian los
 * datos, el ancho, la vista o el tema. Lo que se mueve (aguja, tramo,
 * marcadores) es DOM colocado en tantos por ciento.
 *
 * Con las pistas separadas sonando (`lanes`), la onda es un carril por
 * instrumento, cada uno con su onda y, a la izquierda, sus mandos.
 */
import {
  ref,
  reactive,
  computed,
  watch,
  onMounted,
  onUnmounted,
  nextTick,
  useTemplateRef,
  shallowRef
} from 'vue'
import { api, errorMessage } from '../api.js'
import { usePreferences } from '../composables/usePreferences.js'
import { openMenu } from '../composables/useContextMenu.js'
import { formatTime } from '../utils/format.js'
import { isDownbeat, nearestBeat } from '../utils/beats.js'
import { cleanSegments } from '../utils/segments.js'
import { isMobileApp } from '../platform.js'
import { columnsFor, paintWave } from '../utils/wavePaint.js'
import Icon from './Icon.vue'
import SliderField from './ui/SliderField.vue'

const props = defineProps({
  songId: { type: Number, default: null },
  duration: { type: Number, default: 0 },
  position: { type: Number, default: 0 },
  /** los tramos que se repiten: [[a, b], …] en segundos (uno solo, el bucle A-B) */
  loops: { type: Array, default: () => [] },
  /** varios tramos a la vez: arrastrar añade otro en vez de sustituir */
  multi: Boolean,
  /** [{t, end?, label, notes?}]: tramos con nombre, o instantes sueltos */
  markers: { type: Array, default: () => [] },
  /** [{t, text?, file}]: las guías */
  cues: { type: Array, default: () => [] },
  /** la onda y el largo de cada guía, por su archivo: {file: {seconds, peaks}} */
  cueWaves: { type: Object, default: () => ({}) },
  /**
   * el imán: los pulsos del clic (los de la canción, que siguen sus cambios
   * de tempo) a los que se pegan las guías al ponerlas o moverlas; vacío,
   * sin imán
   */
  snapBeats: { type: Array, default: () => [] },
  /** el marcador elegido, para resaltarlo */
  selected: { type: Object, default: null },
  /** la rejilla de pulsos del metrónomo tal como suena, para pintarla sobre la onda */
  grid: { type: Object, default: null },
  /** ajustando los pulsos a mano (el clic propio) */
  editBeats: Boolean,
  /**
   * lo que lleva (0..1) el clic de la canción mientras se carga, o null si
   * no carga: una barra del color del tema avanza sobre la onda
   */
  clickLoading: { type: Number, default: null },
  /** alto de la onda en px */
  height: { type: Number, default: 56 },
  /** el candado: sonando, un clic no mueve la canción */
  locked: Boolean,
  /** si la canción está sonando (el candado solo cuenta entonces) */
  playing: Boolean,
  /**
   * las pistas separadas, un carril cada una: [{key, name, wave: {peaks, rms},
   * on, mute, solo, gain, pan}]. Vacío: la onda de la canción.
   */
  lanes: { type: Array, default: () => [] },
  /** alto de cada carril, en px */
  laneHeight: { type: Number, default: 34 },
  /** los carriles de las guías debajo de la onda (no en el celular) */
  guides: Boolean,
  /** los carriles de las guías: [{key, name, gain, mute, solo}] */
  cueLanes: { type: Array, default: () => [] },
  /** se pueden poner, mover y cambiar guías (con la canción parada) */
  cueEdit: Boolean
})
// `update:loops` lleva los tramos (en orden, los que se pisan juntos) y
// `{ mode, index }`: 'select' si se dibujo uno de cero, 'add' si se anadio
// otro, 'edit' si se movio un borde o un tramo entero (`index`, cual),
// 'remove' si se quito uno y 'clear' si se quitaron todos. `update:grid`
// lleva la rejilla ajustada a mano. `cue`, la guía que se pulsó. Las de los
// carriles de guías: `add-cue` (segundo, carril), `move-cue` (guía, segundo),
// `cue-lane` (carril, cambios), `add-cue-lane`, `remove-cue-lane` y
// `rename-cue-lane` (carril); `cue-lane-menu` (carril, evento, segundo o
// null), las opciones de un carril (su botón ⋯ o un clic derecho en él), y
// `cue-menu` (guía, evento), las de una guía (clic derecho en ella).
const emit = defineEmits([
  'update:loops',
  'update:grid',
  'seek',
  'marker',
  'cue',
  'update:locked',
  'update:multi',
  'options',
  'lane',
  'add-cue',
  'move-cue',
  'cue-lane',
  'add-cue-lane',
  'remove-cue-lane',
  'rename-cue-lane',
  'cue-lane-menu',
  'cue-menu'
])

/** Con las pistas separadas, un carril por instrumento. */
const laneMode = computed(() => props.lanes.length > 0)
/** Los carriles de las guías, debajo de la onda (en el escritorio). */
const showGuides = computed(() => props.guides && !isMobileApp && props.cueLanes.length > 0)
/** Cada carril con su cabecera a la izquierda (los de las pistas o los de las guías). */
const headMode = computed(() => laneMode.value || showGuides.value)
/** Hasta cuántos carriles de guías. */
const MAX_CUE_LANES = 6
// alto de la onda (o de todos los carriles), en px
const WAVE_H = computed(() =>
  laneMode.value ? props.lanes.length * props.laneHeight : props.height
)
const MIN_LOOP = 0.5 // menos que esto no es un bucle, es un clic con temblor
const HANDLE = 7 // a estos px de un borde se coge el borde, no se empieza otro tramo
const THRESHOLD = 4 // px de movimiento a partir de los que un clic pasa a ser arrastre
const SNAP = 6 // px: cerca de un marcador, el borde se pega a él
const MIN_SPAN = 1.5 // segundos a la vista como mucho de cerca
const FOLLOW_PAUSE = 4000 // ms sin seguir a la aguja tras mover la vista a mano

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))
const round2 = (v) => Math.round(v * 100) / 100
const round3 = (v) => Math.round(v * 1000) / 1000

// ------------------------------------------------------------ la vista
/** El trozo de la canción que se ve: de `from` a `to`, en segundos. */
const view = reactive({ from: 0, to: 0 })
const span = computed(() => Math.max(1e-6, (view.to || props.duration) - view.from))
const zoomed = computed(() => props.duration > 0 && span.value < props.duration - 0.05)
watch(
  () => [props.songId, props.duration],
  ([, d]) => {
    view.from = 0
    view.to = d || 0
  },
  { immediate: true }
)
/** Cuándo se movió la vista a mano por última vez (para no seguir la aguja). */
let movedAt = 0
function setView(from, to, byHand = true) {
  const d = props.duration
  if (!d) return
  let len = clamp(to - from, Math.min(MIN_SPAN, d), d)
  let a = clamp(from, 0, d - len)
  view.from = a
  view.to = a + len
  if (byHand) movedAt = Date.now()
}
/** Acercar (`factor` > 1) o alejar alrededor del segundo `at`. */
function zoomBy(factor, at = null) {
  const d = props.duration
  if (!d) return
  const center = at ?? (zoomed.value ? (view.from + view.to) / 2 : props.position)
  const len = clamp(span.value / factor, Math.min(MIN_SPAN, d), d)
  const rel = span.value > 0 ? (center - view.from) / span.value : 0.5
  setView(center - rel * len, center - rel * len + len)
}
function zoomFit() {
  setView(0, props.duration)
}
/** Lo bastante cerca como para coger los pulsos: unos ocho compases. */
function zoomToBeats() {
  const g = props.grid
  const period = g?.bpm ? 60 / g.bpm : 0.5
  const len = clamp(period * 32, 6, props.duration || 6)
  setView(props.position - len * 0.2, props.position - len * 0.2 + len)
}
defineExpose({ zoomBy, zoomFit, zoomToBeats, view, focusCue: (c) => focusCue(c) })

// acercada, la vista sigue a la aguja cuando se va de ella
watch(
  () => props.position,
  (t) => {
    if (!zoomed.value || drag.mode || beatDrag.index >= 0) return
    if (Date.now() - movedAt < FOLLOW_PAUSE) return
    if (t < view.from || t > view.from + span.value * 0.92) {
      setView(t - span.value * 0.1, t - span.value * 0.1 + span.value, false)
    }
  }
)

/**
 * La rueda, como en un editor de música: con Ctrl o ⌘ (o pellizcando el
 * trackpad, que llega así) acerca y aleja donde está el ratón; con Mayús o
 * de lado, recorre la canción; sola, sube y baja el panel como en cualquier
 * sitio (antes acercaba, y no había forma de bajar por encima de la onda).
 */
function onWheel(e) {
  if (!props.duration) return
  if (e.ctrlKey || e.metaKey) {
    e.preventDefault()
    const step = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY
    zoomBy(Math.exp(-step * 0.0018), timeAt(e.clientX))
    return
  }
  const sideways = e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)
  if (!sideways || !zoomed.value) return
  e.preventDefault()
  const delta = (e.shiftKey ? e.deltaY || e.deltaX : e.deltaX) || 0
  const step = e.deltaMode === 1 ? delta * 16 : delta
  const r = box.value.getBoundingClientRect()
  const secs = (step / Math.max(1, r.width)) * span.value
  setView(view.from + secs, view.to + secs)
}

// ------------------------------------------------------------ la onda
const box = useTemplateRef('box')
const canvas = useTemplateRef('canvas')
// {peaks, rms} o null. Superficial: son miles de numeros que solo se leen
// para pintar; hacerlos reactivos uno a uno no aporta nada
const wave = shallowRef(null)
/** La onda con detalle del trozo que se ve: {from, to, peaks, rms} o null. */
const detail = shallowRef(null)
const loading = ref(false)
/** Por qué no hay onda ('' si la hay): lo que dice el núcleo, no una suposición. */
const failed = ref('')

async function load(id) {
  wave.value = null
  detail.value = null
  failed.value = ''
  if (!id) {
    draw()
    return
  }
  loading.value = true
  try {
    const r = await api.waveform(id)
    if (id === props.songId) wave.value = r
  } catch (e) {
    if (id === props.songId) failed.value = errorMessage(e) || 'no se pudo leer la canción'
  } finally {
    if (id === props.songId) loading.value = false
    await nextTick()
    draw()
  }
}
watch(() => props.songId, load, { immediate: true })

/**
 * De cerca, la onda de lo que se ve, con su detalle (se pide al núcleo un
 * poco más ancha que la vista, para desplazarse sin esperar).
 */
let detailTimer = null
function wantDetail() {
  clearTimeout(detailTimer)
  const w = wave.value
  const id = props.songId
  if (!w || !id || !props.duration || laneMode.value || typeof api.waveformRange !== 'function')
    return
  const columns = (span.value / props.duration) * (w.peaks?.length || 0)
  const W = width.value || 800
  if (columns >= W / 2) {
    if (detail.value) {
      detail.value = null
      draw()
    }
    return
  }
  const d = detail.value
  if (d && d.from <= view.from && d.to >= view.to && (d.to - d.from) / span.value < 2.2) return
  detailTimer = setTimeout(async () => {
    const pad = span.value * 0.5
    const from = Math.max(0, view.from - pad)
    const to = Math.min(props.duration, view.to + pad)
    const buckets = clamp(Math.round(((to - from) / span.value) * W), 64, 4000)
    try {
      const r = await api.waveformRange(id, from, to, buckets)
      if (id === props.songId && r?.peaks?.length && wave.value) {
        // el trozo llega a la escala del tope digital, y la onda entera a la
        // de su propio pico: se iguala con lo que la entera tiene en ese
        // trozo, para que acercar no cambie el alto de lo que se ve
        const full = wave.value.peaks
        const n = full.length
        const a = Math.floor((from / props.duration) * n)
        const b = Math.max(a + 1, Math.ceil((to / props.duration) * n))
        const fullTop = Math.max(1e-6, ...full.slice(a, b))
        const partTop = Math.max(1e-6, ...r.peaks)
        const k = fullTop / partTop
        detail.value = {
          from,
          to,
          peaks: r.peaks.map((v) => v * k),
          rms: (r.rms || []).map((v) => v * k)
        }
        draw()
      }
    } catch {
      /* se queda la de toda la canción */
    }
  }, 120)
}
watch(
  () => [view.from, view.to],
  () => {
    draw()
    wantDetail()
  }
)

const { theme, waveStyle } = usePreferences()
watch([theme, waveStyle], () => nextTick(draw))

/**
 * Las columnas de lo que se ve, de una onda de toda la canción (o de su
 * detalle): para cada columna de pantalla, el pico y el RMS medio.
 */
function columnsOf(w, wFrom, wTo, cols) {
  const n = w.peaks.length
  const per = n / Math.max(1e-6, wTo - wFrom)
  const out = []
  for (let i = 0; i < cols; i++) {
    const t0 = view.from + (i / cols) * span.value
    const t1 = view.from + ((i + 1) / cols) * span.value
    let from = Math.floor((t0 - wFrom) * per)
    let to = Math.ceil((t1 - wFrom) * per)
    if (to <= 0 || from >= n) {
      out.push([0, 0])
      continue
    }
    from = clamp(from, 0, n - 1)
    to = clamp(Math.max(to, from + 1), from + 1, n)
    let pk = 0
    let rm = 0
    for (let j = from; j < to; j++) {
      pk = Math.max(pk, w.peaks[j] || 0)
      rm += w.rms?.[j] || 0
    }
    out.push([pk, rm / (to - from)])
  }
  return out
}

/** Pinta la onda: el pico en gris claro (la silueta) y el RMS en el acento. */
function draw() {
  const c = canvas.value
  if (!c) return
  const ctx = c.getContext?.('2d')
  if (!ctx) return // sin maquetacion (las pruebas) no hay donde pintar
  const W = box.value?.clientWidth || 0
  const H = WAVE_H.value
  if (!W) return
  const dpr = window.devicePixelRatio || 1
  c.width = Math.round(W * dpr)
  c.height = Math.round(H * dpr)
  c.style.width = W + 'px'
  c.style.height = H + 'px'
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.clearRect(0, 0, W, H)
  // los colores salen del tema; si por lo que sea no estan, el del texto
  const styles = getComputedStyle(c)
  const text = styles.getPropertyValue('--text').trim() || styles.color || 'gray'
  const accent = styles.getPropertyValue('--accent').trim() || text
  if (laneMode.value) {
    drawLanes(ctx, W, text, accent)
    drawGrid(ctx, W, H)
    return
  }
  const mid = H / 2
  const w = wave.value
  if (!w || !w.peaks?.length || !props.duration) {
    // sin onda (aun cargando, o no se pudo): la linea del centro, y ya
    ctx.globalAlpha = 0.25
    ctx.fillStyle = text
    ctx.fillRect(0, mid - 0.5, W, 1)
    ctx.globalAlpha = 1
    return
  }
  // En un mp3 de hoy los picos van casi todos a tope y el RMS ronda el
  // tercio: el RMS se estira hasta su propio maximo (el de toda la canción,
  // tambien de cerca); el estilo lo elige quien toca (utils/wavePaint.js)
  const rmsTop = Math.max(1e-6, ...(w.rms || [0]))
  const style = waveStyle.value
  const cols = columnsFor(style, W)
  const d = detail.value
  const useDetail = d && d.from <= view.from + 1e-6 && d.to >= view.to - 1e-6
  const data = useDetail ? columnsOf(d, d.from, d.to, cols) : columnsOf(w, 0, props.duration, cols)
  paintWave(ctx, data, { W, top: 0, h: H, rmsTop, text, accent, style, pad: 2, min: 1 })
  drawGrid(ctx, W, H)
}

/**
 * Un carril por pista, todas a la misma escala (el separador las mide juntas):
 * se ve qué instrumento suena más y dónde entra cada uno. Lo que no suena
 * (callado, o fuera de un solo) se pinta apagado.
 */
function drawLanes(ctx, W, text, accent) {
  const lh = props.laneHeight
  let rmsTop = 1e-6
  for (const lane of props.lanes) {
    for (const v of lane.wave?.rms || []) if (v > rmsTop) rmsTop = v
  }
  const style = waveStyle.value
  const cols = columnsFor(style, W)
  props.lanes.forEach((lane, li) => {
    const top = li * lh
    const mid = top + lh / 2
    const dim = lane.on ? 1 : 0.3
    ctx.fillStyle = text
    if (li) {
      ctx.globalAlpha = 0.14
      ctx.fillRect(0, top, W, 1)
    }
    const w = lane.wave
    if (!w?.peaks?.length || !props.duration) {
      ctx.globalAlpha = 0.22 * dim
      ctx.fillRect(0, mid - 0.5, W, 1)
      return
    }
    const data = columnsOf(w, 0, props.duration, cols)
    paintWave(ctx, data, { W, top, h: lh, rmsTop, text, accent, dim, style, pad: 3, min: 0.5 })
  })
  ctx.globalAlpha = 1
}
// lo que cambia lo que se pinta de los carriles (y no la aguja ni el volumen)
const laneLook = computed(() =>
  props.lanes.map((l) => `${l.key}:${l.on ? 1 : 0}:${l.wave?.peaks?.length || 0}`).join('|')
)
watch(laneLook, () => nextTick(draw))

/** El volumen de un carril, para leerlo: «80 %». */
const pctText = (v) => Math.round((Number(v) || 0) * 100) + ' %'
/** El panorama de un carril, para leerlo: «centro», «30 % izq.». */
function panText(v) {
  const p = Math.round((Number(v) || 0) * 100)
  if (!p) return 'centro'
  return `${Math.abs(p)} % ${p < 0 ? 'izq.' : 'der.'}`
}

/**
 * La rejilla del metrónomo: una raya por pulso abajo; el «1», más alta (sin
 * acento, ninguno). De cerca, todas más visibles.
 */
function drawGrid(ctx, W, H) {
  const g = props.grid
  if (!g || !g.beats?.length || !props.duration || props.editBeats) return
  const styles = getComputedStyle(canvas.value)
  const text = styles.getPropertyValue('--text').trim() || styles.color || 'gray'
  ctx.fillStyle = text
  const close = (span.value / props.duration) * g.beats.length < W / 6
  for (let i = 0; i < g.beats.length; i++) {
    const t = g.beats[i]
    if (t < view.from || t > view.to) continue
    const x = Math.round(((t - view.from) / span.value) * W)
    const one = isDownbeat(g, i)
    // el «1» cruza la onda entera, tenue; los demas pulsos son marcas abajo
    ctx.globalAlpha = one ? (close ? 0.45 : 0.28) : close ? 0.6 : 0.45
    const h = one ? H : close ? 12 : 7
    ctx.fillRect(x, one ? 0 : H - h, 1, h)
  }
  ctx.globalAlpha = 1
}
watch(
  () => [props.grid, props.editBeats],
  () => nextTick(draw)
)

// Al cambiar el ancho se vuelve a medir la regla y a pintar la onda.
let observer = null
onMounted(() => {
  measure()
  draw()
  if (typeof ResizeObserver === 'function' && box.value) {
    observer = new ResizeObserver(() => {
      measure()
      draw()
    })
    observer.observe(box.value)
  }
})
onUnmounted(() => {
  observer?.disconnect()
  clearTimeout(detailTimer)
  stopDrag()
  stopBeatDrag()
})

// ------------------------------------------------------------ geometria
/** Dónde cae `t` dentro de lo que se ve, en tantos por ciento (puede salirse). */
const pct = (t) => (props.duration ? ((t - view.from) / span.value) * 100 + '%' : '0%')
/** Una anchura (de tanto a tanto), en tantos por ciento de lo que se ve. */
const pctLen = (len) => (props.duration ? (len / span.value) * 100 + '%' : '0%')
/** Si algo de `a` a `b` se ve. */
const inView = (a, b = a) => b >= view.from - 1e-6 && a <= view.to + 1e-6
/** El segundo que cae bajo esa x de pantalla. */
function timeAt(clientX) {
  const r = box.value.getBoundingClientRect()
  const f = r.width > 0 ? clamp((clientX - r.left) / r.width, 0, 1) : 0
  return clamp(view.from + f * span.value, 0, props.duration)
}
/** Y al reves: en que x (dentro de la caja) cae ese segundo. */
function xOf(t) {
  const r = box.value.getBoundingClientRect()
  return props.duration ? ((t - view.from) / span.value) * r.width : 0
}

// ------------------------------------------------------------ la regla
// Paso entre marcas con numero: el mas fino que deje ~70 px entre ellas.
const STEPS = [0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1200]
const MINOR = {
  0.1: 0,
  0.25: 0,
  0.5: 0.1,
  1: 0,
  2: 1,
  5: 1,
  10: 5,
  15: 5,
  30: 10,
  60: 30,
  120: 60,
  300: 60,
  600: 300,
  1200: 600
}
const width = ref(0)
function measure() {
  width.value = box.value?.clientWidth || 0
}
/** «1:23», o con décimas de cerca: «1:23.5». */
function tickLabel(t, step) {
  if (step >= 1) return formatTime(t)
  const whole = Math.floor(t + 1e-6)
  const frac = Math.round((t - whole) * (step < 0.25 ? 10 : 100))
  const digits = step < 0.25 ? String(frac) : String(frac).padStart(2, '0')
  return `${formatTime(whole)}.${digits}`
}
const ticks = computed(() => {
  const d = props.duration
  const W = width.value || 800
  if (!d) return []
  const s = span.value
  const step = STEPS.find((x) => (x / s) * W >= 70) || STEPS.at(-1)
  const unit = MINOR[step] || step
  const out = []
  const first = Math.ceil(view.from / unit - 1e-6)
  for (let i = first; i * unit <= view.to + 1e-6 && out.length < 400; i++) {
    const t = round3(i * unit)
    if (t > d) break
    const major = Math.abs(t / step - Math.round(t / step)) < 1e-6
    // la ultima con numero no se pisa con la del final
    if (major && t > 0 && view.to - t < step * 0.6 && Math.abs(view.to - d) < 1e-6) continue
    out.push({ t, major, label: major ? tickLabel(t, step) : '' })
  }
  return out
})

// ------------------------------------------------------------ los tramos
// 'select' arrastra un tramo nuevo; 'a' y 'b' mueven un borde del tramo
// `index`; 'move' lo lleva entero. Hasta pasar del umbral no es nada: sera
// un clic.
const drag = reactive({
  mode: null,
  index: -1,
  moved: false,
  startX: 0,
  from: 0,
  a: 0,
  b: 0,
  a0: 0,
  b0: 0
})

/** Los tramos guardados, validos. */
const saved = computed(() =>
  (props.loops || []).filter((s) => Array.isArray(s) && s.length === 2 && s[1] > s[0])
)

/**
 * Los tramos que se ven: los guardados, con el que se esta arrastrando en su
 * sitio. Uno nuevo sustituye a todos, salvo con «varios», que se anade.
 */
const shown = computed(() => {
  const list = saved.value.map((s) => [s[0], s[1]])
  if (!drag.mode || !drag.moved) return list
  if (drag.mode === 'select') {
    const fresh = [Math.min(drag.from, drag.a), Math.max(drag.from, drag.a)]
    return props.multi ? [...list, fresh] : [fresh]
  }
  if (drag.index >= 0 && drag.index < list.length) list[drag.index] = [drag.a, drag.b]
  return list
})
/** Lo de fuera de los tramos, que se atenua: los huecos entre ellos. */
const gaps = computed(() => {
  const list = [...shown.value].sort((x, y) => x[0] - y[0])
  if (!list.length) return []
  const out = []
  let from = 0
  for (const [a, b] of list) {
    if (a > from) out.push({ left: pct(from), width: pctLen(a - from) })
    from = Math.max(from, b)
  }
  out.push({
    left: pct(from),
    right: zoomed.value ? `${100 - ((props.duration - view.from) / span.value) * 100}%` : 0
  })
  return out
})
/** El tramo que se esta arrastrando (el nuevo va el ultimo). */
const isEditing = (i) =>
  !!(drag.mode && drag.moved) &&
  (drag.mode === 'select' ? i === shown.value.length - 1 : i === drag.index)
/** Los trozos de un marcador: sus partes, su tramo, o nada si es un instante. */
const partsOf = (m) => (m.parts?.length ? m.parts : m.end > m.t ? [[m.t, m.end]] : [])

/** Cerca del principio o del final de un marcador, el borde se pega a el. */
function snap(t) {
  for (const m of props.markers || []) {
    if (Math.abs(xOf(m.t) - xOf(t)) <= SNAP) return m.t
    if (m.end > m.t && Math.abs(xOf(m.end) - xOf(t)) <= SNAP) return m.end
  }
  return t
}

/**
 * Que hay bajo esa x (dentro de la caja): el borde de un tramo, su interior o
 * nada. Los bordes mandan sobre el interior de otro, y el mas cercano gana.
 */
function hitAt(x) {
  let edge = null
  saved.value.forEach(([a, b], i) => {
    for (const [side, t] of [
      ['a', a],
      ['b', b]
    ]) {
      const d = Math.abs(x - xOf(t))
      if (d <= HANDLE && (!edge || d < edge.d)) edge = { mode: side, index: i, d }
    }
  })
  if (edge) return edge
  const inside = saved.value.findIndex(([a, b]) => x > xOf(a) && x < xOf(b))
  return inside >= 0 ? { mode: 'move', index: inside } : { mode: 'select', index: -1 }
}
const xIn = (clientX) => clientX - box.value.getBoundingClientRect().left

function listen(on) {
  const f = on ? window.addEventListener : window.removeEventListener
  f.call(window, 'pointermove', onMove)
  f.call(window, 'pointerup', onUp)
  f.call(window, 'pointercancel', stopDrag)
}
function onDown(e) {
  if (e.button || !props.duration) return
  const t = timeAt(e.clientX)
  const hit = hitAt(xIn(e.clientX))
  const cur = hit.index >= 0 ? saved.value[hit.index] : null
  Object.assign(drag, {
    mode: hit.mode,
    index: hit.index,
    moved: false,
    startX: e.clientX,
    from: t,
    a: cur ? cur[0] : t,
    b: cur ? cur[1] : t,
    a0: cur ? cur[0] : t,
    b0: cur ? cur[1] : t
  })
  e.preventDefault()
  listen(true)
}
function onMove(e) {
  if (!drag.mode) return
  if (!drag.moved && Math.abs(e.clientX - drag.startX) < THRESHOLD) return
  drag.moved = true
  const t = snap(timeAt(e.clientX))
  if (drag.mode === 'select') drag.a = t
  else if (drag.mode === 'a') drag.a = clamp(t, 0, drag.b - MIN_LOOP)
  else if (drag.mode === 'b') drag.b = clamp(t, drag.a + MIN_LOOP, props.duration)
  else if (drag.mode === 'move') {
    const len = drag.b0 - drag.a0
    const a = clamp(drag.a0 + (timeAt(e.clientX) - drag.from), 0, props.duration - len)
    drag.a = a
    drag.b = a + len
  }
}
/** Un clic sin arrastrar: quita la seleccion y lleva la cancion ahi. */
function onClick(e, index) {
  const list = saved.value
  const blocked = props.locked && props.playing
  // con «varios», un clic sobre un tramo quita ese; sin «varios», cualquier
  // clic quita la seleccion (la que hubiera)
  if (props.multi && index >= 0) {
    emit(
      'update:loops',
      list.filter((_, i) => i !== index),
      { mode: 'remove' }
    )
    return
  }
  if (!props.multi && list.length) emit('update:loops', [], { mode: 'clear' })
  // con el candado, sonando, el clic no mueve la cancion; si ademas no
  // habia nada que quitar, el candado avisa de por que no paso nada
  if (blocked) {
    if (props.multi || !list.length) nudgeLock()
    return
  }
  emit('seek', round2(timeAt(e.clientX)))
}
function onUp(e) {
  const d = { ...drag }
  stopDrag()
  if (!d.mode) return
  if (!d.moved) return onClick(e, d.mode === 'select' ? -1 : d.index)
  const list = saved.value.map((s) => [s[0], s[1]])
  if (d.mode === 'select') {
    const a = Math.min(d.from, d.a)
    const b = Math.max(d.from, d.a)
    if (b - a < MIN_LOOP) return // demasiado corto para ser un bucle
    const fresh = [round2(a), round2(b)]
    if (props.multi && list.length) {
      emit('update:loops', cleanSegments([...list, fresh]), { mode: 'add' })
    } else {
      emit('update:loops', [fresh], { mode: 'select' })
    }
    return
  }
  list[d.index] = [round2(d.a), round2(d.b)]
  emit('update:loops', cleanSegments(list), { mode: 'edit', index: d.index })
}
function stopDrag() {
  drag.mode = null
  drag.moved = false
  listen(false)
}
/** El clic derecho: las opciones del tramo de debajo (o de ese punto). */
function onContext(e) {
  if (!props.duration) return
  const hit = hitAt(xIn(e.clientX))
  emit('options', {
    index: hit.mode === 'select' ? -1 : hit.index,
    t: round2(timeAt(e.clientX)),
    x: e.clientX,
    y: e.clientY,
    event: e
  })
}
/**
 * El boton de opciones de un tramo. Con el raton, el menu sale donde se pulso;
 * con el teclado (Enter: un clic sin `detail`), debajo del boton.
 */
function optionsOf(e, index) {
  const r = e.currentTarget?.getBoundingClientRect?.()
  emit('options', {
    index,
    t: saved.value[index]?.[0] ?? 0,
    x: r ? r.left : e.clientX,
    y: r ? r.bottom + 4 : e.clientY,
    event: e.detail ? e : null
  })
}

/** Que cursor toca segun lo que hay bajo el puntero (sin arrastre en marcha). */
const hover = ref('')
function onHover(e) {
  if (drag.mode || !props.duration || !saved.value.length) {
    hover.value = ''
    return
  }
  const hit = hitAt(xIn(e.clientX))
  hover.value = hit.mode === 'a' || hit.mode === 'b' ? 'edge' : hit.mode === 'move' ? 'inside' : ''
}

const dragging = computed(() => !!(drag.mode && drag.moved))

// ------------------------------------------------------------ ajustar pulsos
/** Los pulsos que se ven (como mucho 600: más, hay que acercarse). */
const beatsInView = computed(() => {
  const g = props.grid
  if (!props.editBeats || !g?.beats?.length) return []
  const out = []
  for (let i = 0; i < g.beats.length; i++) {
    const t = g.beats[i]
    if (t < view.from || t > view.to) continue
    out.push({ i, t, one: isDownbeat(g, i) })
    if (out.length > 600) return []
  }
  return out
})
const tooManyBeats = computed(() => {
  const g = props.grid
  if (!props.editBeats || !g?.beats?.length) return false
  return !beatsInView.value.length && g.beats.some((t) => t >= view.from && t <= view.to)
})
/** La rejilla que se va ajustando (copia, hasta soltar). */
const editing = shallowRef(null)
const beatDrag = reactive({ index: -1, x0: 0, t0: 0, all: false, moved: false })
const shownBeats = computed(() => {
  if (!editing.value) return beatsInView.value
  const g = editing.value
  return beatsInView.value.map((b) => ({ ...b, t: g.beats[b.i] }))
})
function copyGrid() {
  const g = props.grid
  return { ...g, beats: [...g.beats], bars: [...(g.bars || [])] }
}
function listenBeats(on) {
  const f = on ? window.addEventListener : window.removeEventListener
  f.call(window, 'pointermove', onBeatMove)
  f.call(window, 'pointerup', onBeatUp)
  f.call(window, 'pointercancel', stopBeatDrag)
}
function onBeatDown(e, b) {
  if (e.button) return
  e.preventDefault()
  e.stopPropagation()
  editing.value = copyGrid()
  Object.assign(beatDrag, { index: b.i, x0: e.clientX, t0: b.t, all: e.shiftKey, moved: false })
  listenBeats(true)
}
function onBeatMove(e) {
  if (beatDrag.index < 0 || !editing.value) return
  if (!beatDrag.moved && Math.abs(e.clientX - beatDrag.x0) < 2) return
  beatDrag.moved = true
  const r = box.value.getBoundingClientRect()
  const dt = ((e.clientX - beatDrag.x0) / Math.max(1, r.width)) * span.value
  const g = copyGrid()
  const i = beatDrag.index
  const beats = g.beats
  if (beatDrag.all) {
    // con Mayús, este y todos los de detrás, juntos
    const prev = i > 0 ? beats[i - 1] + 0.02 : 0
    const shift = Math.max(prev - props.grid.beats[i], dt)
    for (let k = i; k < beats.length; k++) beats[k] = round3(props.grid.beats[k] + shift)
  } else {
    const lo = i > 0 ? beats[i - 1] + 0.02 : 0
    const hi = i < beats.length - 1 ? beats[i + 1] - 0.02 : props.duration
    beats[i] = round3(clamp(beatDrag.t0 + dt, lo, hi))
  }
  editing.value = g
}
function onBeatUp() {
  const g = editing.value
  const moved = beatDrag.moved
  stopBeatDrag()
  if (g && moved) emit('update:grid', g)
}
function stopBeatDrag() {
  beatDrag.index = -1
  beatDrag.moved = false
  editing.value = null
  listenBeats(false)
}
/** Doble clic en la onda, ajustando: un pulso ahí. */
function onDouble(e) {
  if (!props.editBeats || !props.grid?.beats?.length) return
  const t = round3(timeAt(e.clientX))
  const g = copyGrid()
  const i = g.beats.findIndex((b) => b > t)
  const at = i < 0 ? g.beats.length : i
  if (g.beats[at - 1] != null && t - g.beats[at - 1] < 0.03) return
  if (g.beats[at] != null && g.beats[at] - t < 0.03) return
  g.beats.splice(at, 0, t)
  g.bars = (g.bars || []).map((b) => (b >= at ? b + 1 : b))
  emit('update:grid', g)
}
/** El clic derecho sobre un pulso: quitarlo, o hacerlo el «1». */
function onBeatMenu(e, b) {
  e.preventDefault()
  e.stopPropagation()
  const items = [
    {
      label: 'Este es el «1»',
      icon: 'note',
      action: () => {
        const g = copyGrid()
        const meter = g.meter || 4
        // desde aquí, un «1» cada compás hasta el siguiente que ya hubiera
        const next = (g.bars || []).find((x) => x > b.i) ?? g.beats.length
        const kept = (g.bars || []).filter((x) => x < b.i || x >= next)
        const fresh = []
        for (let k = b.i; k < next; k += meter) fresh.push(k)
        g.bars = [...new Set([...kept, ...fresh])].sort((x, y) => x - y)
        if (!g.bars.length || g.bars[0] > b.i) g.first_downbeat = b.i
        emit('update:grid', g)
      }
    },
    {
      label: 'Quitar este pulso',
      icon: 'close',
      danger: true,
      action: () => {
        const g = copyGrid()
        if (g.beats.length <= 2) return
        g.beats.splice(b.i, 1)
        g.bars = (g.bars || []).filter((x) => x !== b.i).map((x) => (x > b.i ? x - 1 : x))
        emit('update:grid', g)
      }
    }
  ]
  openMenu(e, items, `Pulso en ${tickLabel(b.t, 0.1)}`)
}

// ------------------------------------------------------------ los carriles de las guías
/** El carril de una guía (sin carril, o uno que ya no está: el primero). */
function laneOfCue(c) {
  const keys = props.cueLanes.map((l) => l.key)
  return keys.includes(c.lane) ? c.lane : keys[0]
}
/**
 * Lo que ocupa un bloque: lo que dura su audio (cuando ya se leyó su onda),
 * o a ojo, lo que se tarda en decirlo.
 */
const cueLen = (c) =>
  props.cueWaves[c.file]?.seconds ||
  Math.max(1.2, Math.min(6, (c.text || 'audio').length * 0.09 + 0.6))
/** La onda de una guía dentro de su bloque (una figura de 0 a 100 de ancho). */
function cueWavePath(c) {
  const p = props.cueWaves[c.file]?.peaks
  if (!p?.length) return ''
  const n = p.length
  const pts = (sign) =>
    p.map(
      (v, i) =>
        `${(((i + 0.5) / n) * 100).toFixed(2)},${(50 + sign * Math.max(1, v * 44)).toFixed(1)}`
    )
  return `M0,50 L${pts(-1).join(' L')} L100,50 L${pts(1).reverse().join(' L')} Z`
}
/**
 * Una clave para cada guía mientras sea la misma (moverla no la cambia): el
 * bloque solo aparece con su animación al ponerla o al cambiarla. Todas se
 * pintan (las de fuera de la vista quedan recortadas por el carril).
 */
const cueKeys = new WeakMap()
let lastCueKey = 0
function cueKey(c) {
  if (!cueKeys.has(c)) cueKeys.set(c, ++lastCueKey)
  return cueKeys.get(c)
}
/** Con el imán, el pulso más cercano; sin él, tal cual. */
const snapOn = computed(() => props.snapBeats.length > 0)
const snapT = (t) => (snapOn.value ? nearestBeat(props.snapBeats, t) : t)
const cuesByLane = computed(() => {
  /** @type {Record<string, any[]>} */
  const by = Object.fromEntries(props.cueLanes.map((l) => [l.key, []]))
  for (const c of props.cues) by[laneOfCue(c)]?.push(c)
  return by
})
const guideTracks = useTemplateRef('guideTracks')
/** El segundo bajo esa x, en los carriles de las guías (alineados con la onda). */
function timeAtGuides(clientX) {
  const r = guideTracks.value?.getBoundingClientRect()
  if (!r || !r.width) return timeAt(clientX)
  const f = clamp((clientX - r.left) / r.width, 0, 1)
  return clamp(view.from + f * span.value, 0, props.duration)
}
/** Una guía que se arrastra para moverla (solo con la canción parada). */
const cueDrag = reactive({ cue: null, x0: 0, t0: 0, t: 0, moved: false })
function onCueDown(e, c) {
  if (e.button) return
  e.stopPropagation()
  if (!props.cueEdit) return
  Object.assign(cueDrag, { cue: c, x0: e.clientX, t0: c.t, t: c.t, moved: false })
  window.addEventListener('pointermove', onCueMove)
  window.addEventListener('pointerup', onCueUp, { once: true })
}
function onCueMove(e) {
  if (!cueDrag.cue) return
  const dx = e.clientX - cueDrag.x0
  if (!cueDrag.moved && Math.abs(dx) < THRESHOLD) return
  cueDrag.moved = true
  const w = guideTracks.value?.getBoundingClientRect().width || 1
  const t = clamp(cueDrag.t0 + (dx / Math.max(1, w)) * span.value, 0, props.duration)
  cueDrag.t = round2(snapT(t))
}
function onCueUp() {
  window.removeEventListener('pointermove', onCueMove)
  const c = cueDrag.cue
  cueDrag.cue = null
  if (!c) return
  if (!cueDrag.moved) emit('cue', c)
  else if (cueDrag.t !== c.t) emit('move-cue', c, cueDrag.t)
}
/** Con el teclado (Intro o espacio sobre el bloque), cambiarla. */
function onCueKey(e, c) {
  if (e.detail === 0 && props.cueEdit) emit('cue', c)
}
onUnmounted(() => window.removeEventListener('pointermove', onCueMove))
/** Dónde se pinta una guía (mientras se arrastra, donde va). */
const cueAt = (c) => (cueDrag.cue === c ? cueDrag.t : c.t)
/** Doble clic en un carril: una guía nueva ahí (con la canción parada). */
function onGuideDouble(e, lane) {
  if (!props.cueEdit || !props.duration) return
  emit('add-cue', round2(snapT(timeAtGuides(e.clientX))), lane.key)
}
/** Clic derecho en un carril: sus opciones, en ese punto. */
function onGuideMenu(e, lane) {
  emit('cue-lane-menu', lane.key, e, props.duration ? round2(snapT(timeAtGuides(e.clientX))) : null)
}
/**
 * Dónde caería una guía nueva bajo el ratón (con el imán, en su pulso): una
 * raya en el carril, para ponerla sin adivinar.
 */
const ghost = reactive({ lane: '', t: 0 })
function onGuideHover(e, lane) {
  if (!props.cueEdit || cueDrag.cue || !props.duration || e.target?.closest?.('.tl-guide-clip')) {
    ghost.lane = ''
    return
  }
  ghost.lane = lane.key
  ghost.t = snapT(timeAtGuides(e.clientX))
}
/** Los pulsos que se ven en los carriles de las guías, con el imán puesto. */
const guideBeats = computed(() => {
  if (!snapOn.value || !props.duration) return []
  const all = props.snapBeats.filter((t) => t >= view.from && t <= view.to)
  if (all.length <= 160) return all.map((t) => ({ t, one: false }))
  // de lejos son demasiados: uno de cada cuatro (más o menos, cada compás)
  return all.filter((_, i) => i % 4 === 0).map((t) => ({ t, one: true }))
})
/** La guía que se acaba de buscar desde la lista (un destello para verla). */
const flashed = ref(null)
let flashTimer = null
/**
 * Llevar la vista a una guía (sin cambiar el zoom, si ya se ve, ni moverla)
 * y hacerla destellar.
 */
function focusCue(c) {
  if (!c || !props.duration) return
  const len = cueLen(c)
  if (!inView(c.t, c.t + len))
    setView(c.t - span.value * 0.25, c.t - span.value * 0.25 + span.value)
  flashed.value = null
  clearTimeout(flashTimer)
  nextTick(() => {
    flashed.value = c
    flashTimer = setTimeout(() => (flashed.value = null), 1400)
  })
}
onUnmounted(() => clearTimeout(flashTimer))

// ------------------------------------------------------------ la tira de toda la canción
const overview = useTemplateRef('overview')
let overviewDrag = false
function overviewTo(e) {
  const r = overview.value?.getBoundingClientRect()
  if (!r || !r.width || !props.duration) return
  const t = clamp((e.clientX - r.left) / r.width, 0, 1) * props.duration
  setView(t - span.value / 2, t + span.value / 2)
}
function onOverviewDown(e) {
  if (e.button) return
  overviewDrag = true
  overviewTo(e)
  window.addEventListener('pointermove', onOverviewMove)
  window.addEventListener('pointerup', onOverviewUp, { once: true })
}
function onOverviewMove(e) {
  if (overviewDrag) overviewTo(e)
}
function onOverviewUp() {
  overviewDrag = false
  window.removeEventListener('pointermove', onOverviewMove)
}
onUnmounted(onOverviewUp)

// ------------------------------------------------------------ el candado
/** Un clic que el candado paró: el candado se mueve un momento para decirlo. */
const nudged = ref(false)
let nudgeTimer = null
function nudgeLock() {
  nudged.value = false
  clearTimeout(nudgeTimer)
  nextTick(() => {
    nudged.value = true
    nudgeTimer = setTimeout(() => (nudged.value = false), 700)
  })
}
onUnmounted(() => clearTimeout(nudgeTimer))
const lockTitle = computed(() =>
  props.locked
    ? 'Onda bloqueada: mientras suena, un clic no mueve la canción y elegir un tramo no salta a él (entra cuando la canción llega). Pulsa para desbloquear'
    : 'Onda desbloqueada: un clic lleva la canción ahí y elegir un tramo salta a él. Pulsa para bloquear'
)
</script>

<template>
  <div
    class="tl"
    :class="{
      'tl-dragging': dragging,
      ['tl-hover-' + hover]: hover,
      'tl-has-lanes': headMode,
      'tl-slim': headMode && !laneMode,
      'tl-zoomed': zoomed,
      'tl-edit-beats': editBeats
    }"
    :data-mode="drag.mode || null"
    :style="{ '--tl-h': WAVE_H + 'px', '--tl-lane-h': laneHeight + 'px', '--tl-guide-h': '28px' }"
  >
    <!-- los mandos de cada pista, a la altura de su carril -->
    <div v-if="laneMode" class="tl-lanes" role="group" aria-label="Las pistas separadas">
      <div
        v-for="lane in lanes"
        :key="lane.key"
        class="tl-lane"
        :class="{ off: !lane.on }"
        :data-lane="lane.key"
      >
        <span class="tl-lane-name" :title="lane.name">{{ lane.name }}</span>
        <button
          type="button"
          class="tl-lane-btn"
          :class="{ on: lane.mute }"
          :aria-pressed="!!lane.mute"
          :aria-label="(lane.mute ? 'Que vuelva a sonar ' : 'Callar ') + lane.name"
          :title="lane.mute ? 'Callada: pulsa para que vuelva a sonar' : 'Callar ' + lane.name"
          @click="emit('lane', lane.key, { mute: !lane.mute })"
        >
          M
        </button>
        <button
          type="button"
          class="tl-lane-btn solo"
          :class="{ on: lane.solo }"
          :aria-pressed="!!lane.solo"
          :aria-label="(lane.solo ? 'Quitar el solo de ' : 'Dejar sola ') + lane.name"
          :title="
            lane.solo
              ? 'Sola (con las demás que estén en solo): pulsa para quitarlo'
              : 'Dejar sola ' + lane.name + ' (se pueden dejar varias)'
          "
          @click="emit('lane', lane.key, { solo: !lane.solo })"
        >
          S
        </button>
        <!-- doble clic: vuelve a como viene -->
        <SliderField
          class="tl-lane-gain"
          :model-value="lane.gain ?? 1"
          :min="0"
          :max="2"
          :step="0.05"
          :mark="1"
          width="100%"
          :aria-label="'Volumen de ' + lane.name"
          :value-text="pctText(lane.gain ?? 1)"
          :title="'Volumen: ' + pctText(lane.gain ?? 1) + ' · doble clic, como viene'"
          @update:model-value="(v) => emit('lane', lane.key, { gain: v })"
          @dblclick="emit('lane', lane.key, { gain: 1 })"
        />
        <SliderField
          class="tl-lane-pan"
          :model-value="lane.pan ?? 0"
          :min="-1"
          :max="1"
          :step="0.05"
          width="100%"
          :aria-label="'Panorama de ' + lane.name"
          :value-text="panText(lane.pan ?? 0)"
          :title="'Panorama: ' + panText(lane.pan ?? 0) + ' · doble clic, al centro'"
          @update:model-value="(v) => emit('lane', lane.key, { pan: v })"
          @dblclick="emit('lane', lane.key, { pan: 0 })"
        />
      </div>
    </div>
    <!-- sin pistas separadas pero con carriles de guías: la onda lleva su
         cabecera, como cualquier pista de un editor -->
    <div v-if="!laneMode && showGuides" class="tl-song-head">
      <Icon n="music" :t="12" /><span>Canción</span>
    </div>
    <!-- en el celular, «varios» y el candado van en su fila, encima de la
         onda y no sobre ella -->
    <div v-if="isMobileApp" class="tl-tools">
      <button
        type="button"
        class="tl-tool"
        :class="{ on: multi }"
        :aria-pressed="multi"
        @click="emit('update:multi', !multi)"
      >
        <Icon n="plus" :t="14" /> Varios tramos
      </button>
      <button
        type="button"
        class="tl-tool"
        :class="{ on: locked, nudged }"
        :aria-pressed="locked"
        @click="emit('update:locked', !locked)"
      >
        <Icon :n="locked ? 'lockClosed' : 'lockOpen'" :t="14" />
        {{ locked ? 'Onda bloqueada' : 'Bloquear la onda' }}
      </button>
    </div>
    <!-- el clic derecho es un atajo de raton: con el teclado, las mismas
         opciones estan en el boton ⋯ de cada tramo -->
    <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
    <div
      ref="box"
      class="tl-box"
      :title="
        !duration
          ? ''
          : editBeats
            ? 'Arrastra un pulso para moverlo (con Mayús, también los de detrás) · doble clic: un pulso nuevo · clic derecho sobre uno: quitarlo o hacerlo el «1»'
            : multi
              ? 'Arrastra para añadir otro tramo · clic sobre un tramo para quitarlo · Ctrl + rueda: zoom · Mayús + rueda: recorrer · clic derecho: opciones'
              : locked && playing
                ? 'Arrastra para elegir el tramo (la canción sigue donde va) · clic para quitarlo · Ctrl + rueda: zoom · Mayús + rueda: recorrer · clic derecho: opciones'
                : 'Arrastra para elegir el tramo · clic para ir a un punto · Ctrl + rueda: zoom · Mayús + rueda: recorrer · clic derecho: opciones'
      "
      @pointerdown="onDown"
      @pointermove="onHover"
      @pointerleave="hover = ''"
      @contextmenu.prevent="onContext"
      @wheel="onWheel"
      @dblclick="onDouble"
    >
      <canvas ref="canvas" class="tl-wave" :height="WAVE_H"></canvas>
      <!-- en el celular los mandos van debajo de la onda, no a su lado: el
           nombre de cada pista va sobre su carril para no perderse -->
      <div v-if="laneMode && isMobileApp" class="tl-lane-tags" aria-hidden="true">
        <span
          v-for="(lane, i) in lanes"
          :key="lane.key"
          :class="{ off: !lane.on }"
          :style="{ top: i * laneHeight + 'px', height: laneHeight + 'px' }"
          ><b>{{ lane.name }}</b></span
        >
      </div>
      <Transition name="tl-click">
        <div
          v-if="clickLoading != null"
          class="tl-click-load"
          role="progressbar"
          aria-label="Cargando el clic de esta canción"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-valuenow="Math.round(clickLoading * 100)"
        >
          <!-- lo que falta, velado; el borde que avanza, del color del tema -->
          <div class="tl-click-veil" :style="{ left: Math.round(clickLoading * 100) + '%' }"></div>
          <div class="tl-click-edge" :style="{ left: Math.round(clickLoading * 100) + '%' }"></div>
          <span class="tl-click-text mono"
            >cargando el clic · {{ Math.round(clickLoading * 100) }} %</span
          >
        </div>
      </Transition>
      <div v-if="loading" class="tl-note">leyendo la onda…</div>
      <div v-else-if="failed && songId" class="tl-note">sin forma de onda: {{ failed }}</div>
      <div v-else-if="tooManyBeats" class="tl-note">
        acerca la onda (Ctrl + rueda) para ajustar los pulsos
      </div>

      <!-- fuera de los tramos se atenua; cada tramo lleva sus dos asas, su
           numero (con varios) y su boton de opciones -->
      <div v-for="(g, i) in gaps" :key="'g' + i" class="tl-shade" :style="g"></div>
      <div
        v-for="([a, b], i) in shown"
        v-show="inView(a, b)"
        :key="'s' + i"
        class="tl-loop"
        :class="{ editing: isEditing(i) }"
        :style="{ left: pct(a), width: pctLen(b - a) }"
      >
        <span class="tl-handle a"></span>
        <span class="tl-handle b"></span>
        <span v-if="shown.length > 1" class="tl-loop-n mono">{{ i + 1 }}</span>
        <template v-if="isEditing(i)">
          <span class="tl-time a mono">{{ formatTime(a) }}</span>
          <span class="tl-time b mono">{{ formatTime(b) }}</span>
        </template>
        <button
          v-else-if="!dragging && i < saved.length"
          type="button"
          class="tl-loop-menu"
          :title="'Opciones del tramo ' + formatTime(a) + ' – ' + formatTime(b)"
          :aria-label="'Opciones del tramo ' + (i + 1)"
          @pointerdown.stop
          @click.stop="optionsOf($event, i)"
        >
          ⋯
        </button>
      </div>

      <!-- los marcadores: un tramo se ve como banda, un instante como raya;
           la banderita con el nombre es lo que se pulsa -->
      <template v-for="m in markers" :key="m.t + ':' + (m.end || 0)">
        <div
          v-for="([pa, pb], k) in partsOf(m)"
          v-show="inView(pa, pb)"
          :key="k"
          class="tl-region"
          :class="{ on: m === selected }"
          :style="{ left: pct(pa), width: pctLen(pb - pa) }"
        ></div>
        <button
          v-show="inView(m.t)"
          type="button"
          class="tl-marker"
          :class="{ on: m === selected, span: m.end > m.t }"
          :style="{ left: pct(m.t) }"
          :title="
            m.label + ' · ' + formatTime(m.t) + (m.end > m.t ? ' – ' + formatTime(m.end) : '')
          "
          @pointerdown.stop
          @click.stop="emit('marker', m)"
        >
          <span class="tl-marker-name">{{ m.label }}</span>
        </button>
      </template>

      <!-- las guías habladas: un micrófono donde suenan (en el escritorio van
           en sus carriles, debajo) -->
      <button
        v-for="c in showGuides ? [] : cues"
        v-show="inView(c.t)"
        :key="'cue' + c.t + c.file"
        type="button"
        class="tl-cue"
        :style="{ left: pct(c.t) }"
        :title="'Guía: «' + (c.text || 'tu voz') + '» · ' + formatTime(c.t)"
        @pointerdown.stop
        @click.stop="emit('cue', c)"
      >
        <Icon n="mic" :t="10" /><span class="tl-cue-text">{{ c.text || 'voz' }}</span>
      </button>

      <!-- los pulsos, para ajustarlos a mano (con el ratón: el teclado no
           tiene donde coger un pulso) -->
      <template v-if="editBeats">
        <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
        <span
          v-for="b in shownBeats"
          :key="'b' + b.i"
          class="tl-beat"
          :class="{ one: b.one, moving: beatDrag.index === b.i }"
          :style="{ left: pct(b.t) }"
          :title="(b.one ? '«1» · ' : '') + tickLabel(b.t, 0.1)"
          @pointerdown="onBeatDown($event, b)"
          @contextmenu="onBeatMenu($event, b)"
        ></span>
      </template>

      <div
        v-if="duration && inView(position)"
        class="tl-head"
        :style="{ left: pct(position) }"
      ></div>

      <!-- varios tramos a la vez: cada arrastre añade otro -->
      <button
        v-if="!isMobileApp"
        type="button"
        class="tl-multi"
        :class="{ on: multi }"
        :aria-pressed="multi"
        :title="
          multi
            ? 'Varios tramos: cada arrastre añade otro y suenan seguidos. Pulsa para volver a uno solo (se queda el último que tocaste)'
            : 'Un solo tramo. Pulsa para elegir varios y tocarlos seguidos, saltándose lo de en medio'
        "
        @pointerdown.stop
        @click.stop="emit('update:multi', !multi)"
      >
        <Icon n="plus" :t="10" /> varios
      </button>

      <!-- el candado: puesto, mientras suena la onda no mueve la canción -->
      <button
        v-if="!isMobileApp"
        type="button"
        class="tl-lock"
        :class="{ on: locked, nudged }"
        :aria-pressed="locked"
        :aria-label="locked ? 'Desbloquear la onda' : 'Bloquear la onda'"
        :title="lockTitle"
        @pointerdown.stop
        @click.stop="emit('update:locked', !locked)"
      >
        <Icon :n="locked ? 'lockClosed' : 'lockOpen'" :t="12" />
      </button>
    </div>

    <!-- los carriles de las guías: sus mandos a la izquierda, sus bloques
         debajo de la onda y a su misma escala -->
    <div
      v-if="showGuides"
      class="tl-guide-heads"
      role="group"
      aria-label="Los carriles de las guías"
    >
      <div
        v-for="lane in cueLanes"
        :key="lane.key"
        class="tl-lane tl-guide-lane"
        :class="{ off: lane.mute }"
      >
        <button
          type="button"
          class="tl-lane-name tl-guide-name"
          :title="'«' + (lane.name || 'Guías') + '» · pulsa para cambiarle el nombre'"
          @click="emit('rename-cue-lane', lane.key)"
        >
          {{ lane.name || 'Guías' }}
        </button>
        <button
          type="button"
          class="tl-lane-btn"
          :class="{ on: lane.mute }"
          :aria-pressed="!!lane.mute"
          :aria-label="
            (lane.mute ? 'Que vuelvan a sonar las guías de ' : 'Callar las guías de ') +
            (lane.name || 'Guías')
          "
          :title="lane.mute ? 'Callado: pulsa para que vuelva a sonar' : 'Callar este carril'"
          @click="emit('cue-lane', lane.key, { mute: !lane.mute })"
        >
          M
        </button>
        <button
          type="button"
          class="tl-lane-btn solo"
          :class="{ on: lane.solo }"
          :aria-pressed="!!lane.solo"
          :aria-label="(lane.solo ? 'Quitar el solo de ' : 'Dejar solo ') + (lane.name || 'Guías')"
          :title="lane.solo ? 'Solo: pulsa para quitarlo' : 'Dejar solo este carril de guías'"
          @click="emit('cue-lane', lane.key, { solo: !lane.solo })"
        >
          S
        </button>
        <SliderField
          class="tl-lane-gain"
          :model-value="lane.gain ?? 1"
          :min="0"
          :max="2"
          :step="0.05"
          :mark="1"
          width="100%"
          :aria-label="'Volumen de ' + (lane.name || 'Guías')"
          :value-text="pctText(lane.gain ?? 1)"
          :title="'Volumen: ' + pctText(lane.gain ?? 1) + ' · doble clic, como viene'"
          @update:model-value="(v) => emit('cue-lane', lane.key, { gain: v })"
          @dblclick="emit('cue-lane', lane.key, { gain: 1 })"
        />
        <button
          type="button"
          class="tl-lane-btn tl-lane-more"
          :aria-label="'Opciones de ' + (lane.name || 'Guías')"
          title="Una guía aquí, importar un audio, renombrar o quitar el carril"
          @click="emit('cue-lane-menu', lane.key, $event, null)"
        >
          <Icon n="more" :t="10" />
        </button>
      </div>
      <button
        type="button"
        class="tl-guide-add"
        :disabled="cueLanes.length >= MAX_CUE_LANES"
        :aria-label="'Otro carril de guías'"
        :title="
          cueLanes.length >= MAX_CUE_LANES
            ? 'Como mucho ' + MAX_CUE_LANES + ' carriles de guías'
            : 'Otro carril de guías (hasta ' +
              MAX_CUE_LANES +
              '): cada uno con su volumen y su salida, para la banda, el baterista, los coros…'
        "
        @click="emit('add-cue-lane')"
      >
        <Icon n="plus" :t="10" /> Carril
      </button>
    </div>
    <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
    <div
      v-if="showGuides"
      ref="guideTracks"
      class="tl-guide-tracks"
      :class="{ editable: cueEdit, snapping: snapOn }"
      @wheel="onWheel"
      @pointerleave="ghost.lane = ''"
    >
      <!-- con el imán, los pulsos a los que se pegan -->
      <span
        v-for="b in guideBeats"
        :key="'gb' + b.t"
        class="tl-guide-beat"
        :class="{ one: b.one }"
        :style="{ left: pct(b.t) }"
        aria-hidden="true"
      ></span>
      <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
      <div
        v-for="lane in cueLanes"
        :key="lane.key"
        class="tl-guide-track"
        :class="{ off: lane.mute }"
        :data-lane="lane.key"
        :title="
          cueEdit
            ? 'Doble clic: una guía aquí · arrastra una guía para moverla · púlsala para cambiarla · clic derecho: opciones'
            : 'Para la canción para poner, mover o cambiar guías'
        "
        @dblclick="onGuideDouble($event, lane)"
        @pointermove="onGuideHover($event, lane)"
        @contextmenu.prevent="onGuideMenu($event, lane)"
      >
        <span
          v-if="ghost.lane === lane.key"
          class="tl-guide-ghost"
          :style="{ left: pct(ghost.t) }"
          aria-hidden="true"
        ></span>
        <TransitionGroup name="tl-clip">
          <button
            v-for="c in cuesByLane[lane.key]"
            :key="cueKey(c)"
            type="button"
            class="tl-guide-clip"
            :class="{
              moving: cueDrag.cue === c,
              flash: flashed === c,
              spoken: !!c.text,
              waved: !!cueWaves[c.file]?.peaks?.length
            }"
            :style="{ left: pct(cueAt(c)), width: pctLen(cueLen(c)) }"
            :title="
              (c.text ? 'Guía: «' + c.text + '»' : 'Guía grabada o importada') +
              ' · ' +
              formatTime(cueAt(c)) +
              (cueWaves[c.file]?.seconds
                ? ' · dura ' + cueWaves[c.file].seconds.toFixed(1) + ' s'
                : '')
            "
            @pointerdown="onCueDown($event, c)"
            @click="onCueKey($event, c)"
            @dblclick.stop
            @contextmenu.prevent.stop="emit('cue-menu', c, $event)"
          >
            <svg
              v-if="cueWaves[c.file]?.peaks?.length"
              class="tl-guide-wave"
              viewBox="0 0 100 100"
              preserveAspectRatio="none"
              aria-hidden="true"
            >
              <path :d="cueWavePath(c)" />
            </svg>
            <Icon :n="c.text ? 'speech' : 'mic'" :t="9" /><span>{{ c.text || 'audio' }}</span>
          </button>
        </TransitionGroup>
      </div>
      <span
        v-show="inView(position)"
        class="tl-guide-needle"
        :style="{ left: pct(position) }"
        aria-hidden="true"
      ></span>
    </div>

    <!-- la regla: los minutos (de cerca, con décimas) -->
    <div v-if="headMode" class="tl-ruler-gap" aria-hidden="true"></div>
    <div class="tl-ruler" aria-hidden="true">
      <span
        v-for="k in ticks"
        :key="k.t"
        class="tl-tick"
        :class="{ minor: !k.major }"
        :style="{ left: pct(k.t) }"
      >
        <span v-if="k.label" class="tl-tick-label mono">{{ k.label }}</span>
      </span>
      <span v-if="duration && !zoomed" class="tl-tick end"
        ><span class="tl-tick-label mono">{{ formatTime(duration) }}</span></span
      >
    </div>
    <!-- acercada: toda la canción, con el trozo que se ve; se pincha o se
         arrastra para ir a otro -->
    <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
    <div
      v-if="zoomed"
      ref="overview"
      class="tl-overview"
      title="Toda la canción: pincha o arrastra para ver otro trozo · doble clic, toda"
      @pointerdown="onOverviewDown"
      @dblclick="zoomFit"
    >
      <span
        class="tl-overview-view"
        :style="{
          left: (view.from / duration) * 100 + '%',
          width: (span / duration) * 100 + '%'
        }"
      ></span>
      <span class="tl-overview-head" :style="{ left: (position / duration) * 100 + '%' }"></span>
    </div>
  </div>
</template>
