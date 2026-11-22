<script setup>
/**
 * La cola entera, en el panel del reproductor («Ver la cola entera»).
 *
 * Aparte del reproductor porque él se repinta con cada aviso de posición (la
 * barra y el tiempo): con las filas dentro, cuatro veces por segundo se
 * volvían a crear todas, y la cola puede ser la biblioteca entera. Aquí solo
 * cambia lo que cambia la cola o lo que suena. Y se pintan las de alrededor
 * de la que suena (`AROUND` a cada lado), no miles.
 */
import { computed, nextTick, useTemplateRef } from 'vue'
import Icon from './Icon.vue'
import { formatTime } from '../utils/format.js'

const props = defineProps({
  queue: { type: Array, default: () => [] },
  /** el id de lo que suena */
  currentId: { type: Number, default: null }
})
const emit = defineEmits(['jump'])

/** Cuántas se pintan antes y después de la que suena. */
const AROUND = 150

const at = computed(() => props.queue.findIndex((s) => s.id === props.currentId))
const range = computed(() => {
  const n = props.queue.length
  const i = Math.max(0, at.value)
  const from = Math.max(0, Math.min(i - AROUND, n - 2 * AROUND - 1))
  return { from, to: Math.min(n, from + 2 * AROUND + 1) }
})
const rows = computed(() =>
  props.queue.slice(range.value.from, range.value.to).map((song, k) => ({
    song,
    n: range.value.from + k + 1
  }))
)
const current = useTemplateRef('current')

/** Deja a la vista la que suena (al abrir la cola entera). */
async function showCurrent() {
  await nextTick()
  const el = current.value?.[0]
  if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'center' })
}
defineExpose({ showCurrent })
</script>

<template>
  <div v-if="range.from > 0" class="queue-empty">{{ range.from }} antes…</div>
  <template v-for="{ song, n } in rows" :key="song.id">
    <button
      v-if="song.id === currentId"
      ref="current"
      type="button"
      class="queue-row flat current"
      aria-current="true"
      @click="emit('jump', song.id)"
    >
      <span class="queue-n"><Icon n="play" :t="11" /></span>
      <span class="queue-title"
        >{{ song.title }}<span class="sub"> · {{ song.artist || '—' }}</span></span
      >
      <span class="mono sub queue-dur">{{ formatTime(song.duration) }}</span>
    </button>
    <button v-else type="button" class="queue-row flat" @click="emit('jump', song.id)">
      <span class="queue-n">{{ n }}</span>
      <span class="queue-title"
        >{{ song.title }}<span class="sub"> · {{ song.artist || '—' }}</span></span
      >
      <span class="mono sub queue-dur">{{ formatTime(song.duration) }}</span>
    </button>
  </template>
  <div v-if="range.to < queue.length" class="queue-empty">…y {{ queue.length - range.to }} más</div>
</template>
