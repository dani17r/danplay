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
 *
 * Varios a la vez: Ctrl (o Cmd) + clic pone o quita un resultado, Mayús + clic
 * coge el tramo, y el botón «Seleccionar» de la cabecera pone una casilla en
 * cada fila (entonces el clic normal también pone o quita). Desde la caja, sin
 * soltar el foco: Ctrl+Espacio pone o quita el resaltado y Mayús+↑/↓ amplía
 * (Ctrl+A es de la caja: selecciona su texto). Con alguno elegido la cabecera
 * pasa a ser una barra pequeña: cuántos, «Añadir a una lista ▾», «⋯» (lo que se
 * hace con varios) y «Ninguno». Es estado de aquí y no de la lista de la
 * página, porque los resultados no son filas suyas; al cambiar la búsqueda o
 * cerrar el desplegable se olvida.
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
// groupMenu y addToList llevan los elegidos (en el orden de los resultados) y
// el evento o el sitio donde abrir el menú; la app hace lo mismo que con
// varias canciones de la lista (useSongMenus, con `fromSearch`).
const emit = defineEmits(['pick', 'play', 'menu', 'groupMenu', 'addToList', 'seeAll', 'close'])

// Un resultado se puede coger y soltar en un repertorio del lateral, igual
// que una fila de la lista. Si es uno de los elegidos, van todos.
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

const hay = computed(() => props.songs.length > 0)

// ------------------------------------------------------- varios a la vez
/** El modo «Seleccionar»: una casilla en cada fila y el clic que alterna. */
const modo = ref(false)
/** Los ids elegidos, en el orden de los resultados. */
const ids = ref(/** @type {number[]} */ ([]))
const idsSet = computed(() => new Set(ids.value))
const esElegido = (id) => idsSet.value.has(id)
/** Los elegidos, en el orden de los resultados: sobre ellos actúa la barra. */
const elegidos = computed(() => props.songs.filter((c) => idsSet.value.has(c.id)))
const cuenta = computed(() => elegidos.value.length)
/**
 * Hay una selección en marcha (el modo o alguno elegido): para quien no ve la
 * pantalla la lista pasa a ser de elección múltiple y «seleccionado» es estar
 * elegido; si no, como siempre, es el resaltado.
 */
const multiple = computed(() => modo.value || cuenta.value > 0)
// De dónde sale el tramo de Mayús, y lo que se queda elegido aparte del tramo:
// Mayús+flechas lo rehacen desde el ancla sumado a eso, así que volver hacia
// atrás encoge el tramo y no se lleva lo de antes.
let ancla = null
let base = /** @type {number[]} */ ([])

/** Esos ids, en el orden en que se ven los resultados. @param {Set<number>} set */
function enOrden(set) {
  return props.songs.filter((c) => set.has(c.id)).map((c) => c.id)
}

/** Pone o quita un resultado (Ctrl+clic, el clic en el modo, Ctrl+Espacio). */
function alternar(c) {
  const s = new Set(ids.value)
  if (s.has(c.id)) s.delete(c.id)
  else s.add(c.id)
  ids.value = enOrden(s)
  ancla = c.id
  base = ids.value
}

/**
 * El tramo desde el ancla hasta la fila `hasta`, sumado a `con` (nada, o lo
 * que ya había). Sin ancla, el tramo empieza (y acaba) en esa fila.
 * @param {number} hasta
 * @param {number[]} con
 */
function tramo(hasta, con) {
  const orden = props.songs.map((c) => c.id)
  if (ancla == null || !orden.includes(ancla)) ancla = orden[hasta]
  base = con
  const [a, b] = [orden.indexOf(ancla), hasta].sort((x, y) => x - y)
  ids.value = enOrden(new Set([...con, ...orden.slice(a, b + 1)]))
}

/** Mayús+↑/↓: el tramo crece (o encoge) hasta la de al lado, que pasa a ser la resaltada. */
function ampliar(paso) {
  if (ancla == null || !props.songs.some((c) => c.id === ancla)) {
    ancla = props.songs[marcada.value]?.id ?? null
    base = ids.value
  }
  mover(paso)
  tramo(marcada.value, base)
}

/** «Ninguno»: nada elegido. El modo sigue puesto (el botón «Seleccionar» lo quita). */
function ninguno() {
  ids.value = []
  ancla = null
  base = []
}

/** Fuera del modo y nada elegido: el botón «Seleccionar» al quitarlo, y Escape. */
function terminar() {
  modo.value = false
  ninguno()
}

