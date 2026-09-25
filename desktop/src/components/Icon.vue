<script setup>
import { ICONS } from '../icons.js'
defineProps({
  n: { type: String, required: true },
  t: { type: [Number, String], default: 16 },
  grosor: { type: [Number, String], default: 1.6 }
})
</script>

<template>
  <!-- v-html con los trazos de icons.js, generados de heroicons: constantes propias -->
  <!-- eslint-disable vue/no-v-html -->
  <svg
    v-if="ICONS[n]"
    class="ico"
    :width="t"
    :height="t"
    viewBox="0 0 24 24"
    :fill="ICONS[n].e === 's' ? 'currentColor' : 'none'"
    :stroke="ICONS[n].e === 's' ? 'none' : 'currentColor'"
    :stroke-width="grosor"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
    v-html="ICONS[n].d"
  />
  <!-- eslint-enable vue/no-v-html -->
</template>
