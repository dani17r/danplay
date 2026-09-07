<script setup>
/**
 * Resultados sueltos, colgando del buscador.
 *
 * En «Todas las canciones» el buscador filtra la propia lista, que es lo que
 * se espera. Pero en Favoritos, en Artistas o dentro de un repertorio esa
 * lista es otra cosa y filtrarla no serviria: ahi el buscador mira TODA la
 * biblioteca y enseña lo que encuentra aqui, sin sacarte de donde estabas.
 *
 * Se ven cinco de golpe y el resto se alcanza con la rueda; la altura sale de
 * medir una fila, no de un numero a ojo, asi que sigue cuadrando aunque
 * cambie la densidad o el tamaño de la app.
 */
import { ref, watch, nextTick, computed } from 'vue'
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import Loading from './ui/Loading.vue'
import { useDragSong } from '../composables/useDragSong.js'

const props = defineProps({
  songs: { type: Array, default: () => [] },
  loading: Boolean,
  query: { type: String, default: '' },
  visibles: { type: Number, default: 5 }
})
const emit = defineEmits(['pick', 'play', 'seeAll', 'close'])

// Un resultado se puede coger y soltar en un repertorio del lateral, igual
// que una fila de la lista.
const { startDrag, isDragged } = useDragSong()

const caja = ref(null)
const alto = ref(null)
const marcada = ref(0)

/** Alto para que se vean justo `visibles` filas: se mide una de verdad. */
async function medir () {
  await nextTick()
  const fila = caja.value?.querySelector('.sr-row')
  if (!fila) { alto.value = null; return }
  const h = fila.getBoundingClientRect().height
  alto.value = h > 0 && props.songs.length > props.visibles
    ? Math.round(h * props.visibles) : null
}
watch(() => props.songs, () => { marcada.value = 0; medir() }, { immediate: true })

const hay = computed(() => props.songs.length > 0)

function mover (paso) {
  if (!hay.value) return
  marcada.value = Math.max(0, Math.min(props.songs.length - 1, marcada.value + paso))
  nextTick(() => caja.value?.querySelectorAll('.sr-row')[marcada.value]
    ?.scrollIntoView({ block: 'nearest' }))
}
/** El buscador delega aqui las flechas y el Enter. */
function onKey (e) {
  if (e.key === 'ArrowDown') { e.preventDefault(); mover(1); return true }
  if (e.key === 'ArrowUp') { e.preventDefault(); mover(-1); return true }
  if (e.key === 'Enter' && hay.value) {
    e.preventDefault(); emit('pick', props.songs[marcada.value]); return true
  }
  if (e.key === 'Escape') { emit('close'); return true }
  return false
}
defineExpose({ onKey })

const fmt = (s) => {
  if (!s) return ''
  const m = Math.floor(s / 60), r = Math.floor(s % 60)
  return `${m}:${String(r).padStart(2, '0')}`
}
</script>

<template>
  <div class="search-results card">
    <div v-if="loading" class="sr-head"><Loading text="buscando…" inline /></div>
    <template v-else-if="hay">
      <div class="sr-head">
        <span>{{ songs.length }} {{ songs.length === 1 ? 'resultado' : 'resultados' }}
          en toda la biblioteca</span>
        <span class="sr-tip">pulsa para verla · arrastrala a un repertorio</span>
      </div>
      <div class="sr-list" ref="caja" :style="alto ? {maxHeight: alto + 'px'} : null">
        <!-- Un div y no un boton: `startDrag` se aparta de los botones a
             proposito (ahi el clic tiene otra cosa que hacer), asi que dentro
             de uno no se podria arrastrar nunca. -->
        <div v-for="(c, i) in songs" :key="c.id" class="sr-row" role="option"
             :aria-selected="i === marcada"
             :class="{on: i === marcada, dragged: isDragged(c.id)}"
             @pointerdown="startDrag(c, $event)"
             @mouseenter="marcada = i"
             @click="emit('pick', c)" @dblclick="emit('play', c)">
          <span class="sr-art-wrap">
            <CoverArt :id="c.id" :blur="!!c.blur" class="sr-art" :icon-size="13"
                      :alt="c.title || c.file" />
            <button type="button" class="sr-play" title="Reproducir"
                    @click.stop="emit('play', c)">
              <Icon n="play" :t="12" /></button>
          </span>
          <span class="sr-txt">
            <span class="sr-title">{{ c.title || c.file }}</span>
            <span class="sr-sub sub">{{ c.artist || 'Sin artista' }}</span>
          </span>
          <span class="sr-dur sub mono">{{ fmt(c.duration) }}</span>
        </div>
      </div>
      <button type="button" class="sr-foot" @click="emit('seeAll')">
        <Icon n="viewList" :t="13" /> Verlos todos en la biblioteca
      </button>
    </template>
    <div v-else class="sr-head sr-empty">
      Nada que se parezca a «{{ query }}»
    </div>
  </div>
</template>
