<script setup>
/**
 * La letra siguiendo a la canción: la línea que suena resaltada y a la
 * vista, y un clic en cualquier otra salta ahí. Si la canción de la ficha no
 * es la que suena, se ven las líneas sin resaltar (y pulsar no salta).
 */
import { ref, computed, watch, nextTick } from 'vue'
import { currentLine } from '../../utils/lrc.js'

const props = defineProps({
  lines: { type: Array, required: true },     // [{t, text}] de utils/lrc.js
  position: { type: Number, default: 0 },
  active: Boolean                              // esta cancion es la que suena
})
const emit = defineEmits(['seek'])
const root = ref(null)
const current = computed(() => (props.active ? currentLine(props.lines, props.position) : -1))

// la linea que suena se mantiene a la vista, sin sacudir el panel entero
watch(current, async (i) => {
  if (i < 0) return
  await nextTick()
  const el = root.value?.children?.[i]
  if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'center', behavior: 'smooth' })
})
function mmss (t) {
  const m = Math.floor(t / 60)
  const s = Math.floor(t % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}
</script>

<template>
  <div class="lrc" ref="root" :class="{active}">
    <div v-for="(l, i) in lines" :key="i" class="lrc-line"
         :class="{current: i === current, past: current >= 0 && i < current, blank: !l.text}"
         :title="active ? 'Ir a ' + mmss(l.t) : mmss(l.t)"
         @click="active && emit('seek', l.t)">{{ l.text || '♪' }}</div>
  </div>
</template>
