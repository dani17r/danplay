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
import GroupHead from './GroupHead.vue'
import StarRating from './StarRating.vue'
import CoverArt from './ui/CoverArt.vue'
import EmptyState from './ui/EmptyState.vue'
import CopyButton from './ui/CopyButton.vue'
import { useTemplateRef } from 'vue'
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
  jumpTo: { type: Number, default: null },
  sortable: Boolean,
  /** como se anuncia la lista */
  label: { type: String, default: 'Canciones' },
  /** los grupos, si la lista va agrupada (ver GroupedSongs) */
  // eslint-disable-next-line vue/no-unused-properties
  groups: { type: Array, default: null }
})
const emit = defineEmits(['select', 'play', 'setStars', 'toggleFavorite', 'context', 'toggleGroup'])

const caja = useTemplateRef('caja')
const {
  picked,
  before,
  after,
  dropKey,
  rowIcon,
  rowTitle,
  isDragged,
  startDrag,
  refFor,
  sections,
  hasRows,
  sectionId,
  padTop,
  padBottom,
  tabindex,
  onKey,
  onFocus
} = useSongList(props, emit, { anchor: () => caja.value, grid: true })
</script>

<template>
  <div
    ref="caja"
    class="cards"
    :data-sort-list="sortable ? '' : null"
    :role="hasRows ? 'listbox' : null"
    :aria-multiselectable="hasRows ? 'true' : null"
    :aria-label="hasRows ? label : null"
  >
    <div
      v-if="padTop"
      data-spacer
      aria-hidden="true"
      :style="{ gridColumn: '1 / -1', height: padTop + 'px' }"
    ></div>
    <!-- un trozo por grupo, con su cabecera (sin agrupar, uno solo y sin
         nada): `display: contents`, asi sus filas siguen siendo de la lista.
         Sin v-memo: con la lista recortada a lo que se ve repintarla es
         barato, y dentro de otro v-for no funciona (las filas de todos los
         grupos comparten su memoria). -->
    <div
      v-for="sec in sections"
      :key="sec.key"
      class="list-section"
      :role="sec.group ? 'group' : 'none'"
      :aria-labelledby="sec.group ? sectionId(sec) : undefined"
    >
      <div v-if="sec.group" data-group-row class="group-row">
        <GroupHead
          :id="sectionId(sec)"
          :group="sec.group"
          @toggle="(k) => emit('toggleGroup', k)"
        />
      </div>
      <!-- el tabindex va enlazado (foco itinerante: una sola parada del
           tabulador por lista) y la regla solo entiende los escritos a mano -->
      <!-- eslint-disable-next-line vuejs-accessibility/interactive-supports-focus -->
      <div
        v-for="(c, i) in sec.songs"
        :key="c.id"
        :ref="refFor(c.id)"
        class="card-song"
        :class="{
          selected: picked(c.id),
          playing: playing === c.id,
          flash: jumpTo === c.id,
          dragged: isDragged(c.id),
          'drop-before': before(c),
          'drop-after': after(c)
        }"
        :data-drop="sortable ? dropKey(c) : null"
        data-drop-axis="x"
        data-song-row
        role="option"
        :tabindex="tabindex(c)"
        :aria-selected="picked(c.id)"
        :aria-setsize="songs.length"
        :aria-posinset="sec.from + i + 1"
        @pointerdown="startDrag(c, $event)"
        @click="emit('select', c.id, $event)"
        @dblclick="emit('play', c)"
        @keydown="onKey($event, c, sec.from + i)"
        @focus="onFocus(c)"
        @contextmenu.prevent="emit('context', $event, c)"
      >
        <div class="card-art">
          <CoverArt
            :id="c.id"
            :blur="!!c.blur"
            class="card-cover"
            :icon-size="18"
            :alt="c.title || c.file"
          />
          <button
            type="button"
            class="card-play"
            tabindex="-1"
            :title="rowTitle(c)"
            :aria-label="rowTitle(c) + ': ' + (c.title || c.file)"
            @click.stop="emit('play', c)"
          >
            <Icon :n="rowIcon(c)" :t="13" />
          </button>
        </div>
        <div class="card-txt">
          <div class="card-title-row">
            <div class="card-title">{{ c.title || c.file }}</div>
            <CopyButton
              class="list-copy"
              :text="c.title || c.file"
              what="el título"
              :size="11"
              @copied="(ok) => notifyCopied(ok, c.title || c.file)"
            />
          </div>
          <div class="card-sub sub">
            {{ c.artist || '—' }}<template v-if="c.album"> · {{ c.album }}</template>
          </div>
          <div class="card-foot">
            <StarRating
              :value="c.stars || 0"
              :t="12"
              :focusable="false"
              @change="(n) => emit('setStars', c, n)"
            />
            <button
              type="button"
              class="heart"
              :class="{ on: c.favorite }"
              tabindex="-1"
              :aria-pressed="!!c.favorite"
              :aria-label="c.favorite ? 'Quitar de favoritos' : 'Marcar como favorito'"
              @click.stop="emit('toggleFavorite', c)"
            >
              <Icon :n="c.favorite ? 'heartFull' : 'heart'" :t="13" />
            </button>
            <span class="sub mono card-dur">{{ formatDuration(c.duration) }}</span>
            <CopyButton
              class="list-copy"
              :text="fullName(c)"
              what="el nombre completo"
              label="nombre"
              :size="10"
              @copied="(ok) => notifyCopied(ok, fullName(c))"
            />
          </div>
        </div>
      </div>
    </div>
    <div
      v-if="padBottom"
      data-spacer
      aria-hidden="true"
      :style="{ gridColumn: '1 / -1', height: padBottom + 'px' }"
    ></div>
    <EmptyState
      v-if="!hasRows"
      style="grid-column: 1/-1"
      title="Nada por aquí"
      hint="Prueba con otra búsqueda o revisa tus carpetas"
    />
  </div>
</template>
