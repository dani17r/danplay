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

const props = defineProps({
  text: { type: [String, Number], default: '' },
  /** Que se copio, para el aviso: «Titulo copiado». */
  what: { type: String, default: '' },
  size: { type: Number, default: 13 }
})
const emit = defineEmits(['copied'])

const listo = ref(false)
let temporizador = null
onUnmounted(() => clearTimeout(temporizador))

async function alPortapapeles (texto) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(texto)
      return true
    }
  } catch { /* sin permiso o sin contexto seguro: se prueba lo de abajo */ }
  try {
    const caja = document.createElement('textarea')
    caja.value = texto
    caja.setAttribute('readonly', '')
    caja.style.cssText = 'position:fixed;top:-1000px;opacity:0'
    document.body.appendChild(caja)
    caja.select()
    const ok = document.execCommand('copy')
    caja.remove()
    return ok
  } catch { return false }
}

async function copiar () {
  const texto = String(props.text ?? '').trim()
  if (!texto) return
  const ok = await alPortapapeles(texto)
  if (!ok) { emit('copied', false, props.what); return }
  listo.value = true
  clearTimeout(temporizador)
  temporizador = setTimeout(() => (listo.value = false), 1400)
  emit('copied', true, props.what)
}
</script>

<template>
  <button v-if="String(text ?? '').trim()" type="button" class="copy-btn"
          :class="{done: listo}"
          :title="listo ? 'Copiado' : (what ? 'Copiar ' + what : 'Copiar')"
          :aria-label="what ? 'Copiar ' + what : 'Copiar'"
          @click.stop="copiar">
    <Icon :n="listo ? 'check' : 'copy'" :t="size" />
  </button>
</template>
