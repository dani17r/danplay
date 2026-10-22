<script setup>
/**
 * La barra de la selección múltiple: cuántas hay elegidas y lo que se hace
 * con todas a la vez (añadirlas a una lista, favoritas, Telegram, quitarlas
 * del repertorio, la papelera y el menú entero en «⋯»).
 *
 * Sale bajo la fila de la lista (`.filters`) con el modo «Seleccionar» puesto
 * o con dos o más elegidas. No hace nada por su cuenta: dice qué se ha
 * pulsado y la app lo hace con las acciones de useSongMenus, las mismas del
 * menú de clic derecho.
 *
 * Es una barra de herramientas: el contador se anuncia al cambiar
 * (aria-live), cada botón lleva su nombre entero aunque en estrecho solo se
 * vea el icono, las flechas pasan de un botón a otro y Escape termina (como
 * «Listo»).
 *
 * El sitio que tiene no depende solo de la ventana: con el panel de la ficha
 * abierto la lista es más estrecha, y con Telegram y «Quitar de la lista»
 * hay más botones. Así que la barra se mide: si no cabe con las etiquetas
 * enteras pasa a las cortas, luego a solo iconos y por último a solo el
 * número (`fit`). Los tramos de la ventana (selection-bar.css) ponen el
 * mínimo; esto, lo que haga falta encima.
 */
import { computed, ref, watch, nextTick, onMounted, onBeforeUnmount, useTemplateRef } from 'vue'
import Icon from './Icon.vue'

const props = defineProps({
  /** cuántas hay elegidas */
  count: { type: Number, default: 0 },
  /** están todas las que se ven (las cargadas) */
  allSelected: Boolean,
  /** el nombre del repertorio que se ve, si es uno: entonces se ofrece quitarlas de él */
  playlist: { type: String, default: '' },
  /** Telegram está instalado */
  telegram: Boolean,
  /** todas las elegidas ya son favoritas */
  allFavorite: Boolean,
  /** el resto de la lista aún está llegando: «todas» son las que han llegado */
  loading: Boolean,
  /**
   * son canciones de la biblioteca (no las de fuera de la lista del
   * reproductor, que ni son favoritas ni se mandan a la papelera desde aquí)
   */
  library: { type: Boolean, default: true }
})
const emit = defineEmits([
  'selectAll',
  'selectNone',
  'addToList',
  'favorites',
  'telegram',
  'removeFromList',
  'trash',
  'more',
  'done'
])

const none = computed(() => props.count === 0)

const favName = computed(() => (props.allFavorite ? 'Ya son favoritas' : 'Marcar como favoritas'))
const favTitle = computed(() =>
  props.allFavorite ? 'Ya son favoritas (para quitarlas, en «⋯»)' : 'Marcar como favoritas'
)
/** Ya lo son todas: el botón no hace nada (quitarlas está en «⋯», a propósito menos a mano). */
function onFavorites() {
  if (!props.allFavorite) emit('favorites')
}

// ------------------------------------------------------------ cabe o no
// 0: etiquetas enteras; 1: cortas; 2: solo iconos; 3: solo el número. Cada
// nivel lleva las clases de los anteriores (`fit-short fit-icons`…), que son
// las mismas reglas que ponen los tramos de la ventana.
const FITS = ['fit-short', 'fit-icons', 'fit-tight']
const fit = ref(0)
const fitClass = computed(() => FITS.slice(0, fit.value))
const bar = useTemplateRef('bar')

/** ¿Cabe todo sin desplazar la barra ni recortar el contador? */
function fits() {
  const el = bar.value
  const status = el?.querySelector('.sel-status')
  if (!el || !el.clientWidth) return true // sin medidas (sin pintar): nada que decidir
  return (
    el.scrollWidth <= el.clientWidth + 1 &&
    (!status || status.scrollWidth <= status.clientWidth + 1)
  )
}

/** Del nivel más holgado al más apretado, el primero que quepa. */
let fitting = false
async function refit() {
  if (fitting) return
  fitting = true
  try {
    for (let level = 0; level <= FITS.length; level++) {
      fit.value = level
      await nextTick()
      if (fits()) break
    }
  } finally {
    fitting = false
  }
}

// Al cambiar el ancho (la ventana, el panel de la ficha) y al cambiar lo que
// hay dentro (el número, los botones que salen o se van).
let observer = null
let lastWidth = -1
onMounted(() => {
  refit()
  if (typeof ResizeObserver === 'undefined' || !bar.value) return
  observer = new ResizeObserver(([entry]) => {
    const width = Math.round(entry.contentRect.width)
    if (width === lastWidth) return // solo el alto (otro nivel): no hay que volver a medir
    lastWidth = width
    refit()
  })
  observer.observe(bar.value)
})
onBeforeUnmount(() => observer?.disconnect())
watch(
  () => [
    props.count,
    props.allSelected,
    props.playlist,
    props.telegram,
    props.loading,
    props.library
  ],
  () => refit()
)

// ------------------------------------------------------------ teclado
// Las flechas recorren los botones (como en cualquier barra de herramientas);
// el tabulador también pasa por todos, que son pocos.
/** @param {KeyboardEvent} e */
function onKey(e) {
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    return emit('done', e)
  }
  const moves = { ArrowRight: 1, ArrowLeft: -1, Home: -Infinity, End: Infinity }
  if (!(e.key in moves) || e.altKey || e.ctrlKey || e.metaKey) return
  const buttons = [...(bar.value?.querySelectorAll('button:not(:disabled)') || [])]
  const at = buttons.indexOf(/** @type {any} */ (document.activeElement))
  if (at < 0) return
  const step = moves[e.key]
  const to = Number.isFinite(step)
    ? (at + step + buttons.length) % buttons.length
    : step < 0
      ? 0
      : buttons.length - 1
  e.preventDefault()
  const next = /** @type {HTMLElement} */ (buttons[to])
  next.focus()
}
</script>

