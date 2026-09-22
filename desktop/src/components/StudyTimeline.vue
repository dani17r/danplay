<script setup>
/**
 * La línea de tiempo del modo estudio: la forma de onda de la canción, una
 * regla con los minutos, el tramo del bucle y la aguja.
 *
 * El tramo se elige sobre la propia onda: se arrastra de donde a donde, y
 * listo. Los bordes del tramo se cogen y se mueven; el tramo entero también,
 * sin cambiar lo que dura. Un clic sin arrastrar lleva la canción a ese
 * punto. Nada de botones de inicio y fin.
 *
 * La onda la pinta un canvas y solo se vuelve a pintar cuando cambian los
 * datos, el ancho o el tema. Lo que se mueve (aguja, tramo, marcadores) es
 * DOM colocado en tantos por ciento: cuatro veces por segundo no hay que
 * redibujar nada.
 */
import { ref, reactive, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { api } from '../api.js'
import { usePreferences } from '../composables/usePreferences.js'
import { formatTime } from '../utils/format.js'

const props = defineProps({
  songId: { type: Number, default: null },
  duration: { type: Number, default: 0 },
  position: { type: Number, default: 0 },
  /** [a, b] en segundos, o null */
  loop: { type: Array, default: null },
  /** [{t, end?, label, notes?}]: tramos con nombre, o instantes sueltos */
  markers: { type: Array, default: () => [] },
  /** el marcador elegido, para resaltarlo */
  selected: { type: Object, default: null },
  /** la rejilla de pulsos del metrónomo, para pintarla sobre la onda */
  grid: { type: Object, default: null },
  /** alto de la onda en px */
  height: { type: Number, default: 56 }
})
// `update:loop` lleva el tramo y `{ mode }`: 'select' si es uno nuevo dibujado
// de cero, 'edit' si se movio un borde o el tramo entero. Quien guarda
// marcadores necesita distinguirlo: editar el tramo de un marcador elegido lo
// cambia a el; dibujar otro nuevo, no.
const emit = defineEmits(['update:loop', 'seek', 'marker'])

const WAVE_H = computed(() => props.height) // alto de la onda, en px
const MIN_LOOP = 0.5 // menos que esto no es un bucle, es un clic con temblor
const HANDLE = 7 // a estos px de un borde se coge el borde, no se empieza otro tramo
const THRESHOLD = 4 // px de movimiento a partir de los que un clic pasa a ser arrastre
const SNAP = 6 // px: cerca de un marcador, el borde se pega a él

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))
const round2 = (v) => Math.round(v * 100) / 100

// ------------------------------------------------------------ la onda
const box = ref(null)
const canvas = ref(null)
const wave = ref(null) // {peaks, rms} o null
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

/** La rejilla del metrónomo: una raya por pulso abajo; el «1», más alta. */
function drawGrid(ctx, W, H) {
  const g = props.grid
  if (!g || !g.beats?.length || !props.duration) return
  const styles = getComputedStyle(canvas.value)
  const text = styles.getPropertyValue('--text').trim() || styles.color || 'gray'
  ctx.fillStyle = text
  const m = Math.max(1, g.meter || 4)
  for (let i = 0; i < g.beats.length; i++) {
    const x = Math.round((g.beats[i] / props.duration) * W)
    if (x < 0 || x > W) continue
    const one = ((i - g.first_downbeat) % m + m) % m === 0
    // el «1» cruza la onda entera, tenue; los demas pulsos son marcas abajo
    ctx.globalAlpha = one ? 0.28 : 0.45
    ctx.fillRect(x, one ? 0 : H - 7, 1, one ? H : 7)
  }
  ctx.globalAlpha = 1
}
watch(() => props.grid, () => nextTick(draw))

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
const MINOR = { 1: 0, 2: 1, 5: 1, 10: 5, 15: 5, 30: 10, 60: 30, 120: 60, 300: 60, 600: 300, 1200: 600 }
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

// ------------------------------------------------------------ el tramo
// 'select' arrastra un tramo nuevo; 'a' y 'b' mueven un borde; 'move' lleva
// el tramo entero. Hasta pasar del umbral no es nada: sera un clic.
const drag = reactive({ mode: null, moved: false, startX: 0, from: 0, a: 0, b: 0, a0: 0, b0: 0 })

/** El tramo que se ve: el que se esta arrastrando, o el guardado. */
const shown = computed(() => {
  if (drag.mode && drag.moved) {
    if (drag.mode === 'select') return [Math.min(drag.from, drag.a), Math.max(drag.from, drag.a)]
    return [drag.a, drag.b]
  }
  return props.loop && props.loop.length === 2 && props.loop[1] > props.loop[0] ? props.loop : null
})

/** Cerca del principio o del final de un marcador, el borde se pega a el. */
function snap(t) {
  for (const m of props.markers || []) {
    if (Math.abs(xOf(m.t) - xOf(t)) <= SNAP) return m.t
    if (m.end > m.t && Math.abs(xOf(m.end) - xOf(t)) <= SNAP) return m.end
  }
  return t
}

