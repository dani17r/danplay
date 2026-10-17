<script setup>
/**
 * Por dónde sale una cosa (la canción, una pista, el clic o un carril de
 * guías), en su carril: un botón por canal (L y R; con una interfaz de más
 * salidas, 1, 2, 3…) y si va en estéreo o en mono. Como en una secuencia:
 * la canción por la derecha y las guías por la izquierda, o la batería sola
 * a la 3.
 *
 * Con un canal encendido va en mono por él; con dos, el primero a la
 * izquierda y el segundo a la derecha, en estéreo o, con «Mono», lo mismo
 * por los dos. Encender un tercero apaga el más antiguo. Apagarlos todos la
 * calla en la salida (no en la mezcla: para eso está «M»).
 */
import { computed, ref } from 'vue'
import { useRouting, channelsOf, destOfChannels } from '../composables/useRouting.js'

const props = defineProps({
  /** 'song' | 'click' | 'cues' | 'stem' */
  kind: { type: String, required: true },
  /** la pista o el carril (su clave en el reparto) */
  route: { type: String, default: '' },
  /** su papel ('guide', 'click'): de él sale dónde va si no se le dice */
  part: { type: String, default: '' },
  /** cómo se llama, para los avisos */
  name: { type: String, default: '' }
})
const r = useRouting()
const now = computed(() => r.routeOf(props.kind, props.route, props.part))
const on = computed(() => channelsOf(now.value.dest))
const single = computed(() => on.value.length === 1)
/** Los botones: L y R, o un número por canal de la interfaz. */
const buttons = computed(() => {
  const n = Math.max(r.deviceChannels.value, ...on.value)
  if (n <= 2)
    return [
      { c: 1, n: 'L', title: 'Izquierda' },
      { c: 2, n: 'R', title: 'Derecha' }
    ]
  return Array.from({ length: n }, (_, i) => ({
    c: i + 1,
    n: String(i + 1),
    title: `Salida ${i + 1}`
  }))
})
/** El orden en que se encendieron (para apagar el más antiguo con un tercero). */
const order = ref([])
function toggle(c) {
  const list = on.value
  let next
  if (list.includes(c)) next = list.filter((x) => x !== c)
  else if (list.length < 2) next = [...list, c]
  else {
    const oldest = order.value.find((x) => list.includes(x)) ?? list[0]
    next = [...list.filter((x) => x !== oldest), c]
  }
  order.value = [...order.value.filter((x) => x !== c), c]
  r.setRoute(props.kind, props.route, destOfChannels(next))
}
function toggleMono() {
  if (single.value || !on.value.length) return
  r.setMono(props.kind, !now.value.mono, props.route)
}
const where = computed(() => {
  const list = on.value
  if (!list.length) return 'no suena'
  if (buttons.value.length === 2) {
    if (list.length === 2)
      return now.value.mono ? 'los dos lados, en mono' : 'los dos lados, en estéreo'
    return list[0] === 1 ? 'solo la izquierda, en mono' : 'solo la derecha, en mono'
  }
  return list.length === 2
    ? `las salidas ${list.join(' y ')}${now.value.mono ? ', en mono' : ''}`
    : `la salida ${list[0]}, en mono`
})
</script>

<template>
  <span
    class="chans"
    role="group"
    :aria-label="'Por dónde suena ' + (name || 'esto')"
    :title="(name ? name + ': ' : '') + where"
  >
    <button
      v-for="b in buttons"
      :key="b.c"
      type="button"
      class="chan"
      :class="{ on: on.includes(b.c) }"
      :aria-pressed="on.includes(b.c)"
      :aria-label="b.title"
      @click="toggle(b.c)"
    >
      {{ b.n }}
    </button>
    <button
      type="button"
      class="chan chan-mode"
      :class="{ on: now.mono || single }"
      :aria-pressed="now.mono || single"
      :disabled="single || !on.length"
      :aria-label="now.mono || single ? 'En mono' : 'En estéreo'"
      :title="
        single
          ? 'Por un solo canal va en mono'
          : now.mono
            ? 'En mono: junta sus dos lados y saca lo mismo por los dos (pulsa para estéreo)'
            : 'En estéreo (pulsa para mono: junta sus dos lados, para una mesa o una sala en mono)'
      "
      @click="toggleMono"
    >
      {{ now.mono || single ? 'Mono' : 'Est' }}
    </button>
  </span>
</template>
