<script setup>
import { ref, computed, nextTick } from 'vue'
import { onClickOutside } from '../../composables/useClickOutside.js'
import Icon from '../Icon.vue'

const props = defineProps({
  modelValue: [String, Number, Boolean],
  options: { type: Array, required: true },   // [{v, n, nota?, color?}]
  label: String, width: String, disabled: Boolean
})
const emit = defineEmits(['update:modelValue'])
const open = ref(false)
const root = ref(null)
const highlighted = ref(0)

const current = computed(() =>
  props.options.find(o => o.v === props.modelValue) || props.options[0] || { n: '—' })

function togglePlay () {
  if (props.disabled) return
  open.value = !open.value
  if (open.value) {
    highlighted.value = Math.max(0, props.options.findIndex(o => o.v === props.modelValue))
    nextTick(() => {
      const el = root.value?.querySelector('.select-opt.res')
      if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'nearest' })
    })
  }
}
function choose (o) { emit('update:modelValue', o.v); open.value = false }
function onKey (e) {
  if (!open.value) {
    if (['Enter', ' ', 'ArrowDown'].includes(e.key)) { e.preventDefault(); togglePlay() }
    return
  }
  if (e.key === 'Escape') { open.value = false; return }
  if (e.key === 'ArrowDown') { e.preventDefault(); highlighted.value = Math.min(props.options.length - 1, highlighted.value + 1) }
  if (e.key === 'ArrowUp') { e.preventDefault(); highlighted.value = Math.max(0, highlighted.value - 1) }
  if (e.key === 'Enter') { e.preventDefault(); choose(props.options[highlighted.value]) }
}
onClickOutside(root, () => { open.value = false })
</script>

<template>
  <div class="field" :class="{off: disabled}" :style="width ? {width: width} : null" ref="root">
    <span v-if="label" class="field-label">{{ label }}</span>
    <button class="select-box" type="button" :class="{open}" :disabled="disabled"
            @click="togglePlay" @keydown="onKey">
      <span v-if="current.color" class="select-dot" :style="{background: current.color}"></span>
      <span class="select-text">{{ current.n }}</span>
      <Icon n="down" :t="14" class="select-arrow" />
    </button>
    <transition name="dropdown">
      <div v-if="open" class="select-menu">
        <button v-for="(o, i) in options" :key="o.v" type="button" class="select-opt"
                :class="{current: o.v === modelValue, hilite: i === highlighted}"
                @click="choose(o)" @mouseenter="highlighted = i">
          <span v-if="o.color" class="select-dot" :style="{background: o.color}"></span>
          <span style="flex:1;min-width:0">
            <span class="select-opt-name">{{ o.n }}</span>
            <span v-if="o.note" class="select-opt-note">{{ o.note }}</span>
          </span>
          <Icon v-if="o.v === modelValue" n="check" :t="14" />
        </button>
      </div>
    </transition>
  </div>
</template>
