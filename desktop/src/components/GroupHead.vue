<script setup>
/**
 * La cabecera de un grupo en la lista agrupada: su nombre, cuántas canciones
 * lleva y cuánto duran, y el botón que lo pliega. Va dentro de la propia
 * lista, entre sus filas; cada vista (tabla, lista fina, fichas o
 * cuadrícula) la envuelve a su manera.
 */
import Icon from './Icon.vue'
import { formatMinutes } from '../utils/format.js'

defineProps({
  /** {key, count, seconds, open} (ver useSongList) */
  group: { type: Object, required: true }
})
const emit = defineEmits(['toggle'])
</script>

<template>
  <button
    type="button"
    class="group-head collapsible"
    :aria-expanded="group.open"
    @click="emit('toggle', group.key)"
  >
    <Icon n="right" :t="13" class="group-chevron" :class="{ open: group.open }" />
    <span class="group-name">{{ group.key }}</span>
    <span class="cnt">{{ group.count }} temas · {{ formatMinutes(group.seconds) }}</span>
  </button>
</template>
