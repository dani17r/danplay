<script setup>
/**
 * Un grupo de copias de la misma cancion: se escucha cada una y se elige con
 * cual quedarse.
 */
import Icon from './Icon.vue'
import StarRating from './StarRating.vue'
import { formatDuration, formatMegabytes } from '../utils/format.js'

const props = defineProps({
  group: { type: Object, required: true },
  /** el id de la cancion que suena, o null */
  playing: { type: Number, default: null },
  /** la que suena esta en pausa */
  paused: Boolean,
  busy: Boolean
})
const emit = defineEmits(['play', 'keepOne'])

// Sobre la que suena, el boton es pausa (o reanudar si esta en pausa); antes
// pintaba la pausa y al pulsarla volvia a empezar la copia.
const isPlaying = (t) => props.playing != null && props.playing === t.id
const icon = (t) => (isPlaying(t) && !props.paused ? 'pause' : 'play')
const title = (t) =>
  !t.id ? 'No indexada' : !isPlaying(t) ? 'Escuchar' : props.paused ? 'Reanudar' : 'Pausar'
</script>

<template>
  <div class="dup-group">
    <div
      v-for="t in group.items"
      :key="t.path"
      class="dup-row"
      :class="{ playing: isPlaying(t), suggested: t.path === group.suggested }"
    >
      <button
        type="button"
        class="dup-play"
        :disabled="!t.id"
        :title="title(t)"
        :aria-label="title(t) + ': ' + t.file"
        @click="emit('play', t)"
      >
        <Icon :n="icon(t)" :t="14" />
      </button>

      <div class="dup-datos">
        <div class="dup-name">
          {{ t.file }}
          <span v-if="t.path === group.suggested" class="badge ok">mejor calidad</span>
          <span v-if="t.has_suffix" class="badge">lleva « - r»</span>
        </div>
        <div class="dup-meta mono">
          <!-- Los datos en su propio span: como texto suelto dentro de un flex
               se partian por cualquier espacio y el «MB» acababa en la linea
               de abajo, separado de su numero. -->
          <span class="dup-stats"
            >{{ Math.round(t.bitrate / 1000) || '—' }} kbps · {{ formatDuration(t.duration) }} ·
            {{ formatMegabytes(t.size) }}</span
          >
          <span class="dup-path">{{ t.relative }}</span>
        </div>
      </div>

      <StarRating v-if="t.stars" :value="t.stars" :editable="false" :t="12" />
      <Icon v-if="t.favorite" n="heartFull" :t="14" style="color: var(--red)" />

      <button
        type="button"
        class="btn mini dup-keep"
        :disabled="busy"
        @click="emit('keepOne', group, t)"
      >
        <Icon n="check" :t="13" /> Mantener esta
      </button>
    </div>
  </div>
</template>
