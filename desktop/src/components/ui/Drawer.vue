<script setup>
/**
 * Panel que se abre por un lado, encima del contenido.
 *
 * En pantallas estrechas los laterales fijos no caben, asi que pasan a
 * abrirse asi. Se puede cerrar de tres formas, porque en movil cada uno usa
 * la que le sale: arrastrando hacia su lado, tocando fuera, o con Escape.
 *
 * El arrastre sigue al dedo en tiempo real y decide al soltar: si has
 * recorrido mas de un tercio del ancho, o lo has lanzado con brio, se cierra;
 * si no, vuelve a su sitio. Sin eso, un arrastre a medias deja el panel en
 * una posicion rara.
 */
import { ref, computed, watch, onUnmounted } from 'vue'
import Icon from '../Icon.vue'

const props = defineProps({
  open: Boolean,
  side: { type: String, default: 'left' },   // left | right
  title: String,
  width: { type: String, default: 'min(320px, 86vw)' }
})
const emit = defineEmits(['close'])

const panel = ref(null)
const dragX = ref(0)          // cuanto se ha arrastrado, en px
const dragging = ref(false)
let startX = 0
let startAt = 0

const closingSign = computed(() => (props.side === 'left' ? -1 : 1))
const style = computed(() => ({
  width: props.width,
  transform: dragX.value ? `translateX(${dragX.value}px)` : '',
  transition: dragging.value ? 'none' : ''
}))

// immediate: si el panel se monta ya abierto, el watch normal no corre y el
// fondo se quedaba desplazandose por detras
watch(() => props.open, (v) => {
  dragX.value = 0
  dragging.value = false
  // el fondo no debe desplazarse mientras hay un panel abierto encima
  if (typeof document !== 'undefined') {
    document.body.classList.toggle('drawer-open', !!v)
  }
}, { immediate: true })
onUnmounted(() => {
  if (typeof document !== 'undefined') document.body.classList.remove('drawer-open')
})

function onDown (e) {
  if (e.pointerType === 'mouse') return       // con raton se cierra pulsando fuera
  dragging.value = true
  startX = e.clientX
  startAt = Date.now()
}
function onMove (e) {
  if (!dragging.value) return
  const dx = e.clientX - startX
  // solo se deja arrastrar hacia el lado por el que se cierra
  dragX.value = closingSign.value < 0 ? Math.min(0, dx) : Math.max(0, dx)
}
function onUp () {
  if (!dragging.value) return
  const el = panel.value
  const w = el ? el.offsetWidth : 320
  const moved = Math.abs(dragX.value)
  const speed = moved / Math.max(1, Date.now() - startAt)   // px por ms
  dragging.value = false
  if (moved > w / 3 || speed > 0.5) emit('close')
  else dragX.value = 0
}
</script>

<template>
  <teleport to="body">
    <transition name="drawer">
      <div v-if="open" class="drawer-backdrop" @click.self="emit('close')"
           @keydown.esc="emit('close')">
        <aside class="drawer" :class="'drawer-' + side" ref="panel" :style="style"
               role="dialog" aria-modal="true" :aria-label="title"
               @pointerdown="onDown" @pointermove="onMove"
               @pointerup="onUp" @pointercancel="onUp">
          <header v-if="title" class="drawer-head">
            <span>{{ title }}</span>
            <button class="icon-btn" title="Cerrar" @click="emit('close')">
              <Icon n="close" :t="15" /></button>
          </header>
          <div class="drawer-grip" aria-hidden="true"></div>
          <div class="drawer-body"><slot /></div>
        </aside>
      </div>
    </transition>
  </teleport>
</template>
