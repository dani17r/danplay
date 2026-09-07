<script setup>
/** Girito + texto. Estaba suelto en siete sitios con el mismo marcado. */
defineProps({ text: { type: String, default: 'cargando…' }, inline: Boolean })
</script>

<template>
  <span class="loading" :class="{'loading-inline': inline}">
    <span class="spinner"></span>
    <span v-if="text">{{ text }}</span>
  </span>
</template>
