<script setup>
import { computed } from 'vue'
const props = defineProps({
  modelValue: { type: Number, default: 0 },
  min: { type: Number, default: 0 }, max: { type: Number, default: 1 },
  step: { type: Number, default: 0.01 },
  label: String, width: String, valueText: String
})
const emit = defineEmits(['update:modelValue'])
const pct = computed(() =>
  Math.max(0, Math.min(100, ((props.modelValue - props.min) / (props.max - props.min)) * 100)))
</script>

<template>
  <label class="slider" :style="width ? {width: width} : null">
    <span v-if="label" class="field-label">
      {{ label }}<em v-if="valueText">{{ valueText }}</em></span>
    <span class="slider-track" :style="{'--pct': pct + '%'}">
      <input type="range" :min="min" :max="max" :step="step" :value="modelValue"
             @input="emit('update:modelValue', Number($event.target.value))" />
    </span>
  </label>
</template>
