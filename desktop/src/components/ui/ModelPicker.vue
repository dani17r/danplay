<script setup>
/**
 * Elegir un modelo de IA: una caja donde se escribe (vale cualquier nombre,
 * por si el proveedor no lo lista) con una lista debajo que enseña lo que
 * se sabe de cada uno: si usa herramientas, precio, contexto, fecha, si
 * está obsoleto y cuál se recomienda. La lista viene del proveedor (lo que
 * tu clave puede usar) o del catálogo de models.dev, y se filtra por lo que
 * vas escribiendo.
 */
import { ref, computed, watch, nextTick } from 'vue'
import { onClickOutside } from '../../composables/useClickOutside.js'
import Icon from '../Icon.vue'
import Loading from './Loading.vue'

const props = defineProps({
  modelValue: { type: String, default: '' },
  /** [{id, name, tools, cost_in, cost_out, context, released, deprecated, known}] */
  models: { type: Array, default: () => [] },
  label: String,
  hint: String,
  placeholder: { type: String, default: 'escribe o elige un modelo' },
  /** el id recomendado para este papel (se marca y va el primero) */
  suggest: { type: String, default: '' },
  /** este papel exige herramientas: se filtran de entrada los que no las usan */
  needTools: Boolean,
  loading: Boolean,
  disabled: Boolean
})
const emit = defineEmits(['update:modelValue'])

const open = ref(false)
const root = ref(null)
// lo escrito, aparte del valor: asi la lista se filtra al momento aunque el
// padre tarde en devolver el v-model
const text = ref(props.modelValue || '')
watch(() => props.modelValue, (v) => { text.value = v || '' })
const highlighted = ref(0)
const onlyTools = ref(props.needTools)
const hideOld = ref(true)
const sort = ref('new')          // new | cheap | name
const SORTS = { new: 'más nuevos', cheap: 'más baratos', name: 'por nombre' }

const known = computed(() => props.models.some((m) => m.known || m.released || m.cost_out != null))

const shown = computed(() => {
  const q = (text.value || '').trim().toLowerCase()
  let list = props.models
  // lo escrito filtra; si coincide exacto con uno, se enseña la lista entera
  // (ya está elegido: lo que se quiere es ver alternativas, no una sola fila)
  const exact = list.some((m) => m.id.toLowerCase() === q)
  if (q && !exact) list = list.filter((m) => (m.id + ' ' + (m.name || '')).toLowerCase().includes(q))
  if (onlyTools.value) list = list.filter((m) => m.tools !== false)
  if (hideOld.value) list = list.filter((m) => !m.deprecated)
  const by = {
    new: (a, b) => (b.released || '').localeCompare(a.released || '') || a.id.localeCompare(b.id),
    cheap: (a, b) => (a.cost_out ?? 1e9) - (b.cost_out ?? 1e9) || a.id.localeCompare(b.id),
    name: (a, b) => (a.name || a.id).localeCompare(b.name || b.id)
  }
  list = [...list].sort(by[sort.value])
  if (props.suggest) {
    const i = list.findIndex((m) => m.id === props.suggest)
    if (i > 0) list.unshift(...list.splice(i, 1))
  }
  return list.slice(0, 400)
})

const current = computed(() => props.models.find((m) => m.id === props.modelValue))
const warn = computed(() => (current.value?.tools === false ? 'sin herramientas' : current.value?.deprecated ? 'obsoleto' : ''))

/** Precio por millón de tokens, corto: «$0.15 → $0.60». */
function price (m) {
  if (m.cost_in == null && m.cost_out == null) return ''
  const f = (v) => (v == null ? '?' : v === 0 ? 'gratis' : '$' + (v < 1 ? v.toFixed(2).replace(/0$/, '') : v.toFixed(v >= 10 ? 0 : 1)))
  return `${f(m.cost_in)} → ${f(m.cost_out)}`
}
function context (m) {
  if (!m.context) return ''
  return m.context >= 1e6 ? (m.context / 1e6).toFixed(1).replace('.0', '') + 'M' : Math.round(m.context / 1000) + 'k'
}

