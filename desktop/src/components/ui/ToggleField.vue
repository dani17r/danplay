<script setup>
defineProps({ modelValue: Boolean, title: String, hint: String, disabled: Boolean })
const emit = defineEmits(['update:modelValue'])
</script>

<template>
  <label class="toggle" :class="{off: disabled}">
    <button type="button" class="toggle-track" :class="{on: modelValue}" :disabled="disabled"
            role="switch" :aria-checked="modelValue"
            @click="emit('update:modelValue', !modelValue)">
      <span class="toggle-knob"></span>
    </button>
    <span class="toggle-txt" @click="!disabled && emit('update:modelValue', !modelValue)">
      <strong v-if="title">{{ title }}</strong>
      <span v-if="hint">{{ hint }}</span>
      <slot />
    </span>
  </label>
</template>
