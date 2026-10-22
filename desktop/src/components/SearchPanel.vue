<script setup>
/**
 * Busqueda avanzada.
 *
 * Todo lo que se puede escribir a mano en la caja («artista:barak bpm>100»)
 * tambien se puede elegir aqui, y al reves: el panel ESCRIBE en la misma caja
 * en vez de llevar su propio estado aparte. Asi no hay dos verdades que
 * puedan discrepar, y de paso se aprende la sintaxis viendo lo que aparece.
 *
 * Lo que no cabe en esa sintaxis —favoritos, estrellas minimas, el orden—
 * viaja como parametros propios.
 */
import { computed } from 'vue'
import Icon from './Icon.vue'
import SelectField from './ui/SelectField.vue'
import TextField from './ui/TextField.vue'
import ToggleField from './ui/ToggleField.vue'
import SliderField from './ui/SliderField.vue'

const props = defineProps({
  query: { type: String, default: '' },
  facets: { type: Object, default: () => ({}) },
  onlyFavorites: Boolean,
  minStars: { type: Number, default: 0 },
  sort: { type: String, default: 'artist' },
  desc: Boolean
})
const emit = defineEmits([
  'update:query',
  'update:onlyFavorites',
  'update:minStars',
  'sort',
  'close'
])

// Campo de la caja -> como se escribe. Se usan los nombres en castellano
// porque son los que ve el usuario en el marcador de posicion.
const TEXT_FIELDS = [
  { k: 'artista', label: 'Artista', facet: 'artists' },
  { k: 'album', label: 'Album', facet: 'albums' },
  { k: 'genero', label: 'Genero', facet: 'genres' },
  { k: 'tono', label: 'Tono', facet: 'keys' },
  { k: 'carpeta', label: 'Carpeta', facet: 'folders' }
]
const NUMBER_FIELDS = [
  { k: 'bpm', label: 'BPM', hint: 'pulsos por minuto' },
  { k: 'duracion', label: 'Duración (s)', hint: 'en segundos' },
  { k: 'bitrate', label: 'Calidad (bps)', hint: 'p. ej. 320000' },
  { k: 'anio', label: 'Año' }
]

const SORTS = [
  { v: 'artist', n: 'Artista' },
  { v: 'title', n: 'Titulo' },
  { v: 'album', n: 'Album' },
  { v: 'genre', n: 'Genero' },
  { v: 'year', n: 'Año' },
  { v: 'key', n: 'Tono' },
  { v: 'folder', n: 'Carpeta' },
  { v: 'duration', n: 'Duracion' },
  { v: 'bpm', n: 'BPM' },
  { v: 'bitrate', n: 'Calidad' },
  { v: 'size', n: 'Tamaño' },
  { v: 'stars', n: 'Estrellas' },
  { v: 'recent', n: 'Más recientes' }
]

/** Lo que hay escrito ahora, partido en filtros, comparaciones y texto libre. */
const parts = computed(() => {
  const fields = {}
  const comparisons = {}
  const loose = []
  for (const t of (props.query || '').split(/\s+/).filter(Boolean)) {
    const colon = t.indexOf(':')
    if (colon > 0) {
      fields[t.slice(0, colon).toLowerCase()] = t.slice(colon + 1)
      continue
    }
    const m = t.match(/^(\w+)(>=|<=|>|<)(.+)$/)
    if (m) {
      ;(comparisons[m[1].toLowerCase()] ||= []).push([m[2], m[3]])
      continue
    }
    loose.push(t)
  }
  return { fields, comparisons, loose }
})

/** Vuelve a montar la caja a partir de las piezas. */
function write(fields, comparisons, loose) {
  const pieces = []
  for (const [k, v] of Object.entries(fields)) if (v) pieces.push(`${k}:${v}`)
  for (const [k, ops] of Object.entries(comparisons)) {
    for (const [op, v] of ops) if (v !== '' && v != null) pieces.push(`${k}${op}${v}`)
  }
  emit('update:query', [...loose, ...pieces].join(' '))
}

const textOf = (k) => parts.value.fields[k] || ''
function setText(k, v) {
  const { fields, comparisons, loose } = parts.value
  const nextFields = { ...fields }
  if (v) nextFields[k] = String(v).replace(/\s+/g, '')
  else delete nextFields[k]
  write(nextFields, comparisons, loose)
}

