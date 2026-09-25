<script setup>
/**
 * La línea de tiempo del modo estudio: la forma de onda de la canción, una
 * regla con los minutos, el tramo del bucle y la aguja.
 *
 * El tramo se elige sobre la propia onda: se arrastra de donde a donde, y
 * listo. Los bordes del tramo se cogen y se mueven; el tramo entero también,
 * sin cambiar lo que dura. Un clic sin arrastrar quita la selección y lleva
 * la canción a ese punto. Nada de botones de inicio y fin.
 *
 * Con «varios» puesto, cada arrastre añade otro tramo en vez de sustituir el
 * que hubiera, y un clic sobre un tramo lo quita: se repiten todos seguidos,
 * saltándose lo de entre medias. Cada tramo lleva un botón de opciones (y el
 * clic derecho): quien las ofrece es StudyBar.
 *
 * Con el candado (arriba a la derecha, puesto de entrada), mientras suena un
 * clic no mueve la canción: tocando encima de la grabación, un clic sin
 * querer la mandaba a otro sitio. Elegir un tramo sí se puede; quien lo
 * recibe decide no saltar a él (StudyBar).
 *
 * La onda la pinta un canvas y solo se vuelve a pintar cuando cambian los
 * datos, el ancho o el tema. Lo que se mueve (aguja, tramo, marcadores) es
 * DOM colocado en tantos por ciento: cuatro veces por segundo no hay que
 * redibujar nada.
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
import { api } from '../api.js'
import { usePreferences } from '../composables/usePreferences.js'
import { formatTime } from '../utils/format.js'
import { isDownbeat } from '../utils/beats.js'
import { cleanSegments } from '../utils/segments.js'
import Icon from './Icon.vue'

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
  /** el marcador elegido, para resaltarlo */
  selected: { type: Object, default: null },
  /** la rejilla de pulsos del metrónomo tal como suena, para pintarla sobre la onda */
  grid: { type: Object, default: null },
  /** alto de la onda en px */
  height: { type: Number, default: 56 },
  /** el candado: sonando, un clic no mueve la canción */
  locked: Boolean,
  /** si la canción está sonando (el candado solo cuenta entonces) */
  playing: Boolean
})
// `update:loops` lleva los tramos (en orden, los que se pisan juntos) y
// `{ mode }`: 'select' si se dibujo uno de cero, 'add' si se anadio otro,
// 'edit' si se movio un borde o un tramo entero, 'remove' si se quito uno y
// 'clear' si se quitaron todos. Quien guarda marcadores necesita
// distinguirlo: editar el tramo de un marcador elegido lo cambia a el;
// dibujar otro nuevo, no. `options` pide las opciones de un tramo (o de un
// punto de la onda, con `index` -1), con donde abrirlas.
const emit = defineEmits([
  'update:loops',
  'seek',
  'marker',
  'update:locked',
  'update:multi',
  'options'
])

const WAVE_H = computed(() => props.height) // alto de la onda, en px
const MIN_LOOP = 0.5 // menos que esto no es un bucle, es un clic con temblor
const HANDLE = 7 // a estos px de un borde se coge el borde, no se empieza otro tramo
const THRESHOLD = 4 // px de movimiento a partir de los que un clic pasa a ser arrastre
const SNAP = 6 // px: cerca de un marcador, el borde se pega a él

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))
const round2 = (v) => Math.round(v * 100) / 100

// ------------------------------------------------------------ la onda
const box = useTemplateRef('box')
const canvas = useTemplateRef('canvas')
// {peaks, rms} o null. Superficial: son miles de numeros que solo se leen
// para pintar; hacerlos reactivos uno a uno no aporta nada
const wave = shallowRef(null)
const loading = ref(false)
const failed = ref(false)

async function load(id) {
  wave.value = null
  failed.value = false
  if (!id) {
    draw()
    return
  }
  loading.value = true
  try {
    const r = await api.waveform(id)
    if (id === props.songId) wave.value = r
  } catch {
    if (id === props.songId) failed.value = true
  } finally {
    if (id === props.songId) loading.value = false
    await nextTick()
    draw()
  }
}
watch(() => props.songId, load, { immediate: true })

