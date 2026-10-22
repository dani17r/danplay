<script setup>
/**
 * La tabla de canciones.
 *
 * Las cabeceras ordenan y enseñan por donde va el orden, y las columnas se
 * pueden ensanchar arrastrando su borde. Los anchos se recuerdan.
 *
 * Que columnas se ven sale de UN solo sitio (`show`), y de ahi beben el
 * <colgroup>, la cabecera y las celdas. Antes cada uno llevaba su propia
 * condicion y en la vista compacta el cuerpo pintaba una celda MAS que la
 * cabecera —kbps no tenia condicion abajo—, asi que las columnas quedaban
 * corridas un sitio.
 *
 * Con el teclado es una rejilla: una sola parada del tabulador, las flechas
 * pasan de fila en fila y Enter la pone a sonar (ver useSongList).
 *
 * Con `selectMode` (el botón «Seleccionar») nace una columna más, la primera,
 * con la casilla de cada fila y, en la cabecera, la de «seleccionar todas».
 */
import StarRating from './StarRating.vue'
import Icon from './Icon.vue'
import GroupHead from './GroupHead.vue'
import EmptyState from './ui/EmptyState.vue'
import CopyButton from './ui/CopyButton.vue'
import { ref, watch, computed, onUnmounted, useTemplateRef } from 'vue'
import { useSongList } from '../composables/useSongList.js'
import { notifyCopied } from '../composables/useNotices.js'
import { formatDuration, fullName } from '../utils/format.js'

const props = defineProps({
  songs: { type: Array, required: true },
  // la elegida y la seleccion multiple las lee useSongList (la regla de
  // props sin usar no ve a traves de la llamada)
  // eslint-disable-next-line vue/no-unused-properties
  selected: { type: Number, default: null },
  // eslint-disable-next-line vue/no-unused-properties
  selectedIds: { type: Array, default: () => [] },
  playing: { type: Number, default: null },
  sort: { type: String, default: '' },
  desc: Boolean,
  /** sin la fila de las columnas (la vista agrupada) */
  noHeader: Boolean,
  /** los grupos, si la lista va agrupada (ver GroupedSongs) */
  groups: { type: Array, default: null },
  jumpTo: { type: Number, default: null },
  sortable: Boolean,
  /** como se anuncia la lista */
  label: { type: String, default: 'Canciones' },
  /** el modo «Seleccionar»: una casilla en cada fila (y la de todas, en la cabecera) */
  selectMode: Boolean
})
const emit = defineEmits([
  'select',
  'play',
  'setStars',
  'toggleFavorite',
  'sortBy',
  'context',
  'toggleGroup',
  // «todas» desde la casilla de la cabecera; y, con el teclado, Ctrl+A, Supr y
  // Escape, que emite useSongList. La app decide qué hacer con ellos.
  'selectAll',
  'remove',
  'clearSelection'
])

const body = useTemplateRef('body')
const {
  picked,
  before,
  after,
  dropKey,
  rowIcon,
  rowTitle,
  sounding,
  isDragged,
  startDrag,
  refFor,
  sections,
  numberOf,
  hasRows,
  padTop,
  padBottom,
  tabindex,
  onKey,
  onFocus
} = useSongList(props, emit, { anchor: () => body.value })

// Con grupos, la cabecera de cada uno es una fila más de la tabla: cuentan
// para el número de fila que se anuncia.
const theadRows = computed(() => (props.noHeader ? 0 : 1))
const rowCount = computed(() => props.songs.length + (props.groups?.length || 0) + theadRows.value)
/** El número de la fila de una canción, contando cabeceras. */
const rowIndex = (sec, i) => sec.from + i + 1 + (sec.group ? sec.index + 1 : 0) + theadRows.value
const headRowIndex = (sec) => sec.group.start + sec.index + 1 + theadRows.value

/**
 * El estado de la casilla de la cabecera, el de un `role="checkbox"` de tres
 * estados: 'true' si están elegidas todas las de la lista, 'mixed' si solo
 * algunas y 'false' si ninguna. Cuenta lo MISMO que pinta cada fila
 * (`picked`: la elegida y la selección múltiple), así que la cabecera y las
 * casillas nunca discrepan. Fuera del modo no se calcula.
 */