function choose (m) {
  emit('update:modelValue', m.id)
  open.value = false
}
function type (e) {
  text.value = e.target.value
  emit('update:modelValue', e.target.value)
  open.value = true
  highlighted.value = 0
}
function onKey (e) {
  if (e.key === 'Escape') { open.value = false; return }
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    if (!open.value) { open.value = true; return }
    highlighted.value = Math.min(shown.value.length - 1, highlighted.value + 1)
    scrollToHighlighted()
  }
  if (e.key === 'ArrowUp') { e.preventDefault(); highlighted.value = Math.max(0, highlighted.value - 1); scrollToHighlighted() }
  if (e.key === 'Enter' && open.value && shown.value[highlighted.value]) {
    e.preventDefault(); choose(shown.value[highlighted.value])
  }
}
async function scrollToHighlighted () {
  await nextTick()
  const el = root.value?.querySelector('.model-row.hilite')
  if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'nearest' })
}
watch(() => props.needTools, (v) => { onlyTools.value = v })
onClickOutside(root, () => { open.value = false })
</script>

<template>
  <div class="field model-picker" :class="{off: disabled, open}" ref="root">
    <span v-if="label || warn" class="field-label">{{ label }}
      <em v-if="warn" class="model-warn">{{ warn }}</em>
    </span>
    <span class="field-box">
      <Icon n="sparkles" :t="15" class="field-icon" />
      <input :value="text" :placeholder="placeholder" :disabled="disabled" spellcheck="false"
             autocomplete="off" @input="type" @focus="open = true" @keydown="onKey" />
      <Loading v-if="loading" text="" inline />
      <button v-else type="button" class="field-btn" tabindex="-1" title="Ver la lista"
              @click.prevent="open = !open">
        <Icon n="down" :t="14" />
      </button>
    </span>
    <span v-if="hint" class="field-hint">{{ hint }}</span>

    <transition name="dropdown">
      <div v-if="open" class="model-menu">
        <div class="model-filters">
          <label class="chip" :class="{on: onlyTools}">
            <input type="checkbox" v-model="onlyTools" /> con herramientas</label>
          <label class="chip" :class="{on: hideOld}">
            <input type="checkbox" v-model="hideOld" /> sin obsoletos</label>
          <span class="model-sort">
            <button v-for="(n, k) in SORTS" :key="k" type="button" class="chip" :class="{on: sort === k}"
                    @click="sort = k">{{ n }}</button>
          </span>
        </div>
        <div v-if="!models.length" class="model-empty">
          {{ loading ? 'buscando modelos…' : 'sin lista: escribe el nombre del modelo tal cual lo llama el proveedor' }}
        </div>
        <div v-else-if="!shown.length" class="model-empty">nada coincide con lo escrito
          <span v-if="onlyTools || hideOld"> (prueba quitando los filtros)</span></div>
        <div v-else class="model-list">
          <button v-for="(m, i) in shown" :key="m.id" type="button" class="model-row"
                  :class="{hilite: i === highlighted, current: m.id === modelValue, old: m.deprecated}"
                  @click="choose(m)" @mouseenter="highlighted = i">
            <span class="model-main">
              <span class="model-name">{{ m.name && m.name !== m.id ? m.name : m.id }}</span>
              <span class="model-badges">
                <span v-if="m.id === suggest" class="tag accent">recomendado</span>
                <span v-if="m.tools" class="tag ok">herramientas</span>
                <span v-else-if="m.tools === false" class="tag bad">sin herramientas</span>
                <span v-if="m.reasoning" class="tag">razona</span>
                <span v-if="m.deprecated" class="tag bad">obsoleto</span>
                <span v-if="m.trains" class="tag bad" title="El proveedor avisa de que puede entrenar con lo que le mandes">entrena con tus datos</span>
                <span v-if="!m.known && known" class="tag">sin ficha</span>
              </span>
            </span>
            <span class="model-sub mono">
              <span v-if="m.name && m.name !== m.id">{{ m.id }}</span>
              <span v-if="price(m)" :title="'por millón de tokens: entrada → salida'">{{ price(m) }}</span>
              <span v-if="context(m)">{{ context(m) }} ctx</span>
              <span v-if="m.released">{{ m.released }}</span>
            </span>
          </button>
        </div>
      </div>
    </transition>
  </div>
</template>