function alternarModo() {
  if (modo.value) terminar()
  else modo.value = true
}

/** Lo elegido que ya no está entre los resultados (a la papelera) se suelta. */
function podar() {
  const quedan = new Set(props.songs.map((c) => c.id))
  if (ids.value.some((id) => !quedan.has(id))) ids.value = ids.value.filter((id) => quedan.has(id))
  base = base.filter((id) => quedan.has(id))
  if (ancla != null && !quedan.has(ancla)) ancla = null
}

watch(
  () => props.songs,
  () => {
    marcada.value = 0
    medir()
    podar()
  },
  { immediate: true }
)
// Otra búsqueda: lo elegido era de la anterior. El modo se queda: quien lo
// puso sigue eligiendo, ahora entre lo nuevo.
watch(
  () => props.query,
  () => ninguno()
)

function mover(paso) {
  if (!hay.value) return
  marcada.value = Math.max(0, Math.min(props.songs.length - 1, marcada.value + paso))
  nextTick(() =>
    caja.value?.querySelectorAll('.sr-row')[marcada.value]?.scrollIntoView?.({ block: 'nearest' })
  )
}

/** Un clic en la fila: elegirla (abre su ficha) o, en el modo o con Ctrl/Mayús, ponerla o quitarla. */
function onClick(c, i, ev) {
  marcada.value = i
  const ctrl = ev.ctrlKey || ev.metaKey
  // Mayús a secas deja solo el tramo; con Ctrl, o en el modo (con casillas
  // nada se suelta sin querer), lo suma a lo que había. Como en la lista.
  if (ev.shiftKey) return tramo(i, modo.value || ctrl ? ids.value : [])
  if (ctrl || modo.value) return alternar(c)
  emit('pick', c)
}

/**
 * Las opciones de una fila: la deja resaltada y avisa de donde se pulso. Si
 * es una de las elegidas, son las de todas (como en la lista).
 */
function onMenu(song, i, ev) {
  marcada.value = i
  if (esElegido(song.id)) emit('groupMenu', elegidos.value, ev)
  else emit('menu', song, ev)
}

/** El buscador delega aqui las flechas, el Enter y lo de elegir varios. */
function onKey(e) {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    const paso = e.key === 'ArrowDown' ? 1 : -1
    // con Mayús amplía lo elegido hasta la de al lado; sin ella solo se mueve,
    // y el próximo tramo empieza donde esté
    if (e.shiftKey && hay.value && !props.loading) ampliar(paso)
    else {
      ancla = null
      mover(paso)
    }
    return true
  }
  // Ctrl+Espacio pone o quita la resaltada; el espacio a secas es de la caja
  if (e.key === ' ' && (e.ctrlKey || e.metaKey) && !e.altKey) {
    if (!hay.value || props.loading) return false
    e.preventDefault()
    alternar(props.songs[marcada.value])
    return true
  }
  if (e.key === 'Enter' && hay.value && !props.loading) {
    e.preventDefault()
    emit('pick', props.songs[marcada.value])
    return true
  }
  if (e.key === 'Escape') {
    // Primero se suelta lo elegido (y el modo), como en la lista; con nada que
    // soltar, se cierra. Que no llegue al «Escape cierra» del documento.
    if (multiple.value) {
      e.preventDefault()
      e.stopPropagation()
      terminar()
      return true
    }
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
    const donde = { clientX: r?.left ?? 0, clientY: r?.bottom ?? 0 }
    const song = props.songs[marcada.value]
    if (esElegido(song.id)) emit('groupMenu', elegidos.value, donde)
    else emit('menu', song, donde)
    return true
  }
  return false
}
defineExpose({ onKey, listId, activeId })

const fmt = (s) => formatDuration(s, '')
</script>

