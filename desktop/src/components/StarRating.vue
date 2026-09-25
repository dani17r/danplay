<script setup>
/**
 * Valoracion de 1 a 5.
 *
 * Las estrellas van al reves en el DOM (5,4,3,2,1) y el contenedor las
 * endereza con `row-reverse`. Suena raro, pero es la unica forma: al pasar el
 * raton hay que pintar la estrella y TODAS LAS ANTERIORES, y CSS solo sabe
 * seleccionar hermanos posteriores (`~`). Con el orden invertido, «posterior
 * en el DOM» es «anterior en pantalla». Antes se pintaban las de la derecha.
 */
import { ref, watch } from 'vue'
import Icon from './Icon.vue'

const props = defineProps({
  value: { type: Number, default: 0 },
  editable: { type: Boolean, default: true },
  t: { type: Number, default: 16 }
})
const emit = defineEmits(['change'])

const ORDER = [5, 4, 3, 2, 1]
const justSet = ref(0)          // para el golpecito al elegir

watch(() => props.value, () => {
  justSet.value++
})

function pick (n) {
  if (!props.editable) return
  emit('change', n === props.value ? 0 : n)   // volver a pulsar quita la nota
}
</script>

<template>
  <span class="stars" :class="{'stars-ro': !editable}" :key="justSet" @click.stop>
    <Icon v-for="n in ORDER" :key="n" :n="n <= value ? 'star' : 'starOutline'" :t="t"
          :class="{on: n <= value}"
          :title="editable ? (n === value ? 'Quitar valoracion' : n + ' de 5') : (value + ' de 5')"
          @click="pick(n)" />
  </span>
</template>
