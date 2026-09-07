<script setup>
defineProps({ modelValue: String, title: String, hint: String })
const emit = defineEmits(['update:modelValue'])
</script>

<template>
  <div class="color-row">
    <label class="color-swatch" :style="{background: modelValue}">
      <input type="color" :value="modelValue"
             @input="emit('update:modelValue', $event.target.value)" />
    </label>
    <div class="color-txt">
      <div class="color-name">{{ title }}</div>
      <div v-if="hint" class="color-about">{{ hint }}</div>
    </div>
    <input class="color-hex mono" :value="modelValue"
           @input="emit('update:modelValue', $event.target.value)" spellcheck="false" />
  </div>
</template>
