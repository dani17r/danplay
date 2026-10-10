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
import { ref, watch, nextTick, computed, useId, useTemplateRef } from 'vue'
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import Loading from './ui/Loading.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { formatDuration } from '../utils/format.js'

const props = defineProps({
  songs: { type: Array, default: () => [] },
  loading: Boolean,
  query: { type: String, default: '' },
  visibles: { type: Number, default: 5 }
})
const emit = defineEmits(['pick', 'play', 'menu', 'seeAll', 'close'])

// Un resultado se puede coger y soltar en un repertorio del lateral, igual
// que una fila de la lista.
const { startDrag, isDragged } = useDragSong()

const caja = useTemplateRef('caja')
const alto = ref(null)
const marcada = ref(0)
// La caja de busqueda es un «combobox» que manda en esta lista: estos ids
// le dicen a quien no ve la pantalla que resultado esta resaltado.
const uid = useId()
const listId = `${uid}-list`
const optionId = (i) => `${uid}-opt-${i}`
const activeId = computed(() => (props.songs.length ? optionId(marcada.value) : undefined))

/** Alto para que se vean justo `visibles` filas: se mide una de verdad. */
async function medir() {
  await nextTick()
  const fila = caja.value?.querySelector('.sr-row')
  if (!fila) {
    alto.value = null
    return
  }
  const h = fila.getBoundingClientRect().height
  alto.value = h > 0 && props.songs.length > props.visibles ? Math.round(h * props.visibles) : null
}
watch(
  () => props.songs,
  () => {
    marcada.value = 0
    medir()
  },
  { immediate: true }
)

const hay = computed(() => props.songs.length > 0)

function mover(paso) {
  if (!hay.value) return
  marcada.value = Math.max(0, Math.min(props.songs.length - 1, marcada.value + paso))
  nextTick(() =>
    caja.value?.querySelectorAll('.sr-row')[marcada.value]?.scrollIntoView?.({ block: 'nearest' })
  )
}
/** El buscador delega aqui las flechas y el Enter. */
function onKey(e) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    mover(1)
    return true
  }
  if (e.key === 'ArrowUp') {
    e.preventDefault()
    mover(-1)
    return true
  }
  if (e.key === 'Enter' && hay.value && !props.loading) {
    e.preventDefault()
    emit('pick', props.songs[marcada.value])
    return true
  }
  if (e.key === 'Escape') {
    emit('close')
    return true
  }
  // Las opciones de la fila resaltada, con la tecla de menu o Mayus+F10 (lo
  // que se hace en una fila de la lista). El foco se queda en la caja: aqui
  // solo se dice DONDE abrir el menu, bajo el boton de la fila.
  if (hay.value && !props.loading && (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey))) {
    e.preventDefault()
    const fila = caja.value?.querySelectorAll('.sr-row')[marcada.value]
    const r = (fila?.querySelector('.sr-more') || fila)?.getBoundingClientRect()
    emit('menu', props.songs[marcada.value], { clientX: r?.left ?? 0, clientY: r?.bottom ?? 0 })
    return true
  }
  return false
}
defineExpose({ onKey, listId, activeId })

const fmt = (s) => formatDuration(s, '')

/** Las opciones de una fila: la deja resaltada y avisa de donde se pulso. */
function onMenu(song, i, ev) {
  marcada.value = i
  emit('menu', song, ev)
}
</script>

<template>
  <div class="search-results card">
    <div v-if="loading" class="sr-head"><Loading text="buscando…" inline /></div>
    <template v-else-if="hay">
      <div class="sr-head">
        <span class="sr-count"
          >{{ songs.length }} {{ songs.length === 1 ? 'resultado' : 'resultados' }} en toda la
          biblioteca</span
        >
        <span class="sr-tip">arrástrala a un repertorio · clic derecho: opciones</span>
      </div>
      <div
        :id="listId"
        ref="caja"
        class="sr-list"
        role="listbox"
        aria-label="Resultados en toda la biblioteca"
        :style="alto ? { maxHeight: alto + 'px' } : null"
      >
        <!-- Un div y no un boton: `startDrag` se aparta de los botones a
             proposito (ahi el clic tiene otra cosa que hacer), asi que dentro
             de uno no se podria arrastrar nunca. -->
        <!-- el teclado va por la caja de busqueda (flechas y Enter); si una
             fila llega a tener el foco, responde igual -->
        <div
          v-for="(c, i) in songs"
          :id="optionId(i)"
          :key="c.id"
          class="sr-row"
          role="option"
          tabindex="-1"
          :aria-selected="i === marcada"
          :class="{ on: i === marcada, dragged: isDragged(c.id) }"
          @pointerdown="startDrag(c, $event)"
          @mouseenter="marcada = i"
          @focus="marcada = i"
          @click="emit('pick', c)"
          @dblclick="emit('play', c)"
          @contextmenu.prevent="onMenu(c, i, $event)"
          @keydown.enter.prevent="emit('pick', c)"
        >
          <span class="sr-art-wrap">
            <CoverArt
              :id="c.id"
              :blur="!!c.blur"
              class="sr-art"
              :icon-size="13"
              :alt="c.title || c.file"
            />
            <button
              type="button"
              class="sr-play"
              title="Reproducir"
              tabindex="-1"
              :aria-label="'Reproducir ' + (c.title || c.file)"
              @click.stop="emit('play', c)"
              @dblclick.stop
            >
              <Icon n="play" :t="12" />
            </button>
          </span>
          <span class="sr-txt">
            <span class="sr-title">{{ c.title || c.file }}</span>
            <span class="sr-sub sub">{{ c.artist || 'Sin artista' }}</span>
          </span>
          <span class="sr-dur sub mono">{{ fmt(c.duration) }}</span>
          <!-- Las mismas opciones que el clic derecho de una fila de la lista
               (añadir a un repertorio, favorita, estrellas…), a la vista para
               quien no sepa que existe el clic derecho. El menu se abre FUERA
               de este desplegable, y el desplegable sigue abierto: asi se
               pueden hacer varias cosas seguidas con lo encontrado. -->
          <!-- aria-hidden: dentro de un «option» sus hijos son presentacionales y
               el nombre de la opcion ya dice el titulo; quien no ve la pantalla
               llega a las opciones con la tecla de menu (Mayus+F10) sobre la caja -->
          <button
            type="button"
            class="sr-more"
            title="Opciones"
            tabindex="-1"
            aria-hidden="true"
            @click.stop="onMenu(c, i, $event)"
            @dblclick.stop
          >
            <Icon n="more" :t="15" />
          </button>
        </div>
      </div>
      <button type="button" class="sr-foot" @click="emit('seeAll')">
        <Icon n="viewList" :t="13" /> Verlos todos en la biblioteca
      </button>
    </template>
    <div v-else class="sr-head sr-empty">Nada que se parezca a «{{ query }}»</div>
  </div>
</template>