<template>
  <div class="search-results card">
    <div v-if="loading" class="sr-head"><Loading text="buscando…" inline /></div>
    <template v-else-if="hay">
      <!-- La cabecera: cuántos hay y el botón «Seleccionar»; con alguno elegido,
           la barra pequeña con lo que se hace con ellos. Siempre en una línea.
           Sus botones no se quedan el foco al pulsarlos con el ratón
           (mousedown.prevent): el teclado sigue en la caja de búsqueda. -->
      <div class="sr-head sr-tools" :class="{ 'sr-picking': cuenta }">
        <!-- el mismo elemento en los dos casos: así el lector de pantalla
             anuncia la cuenta al cambiar -->
        <span class="sr-count" aria-live="polite"
          ><template v-if="cuenta"
            ><strong class="sr-n">{{ cuenta }}</strong
            >&#32;<span class="sr-word">{{
              cuenta === 1 ? 'seleccionado' : 'seleccionados'
            }}</span></template
          ><template v-else
            >{{ songs.length }} {{ songs.length === 1 ? 'resultado' : 'resultados' }}</template
          ></span
        >
        <template v-if="cuenta">
          <span class="sr-acts">
            <button
              type="button"
              class="btn mini sr-act"
              aria-label="Añadir a una lista"
              title="Añadir a una lista"
              aria-haspopup="menu"
              @mousedown.prevent
              @click="emit('addToList', elegidos, $event)"
            >
              <Icon n="plus" :t="13" />
              <span class="sr-label"
                ><span class="sr-long">Añadir a una lista</span
                ><span class="sr-short">Añadir</span></span
              >
              <Icon n="down" :t="11" class="sr-caret" />
            </button>
            <button
              type="button"
              class="btn mini sr-act"
              aria-label="Más opciones"
              title="Más opciones: reproducir, favoritas, la papelera…"
              aria-haspopup="menu"
              @mousedown.prevent
              @click="emit('groupMenu', elegidos, $event)"
            >
              <Icon n="more" :t="13" />
            </button>
            <button
              type="button"
              class="btn mini sr-act"
              aria-label="No seleccionar ninguno"
              title="No seleccionar ninguno (Escape)"
              @mousedown.prevent
              @click="ninguno"
            >
              <Icon n="close" :t="12" />
              <span class="sr-label">Ninguno</span>
            </button>
          </span>
        </template>
        <template v-else>
          <span
            class="sr-tip"
            title="Arrástralos a un repertorio del lateral · Ctrl o Mayús + clic: varios"
            >{{ modo ? 'marca los que quieras' : 'clic derecho: opciones' }}</span
          >
          <button
            type="button"
            class="btn mini sr-act sr-select"
            :class="{ on: modo }"
            :aria-pressed="modo"
            aria-label="Seleccionar resultados"
            title="Elegir varios con casillas (también con Ctrl o Mayús + clic)"
            @mousedown.prevent
            @click="alternarModo"
          >
            <Icon n="check" :t="13" />
            <span class="sr-label">Seleccionar</span>
          </button>
        </template>
      </div>
      <div
        :id="listId"
        ref="caja"
        class="sr-list"
        role="listbox"
        aria-label="Resultados en toda la biblioteca"
        :aria-multiselectable="multiple ? 'true' : undefined"
        :style="alto ? { maxHeight: alto + 'px' } : null"
      >
        <!-- Un div y no un boton: `startDrag` se aparta de los botones a
             proposito (ahi el clic tiene otra cosa que hacer), asi que dentro
             de uno no se podria arrastrar nunca. -->
        <!-- el teclado va por la caja de busqueda (flechas y Enter); si una
             fila llega a tener el foco, responde igual. Con el ratón la fila
             no se queda el foco (mousedown.prevent): elegir varios con Ctrl
             deja el desplegable abierto y lo escrito tiene que seguir llegando
             a la caja. -->
        <div
          v-for="(c, i) in songs"
          :id="optionId(i)"
          :key="c.id"
          class="sr-row"
          role="option"
          tabindex="-1"
          :aria-selected="multiple ? esElegido(c.id) : i === marcada"
          :class="{ on: i === marcada, picked: esElegido(c.id), dragged: isDragged(c.id) }"
          @mousedown.prevent
          @pointerdown="startDrag(c, $event, elegidos)"
          @mouseenter="marcada = i"
          @focus="marcada = i"
          @click="onClick(c, i, $event)"
          @dblclick="emit('play', c)"
          @contextmenu.prevent="onMenu(c, i, $event)"
          @keydown.enter.prevent="emit('pick', c)"
        >
          <!-- la casilla, en el modo «Seleccionar»: decorativa (aria-hidden; la
               verdad es el aria-selected de la fila). Un clic en ella cae en la
               fila, que alterna; el doble clic se queda aquí: marcar y desmarcar
               deprisa no pone la canción -->
          <span
            v-if="modo"
            class="pick-box"
            :class="{ on: esElegido(c.id) }"
            aria-hidden="true"
            @dblclick.stop
          >
            <Icon v-if="esElegido(c.id)" n="check" :t="12" :grosor="2.4" />
          </span>
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
