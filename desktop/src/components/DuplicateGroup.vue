<script setup>
import Icon from './Icon.vue'
import StarRating from './StarRating.vue'
defineProps(['group','playing','busy'])
const emit = defineEmits(['play','keepOne'])
const fmtDuration = (s) => s ? `${Math.floor(s/60)}:${String(Math.floor(s%60)).padStart(2,'0')}` : '—'
const mb = (b) => b ? (b / 1048576).toFixed(1) + ' MB' : '—'
</script>

<template>
  <div class="dup-group">
    <div v-for="t in group.items" :key="t.path" class="dup-row"
         :class="{playing: playing === t.id, suggested: t.path === group.suggested}">
      <button class="dup-play" :disabled="!t.id" :title="t.id ? 'Escuchar' : 'No indexada'"
              @click="emit('play', t)">
        <Icon :n="playing === t.id ? 'pause' : 'play'" :t="14" />
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
          <span class="dup-stats">{{ Math.round(t.bitrate/1000) || '—' }} kbps ·
            {{ fmtDuration(t.duration) }} · {{ mb(t.size) }}</span>
          <span class="dup-path">{{ t.relative }}</span>
        </div>
      </div>

      <StarRating v-if="t.stars" :value="t.stars" :editable="false" :t="12" />
      <Icon v-if="t.favorite" n="heartFull" :t="14" style="color:var(--red)" />

      <button class="btn mini dup-keep" :disabled="busy"
              @click="emit('keepOne', group, t)">
        <Icon n="check" :t="13" /> Mantener esta
      </button>
    </div>
  </div>
</template>
