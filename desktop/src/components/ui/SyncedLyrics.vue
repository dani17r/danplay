<script setup>
/**
 * La letra siguiendo a la canción: la línea que suena resaltada y a la
 * vista, y un clic en cualquier otra salta ahí. Si la canción de la ficha no
 * es la que suena, se ven las líneas sin resaltar (y pulsar no salta).
 *
 * Sonando, cada línea es un botón; con el teclado la letra es una sola
 * parada del tabulador (la línea que suena) y las flechas pasan de una a
 * otra.
 *
 * Para tener la línea a la vista se mueve SOLO la caja de la letra. Antes
 * se usaba `scrollIntoView`, que mueve también todo lo que la contiene: en
 * WebKitGTK (la app en Linux) las primeras líneas no desplazaban la letra
 * sino la ficha entera, que iba subiendo con la portada y los acordes
 * mientras la letra se quedaba pegada en medio sin avanzar; y con las dos
 * cosas desplazándose a la vez se llegaba a ver letra sobre letra.
 */
import { computed, watch, nextTick, onMounted, useTemplateRef } from 'vue'
import { currentLine } from '../../utils/lrc.js'
import { formatTime } from '../../utils/format.js'

const props = defineProps({
  lines: { type: Array, required: true }, // [{t, text}] de utils/lrc.js
  position: { type: Number, default: 0 },
  active: Boolean // esta cancion es la que suena
})
const emit = defineEmits(['seek'])
const root = useTemplateRef('root')
const current = computed(() => (props.active ? currentLine(props.lines, props.position) : -1))
const stop = computed(() => Math.max(0, current.value))

/** Dónde queda la línea que suena: algo por encima de la mitad, para ver las que vienen. */
const LEAD = 0.4
/** Tras mover la letra a mano (rueda, dedo, teclado), se deja estar este rato. */
const HOLD_MS = 4000
let heldUntil = 0
function hold() {
  heldUntil = Date.now() + HOLD_MS
}

/** Desplaza la caja (y nada más) para que la línea `i` quede a la vista. */
function follow(i, smooth = true) {
  const box = root.value
  const el = box?.children?.[i]
  if (!box || !el || typeof el.getBoundingClientRect !== 'function') return
  const inBox = el.getBoundingClientRect().top - box.getBoundingClientRect().top
  const wanted = box.scrollTop + inBox + el.offsetHeight / 2 - box.clientHeight * LEAD
  const top = Math.round(Math.max(0, Math.min(box.scrollHeight - box.clientHeight, wanted)))
  if (Math.abs(top - box.scrollTop) < 2) return
  if (typeof box.scrollTo === 'function') {
    box.scrollTo({ top, behavior: smooth ? 'smooth' : 'instant' })
  } else {
    box.scrollTop = top
  }
}

watch(current, async (i) => {
  if (i < 0 || Date.now() < heldUntil) return
  await nextTick()
  follow(i)
})
// al abrir la ficha con la canción ya sonando, la línea se pone a la vista
// de golpe: no hay nada que animar
onMounted(() => {
  if (current.value >= 0) follow(current.value, false)
})
watch(
  () => props.active,
  async (on) => {
    if (!on || current.value < 0) return
    await nextTick()
    follow(current.value, false)
  }
)

function onKey(e, i) {
  const to = e.key === 'ArrowDown' ? i + 1 : e.key === 'ArrowUp' ? i - 1 : -1
  const el = root.value?.children?.[to]
  if (!el) return
  e.preventDefault()
  e.stopPropagation()
  hold()
  // sin que el foco desplace la ficha: la letra se mueve sola, dentro
  el.focus({ preventScroll: true })
  follow(to)
}
</script>

<template>
  <div
    v-if="active"
    ref="root"
    class="lrc active"
    role="group"
    aria-label="Letra: pulsa una línea para ir ahí"
    @wheel.passive="hold"
    @touchmove.passive="hold"
    @pointerdown.self="hold"
  >
    <button
      v-for="(l, i) in lines"
      :key="i"
      type="button"
      class="lrc-line"
      :class="{ current: i === current, past: current >= 0 && i < current, blank: !l.text }"
      :tabindex="i === stop ? 0 : -1"
      :title="'Ir a ' + formatTime(l.t)"
      :aria-current="i === current ? 'true' : undefined"
      @click="emit('seek', l.t)"
      @keydown="onKey($event, i)"
    >
      {{ l.text || '♪' }}
    </button>
  </div>
  <div v-else ref="root" class="lrc">
    <div
      v-for="(l, i) in lines"
      :key="i"
      class="lrc-line"
      :class="{ blank: !l.text }"
      :title="formatTime(l.t)"
    >
      {{ l.text || '♪' }}
    </div>
  </div>
</template>
