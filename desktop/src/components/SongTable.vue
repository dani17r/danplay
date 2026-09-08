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
 */
import StarRating from './StarRating.vue'
import Icon from './Icon.vue'
import EmptyState from './ui/EmptyState.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { ref, watch, nextTick, computed, onUnmounted } from 'vue'
import { useVirtualRows } from '../composables/useVirtualRows.js'

const { startDrag, isDragged } = useDragSong()
const props = defineProps(['songs','selected','playing','sort','desc','noHeader',
                           'jumpTo'])
const emit = defineEmits(['select','play','setStars','toggleFavorite','sortBy','context'])

// Un Map normal, fuera de la reactividad: aqui solo se guardan nodos del DOM
// para poder hacerles scroll, y nadie los pinta. Se vacia al cambiar la lista
// porque si no se van acumulando las filas de cada busqueda anterior.
const rows = new Map()
watch(() => props.songs, () => rows.clear())

// ---------------------------------------------------------------- columnas
// `sort` es por que campo ordena esa columna; sin el, no se puede ordenar.
// `desc` es hacia donde ordena la PRIMERA vez que la pulsas: por artista se
// espera de la A a la Z, pero por duracion o estrellas se espera lo mas
// grande primero.
const COLS = [
  { k: 'n',      cls: 'col-n',      label: '#' },
  { k: 'fav',    cls: 'col-fav',    label: '' },
  { k: 'title',  cls: 'col-title',  label: 'Titulo',    sort: 'title' },
  { k: 'artist', cls: 'col-artist', label: 'Artista',   sort: 'artist' },
  { k: 'album',  cls: 'col-album',  label: 'Album',     sort: 'album' },
  { k: 'stars',  cls: 'col-stars',  label: 'Estrellas', sort: 'stars', desc: true },
  { k: 'key',    cls: 'col-key',    label: 'Tono',      sort: 'key' },
  { k: 'bpm',    cls: 'col-bpm',    label: 'BPM',       sort: 'bpm', desc: true },
  { k: 'dur',    cls: 'col-dur',    label: 'Dur.',      sort: 'duration', desc: true },
  { k: 'kbps',   cls: 'col-kbps',   label: 'Kbps',      sort: 'bitrate', desc: true }
]

// La tabla enseña todas sus columnas. Hubo un `columns` que venia de la vista
// para esconder algunas, pero se quedo siempre en null: era codigo muerto que
// ademas invalidaba el v-memo de cada fila. Se deja `show` porque el marcado
// lo consulta columna a columna y es donde se volveria a enganchar.
const show = Object.fromEntries(COLS.map(c => [c.k, true]))
const cols = computed(() => COLS)

// ------------------------------------------------------------ anchos
//
// Mientras nadie toque nada NO se fija ningun ancho: la tabla se reparte como
// siempre y se adapta al panel. En cuanto arrastras por primera vez se toma
// una foto de los anchos que hay en ese momento y a partir de ahi mandas tu.
// Asi la vista de fabrica sigue cabiendo, y solo aparece barra horizontal si
// eres tu quien se pasa de ancho.
const CLAVE = 'danplay.colWidths'
const MINIMO = 38
function guardados () {
  try { return JSON.parse(localStorage.getItem(CLAVE) || 'null') || {} } catch { return {} }
}
const widths = ref(guardados())
const aMedida = computed(() => Object.keys(widths.value).length > 0)
watch(widths, (v) => {
  try {
    if (Object.keys(v).length) localStorage.setItem(CLAVE, JSON.stringify(v))
    else localStorage.removeItem(CLAVE)
  } catch { /* modo privado */ }
}, { deep: true })

/**
 * Los anchos que se ven ahora mismo, para arrancar desde ahi.
 *
 * Se emparejan por NOMBRE (`data-col`) y no por posicion: una columna que una
 * media query esconda no ocupa sitio pero sigue en la lista, asi que contar
 * por indice desalineaba los anchos y la primera vez que arrastrabas la
 * columna daba un salto.
 */
function fotoDeAnchos () {
  const tabla = body.value?.closest('table')
  const foto = {}
  for (const th of tabla?.querySelectorAll('thead th[data-col]') || []) {
    const w = th.getBoundingClientRect().width
    if (w > 0) foto[th.dataset.col] = Math.round(w)
  }
  return foto
}

let arrastre = null
function startResize (col, e) {
  e.preventDefault(); e.stopPropagation()
  // La foto se toma ahora pero NO se aplica todavia: escribir en `widths` aqui
  // repinta la tabla en pleno pointerdown, y con el elemento reemplazado el
  // navegador cancela el puntero y deja de mandar movimientos. Se aplica al
  // primer movimiento de verdad, que ademas es cuando hace falta.
  const base = aMedida.value ? { ...widths.value } : fotoDeAnchos()
  arrastre = { k: col.k, x: e.clientX, base, desde: base[col.k] || 120 }
  window.addEventListener('pointermove', moveResize)
  window.addEventListener('pointerup', endResize)
  window.addEventListener('pointercancel', endResize)
  document.body.classList.add('resizing-col')
}
function moveResize (e) {
  if (!arrastre) return
  widths.value = { ...arrastre.base,
    [arrastre.k]: Math.max(MINIMO, arrastre.desde + (e.clientX - arrastre.x)) }
}
function endResize () {
  arrastre = null
  window.removeEventListener('pointermove', moveResize)
  window.removeEventListener('pointerup', endResize)
  window.removeEventListener('pointercancel', endResize)
  document.body.classList.remove('resizing-col')
}
/** Doble clic en el borde: se olvidan los anchos y vuelve el reparto normal. */
function resetColumn () {
  widths.value = {}
}
defineExpose({ resetColumn })
onUnmounted(endResize)