const { theme } = usePreferences()
watch(theme, () => nextTick(draw))

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
  const mid = H / 2
  const w = wave.value
  if (!w || !w.peaks?.length) {
    // sin onda (aun cargando, o no se pudo): la linea del centro, y ya
    ctx.globalAlpha = 0.25
    ctx.fillStyle = text
    ctx.fillRect(0, mid - 0.5, W, 1)
    ctx.globalAlpha = 1
    return
  }
  const n = w.peaks.length
  // En un mp3 de hoy los picos van casi todos a tope y el RMS ronda el
  // tercio: pintado tal cual, la banda de acento salia aplastada y no se
  // distinguia el verso del estribillo. El RMS se estira hasta su propio
  // maximo y a los dos se les da una curva suave, que levanta lo flojo sin
  // cambiar el orden. La banda nunca sale de la silueta de su columna.
  const rmsTop = Math.max(1e-6, ...(w.rms || [0]))
  const curve = (v) => Math.pow(Math.max(0, Math.min(1, v)), 0.7)
  // una columna cada dos pixeles: si hay mas columnas de datos se funden, y
  // si hay menos se ensanchan
  const cols = Math.max(1, Math.min(n, Math.floor(W / 2)))
  const cw = W / cols
  const bar = cw >= 3 ? cw - 1 : cw // con sitio, un pixel de aire entre barras
  const room = mid - 2
  for (let i = 0; i < cols; i++) {
    const from = Math.floor((i * n) / cols)
    const to = Math.max(from + 1, Math.floor(((i + 1) * n) / cols))
    let pk = 0
    let rm = 0
    for (let j = from; j < to; j++) {
      pk = Math.max(pk, w.peaks[j] || 0)
      rm += w.rms?.[j] || 0
    }
    rm /= to - from
    const x = i * cw
    const silhouette = curve(pk)
    const band = Math.min(silhouette, curve(rm / rmsTop))
    const hp = Math.max(1, silhouette * room)
    const hr = Math.max(1, band * room)
    ctx.globalAlpha = 0.26
    ctx.fillStyle = text
    ctx.fillRect(x, mid - hp, bar, hp * 2)
    ctx.globalAlpha = 0.95
    ctx.fillStyle = accent
    ctx.fillRect(x, mid - hr, bar, hr * 2)
  }
  ctx.globalAlpha = 1
  drawGrid(ctx, W, H)
}

/**
 * La rejilla del metrónomo: una raya por pulso abajo; el «1», más alta (sin
 * acento, ninguno).
 */
function drawGrid(ctx, W, H) {
  const g = props.grid
  if (!g || !g.beats?.length || !props.duration) return
  const styles = getComputedStyle(canvas.value)
  const text = styles.getPropertyValue('--text').trim() || styles.color || 'gray'
  ctx.fillStyle = text
  for (let i = 0; i < g.beats.length; i++) {
    const x = Math.round((g.beats[i] / props.duration) * W)
    if (x < 0 || x > W) continue
    const one = isDownbeat(g, i)
    // el «1» cruza la onda entera, tenue; los demas pulsos son marcas abajo
    ctx.globalAlpha = one ? 0.28 : 0.45
    ctx.fillRect(x, one ? 0 : H - 7, 1, one ? H : 7)
  }
  ctx.globalAlpha = 1
}
watch(
  () => props.grid,
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
  stopDrag()
})

// ------------------------------------------------------------ geometria
const pct = (t) => (props.duration ? clamp(t / props.duration, 0, 1) * 100 + '%' : '0%')
/** El segundo que cae bajo esa x de pantalla. */
function timeAt(clientX) {
  const r = box.value.getBoundingClientRect()
  const f = r.width > 0 ? clamp((clientX - r.left) / r.width, 0, 1) : 0
  return f * props.duration
}
/** Y al reves: en que x (dentro de la caja) cae ese segundo. */
function xOf(t) {
  const r = box.value.getBoundingClientRect()
  return props.duration ? (t / props.duration) * r.width : 0
}

