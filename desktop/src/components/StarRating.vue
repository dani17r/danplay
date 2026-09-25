<script setup>
/**
 * Valoracion de 1 a 5.
 *
 * Las estrellas van al reves en el DOM (5,4,3,2,1) y el contenedor las
 * endereza con `row-reverse`. Suena raro, pero es la unica forma: al pasar el
 * raton hay que pintar la estrella y TODAS LAS ANTERIORES, y CSS solo sabe
 * seleccionar hermanos posteriores (`~`). Con el orden invertido, «posterior
 * en el DOM» es «anterior en pantalla». Antes se pintaban las de la derecha.
 *
 * Con el teclado es un deslizador: las flechas suben y bajan, Inicio la
 * quita, Fin pone cinco, y un numero del 0 al 5 la deja en ese. Dentro de
 * una fila de la lista no es parada del tabulador (`focusable: false`): ahi
 * se valora desde el menu de la cancion.
 */
import { ref, watch, computed } from 'vue'
import Icon from './Icon.vue'

const props = defineProps({
  value: { type: Number, default: 0 },
  editable: { type: Boolean, default: true },
  t: { type: Number, default: 16 },
  /** se alcanza con el tabulador */
  focusable: { type: Boolean, default: true }
})
const emit = defineEmits(['change'])

const ORDER = [5, 4, 3, 2, 1]
const justSet = ref(0) // para el golpecito al elegir

watch(
  () => props.value,
  () => {
    justSet.value++
  }
)

function pick(n) {
  if (!props.editable) return
  emit('change', n === props.value ? 0 : n) // volver a pulsar quita la nota
}

const said = computed(() => (props.value ? `${props.value} de 5` : 'sin valorar'))

function onKey(e) {
  if (!props.editable) return
  const v = props.value || 0
  let next = null
  if (e.key === 'ArrowRight' || e.key === 'ArrowUp') next = Math.min(5, v + 1)
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') next = Math.max(0, v - 1)
  else if (e.key === 'Home') next = 0
  else if (e.key === 'End') next = 5
  else if (/^[0-5]$/.test(e.key)) next = Number(e.key)
  if (next == null) return
  e.preventDefault()
  e.stopPropagation()
  if (next !== v) emit('change', next)
}
</script>

<template>
  <!-- con tabindex enlazado (dentro de una fila no es parada); la regla solo
       entiende los escritos a mano -->
  <!-- eslint-disable-next-line vuejs-accessibility/interactive-supports-focus -->
  <span
    v-if="editable"
    :key="justSet"
    class="stars"
    role="slider"
    aria-label="Valoración"
    aria-valuemin="0"
    aria-valuemax="5"
    :aria-valuenow="value || 0"
    :aria-valuetext="said"
    :tabindex="focusable ? 0 : -1"
    @click.stop
    @keydown="onKey"
  >
    <Icon
      v-for="n in ORDER"
      :key="n"
      :n="n <= value ? 'star' : 'starOutline'"
      :t="t"
      :class="{ on: n <= value }"
      :title="n === value ? 'Quitar valoración' : n + ' de 5'"
      @click="pick(n)"
    />
  </span>
  <span v-else :key="justSet" class="stars stars-ro" role="img" :aria-label="'Valoración: ' + said">
    <Icon
      v-for="n in ORDER"
      :key="n"
      :n="n <= value ? 'star' : 'starOutline'"
      :t="t"
      :class="{ on: n <= value }"
      :title="value + ' de 5'"
    />
  </span>
</template>