const pickState = computed(() => {
  if (!props.selectMode || !props.songs.length) return 'false'
  let n = 0
  for (const c of props.songs) if (picked(c.id)) n++
  return n === 0 ? 'false' : n === props.songs.length ? 'true' : 'mixed'
})

// ---------------------------------------------------------------- columnas
// `sort` es por que campo ordena esa columna; sin el, no se puede ordenar.
// `desc` es hacia donde ordena la PRIMERA vez que la pulsas: por artista se
// espera de la A a la Z, pero por duracion o estrellas se espera lo mas
// grande primero.
const COLS = [
  { k: 'n', cls: 'col-n', label: '#' },
  { k: 'fav', cls: 'col-fav', label: '', hidden: 'Favorita' },
  { k: 'title', cls: 'col-title', label: 'Título', sort: 'title' },
  { k: 'artist', cls: 'col-artist', label: 'Artista', sort: 'artist' },
  { k: 'album', cls: 'col-album', label: 'Álbum', sort: 'album' },
  { k: 'stars', cls: 'col-stars', label: 'Estrellas', sort: 'stars', desc: true },
  { k: 'key', cls: 'col-key', label: 'Tono', sort: 'key' },
  { k: 'bpm', cls: 'col-bpm', label: 'BPM', sort: 'bpm', desc: true },
  { k: 'dur', cls: 'col-dur', label: 'Dur.', sort: 'duration', desc: true },
  { k: 'kbps', cls: 'col-kbps', label: 'Kbps', sort: 'bitrate', desc: true },
  { k: 'copy', cls: 'col-copy', label: '', hidden: 'Copiar el nombre completo' }
]

// La casilla del modo «Seleccionar»: la primera columna, y solo con el modo
// puesto. Fuera de él no hay ni columna ni `<col>` (WebKitGTK conserva el
// ancho de una `<col>` escondida), así que no cuesta nada ni descuadra nada.
// Es fija: no se ensancha, y por eso no lleva tirador ni se guarda su ancho.
const CHECK = { k: 'check', cls: 'col-check', label: '' }

// Las columnas que hay se deciden AQUÍ, en un solo sitio: de `cols` beben el
// <colgroup>, la cabecera y lo que abarca la celda de un grupo, y de `show`
// las celdas. Hubo un `columns` que venía de la vista para esconder algunas,
// pero se quedó siempre en null: era código muerto que además invalidaba el
// v-memo de cada fila. `show` se deriva de `cols` para que no puedan
// discrepar (la cabecera y el cuerpo corridos un sitio, ya vivido).
const cols = computed(() => (props.selectMode ? [CHECK, ...COLS] : COLS))
const show = computed(() => Object.fromEntries(cols.value.map((c) => [c.k, true])))

// La celda de la cabecera de un grupo abarca las columnas que SE VEN, no
// todas. Las media queries esconden algunas (y su <col>), y cada columna de
// más que abarcase la celda nacía como columna fantasma, sin <col> y con su
// parte del ancho: medido en WebKitGTK, en estrecho el título se quedaba en
// 38 px y en medio sobraba un hueco de 224 px a la derecha de las filas. Se
// cuentan al aparecer la tabla, al cambiar sus columnas (el modo
// «Seleccionar») y al cambiar el tamaño de la ventana, que es lo que mueve las
// media queries. Sin grupos no hace falta: no hay celda que abarque nada.
const shownCols = ref(0)
function countCols() {
  const table = body.value?.closest('table')
  if (!table || !props.groups) return
  const visibleCols = [...table.querySelectorAll('col')].filter(
    (c) => getComputedStyle(c).display !== 'none'
  )
  shownCols.value = visibleCols.length
}
watch([cols, body], countCols, { flush: 'post' })
window.addEventListener('resize', countCols)
onUnmounted(() => window.removeEventListener('resize', countCols))

// ------------------------------------------------------------ anchos
//
// Mientras nadie toque nada NO se fija ningun ancho: la tabla se reparte como
// siempre y se adapta al panel. En cuanto arrastras por primera vez se toma
// una foto de los anchos que hay en ese momento y a partir de ahi mandas tu.
// Asi la vista de fabrica sigue cabiendo, y solo aparece barra horizontal si
// eres tu quien se pasa de ancho.
const WIDTHS_KEY = 'danplay.colWidths'
const MIN_WIDTH = 38
function savedWidths() {
  try {
    return JSON.parse(localStorage.getItem(WIDTHS_KEY) || 'null') || {}
  } catch {
    return {}
  }
}
const widths = ref(savedWidths())
const customWidths = computed(() => Object.keys(widths.value).length > 0)
watch(
  widths,
  (v) => {
    try {
      if (Object.keys(v).length) localStorage.setItem(WIDTHS_KEY, JSON.stringify(v))
      else localStorage.removeItem(WIDTHS_KEY)
    } catch {
      /* modo privado */
    }
  },
  { deep: true }
)

