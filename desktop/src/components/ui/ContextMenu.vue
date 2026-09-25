<script setup>
/**
 * Menu contextual generico. Se le pasan las opciones ya montadas, asi cada
 * sitio decide que ofrecer segun donde se pulso.
 *
 * Una opcion con `children` no ejecuta nada: abre ese submenu dentro del mismo
 * panel, con una flecha para volver. Se hace asi y no con submenus flotantes
 * porque estos se cierran solos al mover el raton en diagonal y acaban siendo
 * incomodos.
 *
 * Con el teclado (se abre con Mayus+F10 o la tecla de menu sobre una fila):
 * el foco entra en la primera opcion, las flechas recorren, Enter elige,
 * flecha izquierda vuelve del submenu, y Escape cierra devolviendo el foco a
 * donde estaba.
 */
import { ref, computed, watch, nextTick, onMounted, onUnmounted, useTemplateRef } from 'vue'
import { useFocusTrap } from '../../composables/useFocusTrap.js'
import Icon from '../Icon.vue'

const props = defineProps({
  open: Boolean,
  x: { type: Number, default: 0 },
  y: { type: Number, default: 0 },
  items: { type: Array, default: () => [] },
  title: { type: String, default: '' },
  /** se abrio con el teclado: al cerrar, el foco vuelve a donde estaba */
  keyboard: Boolean
})
const emit = defineEmits(['close'])

const panel = useTemplateRef('panel')
const drill = ref(null) // el submenu abierto, si hay alguno
const pos = ref({ left: 0, top: 0 })

const shown = computed(() => (drill.value ? drill.value.children : props.items))
const heading = computed(() => (drill.value ? drill.value.label : props.title))

// Abierto con el raton, al cerrar el foco no se devuelve: se quedaria en la
// fila pulsada y el espacio dejaria de pausar la musica.
useFocusTrap(panel, {
  active: () => props.open,
  initial: '.ctx-item:not([disabled])',
  returnFocus: () => props.keyboard
})

watch(
  () => props.open,
  async (v) => {
    if (!v) {
      drill.value = null
      return
    }
    await nextTick()
    colocar()
  }
)

/** Que no se salga de la ventana. */
function colocar() {
  const el = panel.value
  if (!el) return
  const w = el.offsetWidth || 220
  const h = el.offsetHeight || 200
  pos.value = {
    left: Math.max(6, Math.min(props.x, window.innerWidth - w - 6)),
    top: Math.max(6, Math.min(props.y, window.innerHeight - h - 6))
  }
}

/** Pone el foco en la primera opcion que se pueda elegir (tras cambiar de submenu). */
async function focusFirst() {
  await nextTick()
  panel.value?.querySelector('.ctx-item:not([disabled])')?.focus()
}

function elegir(it) {
  if (it.disabled || it.separator) return
  if (it.children?.length) {
    drill.value = it
    nextTick(colocar)
    focusFirst()
    return
  }
  emit('close')
  it.action?.()
}
function volver() {
  drill.value = null
  focusFirst()
}

function fuera(e) {
  if (props.open && panel.value && !panel.value.contains(e.target)) emit('close')
}
function teclas(e) {
  if (!props.open) return
  if (e.key === 'Escape') {
    e.preventDefault()
    drill.value ? volver() : emit('close')
    return
  }
  if (e.key === 'Tab') {
    emit('close')
    return
  }
  const items = [...(panel.value?.querySelectorAll('.ctx-item:not([disabled]), .ctx-back') || [])]
  if (!items.length) return
  const i = items.indexOf(document.activeElement)
  let to = -1
  if (e.key === 'ArrowDown') to = i < 0 ? 0 : (i + 1) % items.length
  else if (e.key === 'ArrowUp') to = i <= 0 ? items.length - 1 : i - 1
  else if (e.key === 'Home') to = 0
  else if (e.key === 'End') to = items.length - 1
  else if (e.key === 'ArrowLeft' && drill.value) {
    e.preventDefault()
    volver()
    return
  } else if (e.key === 'ArrowRight' && document.activeElement?.dataset?.drill) {
    e.preventDefault()
    document.activeElement.click()
    return
  }
  if (to < 0) return
  e.preventDefault()
  items[to].focus()
}
// Con nombre, no una funcion suelta: una arrow anonima no se puede quitar
// despues, y quedaba escuchando para siempre.
function alRedimensionar() {
  if (props.open) emit('close')
}

onMounted(() => {
  window.addEventListener('mousedown', fuera, true)
  window.addEventListener('keydown', teclas)
  window.addEventListener('resize', alRedimensionar)
})
onUnmounted(() => {
  window.removeEventListener('mousedown', fuera, true)
  window.removeEventListener('keydown', teclas)
  window.removeEventListener('resize', alRedimensionar)
})
</script>

<template>
  <transition name="dropdown">
    <div
      v-if="open"
      ref="panel"
      class="ctx"
      role="menu"
      :aria-label="heading || 'Opciones'"
      :style="{ left: pos.left + 'px', top: pos.top + 'px' }"
    >
      <div v-if="heading" class="ctx-head">
        <button v-if="drill" type="button" class="ctx-back" title="Volver" @click="volver">
          <Icon n="right" :t="12" style="transform: rotate(180deg)" />
        </button>
        <span>{{ heading }}</span>
      </div>

      <div class="ctx-body">
        <template v-for="(it, i) in shown" :key="i">
          <div v-if="it.separator" class="ctx-sep" role="separator"></div>
          <div v-else-if="it.empty" class="ctx-empty">{{ it.label }}</div>
          <button
            v-else
            type="button"
            class="ctx-item"
            role="menuitem"
            :class="{ danger: it.danger, off: it.disabled }"
            :disabled="it.disabled"
            :aria-haspopup="it.children?.length ? 'menu' : undefined"
            :data-drill="it.children?.length ? '1' : undefined"
            @click="elegir(it)"
          >
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
