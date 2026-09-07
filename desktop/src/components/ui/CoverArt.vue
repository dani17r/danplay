<script setup>
/**
 * Caratula con hueco de reserva.
 *
 * Estaba escrita tres veces (rejilla, reproductor y panel de detalle), cada
 * una a su manera y una de ellas manipulando el DOM a mano. El hueco ocupa
 * exactamente lo mismo que la imagen, asi que nada se descuadra cuando falta.
 *
 * `version` sirve para obligar a recargarla: la URL de una portada no cambia
 * al buscarla, y sin esto el navegador reusaba la cacheada (o el 404).
 */
import { ref, watch, computed } from 'vue'
import { api } from '../../api.js'
import Icon from '../Icon.vue'

const props = defineProps({
  id: [Number, String],
  version: { type: Number, default: 0 },
  iconSize: { type: Number, default: 24 },
  alt: { type: String, default: '' }
})

const failed = ref(false)
const triedAlt = ref(false)

const src = computed(() => props.id == null || props.id === ''
  ? '' : api.coverUrl(props.id) + (props.version ? `?v=${props.version}` : ''))

watch(() => `${props.id}:${props.version}`, () => {
  failed.value = false
  triedAlt.value = false
})

function onError (e) {
  // Tauri expone el protocolo propio de dos formas segun el sistema; si la
  // primera no vale, se prueba la otra antes de rendirse.
  const other = api.coverUrlAlt?.(props.id)
  if (other && !triedAlt.value) {
    triedAlt.value = true
    e.target.src = other
    return
  }
  failed.value = true
}
</script>

<template>
  <img v-if="src && !failed" class="cover-art" :src="src" :alt="alt"
       loading="lazy" @error="onError" />
  <span v-else class="cover-art no-art" aria-hidden="true">
    <Icon n="music" :t="iconSize" />
  </span>
</template>