/**
 * Los anchos que se ven ahora mismo, para arrancar desde ahi.
 *
 * Se emparejan por NOMBRE (`data-col`) y no por posicion: una columna que una
 * media query esconda no ocupa sitio pero sigue en la lista, asi que contar
 * por indice desalineaba los anchos y la primera vez que arrastrabas la
 * columna daba un salto.
 */
function measureWidths() {
  const table = body.value?.closest('table')
  const snapshot = {}
  for (const th of table?.querySelectorAll('thead th[data-col]') || []) {
    const w = th.getBoundingClientRect().width
    // la casilla es fija y no se ensancha: su ancho no se guarda
    if (w > 0 && th.dataset.col !== CHECK.k) snapshot[th.dataset.col] = Math.round(w)
  }
  return snapshot
}

let drag = null
function startResize(col, e) {
  e.preventDefault()
  e.stopPropagation()
  // La foto se toma ahora pero NO se aplica todavia: escribir en `widths` aqui
  // repinta la tabla en pleno pointerdown, y con el elemento reemplazado el
  // navegador cancela el puntero y deja de mandar movimientos. Se aplica al
  // primer movimiento de verdad, que ademas es cuando hace falta.
  const base = customWidths.value ? { ...widths.value } : measureWidths()
  drag = { k: col.k, x: e.clientX, base, startWidth: base[col.k] || 120 }
  window.addEventListener('pointermove', moveResize)
  window.addEventListener('pointerup', endResize)
  window.addEventListener('pointercancel', endResize)
  document.body.classList.add('resizing-col')
}
function moveResize(e) {
  if (!drag) return
  widths.value = {
    ...drag.base,
    [drag.k]: Math.max(MIN_WIDTH, drag.startWidth + (e.clientX - drag.x))
  }
}
function endResize() {
  drag = null
  window.removeEventListener('pointermove', moveResize)
  window.removeEventListener('pointerup', endResize)
  window.removeEventListener('pointercancel', endResize)
  document.body.classList.remove('resizing-col')
}
/** Con el teclado: las flechas ensanchan o estrechan la columna de a 10 px. */
function keyResize(col, e) {
  const step = e.key === 'ArrowRight' ? 10 : e.key === 'ArrowLeft' ? -10 : 0
  if (!step) return
  e.preventDefault()
  e.stopPropagation()
  const base = customWidths.value ? { ...widths.value } : measureWidths()
  widths.value = { ...base, [col.k]: Math.max(MIN_WIDTH, (base[col.k] || 120) + step) }
}
/** Doble clic en el borde: se olvidan los anchos y vuelve el reparto normal. */
function resetColumn() {
  widths.value = {}
}
defineExpose({ resetColumn })
onUnmounted(endResize)

// ------------------------------------------------------------ orden
function clickHeader(col) {
  if (!col.sort) return
  emit('sortBy', col.sort, col.desc === true)
}
const sortedBy = (col) => col.sort && props.sort === col.sort
</script>