// ------------------------------------------------------------ la regla
// Paso entre marcas con numero: el mas fino que deje ~70 px entre ellas.
const STEPS = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1200]
const MINOR = {
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
const ticks = computed(() => {
  const d = props.duration
  const W = width.value || 800
  if (!d) return []
  const step = STEPS.find((s) => (s / d) * W >= 70) || STEPS.at(-1)
  const unit = MINOR[step] || step
  const out = []
  for (let i = 0; i * unit < d; i++) {
    const t = i * unit
    const major = (i * unit) % step === 0
    // la ultima con numero no se pisa con la del final
    if (major && t > 0 && d - t < step * 0.6) continue
    out.push({ t, major, label: major ? formatTime(t) : '' })
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
    if (a > from) out.push({ left: pct(from), width: pct(a - from) })
    from = Math.max(from, b)
  }
  out.push({ left: pct(from), right: 0 }) // hasta el final
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
  emit('update:loops', cleanSegments(list), { mode: 'edit' })
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
 * con el teclado (Enter: un clic sin `detail`), debajo del boton, que es lo
 * que tiene el foco.
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
    :class="{ 'tl-dragging': dragging, ['tl-hover-' + hover]: hover }"
    :data-mode="drag.mode || null"
    :style="{ '--tl-h': WAVE_H + 'px' }"
  >
    <!-- el clic derecho es un atajo de raton: con el teclado, las mismas
         opciones estan en el boton ⋯ de cada tramo -->
    <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
    <div
      ref="box"
      class="tl-box"
      :title="
        !duration
          ? ''
          : multi
            ? 'Arrastra para añadir otro tramo · clic sobre un tramo para quitarlo · clic derecho: opciones'
            : locked && playing
              ? 'Arrastra para elegir el tramo que se repite (la canción sigue donde va) · clic para quitarlo · clic derecho: opciones'
              : 'Arrastra para elegir el tramo que se repite · clic para ir a un punto · clic derecho: opciones'
      "
      @pointerdown="onDown"
      @pointermove="onHover"
      @pointerleave="hover = ''"
      @contextmenu.prevent="onContext"
    >
      <canvas ref="canvas" class="tl-wave" :height="WAVE_H"></canvas>
      <div v-if="loading" class="tl-note">leyendo la onda…</div>
      <div v-else-if="failed && songId" class="tl-note">sin forma de onda (hace falta ffmpeg)</div>

      <!-- fuera de los tramos se atenua; cada tramo lleva sus dos asas, su
           numero (con varios) y su boton de opciones -->
      <div v-for="(g, i) in gaps" :key="'g' + i" class="tl-shade" :style="g"></div>
      <div
        v-for="([a, b], i) in shown"
        :key="'s' + i"
        class="tl-loop"
        :class="{ editing: isEditing(i) }"
        :style="{ left: pct(a), width: pct(b - a) }"
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

      <!-- los marcadores: un tramo se ve como banda (que no estorba al
           puntero: por encima se sigue pudiendo arrastrar), un instante como
           raya; la banderita con el nombre es lo que se pulsa -->
      <template v-for="m in markers" :key="m.t + ':' + (m.end || 0)">
        <div
          v-for="([pa, pb], k) in partsOf(m)"
          :key="k"
          class="tl-region"
          :class="{ on: m === selected }"
          :style="{ left: pct(pa), width: pct(pb - pa) }"
        ></div>
        <button
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

      <div v-if="duration" class="tl-head" :style="{ left: pct(position) }"></div>

      <!-- varios tramos a la vez: cada arrastre añade otro -->
      <button
        type="button"
        class="tl-multi"
        :class="{ on: multi }"
        :aria-pressed="multi"
        :title="
          multi
            ? 'Varios tramos: cada arrastre añade otro y se repiten seguidos. Pulsa para volver a uno solo'
            : 'Un solo tramo. Pulsa para elegir varios y repetirlos seguidos, saltándose lo de en medio'
        "
        @pointerdown.stop
        @click.stop="emit('update:multi', !multi)"
      >
        <Icon n="plus" :t="10" /> varios
      </button>

      <!-- el candado: puesto, mientras suena la onda no mueve la canción -->
      <button
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

    <!-- la regla: los minutos -->
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
      <span v-if="duration" class="tl-tick end"
        ><span class="tl-tick-label mono">{{ formatTime(duration) }}</span></span
      >
    </div>
  </div>
</template>
