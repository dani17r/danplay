<script setup>
import { ref, computed } from 'vue'
import Icon from '../Icon.vue'

const props = defineProps({
  modelValue: [String, Number],
  label: String, hint: String, placeholder: String,
  icon: String, type: { type: String, default: 'text' },
  clearable: { type: Boolean, default: true },
  width: String, error: String, disabled: Boolean,
  compact: Boolean,           // mas bajo, para filtros dentro de paneles
  multiline: Boolean,         // para textos largos, como la letra
  rows: { type: Number, default: 8 }
})
const emit = defineEmits(['update:modelValue', 'enter'])
const focused = ref(false)
const showKey = ref(false)
const realType = computed(() =>
  props.type === 'password' ? (showKey.value ? 'text' : 'password')
  : props.type === 'number' ? 'number' : 'text')
</script>

<template>
  <label class="field" :class="{focused, error: !!error, off: disabled, compact,
                                'field-multi': multiline}"
         :style="width ? {width: width} : null">
    <span v-if="label" class="field-label">{{ label }}</span>
    <span class="field-box">
      <Icon v-if="icon" :n="icon" :t="15" class="field-icon" />
      <textarea v-if="multiline" :value="modelValue" :placeholder="placeholder"
                :disabled="disabled" :rows="rows"
                @input="emit('update:modelValue', $event.target.value)"
                @focus="focused = true" @blur="focused = false"></textarea>
      <input v-else :value="modelValue" :type="realType" :placeholder="placeholder"
             :disabled="disabled"
             @input="emit('update:modelValue', $event.target.value)"
             @focus="focused = true" @blur="focused = false"
             @keyup.enter="emit('enter')" />
      <button v-if="!multiline && type === 'password'" class="field-btn" type="button" tabindex="-1"
              @click.prevent="showKey = !showKey"
              :title="showKey ? 'Ocultar' : 'Mostrar'">
        <Icon :n="showKey ? 'eyeOff' : 'eye'" :t="15" />
      </button>
      <button v-else-if="!multiline && clearable && modelValue" class="field-btn" type="button" tabindex="-1"
              @click.prevent="emit('update:modelValue', '')" title="Vaciar el campo">
        <Icon n="close" :t="14" />
      </button>
      <!-- Sitio para lo que cada campo quiera poner DENTRO de la caja, a la
           derecha: el boton de busqueda avanzada, un «copiar»... Fuera
           quedaba como un boton suelto al lado, no como parte del campo. -->
      <slot name="acciones" />
    </span>
    <span v-if="error" class="field-error">{{ error }}</span>
    <span v-else-if="hint" class="field-hint">{{ hint }}</span>
  </label>
</template>
