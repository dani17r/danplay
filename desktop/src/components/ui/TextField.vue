<script setup>
/**
 * Un campo de texto (o de varias lineas) con su etiqueta, su icono y sus
 * botones dentro de la caja.
 *
 * Lo que se le pase que no sea una prop (aria-label, role, autocomplete…) va
 * al <input>, que es quien lo necesita; la clase y el estilo, a la caja de
 * fuera. Antes todo caia en el <label> de fuera y un `aria-label` no le
 * ponia nombre al campo.
 */
import { ref, computed, useAttrs } from 'vue'
import Icon from '../Icon.vue'

defineOptions({ inheritAttrs: false })

const model = defineModel({ type: [String, Number], default: '' })
const props = defineProps({
  label: { type: String, default: '' },
  hint: { type: String, default: '' },
  placeholder: { type: String, default: '' },
  icon: { type: String, default: '' },
  type: { type: String, default: 'text' },
  clearable: { type: Boolean, default: true },
  width: { type: String, default: '' },
  error: { type: String, default: '' },
  disabled: Boolean,
  compact: Boolean, // mas bajo, para filtros dentro de paneles
  multiline: Boolean, // para textos largos, como la letra
  rows: { type: Number, default: 8 }
})
const emit = defineEmits(['enter'])
const attrs = useAttrs()
const focused = ref(false)
const showKey = ref(false)
const realType = computed(() =>
  props.type === 'password'
    ? showKey.value
      ? 'text'
      : 'password'
    : props.type === 'number'
      ? 'number'
      : 'text'
)
/** Lo de fuera: clase y estilo. */
const outer = computed(() => ({ class: attrs.class, style: attrs.style }))
/** Lo del campo: todo lo demas. */
const inner = computed(() => {
  const { class: _c, style: _s, ...rest } = attrs
  return rest
})
</script>

<template>
  <label
    class="field"
    :class="[
      { focused, error: !!error, off: disabled, compact, 'field-multi': multiline },
      outer.class
    ]"
    :style="[width ? { width: width } : null, outer.style]"
  >
    <span v-if="label" class="field-label">{{ label }}</span>
    <span class="field-box">
      <Icon v-if="icon" :n="icon" :t="15" class="field-icon" />
      <textarea
        v-if="multiline"
        v-bind="inner"
        :value="model"
        :placeholder="placeholder"
        :disabled="disabled"
        :rows="rows"
        @input="model = $event.target.value"
        @focus="focused = true"
        @blur="focused = false"
      ></textarea>
      <input
        v-else
        v-bind="inner"
        :value="model"
        :type="realType"
        :placeholder="placeholder"
        :disabled="disabled"
        :aria-invalid="error ? 'true' : undefined"
        @input="model = $event.target.value"
        @focus="focused = true"
        @blur="focused = false"
        @keyup.enter="emit('enter')"
      />
      <button
        v-if="!multiline && type === 'password'"
        class="field-btn"
        type="button"
        tabindex="-1"
        :title="showKey ? 'Ocultar' : 'Mostrar'"
        @click.prevent="showKey = !showKey"
      >
        <Icon :n="showKey ? 'eyeOff' : 'eye'" :t="15" />
      </button>
      <button
        v-else-if="!multiline && clearable && model"
        class="field-btn"
        type="button"
        tabindex="-1"
        title="Vaciar el campo"
        @click.prevent="model = ''"
      >
        <Icon n="close" :t="14" />
      </button>
      <!-- Sitio para lo que cada campo quiera poner DENTRO de la caja, a la
           derecha: el boton de busqueda avanzada, un «copiar»... Fuera
           quedaba como un boton suelto al lado, no como parte del campo. -->
      <slot name="actions" />
    </span>
    <span v-if="error" class="field-error">{{ error }}</span>
    <span v-else-if="hint" class="field-hint">{{ hint }}</span>
  </label>
</template>
