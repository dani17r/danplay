<script setup>
/**
 * Lista acoplada: cada cancion es una ficha pequeña, con su portada al lado.
 *
 * A medio camino entre la lista y la cuadricula: se ve la caratula, que es lo
 * que ayuda a reconocer un tema de un vistazo, pero sigue habiendo sitio para
 * el titulo entero y los datos. Las fichas se acoplan en varias columnas
 * cuando hay ancho de sobra y pasan a una sola cuando no.
 */
import Icon from './Icon.vue'
import StarRating from './StarRating.vue'
import CoverArt from './ui/CoverArt.vue'
import EmptyState from './ui/EmptyState.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { ref, watch, nextTick, computed } from 'vue'
import { useVirtualRows } from '../composables/useVirtualRows.js'
import { usePlayback } from '../composables/usePlayback.js'

const { startDrag, isDragged } = useDragSong()
const props = defineProps(['songs','selected','playing','jumpTo'])
const emit = defineEmits(['select','play','setStars','toggleFavorite','context'])

// Sobre la que esta puesta, el boton de la fila es pausa (o reanudar si esta
// en pausa); en las demas, reproducir. Lo de «sonando» lo sabe el reproductor.
const { playing: sounding } = usePlayback()
const rowIcon = (c) => (props.playing === c.id && sounding.value ? 'pause' : 'play')
const rowTitle = (c) =>
  props.playing !== c.id ? 'Reproducir' : sounding.value ? 'Pausar' : 'Reanudar'

const fichas = new Map()
watch(() => props.songs, () => fichas.clear())

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
  fichas.get(id)?.scrollIntoView?.({ block: 'center', behavior: 'smooth' })
})

const fmt = (s) => {
  if (!s) return '—'
  const m = Math.floor(s / 60), r = Math.floor(s % 60)
  return `${m}:${String(r).padStart(2, '0')}`
}
</script>

<template>
  <div class="cards" ref="caja">
    <div v-if="padTop" data-spacer aria-hidden="true"
         :style="{gridColumn: '1 / -1', height: padTop + 'px'}"></div>
    <div v-for="(c,i) in visible" :key="c.id" class="card-song"
         v-memo="[from + i, c.id, c.title, c.file, c.artist, c.album, c.stars,
                  c.favorite, c.duration, c.blur, selected===c.id, playing===c.id,
                  playing===c.id && sounding, jumpTo===c.id, isDragged(c.id)]"
         :ref="el => { if (el) fichas.set(c.id, el) }"
         :class="{selected: selected===c.id, playing: playing===c.id,
                  flash: jumpTo===c.id, dragged: isDragged(c.id)}"
         @pointerdown="startDrag(c, $event)"
         @click="emit('select', c.id)"
         @dblclick="emit('play', c)"
         @contextmenu.prevent="emit('context', $event, c)">
      <div class="card-art">
        <CoverArt :id="c.id" :blur="!!c.blur" class="card-cover" :icon-size="18"
                  :alt="c.title || c.file" />
        <button class="card-play" :title="rowTitle(c)" @click.stop="emit('play', c)">
          <Icon :n="rowIcon(c)" :t="13" />
        </button>
      </div>
      <div class="card-txt">
        <div class="card-title">{{ c.title || c.file }}</div>
        <div class="card-sub sub">{{ c.artist || '—' }}<template v-if="c.album"> · {{ c.album }}</template></div>
        <div class="card-foot">
          <StarRating :value="c.stars||0" :t="12" @change="n=>emit('setStars',c,n)" />
          <span class="heart" :class="{on:c.favorite}" @click.stop="emit('toggleFavorite', c)">
            <Icon :n="c.favorite ? 'heartFull' : 'heart'" :t="13" /></span>
          <span class="sub mono card-dur">{{ fmt(c.duration) }}</span>
        </div>
      </div>
    </div>
    <div v-if="padBottom" data-spacer aria-hidden="true"
         :style="{gridColumn: '1 / -1', height: padBottom + 'px'}"></div>
    <EmptyState v-if="!songs.length" style="grid-column:1/-1" title="Nada por aqui"
                hint="Prueba con otra busqueda o revisa tus carpetas" />
  </div>
</template>
