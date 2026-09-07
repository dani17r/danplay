<script setup>
import StarRating from './StarRating.vue'
import Icon from './Icon.vue'
import EmptyState from './ui/EmptyState.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { ref, watch, nextTick, computed } from 'vue'
import { useVirtualRows } from '../composables/useVirtualRows.js'

// Una fila se puede coger y soltar en un repertorio del menu lateral.
const { startDrag, isDragged } = useDragSong()
const props = defineProps(['songs','selected','playing','sort','noHeader','columns','jumpTo'])

// Un Map normal, fuera de la reactividad: aqui solo se guardan nodos del DOM
// para poder hacerles scroll, y nadie los pinta. Se vacia al cambiar la lista
// porque si no se van acumulando las filas de cada busqueda anterior, que ya
// no estan en pantalla pero siguen sujetas en memoria.
const rows = new Map()
watch(() => props.songs, () => rows.clear())

// Solo se pintan las filas visibles. `body` es el <tbody>, que es donde
// empiezan a contarse.
const body = ref(null)
const { from, to, padTop, padBottom, reveal } =
  useVirtualRows(() => body.value, () => props.songs.length)
const visible = computed(() => props.songs.slice(from.value, to.value))

watch(() => props.jumpTo, async (id) => {
  if (!id) return
  // puede que la fila ni siquiera este pintada: primero se trae a la ventana
  const i = props.songs.findIndex(c => c.id === id)
  if (i < 0) return
  const { viewport, rowHeight } = (await reveal(i)) || {}
  await nextTick()
  const el = rows.get(id)
  if (el && typeof el.scrollIntoView === 'function') {
    el.scrollIntoView({ block: 'center', behavior: 'smooth' })
  } else if (viewport && rowHeight > 0) {
    viewport.scrollTop = Math.max(0, i * rowHeight - viewport.clientHeight / 2)
  }
})
const emit = defineEmits(['select','play','setStars','toggleFavorite','sortBy','context'])

const fmtDuration = (s) => {
  if (!s) return '—'
  const m = Math.floor(s / 60), r = Math.floor(s % 60)
  return `${m}:${String(r).padStart(2, '0')}`
}
</script>

<template>
  <div :class="noHeader ? '' : 'table-wrap'">
    <table v-if="songs.length">
      <thead v-if="!noHeader">
        <tr>
          <th class="col-n">#</th>
          <th class="col-fav"></th>
          <th @click="emit('sortBy','title')">Titulo</th>
          <th @click="emit('sortBy','artist')">Artista</th>
          <th v-if="!columns || columns.album" @click="emit('sortBy','album')">Album</th>
          <th v-if="!columns || columns.stars" class="col-stars">Estrellas</th>
          <th v-if="!columns || columns.key" class="col-key" @click="emit('sortBy','key')">Tono</th>
          <th v-if="!columns || columns.bpm" class="col-bpm" @click="emit('sortBy','bpm')">BPM</th>
          <th class="col-dur" @click="emit('sortBy','duration')">Dur.</th>
          <th v-if="!columns || columns.kbps" class="col-kbps">Kbps</th>
        </tr>
      </thead>
      <tbody ref="body">
        <!-- separadores: ocupan el hueco de lo que no se pinta, para que la
             barra de desplazamiento siga midiendo lo mismo -->
        <tr v-if="padTop" data-spacer :style="{height: padTop + 'px'}" aria-hidden="true"></tr>
        <!-- v-memo: una fila solo se vuelve a pintar si cambia algo de LO QUE
             ELLA enseña. Sin esto, seleccionar una cancion obligaba a repasar
             las mil filas de la lista con sus ocho iconos cada una. La lista
             de dependencias tiene que nombrar todo lo que usa la fila. -->
        <tr v-for="(c,i) in visible" :key="c.id"
            v-memo="[from + i, c.id, c.title, c.file, c.feat, c.artist, c.album, c.stars,
                     c.favorite, c.key, c.bpm, c.duration, c.bitrate, columns,
                     selected===c.id, playing===c.id, jumpTo===c.id, isDragged(c.id)]"
            :ref="el => { if (el) rows.set(c.id, el) }"
            :class="{selected: selected===c.id, playing: playing===c.id,
                     flash: jumpTo===c.id, dragged: isDragged(c.id)}"
            @pointerdown="startDrag(c, $event)"
            @click="emit('select', c.id)"
            @dblclick="emit('play', c)"
            @contextmenu.prevent="emit('context', $event, c)">
          <td class="col-n mono">
            <button class="row-play" :title="playing===c.id ? 'Volver a empezar' : 'Reproducir'"
                    @click.stop="emit('play', c)"><Icon n="play" :t="12" /></button>
            <span class="row-n">
              <Icon v-if="playing===c.id" n="play" :t="11" style="margin-left:auto" />
              <template v-else>{{ from + i + 1 }}</template>
            </span>
          </td>
          <td class="col-fav">
            <span class="heart" :class="{on:c.favorite}"
                  @click.stop="emit('toggleFavorite', c)">
              <Icon :n="c.favorite ? 'heartFull' : 'heart'" :t="16" /></span>
          </td>
          <td class="title">
            {{ c.title || c.file }}
            <span class="sub" v-if="c.feat"> · feat. {{ c.feat }}</span>
          </td>
          <td class="sub">{{ c.artist || '—' }}</td>
          <td v-if="!columns || columns.album" class="sub">{{ c.album || '—' }}</td>
          <td v-if="!columns || columns.stars" class="col-stars">
            <StarRating :value="c.stars||0" :t="15" @change="n=>emit('setStars',c,n)" />
          </td>
          <td class="col-key mono sub">{{ c.key || '—' }}</td>
          <td class="col-bpm mono sub">{{ c.bpm ? Math.round(c.bpm) : '—' }}</td>
          <td class="col-dur mono sub">{{ fmtDuration(c.duration) }}</td>
          <td class="col-kbps mono sub">{{ c.bitrate ? Math.round(c.bitrate/1000) : '—' }}</td>
        </tr>
        <tr v-if="padBottom" data-spacer :style="{height: padBottom + 'px'}" aria-hidden="true"></tr>
      </tbody>
    </table>
    <EmptyState v-else title="Nada por aqui"
                hint="Prueba con otra busqueda o revisa tus carpetas" />
  </div>
</template>
