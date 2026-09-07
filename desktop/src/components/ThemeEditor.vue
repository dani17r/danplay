<script setup>
import { ref, watch, onUnmounted } from 'vue'
import { CATALOG, FIELDS, allThemes, saveCustomTheme, applyTheme } from '../themes.js'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'
import ColorField from './ui/ColorField.vue'
import Icon from './Icon.vue'
import Card from './ui/Card.vue'

const props = defineProps(['activeTheme','editing'])
const emit = defineEmits(['close','saved'])

const base = ref(props.editing || props.activeTheme || 'night')
const partida = allThemes()[base.value] || CATALOG.noche
const name = ref(props.editing ? partida.name : partida.name + ' (mio)')
const kind = ref(partida.kind || 'oscuro')
const v = ref({ ...partida.v })
const avanzado = ref(false)
const BASICOS = ['fondo', 'panel', 'text', 'acento', 'tenue', 'ambar']

// vista previa en vivo mientras se toca
watch(v, (nv) => applyTheme('__previa', { v: nv, kind: kind.value }), { deep: true })
watch(kind, () => applyTheme('__previa', { v: v.value, kind: kind.value }))

function partirDe (key) {
  const t = allThemes()[key]
  if (!t) return
  v.value = { ...t.v }; kind.value = t.kind || 'oscuro'
  if (!props.editing) name.value = t.name + ' (mio)'
}
function save () {
  const limpio = (name.value || 'Mi tema').trim()
  const key = props.editing || 'propio-' + limpio.toLowerCase()
    .normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '') || 'propio-' + Date.now()
  saveCustomTheme(key, { name: limpio, kind: kind.value, v: { ...v.value }, custom: true })
  applyTheme(key)
  emit('saved', key)
}
function cancelDownload () { applyTheme(props.activeTheme); emit('close') }
onUnmounted(() => { if (document.documentElement.dataset.theme === '__previa') applyTheme(props.activeTheme) })
</script>

<template>
  <Card style="border-color:var(--accent)" :title="editing ? 'Editar tema' : 'Crear tema custom'" note="Los cambios se ven al momento. Nada se guarda hasta que pulses Guardar.">

    <div style="display:flex;gap:11px;flex-wrap:wrap;align-items:flex-end;margin-bottom:16px">
      <TextField v-model="name" label="Nombre" placeholder="Mi tema"
             width="minmax(180px, 1fr)" style="flex:1;min-width:180px" />
      <SelectField v-if="!editing" :modelValue="base" label="Partir de" width="190px"
                :options="Object.entries(allThemes()).map(([k,t]) =>
                           ({v:k, n:t.name, color:t.v.acento}))"
                @update:modelValue="v => { base = v; partirDe(v) }" />
      <SelectField v-model="kind" label="Base" width="140px"
                :options="[{v:'dark',n:'Oscuro'},{v:'light',n:'Claro'}]" />
    </div>

    <div style="display:grid;grid-template-columns:repeat(auto-fill,minmax(232px,1fr));gap:11px">
      <ColorField v-for="c in FIELDS.filter(c => avanzado || BASICOS.includes(c.k))" :key="c.k"
             v-model="v[c.k]" :title="c.n" :hint="c.d" />
    </div>

    <div class="btn-row" style="margin-top:15px">
      <button class="btn primary" @click="save" style="gap:7px">
        <Icon n="save" :t="15" /> Guardar tema</button>
      <button class="btn" @click="cancelDownload">Cancelar</button>
      <button class="btn mini" @click="avanzado=!avanzado" style="margin-left:auto;gap:6px">
        <Icon n="palette" :t="14" />
        {{ avanzado ? 'Menos colores' : 'Todos los colores (12)' }}</button>
    </div>
  </Card>
</template>
