<script setup>
/**
 * Una ventana grande, con su título y lo que se le ponga dentro: para lo que
 * no cabe en un menú (elegir entre varias formas de exportar, la voz de las
 * guías…), con sitio para explicar cada cosa. En un menú solo van nombres
 * cortos; lo largo, aquí.
 *
 * Escape, la ✕ o un clic fuera la cierran; mientras está abierta el foco no
 * se escapa a lo de detrás.
 */
import { watch, onUnmounted, useId, useTemplateRef } from 'vue'
import { useFocusTrap } from '../../composables/useFocusTrap.js'
import { vTips } from '../../composables/useTooltips.js'
import Icon from '../Icon.vue'

const props = defineProps({
  open: Boolean,
  title: { type: String, default: '' },
  icon: { type: String, default: '' },
  /** lo ancha que es como mucho */
  width: { type: String, default: '560px' }
})
const emit = defineEmits(['close'])
const box = useTemplateRef('box')
const titleId = useId()

useFocusTrap(box, {
  active: () => props.open,
  initial: '.wide-body button:not([disabled]), .wide-body input, .wide-foot .btn:last-child'
})
function keys(e) {
  if (e.key === 'Escape') {
    e.stopPropagation()
    emit('close')
  }
}
watch(
  () => props.open,
  (v) => {
    if (v) document.addEventListener('keydown', keys)
    else document.removeEventListener('keydown', keys)
  },
  { immediate: true }
)
onUnmounted(() => document.removeEventListener('keydown', keys))
</script>

<template>
  <Teleport to="body">
    <transition name="fade">
      <div v-if="open" class="modal-back" role="presentation" @mousedown.self="emit('close')">
        <div
          ref="box"
          v-tips
          class="modal wide"
          role="dialog"
          aria-modal="true"
          :aria-labelledby="titleId"
          :style="{ width: `min(${width}, 100%)` }"
        >
          <header class="wide-head">
            <span v-if="icon" class="wide-icon"><Icon :n="icon" :t="16" /></span>
            <h3 :id="titleId" class="modal-title">{{ title }}</h3>
            <button
              type="button"
              class="btn mini icon-btn wide-close"
              aria-label="Cerrar"
              title="Cerrar (Esc)"
              @click="emit('close')"
            >
              <Icon n="close" :t="13" />
            </button>
          </header>
          <div class="wide-body"><slot /></div>
          <footer v-if="$slots.foot" class="btn-row modal-actions wide-foot">
            <slot name="foot" />
          </footer>
        </div>
      </div>
    </transition>
  </Teleport>
</template>
