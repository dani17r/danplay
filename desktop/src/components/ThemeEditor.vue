<script setup>
/**
 * Crear un tema propio, o editar uno que ya lo sea.
 *
 * Los colores se ven al momento (vista previa `__preview`) y no se guarda
 * nada hasta pulsar Guardar; Cancelar deja el tema que habia.
 */
import { ref, watch, onUnmounted } from 'vue'
import { CATALOG, FIELDS, allThemes, saveCustomTheme, applyTheme } from '../themes.js'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'
import ColorField from './ui/ColorField.vue'
import Icon from './Icon.vue'
import Card from './ui/Card.vue'

const props = defineProps({
  /** el tema que esta puesto, al que se vuelve al cancelar */
  activeTheme: { type: String, default: 'night' },
  /** la clave del tema propio que se edita; sin ella, se crea uno */
  editing: { type: String, default: null }
})
const emit = defineEmits(['close', 'saved'])

const base = ref('night')
const name = ref('')
const kind = ref('dark')
const v = ref({})
const advanced = ref(false)
// Los seis que casi todo el mundo toca. Antes esta lista estaba en castellano
// y solo dos nombres coincidían con los campos de verdad, así que el modo
// básico enseñaba dos colores en vez de seis.
const BASIC = ['bg', 'panel', 'text', 'accent', 'muted', 'amber']

/**
 * Parte del tema que se edita (o del que esta puesto, si se crea uno).
 *
 * Se vuelve a llamar si cambia el que se edita: con el editor abierto,
 * «Editar» en otro tema cambiaba el titulo pero se quedaban los colores del
 * primero, y «Guardar» los escribia en el segundo.
 */
function begin() {
  base.value = props.editing || props.activeTheme || 'night'
  // `CATALOG.noche` no existe (la clave es `night`) y `'oscuro'` no es un kind
  // válido: eran restos del paso a inglés. Con la clave mal, el respaldo dejaba
  // el editor sin colores de partida.
  const source = allThemes()[base.value] || CATALOG.night
  name.value = props.editing ? source.name : source.name + ' (mío)'
  kind.value = source.kind || 'dark'
  v.value = { ...source.v }
}
begin()
watch(() => props.editing, begin)

// vista previa en vivo mientras se toca
watch(v, (nv) => applyTheme('__preview', { v: nv, kind: kind.value }), { deep: true })
watch(kind, () => applyTheme('__preview', { v: v.value, kind: kind.value }))

function startFrom(key) {
  const t = allThemes()[key]
  if (!t) return
  base.value = key
  v.value = { ...t.v }
  kind.value = t.kind || 'dark'
  if (!props.editing) name.value = t.name + ' (mío)'
}
function save() {
  const clean = (name.value || 'Mi tema').trim()
  const key =
    props.editing ||
    'propio-' +
      clean
        .toLowerCase()
        .normalize('NFD')
        .replace(/[̀-ͯ]/g, '')
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-|-$/g, '') ||
    'propio-' + Date.now()
  saveCustomTheme(key, { name: clean, kind: kind.value, v: { ...v.value }, custom: true })
  applyTheme(key)
  emit('saved', key)
}
function cancel() {
  applyTheme(props.activeTheme)
  emit('close')
}
onUnmounted(() => {
  if (document.documentElement.dataset.theme === '__preview') applyTheme(props.activeTheme)
})
</script>

<template>
  <Card
    style="border-color: var(--accent)"
    :title="editing ? 'Editar tema' : 'Crear tema custom'"
    note="Los cambios se ven al momento. Nada se guarda hasta que pulses Guardar."
  >
    <div
      style="display: flex; gap: 11px; flex-wrap: wrap; align-items: flex-end; margin-bottom: 16px"
    >
      <TextField
        v-model="name"
        label="Nombre"
        placeholder="Mi tema"
        width="minmax(180px, 1fr)"
        style="flex: 1; min-width: 180px"
      />
      <SelectField
        v-if="!editing"
        :model-value="base"
        label="Partir de"
        width="190px"
        :options="
          Object.entries(allThemes()).map(([k, t]) => ({ v: k, n: t.name, color: t.v.accent }))
        "
        @update:model-value="startFrom"
      />
      <SelectField
        v-model="kind"
        label="Base"
        width="140px"
        :options="[
          { v: 'dark', n: 'Oscuro' },
          { v: 'light', n: 'Claro' }
        ]"
      />
    </div>

    <div
      style="display: grid; grid-template-columns: repeat(auto-fill, minmax(232px, 1fr)); gap: 11px"
    >
      <ColorField
        v-for="c in FIELDS.filter((c) => advanced || BASIC.includes(c.k))"
        :key="c.k"
        v-model="v[c.k]"
        :title="c.n"
        :hint="c.d"
      />
    </div>

    <div class="btn-row" style="margin-top: 15px">
      <button type="button" class="btn primary" style="gap: 7px" @click="save">
        <Icon n="save" :t="15" /> Guardar tema
      </button>
      <button type="button" class="btn" @click="cancel">Cancelar</button>
      <button
        type="button"
        class="btn mini"
        style="margin-left: auto; gap: 6px"
        :aria-pressed="advanced"
        @click="advanced = !advanced"
      >
        <Icon n="palette" :t="14" />
        {{ advanced ? 'Menos colores' : 'Todos los colores (12)' }}
      </button>
    </div>
  </Card>
</template>
