<script setup>
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import EmptyState from './ui/EmptyState.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { ref, watch, nextTick, computed } from 'vue'
import { useVirtualRows } from '../composables/useVirtualRows.js'
import { usePlayback } from '../composables/usePlayback.js'

// Una ficha se puede coger y soltar en un repertorio del menu lateral.
const { startDrag, isDragged } = useDragSong()
const props = defineProps(['songs','selected','playing','size','jumpTo'])

// Sobre la que esta puesta, el boton es pausa (o reanudar si esta en pausa);
// en las demas, reproducir. Lo de «sonando» lo sabe el reproductor.
const { playing: sounding } = usePlayback()
const rowIcon = (c) => (props.playing === c.id && sounding.value ? 'pause' : 'play')
const rowTitle = (c) =>
  props.playing !== c.id ? 'Reproducir' : sounding.value ? 'Pausar' : 'Reanudar'

// Igual que en la tabla: solo nodos del DOM para poder hacerles scroll, fuera
// de la reactividad y vaciado al cambiar la lista.
const cards = new Map()
watch(() => props.songs, () => cards.clear())

// Igual que en la tabla: solo se pintan las fichas que se ven. Cuantas caben
// por linea no hace falta calcularlo del css: se mira cuantas de las ya
// pintadas empiezan a la misma altura.
const box = ref(null)
const { from, to, padTop, padBottom, reveal } =
  useVirtualRows(() => box.value, () => props.songs.length)
const visible = computed(() => props.songs.slice(from.value, to.value))

watch(() => props.jumpTo, async (id) => {
  if (!id) return
  const i = props.songs.findIndex(c => c.id === id)
  if (i < 0) return
  await reveal(i)
  await nextTick()
  const el = cards.get(id)
  if (el && typeof el.scrollIntoView === 'function') {
    el.scrollIntoView({ block: 'center', behavior: 'smooth' })
  }
})
const emit = defineEmits(['select','play','context'])


</script>

<template>
  <div class="grid" ref="box" :style="{'--card-w': (size||164)+'px'}">
    <div v-if="padTop" data-spacer aria-hidden="true"
         :style="{gridColumn: '1 / -1', height: padTop + 'px'}"></div>
    <div v-for="c in visible" :key="c.id" class="tile"
         v-memo="[c.id, c.title, c.file, c.artist, c.stars, c.blur,
                  selected===c.id, playing===c.id, playing===c.id && sounding,
                  jumpTo===c.id, isDragged(c.id)]"
         :ref="el => { if (el) cards.set(c.id, el) }"
         :class="{selected: selected===c.id, playing: playing===c.id,
                  flash: jumpTo===c.id, dragged: isDragged(c.id)}"
         @pointerdown="startDrag(c, $event)"
         @click="emit('select', c.id)" @dblclick="emit('play', c)"
         @contextmenu.prevent="emit('context', $event, c)">
      <CoverArt :id="c.id" :blur="!!c.blur" class="art" :icon-size="30" :alt="c.title || c.file" />
      <button class="tile-play" :title="rowTitle(c)" @click.stop="emit('play', c)">
        <Icon :n="rowIcon(c)" :t="15" /></button>
      <div class="name" :title="c.title">
        <Icon v-if="playing===c.id" :n="sounding ? 'pause' : 'play'" :t="11"
               style="display:inline-block;color:var(--accent);margin-right:3px" />{{ c.title || c.file }}
      </div>
      <div class="sub2">{{ c.artist || '—' }}</div>
      <div class="sub2" v-if="c.stars" style="display:flex;gap:1px;color:var(--amber)">
        <Icon v-for="n in c.stars" :key="n" n="star" :t="12" /></div>
    </div>
    <div v-if="padBottom" data-spacer aria-hidden="true"
         :style="{gridColumn: '1 / -1', height: padBottom + 'px'}"></div>
    <EmptyState v-if="!songs.length" style="grid-column:1/-1"
                title="Nada por aqui" hint="Prueba con otra busqueda o revisa tus carpetas" />
  </div>
</template>
