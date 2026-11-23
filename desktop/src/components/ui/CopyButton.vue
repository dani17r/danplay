<script setup>
/**
 * Copiar un texto al portapapeles.
 *
 * Se usa el portapapeles del navegador cuando se puede, y si no —algunos
 * WebView lo niegan si la pagina no viene por https— se recurre al truco de
 * siempre: un campo invisible, seleccionarlo y copiar. Sin esa segunda via el
 * boton no haria nada y encima parecia que si.
 *
 * Al copiar se cambia el icono un momento: sin ese acuse no se sabe si ha
 * pasado algo, porque el portapapeles no se ve.
 */
import { ref, onUnmounted } from 'vue'
import Icon from '../Icon.vue'
import { copyText } from '../../utils/clipboard.js'

const props = defineProps({
  text: { type: [String, Number], default: '' },
  /** Que se copio, para el aviso: «Titulo copiado». */
  what: { type: String, default: '' },
  size: { type: Number, default: 13 },
  /** Una palabra al lado del icono, para distinguirlo de otro «copiar» cerca. */
  label: { type: String, default: '' }
})
const emit = defineEmits(['copied'])

const done = ref(false)
let timer = null
onUnmounted(() => clearTimeout(timer))

async function copy() {
  const value = String(props.text ?? '').trim()
  if (!value) return
  const ok = await copyText(value)
  if (!ok) {
    emit('copied', false, props.what)
    return
  }
  done.value = true
  clearTimeout(timer)
  timer = setTimeout(() => (done.value = false), 1400)
  emit('copied', true, props.what)
}
</script>

<template>
  <button
    v-if="String(text ?? '').trim()"
    type="button"
    class="copy-btn"
    :class="{ done: done, 'with-label': !!label }"
    :title="done ? 'Copiado' : what ? 'Copiar ' + what : 'Copiar'"
    :aria-label="what ? 'Copiar ' + what : 'Copiar'"
    @click.stop="copy"
    @dblclick.stop
  >
    <Icon :n="done ? 'check' : 'copy'" :t="size" />
    <span v-if="label" class="copy-label">{{ label }}</span>
  </button>
</template>
