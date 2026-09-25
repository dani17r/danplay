<script setup>
/**
 * La lista agrupada (por carpeta, artista, álbum, inicial, género o tono).
 *
 * Es UNA sola lista, con la cabecera de cada grupo entre sus filas: se
 * recorta a lo que se ve como cualquier otra (ver useVirtualRows) y las
 * flechas pasan de un grupo al siguiente. Antes era una lista por grupo, y
 * las de menos de 80 canciones se pintaban enteras: «Artistas», que casi
 * siempre son grupos pequeños, montaba la biblioteca completa.
 */
import { computed, ref } from 'vue'
import SongTable from './SongTable.vue'
import SongGrid from './SongGrid.vue'
import SongRows from './SongRows.vue'
import SongCards from './SongCards.vue'
import { groupSongs } from '../utils/groups.js'

const props = defineProps({
  songs: { type: Array, required: true },
  /** por qué se agrupa: folder, artist, album, initial, genre o key */
  by: { type: String, required: true },
  selected: { type: Number, default: null },
  selectedIds: { type: Array, default: () => [] },
  playing: { type: Number, default: null },
  /** rows, cards, grid o table */
  layout: { type: String, default: 'table' },
  size: { type: Number, default: 164 },
  jumpTo: { type: Number, default: null },
  sort: { type: String, default: '' },
  desc: Boolean
})
const emit = defineEmits(['select', 'play', 'setStars', 'toggleFavorite', 'sortBy', 'context'])
const collapsed = ref(new Set())

// Los mismos grupos, en el mismo orden, que usa la app para elegir con Mayús
// y armar la cola (utils/groups.js).
const grouped = computed(() => groupSongs(props.songs, props.by))

/** Lo que se pinta: las canciones de los grupos abiertos, y dónde empieza cada grupo. */
const view = computed(() => {
  const songs = []
  const groups = grouped.value.map(([key, items]) => {
    const open = !collapsed.value.has(key)
    const group = {
      key,
      start: songs.length,
      size: open ? items.length : 0,
      count: items.length,
      seconds: items.reduce((t, c) => t + (c.duration || 0), 0),
      open
    }
    if (open) songs.push(...items)
    return group
  })
  return { songs, groups }
})

function toggleGroup(k) {
  const s = new Set(collapsed.value)
  if (s.has(k)) s.delete(k)
  else s.add(k)
  collapsed.value = s
}

const LISTS = { rows: SongRows, cards: SongCards, grid: SongGrid }
const list = computed(() => LISTS[props.layout] || null)

/** Lo que recibe la vista: lo común, lo propio de cada una y sus avisos. */
const bindings = computed(() => {
  const common = {
    songs: view.value.songs,
    groups: view.value.groups,
    selected: props.selected,
    selectedIds: props.selectedIds,
    playing: props.playing,
    jumpTo: props.jumpTo,
    onSelect: (id, ev) => emit('select', id, ev),
    onPlay: (c) => emit('play', c),
    onContext: (ev, c) => emit('context', ev, c),
    onToggleGroup: toggleGroup
  }
  if (props.layout === 'grid') return { ...common, size: props.size }
  const rated = {
    ...common,
    onSetStars: (c, n) => emit('setStars', c, n),
    onToggleFavorite: (c) => emit('toggleFavorite', c)
  }
  if (list.value) return rated
  // la tabla, sin la fila de las columnas: la cabecera de cada grupo se queda
  // arriba y chocarían
  return {
    ...rated,
    noHeader: true,
    sort: props.sort,
    desc: props.desc,
    onSortBy: (campo, d) => emit('sortBy', campo, d)
  }
})
</script>

<template>
  <component :is="list" v-if="list" v-bind="bindings" />
  <!-- la tabla sin cabecera no se desplaza sola: la envuelve su panel -->
  <div v-else class="table-wrap">
    <SongTable v-bind="bindings" />
  </div>
</template>
