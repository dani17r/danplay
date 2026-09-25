<script setup>
/**
 * Cuadricula de caratulas.
 *
 * Una ficha se puede coger y soltar en un repertorio del menu lateral, y con
 * el teclado las flechas se mueven en las dos direcciones.
 */
import Icon from './Icon.vue'
import GroupHead from './GroupHead.vue'
import CoverArt from './ui/CoverArt.vue'
import EmptyState from './ui/EmptyState.vue'
import CopyButton from './ui/CopyButton.vue'
import { useTemplateRef } from 'vue'
import { useSongList } from '../composables/useSongList.js'
import { notifyCopied } from '../composables/useNotices.js'
import { fullName } from '../utils/format.js'

const props = defineProps({
  songs: { type: Array, required: true },
  // la elegida y la seleccion multiple las lee useSongList (la regla de
  // props sin usar no ve a traves de la llamada)
  // eslint-disable-next-line vue/no-unused-properties
  selected: { type: Number, default: null },
  // eslint-disable-next-line vue/no-unused-properties
  selectedIds: { type: Array, default: () => [] },
  playing: { type: Number, default: null },
  /** ancho de ficha, en px */
  size: { type: Number, default: 164 },
  jumpTo: { type: Number, default: null },
  sortable: Boolean,
  /** como se anuncia la lista */
  label: { type: String, default: 'Canciones' },
  /** los grupos, si la lista va agrupada (ver GroupedSongs) */
  // eslint-disable-next-line vue/no-unused-properties
  groups: { type: Array, default: null }
})
const emit = defineEmits(['select', 'play', 'context', 'toggleGroup'])

// Igual que en la tabla: solo se pintan las fichas que se ven. Cuantas caben
// por linea no hace falta calcularlo del css: se mira cuantas de las ya
// pintadas empiezan a la misma altura.
const box = useTemplateRef('box')
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
  hasRows,
  sectionId,
  padTop,
  padBottom,
  tabindex,
  onKey,
  onFocus
} = useSongList(props, emit, { anchor: () => box.value, grid: true })
</script>

<template>
  <div
    ref="box"
    class="grid"
    :style="{ '--card-w': (size || 164) + 'px' }"
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
        class="tile"
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
        <CoverArt
          :id="c.id"
          :blur="!!c.blur"
          class="art"
          :icon-size="30"
          :alt="c.title || c.file"
        />
        <button
          type="button"
          class="tile-play"
          tabindex="-1"
          :title="rowTitle(c)"
          :aria-label="rowTitle(c) + ': ' + (c.title || c.file)"
          @click.stop="emit('play', c)"
        >
          <Icon :n="rowIcon(c)" :t="15" />
        </button>
        <div class="name" :title="c.title">
          <Icon
            v-if="playing === c.id"
            :n="sounding ? 'pause' : 'play'"
            :t="11"
            style="display: inline-block; color: var(--accent); margin-right: 3px"
          />{{ c.title || c.file }}
        </div>
        <div class="sub2">{{ c.artist || '—' }}</div>
        <div class="tile-copy">
          <CopyButton
            class="list-copy"
            :text="c.title || c.file"
            what="el título"
            :size="11"
            @copied="(ok) => notifyCopied(ok, c.title || c.file)"
          />
          <CopyButton
            class="list-copy"
            :text="fullName(c)"
            what="el nombre completo"
            label="nombre"
            :size="10"
            @copied="(ok) => notifyCopied(ok, fullName(c))"
          />
        </div>
        <div
          v-if="c.stars"
          class="sub2"
          style="display: flex; gap: 1px; color: var(--amber)"
          role="img"
          :aria-label="'Valoración: ' + c.stars + ' de 5'"
        >
          <Icon v-for="n in c.stars" :key="n" n="star" :t="12" />
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