function listen(on) {
  const f = on ? window.addEventListener : window.removeEventListener
  f.call(window, 'pointermove', onMove)
  f.call(window, 'pointerup', onUp)
  f.call(window, 'pointercancel', stopDrag)
}
function onDown(e) {
  if (e.button || !props.duration) return
  const t = timeAt(e.clientX)
  let mode = 'select'
  const cur = shown.value
  if (cur) {
    const x = e.clientX - box.value.getBoundingClientRect().left
    if (Math.abs(x - xOf(cur[0])) <= HANDLE) mode = 'a'
    else if (Math.abs(x - xOf(cur[1])) <= HANDLE) mode = 'b'
    else if (x > xOf(cur[0]) && x < xOf(cur[1])) mode = 'move'
  }
  Object.assign(drag, {
    mode,
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
function onUp(e) {
  const d = { ...drag }
  stopDrag()
  if (!d.mode) return
  if (!d.moved) {
    emit('seek', round2(timeAt(e.clientX)))
    return
  }
  let a
  let b
  if (d.mode === 'select') {
    a = Math.min(d.from, d.a)
    b = Math.max(d.from, d.a)
    if (b - a < MIN_LOOP) return // demasiado corto para ser un bucle
  } else {
    a = d.a
    b = d.b
  }
  emit('update:loop', [round2(a), round2(b)], { mode: d.mode === 'select' ? 'select' : 'edit' })
}
function stopDrag() {
  drag.mode = null
  drag.moved = false
  listen(false)
}

/** Que cursor toca segun lo que hay bajo el puntero (sin arrastre en marcha). */
const hover = ref('')
function onHover(e) {
  if (drag.mode || !props.duration || !shown.value) {
    hover.value = ''
    return
  }
  const x = e.clientX - box.value.getBoundingClientRect().left
  const [a, b] = shown.value
  if (Math.abs(x - xOf(a)) <= HANDLE || Math.abs(x - xOf(b)) <= HANDLE) hover.value = 'edge'
  else if (x > xOf(a) && x < xOf(b)) hover.value = 'inside'
  else hover.value = ''
}

const dragging = computed(() => !!(drag.mode && drag.moved))
</script>

<template>
  <div class="tl" :class="{ 'tl-dragging': dragging, ['tl-hover-' + hover]: hover }"
       :data-mode="drag.mode || null" :style="{ '--tl-h': WAVE_H + 'px' }">
    <div ref="box" class="tl-box" :title="duration ? 'Arrastra para elegir el tramo que se repite · clic para ir a un punto' : ''"
         @pointerdown="onDown" @pointermove="onHover" @pointerleave="hover = ''">
      <canvas ref="canvas" class="tl-wave" :height="WAVE_H"></canvas>
      <div v-if="loading" class="tl-note">leyendo la onda…</div>
      <div v-else-if="failed && songId" class="tl-note">sin forma de onda (hace falta ffmpeg)</div>

      <!-- fuera del tramo se atenua; el tramo lleva sus dos asas -->
      <template v-if="shown">
        <div class="tl-shade" :style="{ left: 0, width: pct(shown[0]) }"></div>
        <div class="tl-shade" :style="{ left: pct(shown[1]), right: 0 }"></div>
        <div class="tl-loop" :style="{ left: pct(shown[0]), width: pct(shown[1] - shown[0]) }">
          <span class="tl-handle a"></span>
          <span class="tl-handle b"></span>
          <span v-if="dragging" class="tl-time a mono">{{ formatTime(shown[0]) }}</span>
          <span v-if="dragging" class="tl-time b mono">{{ formatTime(shown[1]) }}</span>
        </div>
      </template>

      <!-- los marcadores: un tramo se ve como banda (que no estorba al
           puntero: por encima se sigue pudiendo arrastrar), un instante como
           raya; la banderita con el nombre es lo que se pulsa -->
      <template v-for="m in markers" :key="m.t + ':' + (m.end || 0)">
        <div v-if="m.end > m.t" class="tl-region" :class="{ on: m === selected }"
             :style="{ left: pct(m.t), width: pct(m.end - m.t) }"></div>
        <button type="button" class="tl-marker" :class="{ on: m === selected, span: m.end > m.t }"
                :style="{ left: pct(m.t) }"
                :title="m.label + ' · ' + formatTime(m.t) + (m.end > m.t ? ' – ' + formatTime(m.end) : '')"
                @pointerdown.stop @click.stop="emit('marker', m)">
          <span class="tl-marker-name">{{ m.label }}</span>
        </button>
      </template>

      <div v-if="duration" class="tl-head" :style="{ left: pct(position) }"></div>
    </div>

    <!-- la regla: los minutos -->
    <div class="tl-ruler" aria-hidden="true">
      <span v-for="k in ticks" :key="k.t" class="tl-tick" :class="{ minor: !k.major }" :style="{ left: pct(k.t) }">
        <span v-if="k.label" class="tl-tick-label mono">{{ k.label }}</span>
      </span>
      <span v-if="duration" class="tl-tick end"><span class="tl-tick-label mono">{{ formatTime(duration) }}</span></span>
    </div>
  </div>
</template>