const numberOf = (k, op) => {
  const pair = (parts.value.comparisons[k] || []).find(([o]) => o === op)
  return pair ? pair[1] : ''
}
function setNumber(k, op, v) {
  const { fields, comparisons, loose } = parts.value
  const nextComparisons = { ...comparisons }
  const rest = (nextComparisons[k] || []).filter(([o]) => o !== op)
  if (v !== '' && v != null) rest.push([op, v])
  if (rest.length) nextComparisons[k] = rest
  else delete nextComparisons[k]
  write(fields, nextComparisons, loose)
}

/** Texto libre: lo que no es ni filtro ni comparacion. */
const freeText = computed({
  get: () => parts.value.loose.join(' '),
  set: (v) => {
    const { fields, comparisons } = parts.value
    write(fields, comparisons, (v || '').split(/\s+/).filter(Boolean))
  }
})

const optionsOf = (facet) => [
  { v: '', n: 'Cualquiera' },
  ...(props.facets[facet] || [])
    .slice(0, 300)
    .map((f) => ({ v: f.value, n: f.value, note: String(f.n) }))
]

const hasAnything = computed(
  () => !!props.query.trim() || props.onlyFavorites || props.minStars > 0
)

function clearAll() {
  emit('update:query', '')
  emit('update:onlyFavorites', false)
  emit('update:minStars', 0)
  emit('sort', 'artist', false)
}
</script>

<template>
  <div class="card search-panel">
    <div class="menu-block">
      <div class="menu-head">Buscar</div>
      <TextField
        :model-value="freeText"
        label="Texto"
        width="100%"
        icon="search"
        placeholder="palabras sueltas"
        @update:model-value="(v) => (freeText = v)"
      />
      <SelectField
        v-for="t in TEXT_FIELDS"
        :key="t.k"
        :label="t.label"
        clearable
        :model-value="textOf(t.k)"
        :options="optionsOf(t.facet)"
        @update:model-value="(v) => setText(t.k, v)"
      />
    </div>

    <div class="menu-block">
      <div class="menu-head">Entre dos valores</div>
      <div v-for="n in NUMBER_FIELDS" :key="n.k" class="range">
        <span class="field-label">{{ n.label }}</span>
        <div class="range-fields">
          <TextField
            type="number"
            placeholder="desde"
            width="100%"
            :model-value="numberOf(n.k, '>')"
            @update:model-value="(v) => setNumber(n.k, '>', v)"
          />
          <span class="range-sep">—</span>
          <TextField
            type="number"
            placeholder="hasta"
            width="100%"
            :model-value="numberOf(n.k, '<')"
            @update:model-value="(v) => setNumber(n.k, '<', v)"
          />
        </div>
      </div>
    </div>

    <div class="menu-block">
      <div class="menu-head">Filtrar</div>
      <ToggleField
        :model-value="onlyFavorites"
        title="Solo favoritos"
        @update:model-value="(v) => emit('update:onlyFavorites', v)"
      />
      <SliderField
        :model-value="minStars"
        :min="0"
        :max="5"
        :step="1"
        label="Estrellas minimas"
        :value-text="minStars ? minStars + ' o más' : 'cualquiera'"
        @update:model-value="(v) => emit('update:minStars', v)"
      />
    </div>

    <div class="menu-block">
      <div class="menu-head">Ordenar</div>
      <SelectField
        :model-value="sort"
        label="Por"
        :options="SORTS"
        @update:model-value="(v) => emit('sort', v, desc)"
      />
      <div class="btn-row">
        <button class="btn mini" :class="{ on: !desc }" @click="emit('sort', sort, false)">
          <Icon n="down" :t="12" style="transform: rotate(180deg)" /> Ascendente
        </button>
        <button class="btn mini" :class="{ on: desc }" @click="emit('sort', sort, true)">
          <Icon n="down" :t="12" /> Descendente
        </button>
      </div>
    </div>

    <div class="btn-row search-panel-foot">
      <button class="btn mini" :disabled="!hasAnything" @click="clearAll">Limpiar todo</button>
      <button class="btn mini primary" @click="emit('close')">Listo</button>
    </div>
  </div>
</template>