// ------------------------------------------------------------ orden
function clickHeader (col) {
  if (!col.sort) return
  emit('sortBy', col.sort, col.desc === true)
}
const sortedBy = (col) => col.sort && props.sort === col.sort

// ------------------------------------------------------------ virtualizado
const body = ref(null)
const { from, to, padTop, padBottom, reveal } =
  useVirtualRows(() => body.value, () => props.songs.length)
const visible = computed(() => props.songs.slice(from.value, to.value))

watch(() => props.jumpTo, async (id) => {
  if (!id) return
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

const fmtDuration = (s) => {
  if (!s) return '—'
  const m = Math.floor(s / 60), r = Math.floor(s % 60)
  return `${m}:${String(r).padStart(2, '0')}`
}
</script>

<template>
  <div :class="noHeader ? '' : 'table-wrap'">
    <table v-if="songs.length" class="song-table" :class="{medida: aMedida}">
      <colgroup v-if="aMedida">
        <col v-for="col in cols" :key="col.k"
             :style="widths[col.k] ? {width: widths[col.k] + 'px'} : null" />
      </colgroup>
      <thead v-if="!noHeader">
        <tr>
          <th v-for="(col, i) in cols" :key="col.k" :data-col="col.k"
              :class="[col.cls, {sortable: !!col.sort}]"
              :title="col.sort ? 'Ordenar por ' + (col.label || 'esta columna') : null"
              :aria-sort="sortedBy(col) ? (desc ? 'descending' : 'ascending') : null"
              @click="clickHeader(col)">
            <span class="th-txt">{{ col.label }}</span>
            <Icon v-if="sortedBy(col)" n="down" :t="11"
                  class="th-arrow" :class="{up: !desc}" />
            <!-- el tirador vive en el borde derecho; el ultimo no lleva -->
            <span v-if="i < cols.length - 1" class="col-resize"
                  title="Arrastra para cambiar el ancho · doble clic para dejarlo como estaba"
                  @pointerdown="startResize(col, $event)"
                  @dblclick.stop="resetColumn()"
                  @click.stop></span>
          </th>
        </tr>
      </thead>
      <tbody ref="body">
        <!-- separadores: ocupan el hueco de lo que no se pinta, para que la
             barra de desplazamiento siga midiendo lo mismo -->
        <tr v-if="padTop" data-spacer :style="{height: padTop + 'px'}" aria-hidden="true"></tr>
        <!-- v-memo: una fila solo se vuelve a pintar si cambia algo de LO QUE
             ELLA enseña. Sin esto, seleccionar una cancion obligaba a repasar
             las mil filas de la lista con sus ocho iconos cada una. -->
        <tr v-for="(c,i) in visible" :key="c.id"
            v-memo="[from + i, c.id, c.title, c.file, c.feat, c.artist, c.album, c.stars,
                     c.favorite, c.key, c.bpm, c.duration, c.bitrate,
                     selected===c.id, playing===c.id, jumpTo===c.id, isDragged(c.id)]"
            :ref="el => { if (el) rows.set(c.id, el) }"
            :class="{selected: selected===c.id, playing: playing===c.id,
                     flash: jumpTo===c.id, dragged: isDragged(c.id)}"
            @pointerdown="startDrag(c, $event)"
            @click="emit('select', c.id)"
            @dblclick="emit('play', c)"
            @contextmenu.prevent="emit('context', $event, c)">
          <td v-if="show.n" class="col-n mono">
            <button class="row-play" :title="playing===c.id ? 'Volver a empezar' : 'Reproducir'"
                    @click.stop="emit('play', c)"><Icon n="play" :t="12" /></button>
            <span class="row-n">
              <Icon v-if="playing===c.id" n="play" :t="11" style="margin-left:auto" />
              <template v-else>{{ from + i + 1 }}</template>
            </span>
          </td>
          <td v-if="show.fav" class="col-fav">
            <span class="heart" :class="{on:c.favorite}"
                  @click.stop="emit('toggleFavorite', c)">
              <Icon :n="c.favorite ? 'heartFull' : 'heart'" :t="16" /></span>
          </td>
          <td v-if="show.title" class="title">
            {{ c.title || c.file }}
            <span class="sub" v-if="c.feat"> · feat. {{ c.feat }}</span>
          </td>
          <td v-if="show.artist" class="sub">{{ c.artist || '—' }}</td>
          <td v-if="show.album" class="sub">{{ c.album || '—' }}</td>
          <td v-if="show.stars" class="col-stars">
            <StarRating :value="c.stars||0" :t="15" @change="n=>emit('setStars',c,n)" />
          </td>
          <td v-if="show.key" class="col-key mono sub">{{ c.key || '—' }}</td>
          <td v-if="show.bpm" class="col-bpm mono sub">{{ c.bpm ? Math.round(c.bpm) : '—' }}</td>
          <td v-if="show.dur" class="col-dur mono sub">{{ fmtDuration(c.duration) }}</td>
          <td v-if="show.kbps" class="col-kbps mono sub">
            {{ c.bitrate ? Math.round(c.bitrate/1000) : '—' }}</td>
        </tr>
        <tr v-if="padBottom" data-spacer :style="{height: padBottom + 'px'}" aria-hidden="true"></tr>
      </tbody>
    </table>
    <EmptyState v-else title="Nada por aqui"
                hint="Prueba con otra busqueda o revisa tus carpetas" />
  </div>
</template>
