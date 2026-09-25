<script setup>
/**
 * La letra siguiendo a la canción: la línea que suena resaltada y a la
 * vista, y un clic en cualquier otra salta ahí. Si la canción de la ficha no
 * es la que suena, se ven las líneas sin resaltar (y pulsar no salta).
 *
 * Sonando, cada línea es un botón; con el teclado la letra es una sola
 * parada del tabulador (la línea que suena) y las flechas pasan de una a
 * otra.
 */
import { computed, watch, nextTick, useTemplateRef } from 'vue'
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

// la linea que suena se mantiene a la vista, sin sacudir el panel entero
watch(current, async (i) => {
  if (i < 0) return
  await nextTick()
  const el = root.value?.children?.[i]
  if (el && typeof el.scrollIntoView === 'function')
    el.scrollIntoView({ block: 'center', behavior: 'smooth' })
})

function onKey(e, i) {
  const to = e.key === 'ArrowDown' ? i + 1 : e.key === 'ArrowUp' ? i - 1 : -1
  const el = root.value?.children?.[to]
  if (!el) return
  e.preventDefault()
  e.stopPropagation()
  el.focus()
}
</script>

<template>
  <div
    v-if="active"
    ref="root"
    class="lrc active"
    role="group"
    aria-label="Letra: pulsa una línea para ir ahí"
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
