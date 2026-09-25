<script setup>
/** Un color del tema: la muestra (que abre el selector) y su codigo, editable. */
const model = defineModel({ type: String, default: '' })
defineProps({
  title: { type: String, default: '' },
  hint: { type: String, default: '' }
})
</script>

<template>
  <div class="color-row">
    <label class="color-swatch" :style="{ background: model }">
      <input type="color" :value="model" :aria-label="title" @input="model = $event.target.value" />
    </label>
    <div class="color-txt">
      <div class="color-name">{{ title }}</div>
      <div v-if="hint" class="color-about">{{ hint }}</div>
    </div>
    <input
      class="color-hex mono"
      :value="model"
      spellcheck="false"
      :aria-label="title + ', en hexadecimal'"
      @input="model = $event.target.value"
    />
  </div>
</template>