<template>
  <div :class="noHeader ? '' : 'table-wrap'" :data-sort-list="sortable ? '' : null">
    <table
      v-if="hasRows"
      class="song-table"
      :class="{ sized: customWidths }"
      role="grid"
      aria-multiselectable="true"
      :aria-label="label"
      :aria-rowcount="rowCount"
    >
      <!-- El ancho de cada columna (su clase, o el tuyo si la arrastraste).
           Va aqui y no solo en la cabecera: la tabla agrupada no la lleva, su
           primera fila es la del grupo (una celda que las abarca todas) y con
           `table-layout: fixed` las repartia a partes iguales, con el titulo
           tan estrecho como el numero. -->
      <colgroup>
        <col
          v-for="col in cols"
          :key="col.k"
          :class="col.cls"
          :style="customWidths && widths[col.k] ? { width: widths[col.k] + 'px' } : null"
        />
      </colgroup>
      <thead v-if="!noHeader">
        <tr aria-rowindex="1">
          <th
            v-for="(col, i) in cols"
            :key="col.k"
            :data-col="col.k"
            :class="[col.cls, { sortable: !!col.sort }]"
            :aria-sort="sortedBy(col) ? (desc ? 'descending' : 'ascending') : null"
          >
            <!-- la casilla de «todas»: un checkbox de tres estados (todas,
                 algunas, ninguna). Es el único control de la columna: las de
                 las filas son decorativas, la verdad de cada fila es su
                 aria-selected. ALTERNA (`toggle: true`): con todas puestas
                 las quita; Ctrl+A (useSongList) manda `toggle: false` -->
            <button
              v-if="col.k === CHECK.k"
              type="button"
              class="pick-all"
              role="checkbox"
              :aria-checked="pickState"
              aria-label="Seleccionar todas"
              :title="pickState === 'true' ? 'Quitar la selección' : 'Seleccionar todas'"
              @click="emit('selectAll', { toggle: true })"
            >
              <span
                class="pick-box"
                :class="{ on: pickState === 'true', mixed: pickState === 'mixed' }"
                aria-hidden="true"
              >
                <Icon v-if="pickState === 'true'" n="check" :t="12" :weight="2.4" />
              </span>
            </button>
            <!-- la cabecera que ordena es un boton: se alcanza y se pulsa
                 con el teclado -->
            <button
              v-else-if="col.sort"
              type="button"
              class="th-sort"
              :title="'Ordenar por ' + col.label"
              @click="clickHeader(col)"
            >
              <span class="th-txt">{{ col.label }}</span>
              <Icon v-if="sortedBy(col)" n="down" :t="11" class="th-arrow" :class="{ up: !desc }" />
            </button>
            <span v-else class="th-txt"
              >{{ col.label }}<span v-if="col.hidden" class="sr-only">{{ col.hidden }}</span></span
            >
            <!-- el tirador vive en el borde derecho; el ultimo no lleva. Es un
                 separador que se mueve con las flechas (el patron de ARIA para
                 un divisor ajustable); la regla lo cree un elemento estatico -->
            <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
            <span
              v-if="i < cols.length - 1 && col.k !== CHECK.k"
              class="col-resize"
              role="separator"
              aria-orientation="vertical"
              tabindex="-1"
              :aria-label="'Ancho de la columna ' + (col.label || col.k)"
              title="Arrastra para cambiar el ancho · doble clic para dejarlo como estaba"
              @pointerdown="startResize(col, $event)"
              @dblclick.stop="resetColumn()"
              @keydown="keyResize(col, $event)"
            ></span>
          </th>
        </tr>
      </thead>
      <tbody ref="body">
        <!-- separadores: ocupan el hueco de lo que no se pinta, para que la
             barra de desplazamiento siga midiendo lo mismo -->
        <tr v-if="padTop" data-spacer :style="{ height: padTop + 'px' }" aria-hidden="true"></tr>
        <template v-for="sec in sections" :key="sec.key">
          <!-- la cabecera del grupo: una fila mas, que se queda arriba
               mientras se recorre su grupo -->
          <tr v-if="sec.group" data-group-row class="group-row" :aria-rowindex="headRowIndex(sec)">
            <th :colspan="shownCols || cols.length" class="group-cell">
              <GroupHead :group="sec.group" @toggle="(k) => emit('toggleGroup', k)" />
            </th>
          </tr>
          <!-- Sin v-memo: con la lista recortada a lo que se ve (unas decenas
               de filas) repintarla es barato, y v-memo no funciona dentro de
               otro v-for (el de los grupos): todas las filas comparten su
               memoria y una podia reaprovecharse en el grupo equivocado. -->
          <tr
            v-for="(c, i) in sec.songs"
            :key="c.id"
            :ref="refFor(c.id)"
            :class="{
              selected: picked(c.id),
              playing: playing === c.id,
              flash: jumpTo === c.id,
              dragged: isDragged(c.id),
              'drop-before': before(c),
              'drop-after': after(c),
              ghost: c.ghost
            }"
            :data-drop="sortable ? dropKey(c) : null"
            data-song-row
            :tabindex="tabindex(c)"
            :aria-rowindex="rowIndex(sec, i)"
            :aria-selected="picked(c.id)"
            @pointerdown="startDrag(c, $event)"
            @click="emit('select', c.id, $event)"
            @dblclick="emit('play', c)"
            @keydown="onKey($event, c, sec.from + i)"
            @focus="onFocus(c)"
            @contextmenu.prevent="emit('context', $event, c)"
          >
            <!-- la casilla: decorativa (aria-hidden). Un clic en ella cae en
                 la fila, que ya emite `select`; la app decide si alterna. El
                 doble clic se queda en la celda: marcar y desmarcar deprisa
                 no pone la canción (en el resto de la fila, sí) -->
            <td v-if="show.check" class="col-check" @dblclick.stop>
              <span class="pick-box" :class="{ on: picked(c.id) }" aria-hidden="true">
                <Icon v-if="picked(c.id)" n="check" :t="12" :weight="2.4" />
              </span>
            </td>
            <td v-if="show.n" class="col-n mono">
              <button
                type="button"
                class="row-play"
                tabindex="-1"
                :title="rowTitle(c)"
                :aria-label="rowTitle(c) + ': ' + (c.title || c.file)"
                @click.stop="emit('play', c)"
              >
                <Icon :n="rowIcon(c)" :t="12" />
              </button>
              <span class="row-n">
                <Icon
                  v-if="playing === c.id"
                  :n="sounding ? 'pause' : 'play'"
                  :t="11"
                  style="margin-left: auto"
                />
                <template v-else>{{ numberOf(sec, i) }}</template>
              </span>
            </td>
            <td v-if="show.fav" class="col-fav">
              <button
                type="button"
                class="heart"
                :class="{ on: c.favorite }"
                tabindex="-1"
                :aria-pressed="!!c.favorite"
                :aria-label="c.favorite ? 'Quitar de favoritos' : 'Marcar como favorito'"
                @click.stop="emit('toggleFavorite', c)"
              >
                <Icon :n="c.favorite ? 'heartFull' : 'heart'" :t="16" />
              </button>
            </td>
            <td v-if="show.title" class="title">
              <!-- el texto se recorta; el copiar se queda siempre a la vista -->
              <span class="cell-copyable"
                ><span
                  class="cell-text"
                  :data-artist="show.artist && c.artist ? ' · ' + c.artist : null"
                  >{{ c.title || c.file
                  }}<span v-if="c.feat" class="sub"> · feat. {{ c.feat }}</span
                  ><span v-if="c.ghost" class="ghost-badge" title="Ya no está en tu biblioteca"
                    >No está</span
                  ></span
                ><CopyButton
                  class="list-copy"
                  :text="c.title || c.file"
                  what="el título"
                  :size="12"
                  @copied="(ok) => notifyCopied(ok, c.title || c.file)"
              /></span>
            </td>
            <td v-if="show.artist" class="sub col-artist">{{ c.artist || '—' }}</td>
            <td v-if="show.album" class="sub col-album">{{ c.album || '—' }}</td>
            <td v-if="show.stars" class="col-stars">
              <StarRating
                :value="c.stars || 0"
                :t="15"
                :focusable="false"
                @change="(n) => emit('setStars', c, n)"
              />
            </td>
            <td v-if="show.key" class="col-key mono sub">{{ c.key || '—' }}</td>
            <td v-if="show.bpm" class="col-bpm mono sub">{{ c.bpm ? Math.round(c.bpm) : '—' }}</td>
            <td v-if="show.dur" class="col-dur mono sub">{{ formatDuration(c.duration) }}</td>
            <td v-if="show.kbps" class="col-kbps mono sub">
              {{ c.bitrate ? Math.round(c.bitrate / 1000) : '—' }}
            </td>
            <td v-if="show.copy" class="col-copy">
              <CopyButton
                class="list-copy"
                :text="fullName(c)"
                what="el nombre completo"
                label="nombre"
                :size="11"
                @copied="(ok) => notifyCopied(ok, fullName(c))"
              />
            </td>
          </tr>
        </template>
        <tr
          v-if="padBottom"
          data-spacer
          :style="{ height: padBottom + 'px' }"
          aria-hidden="true"
        ></tr>
      </tbody>
    </table>
    <EmptyState
      v-else
      title="Nada por aquí"
      hint="Prueba con otra búsqueda o revisa tus carpetas"
    />
  </div>
</template>
