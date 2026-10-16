<script setup>
/**
 * Una casilla con su texto, para lo que se acepta o se confirma («los he
 * leído y los acepto», «quitarla también de sus listas»). Es una casilla de
 * verdad del navegador: se marca con el texto, con Espacio y la lee quien no
 * ve la pantalla. Para encender y apagar ajustes, ToggleField.
 */
const model = defineModel({ type: Boolean, default: false })
defineProps({
  disabled: Boolean
})
</script>

<template>
  <label class="check-field" :class="{ off: disabled }">
    <input v-model="model" type="checkbox" :disabled="disabled" />
    <span><slot /></span>
  </label>
</template>
