<script setup>
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import EmptyState from './ui/EmptyState.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { watch, nextTick } from 'vue'

// Una ficha se puede coger y soltar en un repertorio del menu lateral.
const { startDrag, isDragged } = useDragSong()
const props = defineProps(['songs','selected','playing','size','jumpTo'])

// Igual que en la tabla: solo nodos del DOM para poder hacerles scroll, fuera
// de la reactividad y vaciado al cambiar la lista.
const cards = new Map()
watch(() => props.songs, () => cards.clear())

watch(() => props.jumpTo, async (id) => {
  if (!id) return
  await nextTick()
  const el = cards.get(id)
  if (el && typeof el.scrollIntoView === 'function') {
    el.scrollIntoView({ block: 'center', behavior: 'smooth' })
  }
})
const emit = defineEmits(['select','play','context'])


</script>

<template>
  <div class="grid" :style="{'--card-w': (size||164)+'px'}">
    <div v-for="c in songs" :key="c.id" class="tile"
         v-memo="[c.id, c.title, c.file, c.artist, c.stars,
                  selected===c.id, playing===c.id, jumpTo===c.id, isDragged(c.id)]"
         :ref="el => { if (el) cards.set(c.id, el) }"
         :class="{selected: selected===c.id, playing: playing===c.id,
                  flash: jumpTo===c.id, dragged: isDragged(c.id)}"
         @pointerdown="startDrag(c, $event)"
         @click="emit('select', c.id)" @dblclick="emit('play', c)"
         @contextmenu.prevent="emit('context', $event, c)">
      <CoverArt :id="c.id" class="art" :icon-size="30" :alt="c.title || c.file" />
      <button class="tile-play" @click.stop="emit('play', c)"><Icon n="play" :t="15" /></button>
      <div class="name" :title="c.title">
        <Icon v-if="playing===c.id" n="play" :t="11"
               style="display:inline-block;color:var(--accent);margin-right:3px" />{{ c.title || c.file }}
      </div>
      <div class="sub2">{{ c.artist || '—' }}</div>
      <div class="sub2" v-if="c.stars" style="display:flex;gap:1px;color:var(--amber)">
        <Icon v-for="n in c.stars" :key="n" n="star" :t="12" /></div>
    </div>
    <EmptyState v-if="!songs.length" style="grid-column:1/-1"
                title="Nada por aqui" hint="Prueba con otra busqueda o revisa tus carpetas" />
  </div>
</template>
