<script setup>
/**
 * Un interruptor con su texto. Todo va dentro de un <label>: pulsar el texto
 * activa el interruptor (lo hace el propio navegador) y el texto es su
 * nombre para quien no ve la pantalla. El texto tenia ademas su propio clic,
 * y en un navegador de verdad el interruptor cambiaba dos veces.
 */
import { useId } from 'vue'

const model = defineModel({ type: Boolean, default: false })
defineProps({
  title: { type: String, default: '' },
  hint: { type: String, default: '' },
  disabled: Boolean
})
// la etiqueta envuelve el interruptor y ademas lo nombra por su id: asi la
// reconoce tambien quien no cuenta un boton como control de formulario
const id = useId()
</script>

<template>
  <label class="toggle" :class="{ off: disabled }" :for="id">
    <button
      :id="id"
      type="button"
      class="toggle-track"
      :class="{ on: model }"
      :disabled="disabled"
      role="switch"
      :aria-checked="model"
      @click="model = !model"
    >
      <span class="toggle-knob"></span>
    </button>
    <span class="toggle-txt">
      <strong v-if="title">{{ title }}</strong>
      <span v-if="hint">{{ hint }}</span>
      <slot />
    </span>
  </label>
</template>
