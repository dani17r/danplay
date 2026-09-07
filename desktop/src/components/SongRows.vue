<script setup>
/**
 * Lista fina: una linea por cancion, a lo largo de todo el ancho.
 *
 * Es la vista para recorrer mucho con poco sitio: nada de columnas, solo el
 * titulo con su artista al lado y lo justo a la derecha. Al ser una sola caja
 * por fila se adapta sola a cualquier ancho, sin columnas que descuadrar.
 */
import Icon from './Icon.vue'
import StarRating from './StarRating.vue'
import EmptyState from './ui/EmptyState.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { ref, watch, nextTick, computed } from 'vue'
import { useVirtualRows } from '../composables/useVirtualRows.js'

const { startDrag, isDragged } = useDragSong()
const props = defineProps(['songs','selected','playing','jumpTo'])
const emit = defineEmits(['select','play','setStars','toggleFavorite','context'])

const filas = new Map()
watch(() => props.songs, () => filas.clear())

const caja = ref(null)
const { from, to, padTop, padBottom, reveal } =
  useVirtualRows(() => caja.value, () => props.songs.length)
const visible = computed(() => props.songs.slice(from.value, to.value))

watch(() => props.jumpTo, async (id) => {
  if (!id) return
  const i = props.songs.findIndex(c => c.id === id)
  if (i < 0) return
  await reveal(i)
  await nextTick()
  filas.get(id)?.scrollIntoView?.({ block: 'center', behavior: 'smooth' })
})

const fmt = (s) => {
  if (!s) return '—'
  const m = Math.floor(s / 60), r = Math.floor(s % 60)
  return `${m}:${String(r).padStart(2, '0')}`
}
</script>

<template>
  <div class="rows" ref="caja">
    <div v-if="padTop" data-spacer :style="{height: padTop + 'px'}" aria-hidden="true"></div>
    <div v-for="(c,i) in visible" :key="c.id" class="row"
         v-memo="[from + i, c.id, c.title, c.file, c.artist, c.stars, c.favorite,
                  c.duration, c.key, selected===c.id, playing===c.id,
                  jumpTo===c.id, isDragged(c.id)]"
         :ref="el => { if (el) filas.set(c.id, el) }"
         :class="{selected: selected===c.id, playing: playing===c.id,
                  flash: jumpTo===c.id, dragged: isDragged(c.id)}"
         @pointerdown="startDrag(c, $event)"
         @click="emit('select', c.id)"
         @dblclick="emit('play', c)"
         @contextmenu.prevent="emit('context', $event, c)">
      <span class="row-num mono">{{ from + i + 1 }}</span>
      <button class="row-go" :title="playing===c.id ? 'Volver a empezar' : 'Reproducir'"
              @click.stop="emit('play', c)">
        <Icon :n="playing===c.id ? 'pause' : 'play'" :t="12" />
      </button>
      <span class="row-title">{{ c.title || c.file }}</span>
      <span class="row-artist sub">{{ c.artist || '—' }}</span>
      <span class="row-extra sub mono">{{ c.key || '' }}</span>
      <StarRating class="row-stars" :value="c.stars||0" :t="13"
                  @change="n=>emit('setStars',c,n)" />
      <span class="heart" :class="{on:c.favorite}" @click.stop="emit('toggleFavorite', c)">
        <Icon :n="c.favorite ? 'heartFull' : 'heart'" :t="14" /></span>
      <span class="row-dur sub mono">{{ fmt(c.duration) }}</span>
    </div>
    <div v-if="padBottom" data-spacer :style="{height: padBottom + 'px'}" aria-hidden="true"></div>
    <EmptyState v-if="!songs.length" title="Nada por aqui"
                hint="Prueba con otra busqueda o revisa tus carpetas" />
  </div>
</template>
