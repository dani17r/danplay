<script setup>
/**
 * Un desplegable propio (los <select> del sistema ignoran el tema).
 *
 * Para quien no ve la pantalla es lo mismo que uno nativo: el boton dice que
 * abre una lista, si esta abierta y cual es la opcion resaltada, y su nombre
 * es la etiqueta de encima. Con el teclado: flechas, Enter y Escape.
 */
import { ref, computed, nextTick, useId, useTemplateRef } from 'vue'
import { onClickOutside } from '../../composables/useClickOutside.js'
import Icon from '../Icon.vue'

const model = defineModel({ type: [String, Number, Boolean], default: null })
const props = defineProps({
  options: { type: Array, required: true }, // [{v, n, note?, color?}]
  label: { type: String, default: '' },
  width: { type: String, default: '' },
  disabled: Boolean,
  /** Enseña una «x» para dejarlo sin elegir. `emptyValue` es a que valor vuelve. */
  clearable: Boolean,
  emptyValue: { type: [String, Number, Boolean], default: '' }
})
const open = ref(false)
const root = useTemplateRef('root')
const highlighted = ref(0)
const uid = useId()
const ids = { label: `${uid}-label`, button: `${uid}-button`, list: `${uid}-list` }
const optionId = (i) => `${uid}-opt-${i}`

const current = computed(
  () => props.options.find((o) => o.v === model.value) || props.options[0] || { n: '—' }
)
/** ¿Hay algo elegido que se pueda quitar? */
const canClear = computed(
  () =>
    props.clearable &&
    !props.disabled &&
    model.value !== props.emptyValue &&
    model.value != null &&
    model.value !== ''
)
function clear() {
  model.value = props.emptyValue
  open.value = false
}

function toggle() {
  if (props.disabled) return
  open.value = !open.value
  if (open.value) {
    highlighted.value = Math.max(
      0,
      props.options.findIndex((o) => o.v === model.value)
    )
    nextTick(() => {
      const el = root.value?.querySelector('.select-opt.current')
      if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'nearest' })
    })
  }
}
function choose(o) {
  model.value = o.v
  open.value = false
}
function onKey(e) {
  if (!open.value) {
    if (['Enter', ' ', 'ArrowDown'].includes(e.key)) {
      e.preventDefault()
      toggle()
    }
    return
  }
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    open.value = false
    return
  }
  if (e.key === 'Tab') {
    open.value = false
    return
  }
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    highlighted.value = Math.min(props.options.length - 1, highlighted.value + 1)
  }
  if (e.key === 'ArrowUp') {
    e.preventDefault()
    highlighted.value = Math.max(0, highlighted.value - 1)
  }
  if (e.key === 'Home') {
    e.preventDefault()
    highlighted.value = 0
  }
  if (e.key === 'End') {
    e.preventDefault()
    highlighted.value = props.options.length - 1
  }
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    choose(props.options[highlighted.value])
  }
}
onClickOutside(root, () => {
  open.value = false
})
</script>

<template>
  <div ref="root" class="field" :class="{ off: disabled }" :style="width ? { width: width } : null">
    <span v-if="label" :id="ids.label" class="field-label">{{ label }}</span>
    <button
      :id="ids.button"
      class="select-box"
      type="button"
      :class="{ open }"
      :disabled="disabled"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="open ? ids.list : undefined"
      :aria-labelledby="label ? `${ids.label} ${ids.button}` : undefined"
      :aria-activedescendant="open ? optionId(highlighted) : undefined"
      @click="toggle"
      @keydown="onKey"
    >
      <span v-if="current.color" class="select-dot" :style="{ background: current.color }"></span>
      <span class="select-text">{{ current.n }}</span>
      <Icon n="down" :t="14" class="select-arrow" />
    </button>
    <button
      v-if="canClear"
      type="button"
      class="select-clear"
      tabindex="-1"
      title="Quitar la selección"
      @click.stop="clear"
    >
      <Icon n="close" :t="13" />
    </button>
    <transition name="dropdown">
      <div
        v-if="open"
        :id="ids.list"
        class="select-menu"
        role="listbox"
        :aria-labelledby="label ? ids.label : undefined"
        :aria-label="label ? undefined : current.n"
      >
        <!-- el raton resalta al pasar y el teclado desde el boton de arriba;
             si una opcion llega a tener el foco, tambien se resalta -->
        <button
          v-for="(o, i) in options"
          :id="optionId(i)"
          :key="o.v"
          type="button"
          class="select-opt"
          role="option"
          tabindex="-1"
          :aria-selected="o.v === model"
          :class="{ current: o.v === model, hilite: i === highlighted }"
          @click="choose(o)"
          @mouseenter="highlighted = i"
          @focus="highlighted = i"
        >
          <span v-if="o.color" class="select-dot" :style="{ background: o.color }"></span>
          <span style="flex: 1; min-width: 0">
            <span class="select-opt-name">{{ o.n }}</span>
            <span v-if="o.note" class="select-opt-note">{{ o.note }}</span>
          </span>
          <Icon v-if="o.v === model" n="check" :t="14" />
        </button>
      </div>
    </transition>
  </div>
</template>
