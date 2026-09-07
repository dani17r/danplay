<script setup>
/** Hueco cuando no hay nada que enseñar. Estaba repetido en cuatro sitios. */
import Icon from '../Icon.vue'
defineProps({
  icon: { type: String, default: 'music' },
  title: String,
  hint: String,
  size: { type: Number, default: 30 }
})
</script>

<template>
  <div class="empty">
    <Icon :n="icon" :t="size" class="empty-icon" />
    <div v-if="title" class="empty-title">{{ title }}</div>
    <div v-if="hint" class="empty-hint">{{ hint }}</div>
    <slot />
  </div>
</template>
