<script setup>
/**
 * Carátula con hueco de reserva.
 *
 * Estaba escrita tres veces (rejilla, reproductor y panel de detalle), cada
 * una a su manera y una de ellas manipulando el DOM a mano. El hueco ocupa
 * exactamente lo mismo que la imagen, así que nada se descuadra cuando falta.
 *
 * `size` pide una miniatura al núcleo. Sin ella se sirve la carátula tal cual
 * viene dentro del mp3, que a menudo son dos megas: para un cuadradito de 40
 * píxeles en una lista de mil canciones eso es descodificar dos gigas.
 *
 * `version` sirve para obligar a recargarla: la URL de una portada no cambia
 * al buscarla, y sin esto el navegador reusaba la cacheada (o el 404).
 */
import { ref, watch, computed } from 'vue'
import { api, COVER_SIZES } from '../../api.js'
import Icon from '../Icon.vue'

const props = defineProps({
  id: [Number, String],
  version: { type: Number, default: 0 },
  iconSize: { type: Number, default: 24 },
  alt: { type: String, default: '' },
  /** Ancho en píxeles de la miniatura. Sin esto, la imagen original. */
  size: { type: Number, default: 0 },
  // Para portadas que uno no quiere tener delante. La imagen se guarda
  // entera: esto es solo cómo se pinta, así que quitarlo la devuelve igual.
  blur: Boolean
})

const failed = ref(false)
const triedAlt = ref(false)

/** El tamaño pedido, redondeado al que el núcleo sabe cachear. */
const thumb = computed(() =>
  props.size ? COVER_SIZES.find((s) => s >= props.size) || COVER_SIZES.at(-1) : 0
)

const withVersion = (url) =>
  props.version ? url + (url.includes('?') ? '&' : '?') + `v=${props.version}` : url

const src = computed(() =>
  props.id == null || props.id === '' ? '' : withVersion(api.coverUrl(props.id, thumb.value))
)

watch(
  () => `${props.id}:${props.version}:${props.size}`,
  () => {
    failed.value = false
    triedAlt.value = false
  }
)

function onError(e) {
  // Tauri expone el protocolo propio de dos formas según el sistema; si la
  // primera no vale, se prueba la otra antes de rendirse.
  const other = api.coverUrlAlt?.(props.id, thumb.value)
  if (other && !triedAlt.value) {
    triedAlt.value = true
    e.target.src = withVersion(other)
    return
  }
  failed.value = true
}
</script>

<template>
  <img
    v-if="src && !failed"
    class="cover-art"
    :class="{ blurred: blur }"
    :src="src"
    :alt="blur ? (alt ? alt + ' (portada difuminada)' : 'portada difuminada') : alt"
    loading="lazy"
    decoding="async"
    @error="onError"
  />
  <span v-else class="cover-art no-art" aria-hidden="true">
    <Icon n="music" :t="iconSize" />
  </span>
</template>