<template>
  <div
    ref="bar"
    class="sel-bar"
    :class="fitClass"
    role="toolbar"
    aria-label="Canciones seleccionadas"
  >
    <!-- el número siempre a la vista; la palabra, en columna, solo para el
         lector de pantalla (que anuncia «3 seleccionadas» al cambiar) -->
    <span class="sel-status" aria-live="polite">
      <span v-if="none" class="sel-hint">Elige canciones</span>
      <template v-else>
        <strong class="sel-count">{{ count }}</strong
        >&#32;<span class="sel-word">{{ count === 1 ? 'seleccionada' : 'seleccionadas' }}</span>
      </template>
      <span v-if="loading && allSelected" class="sel-note">
        (las que han llegado; el resto sigue cargando)
      </span>
    </span>

    <div class="sel-actions">
      <button
        v-if="allSelected"
        type="button"
        class="btn mini sel-btn"
        data-act="none"
        aria-label="No seleccionar ninguna"
        title="No seleccionar ninguna"
        @keydown="onKey"
        @click="emit('selectNone')"
      >
        <!-- la casilla de «todas», como la de la cabecera de la tabla: marcada
             (están todas), a medias (algunas) o vacía -->
        <span class="pick-box on" aria-hidden="true"><Icon n="check" :t="10" :weight="2.6" /></span>
        <span class="sel-label">Ninguna</span>
      </button>
      <button
        v-else
        type="button"
        class="btn mini sel-btn"
        data-act="all"
        aria-label="Seleccionar todas"
        :title="loading ? 'Seleccionar todas las que ya han llegado' : 'Seleccionar todas'"
        @keydown="onKey"
        @click="emit('selectAll')"
      >
        <span class="pick-box" :class="{ mixed: !none }" aria-hidden="true"></span>
        <span class="sel-label"
          ><span class="sel-long">Seleccionar todas</span><span class="sel-short">Todas</span></span
        >
      </button>

      <span class="sel-sep" aria-hidden="true"></span>

      <button
        type="button"
        class="btn mini sel-btn"
        data-act="list"
        aria-label="Añadir a una lista"
        title="Añadir a una lista"
        aria-haspopup="menu"
        :disabled="none"
        @keydown="onKey"
        @click="emit('addToList', $event)"
      >
        <Icon n="plus" :t="13" />
        <span class="sel-label"
          ><span class="sel-long">Añadir a una lista</span
          ><span class="sel-short">Añadir</span></span
        >
        <Icon n="down" :t="11" class="sel-caret" />
      </button>
      <button
        v-if="library"
        type="button"
        class="btn mini sel-btn"
        :class="{ on: allFavorite }"
        data-act="favorites"
        :aria-label="favName"
        :aria-disabled="allFavorite ? 'true' : undefined"
        :title="favTitle"
        :disabled="none"
        @keydown="onKey"
        @click="onFavorites"
      >
        <Icon :n="allFavorite ? 'heartFull' : 'heart'" :t="13" />
        <span class="sel-label">Favoritas</span>
      </button>
      <button
        v-if="telegram"
        type="button"
        class="btn mini sel-btn"
        data-act="telegram"
        aria-label="Enviar por Telegram"
        title="Enviar por Telegram"
        :disabled="none"
        @keydown="onKey"
        @click="emit('telegram')"
      >
        <Icon n="send" :t="13" />
        <span class="sel-label">Telegram</span>
      </button>
      <button
        v-if="playlist"
        type="button"
        class="btn mini sel-btn"
        data-act="remove"
        :aria-label="`Quitar de «${playlist}»`"
        :title="`Quitar de «${playlist}» (los archivos no se tocan)`"
        :disabled="none"
        @keydown="onKey"
        @click="emit('removeFromList')"
      >
        <Icon n="close" :t="13" />
        <span class="sel-label"
          ><span class="sel-long">Quitar de la lista</span
          ><span class="sel-short">Quitar</span></span
        >
      </button>
      <button
        v-if="library"
        type="button"
        class="btn mini sel-btn sel-danger"
        data-act="trash"
        aria-label="Mandar a la papelera"
        title="Mandar a la papelera…"
        :disabled="none"
        @keydown="onKey"
        @click="emit('trash')"
      >
        <Icon n="trash" :t="13" />
        <span class="sel-label">Papelera</span>
      </button>
      <button
        type="button"
        class="btn mini sel-btn"
        data-act="more"
        aria-label="Más opciones"
        title="Más opciones"
        aria-haspopup="menu"
        :disabled="none"
        @keydown="onKey"
        @click="emit('more', $event)"
      >
        <Icon n="more" :t="13" />
      </button>
    </div>

    <button
      type="button"
      class="btn mini sel-btn sel-done"
      data-act="done"
      aria-label="Listo"
      title="Listo: dejar de seleccionar (Escape)"
      @keydown="onKey"
      @click="emit('done', $event)"
    >
      <Icon n="check" :t="13" />
      <span class="sel-label">Listo</span>
    </button>
  </div>
</template>
