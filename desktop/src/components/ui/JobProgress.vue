<script setup>
/**
 * Cómo va una tarea larga del núcleo (analizar, importar, convertir, buscar
 * duplicados): qué está haciendo, cuánto lleva de cuánto y una barra. Si el
 * núcleo aún no sabe cuántas hay, solo el girito y lo que está haciendo.
 */
import { computed } from 'vue'
import Loading from './Loading.vue'

const props = defineProps({
  /** la tarea, como la cuenta el núcleo (`JobSnapshot`), o null al empezar */
  job: { type: Object, default: null },
  /** qué se está haciendo, en corto */
  label: { type: String, default: 'trabajando…' }
})

const percent = computed(() => {
  const j = props.job
  return j?.total ? Math.min(100, Math.round((j.done / j.total) * 100)) : null
})
</script>

<template>
  <div class="job-progress" role="status" aria-live="polite">
    <div class="job-head">
      <Loading :text="label" inline />
      <span v-if="job?.total" class="job-count mono">{{ job.done }} / {{ job.total }}</span>
    </div>
    <div
      v-if="percent != null"
      class="track job-track"
      role="progressbar"
      :aria-label="label"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="percent"
    >
      <div class="track-fill" :style="{ width: percent + '%' }"></div>
    </div>
    <div v-if="job?.message" class="job-msg">{{ job.message }}</div>
  </div>
</template>
