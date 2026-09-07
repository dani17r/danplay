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
import { ref, computed, watch } from 'vue'
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
const emit = defineEmits(['update:query', 'update:onlyFavorites', 'update:minStars',
                          'sort', 'close'])

// Campo de la caja -> como se escribe. Se usan los nombres en castellano
// porque son los que ve el usuario en el marcador de posicion.
const TEXTOS = [
  { k: 'artista', label: 'Artista', faceta: 'artists' },
  { k: 'album',   label: 'Album',   faceta: 'albums' },
  { k: 'genero',  label: 'Genero',  faceta: 'genres' },
  { k: 'tono',    label: 'Tono',    faceta: 'keys' },
  { k: 'carpeta', label: 'Carpeta', faceta: 'folders' }
]
const NUMEROS = [
  { k: 'bpm',      label: 'BPM',           hint: 'pulsos por minuto' },
  { k: 'duracion', label: 'Duracion (s)',  hint: 'en segundos' },
  { k: 'bitrate',  label: 'Calidad (bps)', hint: 'p. ej. 320000' },
  { k: 'anio',     label: 'Año' }
]

const SORTS = [
  { v: 'artist', n: 'Artista' }, { v: 'title', n: 'Titulo' },
  { v: 'album', n: 'Album' }, { v: 'genre', n: 'Genero' },
  { v: 'year', n: 'Año' }, { v: 'key', n: 'Tono' },
  { v: 'folder', n: 'Carpeta' }, { v: 'duration', n: 'Duracion' },
  { v: 'bpm', n: 'BPM' }, { v: 'bitrate', n: 'Calidad' },
  { v: 'size', n: 'Tamaño' }, { v: 'stars', n: 'Estrellas' },
  { v: 'recent', n: 'Mas recientes' }
]

/** Lo que hay escrito ahora, partido en filtros, comparaciones y texto libre. */
const partes = computed(() => {
  const campos = {}
  const comparaciones = {}
  const sueltas = []
  for (const t of (props.query || '').split(/\s+/).filter(Boolean)) {
    const dosPuntos = t.indexOf(':')
    if (dosPuntos > 0) { campos[t.slice(0, dosPuntos).toLowerCase()] = t.slice(dosPuntos + 1); continue }
    const m = t.match(/^(\w+)(>=|<=|>|<)(.+)$/)
    if (m) { (comparaciones[m[1].toLowerCase()] ||= []).push([m[2], m[3]]); continue }
    sueltas.push(t)
  }
  return { campos, comparaciones, sueltas }
})

/** Vuelve a montar la caja a partir de las piezas. */
function escribir (campos, comparaciones, sueltas) {
  const trozos = []
  for (const [k, v] of Object.entries(campos)) if (v) trozos.push(`${k}:${v}`)
  for (const [k, ops] of Object.entries(comparaciones)) {
    for (const [op, v] of ops) if (v !== '' && v != null) trozos.push(`${k}${op}${v}`)
  }
  emit('update:query', [...sueltas, ...trozos].join(' '))
}

const texto = (k) => partes.value.campos[k] || ''
function setTexto (k, v) {
  const { campos, comparaciones, sueltas } = partes.value
  const nuevos = { ...campos }
  if (v) nuevos[k] = String(v).replace(/\s+/g, '')
  else delete nuevos[k]
  escribir(nuevos, comparaciones, sueltas)
}

const numero = (k, op) => {
  const par = (partes.value.comparaciones[k] || []).find(([o]) => o === op)
  return par ? par[1] : ''
}
function setNumero (k, op, v) {
  const { campos, comparaciones, sueltas } = partes.value
  const nuevas = { ...comparaciones }
  const resto = (nuevas[k] || []).filter(([o]) => o !== op)
  if (v !== '' && v != null) resto.push([op, v])
  if (resto.length) nuevas[k] = resto; else delete nuevas[k]
  escribir(campos, nuevas, sueltas)
}

/** Texto libre: lo que no es ni filtro ni comparacion. */
const libre = computed({
  get: () => partes.value.sueltas.join(' '),
  set: (v) => {
    const { campos, comparaciones } = partes.value
    escribir(campos, comparaciones, (v || '').split(/\s+/).filter(Boolean))
  }
})

const opciones = (faceta) => [
  { v: '', n: 'Cualquiera' },
  ...((props.facets[faceta] || []).slice(0, 300)
    .map(f => ({ v: f.value, n: f.value, note: String(f.n) })))
]

const hayAlgo = computed(() =>
  !!props.query.trim() || props.onlyFavorites || props.minStars > 0)

function limpiar () {
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
      <TextField :modelValue="libre" @update:modelValue="v => libre = v"
                 label="Texto" width="100%" icon="search"
                 placeholder="palabras sueltas" />
      <SelectField v-for="t in TEXTOS" :key="t.k" :label="t.label" clearable
                   :modelValue="texto(t.k)" :options="opciones(t.faceta)"
                   @update:modelValue="v => setTexto(t.k, v)" />
    </div>

    <div class="menu-block">
      <div class="menu-head">Entre dos valores</div>
      <div v-for="n in NUMEROS" :key="n.k" class="rango">
        <span class="field-label">{{ n.label }}</span>
        <div class="rango-campos">
          <TextField type="number" placeholder="desde" width="100%"
                     :modelValue="numero(n.k, '>')"
                     @update:modelValue="v => setNumero(n.k, '>', v)" />
          <span class="rango-sep">—</span>
          <TextField type="number" placeholder="hasta" width="100%"
                     :modelValue="numero(n.k, '<')"
                     @update:modelValue="v => setNumero(n.k, '<', v)" />
        </div>
      </div>
    </div>

    <div class="menu-block">
      <div class="menu-head">Filtrar</div>
      <ToggleField :modelValue="onlyFavorites" title="Solo favoritos"
                   @update:modelValue="v => emit('update:onlyFavorites', v)" />
      <SliderField :modelValue="minStars" :min="0" :max="5" :step="1"
                   label="Estrellas minimas"
                   :valueText="minStars ? minStars + ' o mas' : 'cualquiera'"
                   @update:modelValue="v => emit('update:minStars', v)" />
    </div>

    <div class="menu-block">
      <div class="menu-head">Ordenar</div>
      <SelectField :modelValue="sort" label="Por" :options="SORTS"
                   @update:modelValue="v => emit('sort', v, desc)" />
      <div class="btn-row">
        <button class="btn mini" :class="{on: !desc}" @click="emit('sort', sort, false)">
          <Icon n="down" :t="12" style="transform:rotate(180deg)" /> Ascendente</button>
        <button class="btn mini" :class="{on: desc}" @click="emit('sort', sort, true)">
          <Icon n="down" :t="12" /> Descendente</button>
      </div>
    </div>

    <div class="btn-row search-panel-foot">
      <button class="btn mini" :disabled="!hayAlgo" @click="limpiar">Limpiar todo</button>
      <button class="btn mini primary" @click="emit('close')">Listo</button>
    </div>
  </div>
</template>
