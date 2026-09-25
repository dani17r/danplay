<script setup>
import { computed, ref } from 'vue'
import SongTable from './SongTable.vue'
import SongGrid from './SongGrid.vue'
import SongRows from './SongRows.vue'
import SongCards from './SongCards.vue'
import Icon from './Icon.vue'
import EmptyState from './ui/EmptyState.vue'

const props = defineProps(['songs','by','selected','selectedIds','playing','layout','size','jumpTo','sort','desc'])
const emit = defineEmits(['select','play','setStars','toggleFavorite','sortBy','context'])
const collapsed = ref(new Set())

const groups = computed(() => {
  const m = new Map()
  const keyOf = (c) => ({
    folder: c.folder || '(root)',
    artist: c.artist || 'Sin artista',
    album:   c.album   || 'Sin album',
    initial: (c.title || c.file || '#')[0].toUpperCase(),
    genre:  c.genre  || 'Sin genero',
    key:    c.key    || 'Sin tono'
  }[props.by] || '—')
  for (const c of props.songs) {
    const k = keyOf(c)
    if (!m.has(k)) m.set(k, [])
    m.get(k).push(c)
  }
  return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0], 'es'))
})

function toggleGroup (k) {
  const s = new Set(collapsed.value)
  s.has(k) ? s.delete(k) : s.add(k)
  collapsed.value = s
}
const fmtDuration = (l) => {
  const s = l.reduce((t, c) => t + (c.duration || 0), 0)
  return `${Math.floor(s / 60)} min`
}
</script>

<template>
  <div class="table-wrap">
    <div v-for="[k, items] in groups" :key="k">
      <div class="group-head collapsible" @click="toggleGroup(k)">
        <Icon n="right" :t="13" class="group-chevron"
              :class="{open: !collapsed.has(k)}" />
        <span>{{ k }}</span>
        <span class="cnt">{{ items.length }} temas · {{ fmtDuration(items) }}</span>
      </div>
      <template v-if="!collapsed.has(k)">
        <SongRows v-if="layout==='rows'" :songs="items" :selected="selected" :selected-ids="selectedIds"
                  :playing="playing" :jumpTo="jumpTo"
                  @select="e=>emit('select',e)" @play="e=>emit('play',e)"
                  @setStars="(c,n)=>emit('setStars',c,n)"
                  @toggleFavorite="c=>emit('toggleFavorite',c)"
                  @context="(ev,c)=>emit('context',ev,c)" />
        <SongCards v-else-if="layout==='cards'" :songs="items" :selected="selected" :selected-ids="selectedIds"
                   :playing="playing" :jumpTo="jumpTo"
                   @select="e=>emit('select',e)" @play="e=>emit('play',e)"
                   @setStars="(c,n)=>emit('setStars',c,n)"
                   @toggleFavorite="c=>emit('toggleFavorite',c)"
                   @context="(ev,c)=>emit('context',ev,c)" />
        <SongGrid v-else-if="layout==='grid'" :songs="items" :selected="selected" :selected-ids="selectedIds"
                    :playing="playing" :size="size" :jumpTo="jumpTo"
                    @select="e=>emit('select',e)" @play="e=>emit('play',e)"
                    @context="(ev,c)=>emit('context',ev,c)" />
        <SongTable v-else :songs="items" :selected="selected" :selected-ids="selectedIds" :playing="playing"
               :noHeader="true" :jumpTo="jumpTo" :sort="sort" :desc="desc"
               @select="e=>emit('select',e)" @play="e=>emit('play',e)"
               @setStars="(c,n)=>emit('setStars',c,n)" @toggleFavorite="c=>emit('toggleFavorite',c)"
               @sortBy="(campo,d)=>emit('sortBy',campo,d)"
               @context="(ev,c)=>emit('context',ev,c)" />
      </template>
    </div>
    <EmptyState v-if="!groups.length" title="Nada por aqui"
                hint="Prueba con otra busqueda o revisa tus carpetas" />
  </div>
</template>
