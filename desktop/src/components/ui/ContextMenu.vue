<script setup>
/**
 * Menu contextual generico. Se le pasan las opciones ya montadas, asi cada
 * sitio decide que ofrecer segun donde se pulso.
 *
 * Una opcion con `children` no ejecuta nada: abre ese submenu dentro del mismo
 * panel, con una flecha para volver. Se hace asi y no con submenus flotantes
 * porque estos se cierran solos al mover el raton en diagonal y acaban siendo
 * incomodos.
 */
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import Icon from '../Icon.vue'

const props = defineProps({
  open: Boolean,
  x: { type: Number, default: 0 },
  y: { type: Number, default: 0 },
  items: { type: Array, default: () => [] },
  title: String
})
const emit = defineEmits(['close'])

const panel = ref(null)
const drill = ref(null)          // el submenu abierto, si hay alguno
const pos = ref({ left: 0, top: 0 })

const shown = computed(() => drill.value ? drill.value.children : props.items)
const heading = computed(() => drill.value ? drill.value.label : props.title)

watch(() => props.open, async (v) => {
  if (!v) { drill.value = null; return }
  await nextTick()
  colocar()
})

/** Que no se salga de la ventana. */
function colocar () {
  const el = panel.value
  if (!el) return
  const w = el.offsetWidth || 220
  const h = el.offsetHeight || 200
  pos.value = {
    left: Math.max(6, Math.min(props.x, window.innerWidth - w - 6)),
    top: Math.max(6, Math.min(props.y, window.innerHeight - h - 6))
  }
}

function elegir (it) {
  if (it.disabled || it.separator) return
  if (it.children?.length) { drill.value = it; nextTick(colocar); return }
  emit('close')
  it.action?.()
}

function fuera (e) {
  if (props.open && panel.value && !panel.value.contains(e.target)) emit('close')
}
function teclas (e) {
  if (!props.open) return
  if (e.key === 'Escape') { e.preventDefault(); drill.value ? (drill.value = null) : emit('close') }
}
onMounted(() => {
  window.addEventListener('mousedown', fuera, true)
  window.addEventListener('keydown', teclas)
  window.addEventListener('resize', () => props.open && emit('close'))
})
onUnmounted(() => {
  window.removeEventListener('mousedown', fuera, true)
  window.removeEventListener('keydown', teclas)
})
</script>

<template>
  <transition name="dropdown">
    <div v-if="open" class="ctx" ref="panel"
         :style="{left: pos.left + 'px', top: pos.top + 'px'}">
      <div v-if="heading" class="ctx-head">
        <button v-if="drill" class="ctx-back" title="Volver" @click="drill = null">
          <Icon n="right" :t="12" style="transform:rotate(180deg)" /></button>
        <span>{{ heading }}</span>
      </div>

      <div class="ctx-body">
        <template v-for="(it, i) in shown" :key="i">
          <div v-if="it.separator" class="ctx-sep"></div>
          <div v-else-if="it.empty" class="ctx-empty">{{ it.label }}</div>
          <button v-else class="ctx-item" :class="{danger: it.danger, off: it.disabled}"
                  :disabled="it.disabled" @click="elegir(it)">
            <span class="ctx-ico"><Icon v-if="it.icon" :n="it.icon" :t="14" /></span>
            <span class="ctx-label">{{ it.label }}</span>
            <span v-if="it.note" class="ctx-note">{{ it.note }}</span>
            <Icon v-if="it.children?.length" n="right" :t="12" class="ctx-more" />
          </button>
        </template>
      </div>
    </div>
  </transition>
</template>
