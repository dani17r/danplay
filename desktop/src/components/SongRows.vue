<script setup>
/**
 * Lista fina: una linea por cancion, a lo largo de todo el ancho.
 *
 * Es la vista para recorrer mucho con poco sitio: nada de columnas, solo el
 * titulo con su artista al lado y lo justo a la derecha. Al ser una sola caja
 * por fila se adapta sola a cualquier ancho, sin columnas que descuadrar.
 */
import Icon from './Icon.vue'
import GroupHead from './GroupHead.vue'
import StarRating from './StarRating.vue'
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
  numberOf,
  hasRows,
  sectionId,
  padTop,
  padBottom,
  tabindex,
  onKey,
  onFocus
} = useSongList(props, emit, { anchor: () => caja.value })
</script>

<template>
  <div
    ref="caja"
    class="rows"
    :data-sort-list="sortable ? '' : null"
    :role="hasRows ? 'listbox' : null"
    :aria-multiselectable="hasRows ? 'true' : null"
    :aria-label="hasRows ? label : null"
  >
    <div v-if="padTop" data-spacer :style="{ height: padTop + 'px' }" aria-hidden="true"></div>
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
        class="row"
        :class="{
          selected: picked(c.id),
          playing: playing === c.id,
          flash: jumpTo === c.id,
          dragged: isDragged(c.id),
          'drop-before': before(c),
          'drop-after': after(c)
        }"
        :data-drop="sortable ? dropKey(c) : null"
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
        <span class="row-num mono">{{ numberOf(sec, i) }}</span>
        <button
          type="button"
          class="row-go"
          tabindex="-1"
          :title="rowTitle(c)"
          :aria-label="rowTitle(c) + ': ' + (c.title || c.file)"
          @click.stop="emit('play', c)"
        >
          <Icon :n="rowIcon(c)" :t="12" />
        </button>
        <span class="row-title">{{ c.title || c.file }}</span>
        <CopyButton
          class="list-copy"
          :text="c.title || c.file"
          what="el título"
          :size="12"
          @copied="(ok) => notifyCopied(ok, c.title || c.file)"
        />
        <span class="row-artist sub">{{ c.artist || '—' }}</span>
        <span class="row-extra sub mono">{{ c.key || '' }}</span>
        <StarRating
          class="row-stars"
          :value="c.stars || 0"
          :t="13"
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
          <Icon :n="c.favorite ? 'heartFull' : 'heart'" :t="14" />
        </button>
        <span class="row-dur sub mono">{{ formatDuration(c.duration) }}</span>
        <CopyButton
          class="list-copy"
          :text="fullName(c)"
          what="el nombre completo"
          label="nombre"
          :size="11"
          @copied="(ok) => notifyCopied(ok, fullName(c))"
        />
      </div>
    </div>
    <div
      v-if="padBottom"
      data-spacer
      :style="{ height: padBottom + 'px' }"
      aria-hidden="true"
    ></div>
    <EmptyState
      v-if="!hasRows"
      title="Nada por aquí"
      hint="Prueba con otra búsqueda o revisa tus carpetas"
    />
  </div>
</template>
