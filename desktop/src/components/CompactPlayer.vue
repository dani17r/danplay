<script setup>
/**
 * El reproductor solo, cuando la ventana es tan pequeña que la biblioteca no
 * cabe: la forma «barra» (una tira baja) y la «cuadrito» (un cuadrado en una
 * esquina), o la ventana encogida a mano hasta ahí (`useViewport().pocket`).
 *
 * Esas dos formas van sin marco, así que la ventana se arrastra desde
 * cualquier sitio que no sea un botón (`data-tauri-drag-region`). La
 * biblioteca no se desmonta: sigue debajo, escondida, y vuelve tal cual.
 */
import { computed, ref, onMounted, onUnmounted } from 'vue'
import { usePlayback, MAX_VOLUME } from '../composables/usePlayback.js'
import { useScrub } from '../composables/useScrub.js'
import { useWindowShape, SHAPES } from '../composables/useWindowShape.js'
import { inTauri } from '../api.js'
import { formatTime } from '../utils/format.js'
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import SliderField from './ui/SliderField.vue'

const props = defineProps({
  /** `barra` o `cuadro` */
  variant: { type: String, default: 'barra' },
  /** hay sitio para el volumen */
  roomy: Boolean
})
const emit = defineEmits(['playSelected'])

const player = usePlayback()
const { track, playing, position, duration, hasPrevious, hasNext, volume } = player
const shapes = useWindowShape()
const loaded = computed(() => !!track.value)
const { scrub, start: grabNeedle } = useScrub({ duration, seek: (s) => player.seek(s) })
const shown = computed(() => (scrub.active ? scrub.value : position.value))
const percent = computed(() =>
  duration.value ? Math.min(100, (shown.value / duration.value) * 100) : 0
)

function toggle() {
  if (!loaded.value) return emit('playSelected')
  player.toggle()
}
// Las formas, en un panel que tapa el reproductor: en una barra de cien
// píxeles de alto, un menú de diez opciones no cabe.
const showShapes = ref(false)
const shapeList = computed(() => SHAPES.filter((s) => inTauri || !s.only))
function pick(v) {
  showShapes.value = false
  shapes.setShape(v)
}
function onKey(e) {
  if (e.key === 'Escape' && showShapes.value) {
    e.stopPropagation()
    showShapes.value = false
  }
}
onMounted(() => window.addEventListener('keydown', onKey, true))
onUnmounted(() => window.removeEventListener('keydown', onKey, true))
</script>

<template>
  <div class="pocket" :class="'pocket-' + props.variant" data-tauri-drag-region>
    <div v-if="props.variant === 'cuadro'" class="pocket-backdrop" aria-hidden="true">
      <CoverArt :id="track?.id" :blur="true" class="pocket-backdrop-art" :icon-size="1" />
    </div>

    <div class="pocket-top" data-tauri-drag-region>
      <CoverArt
        :id="track?.id"
        :blur="!!track?.blur"
        class="pocket-art"
        :icon-size="props.variant === 'cuadro' ? 30 : 20"
        :size="props.variant === 'cuadro' ? 192 : 96"
        :alt="track?.title || ''"
      />
      <div class="pocket-text" data-tauri-drag-region>
        <div class="pocket-title" data-tauri-drag-region :title="track?.title || undefined">
          {{ loaded ? track.title || 'Sin título' : 'Nada sonando' }}
        </div>
        <div class="pocket-artist" data-tauri-drag-region :title="track?.artist || undefined">
          {{ loaded ? track.artist || 'Sin artista' : 'Elige algo en la lista' }}
        </div>
      </div>
      <div class="pocket-window">
        <button
          type="button"
          class="pocket-icon"
          title="Forma de la ventana"
          aria-label="Forma de la ventana"
          :aria-expanded="showShapes"
          @click="showShapes = !showShapes"
        >
          <Icon n="shapes" :t="15" />
        </button>
        <button
          type="button"
          class="pocket-icon"
          title="Volver a la ventana normal, con la biblioteca"
          aria-label="Volver a la ventana normal"
          @click="shapes.setShape('normal')"
        >
          <Icon n="shapeNormal" :t="15" />
        </button>
      </div>
    </div>

    <div class="pocket-seek-row">
      <span class="pocket-time">{{ formatTime(shown) }}</span>
      <div
        class="pocket-seek"
        :class="{ off: !loaded, scrubbing: scrub.active }"
        :title="loaded ? 'Arrastra la aguja o pincha donde quieras ir' : ''"
        @pointerdown="grabNeedle"
      >
        <div class="pocket-seek-fill" :style="{ width: percent + '%' }"></div>
      </div>
      <span class="pocket-time">{{ loaded ? formatTime(duration) : '' }}</span>
    </div>

    <div class="pocket-controls">
      <button
        v-if="props.variant === 'cuadro'"
        type="button"
        class="pocket-btn"
        :disabled="!loaded"
        title="Retroceder 10 s"
        @click="player.nudge(-10)"
      >
        <Icon n="back10" :t="15" />
      </button>
      <button
        type="button"
        class="pocket-btn"
        :disabled="!loaded || !hasPrevious"
        title="Anterior"
        @click="player.previous()"
      >
        <Icon n="previous" :t="16" />
      </button>
      <button
        type="button"
        class="pocket-btn pocket-play"
        :title="playing ? 'Pausar (espacio)' : 'Reproducir (espacio)'"
        @click="toggle"
      >
        <Icon :n="playing ? 'pause' : 'play'" :t="18" />
      </button>
      <button
        type="button"
        class="pocket-btn"
        :disabled="!loaded || !hasNext"
        title="Siguiente"
        @click="player.next()"
      >
        <Icon n="next" :t="16" />
      </button>
      <button
        v-if="props.variant === 'cuadro'"
        type="button"
        class="pocket-btn"
        :disabled="!loaded"
        title="Avanzar 10 s"
        @click="player.nudge(10)"
      >
        <Icon n="forward10" :t="15" />
      </button>
      <div v-if="props.variant === 'barra' && props.roomy" class="pocket-volume">
        <Icon :n="volume ? 'volume' : 'mute'" :t="14" />
        <SliderField
          :model-value="volume"
          :min="0"
          :max="MAX_VOLUME"
          :step="0.01"
          :mark="1"
          width="100%"
          aria-label="Volumen"
          :value-text="Math.round(volume * 100) + ' %'"
          @update:model-value="(v) => player.setVolume(v)"
        />
      </div>
    </div>
    <div v-if="showShapes" class="pocket-shapes" role="menu" aria-label="Forma de la ventana">
      <button
        v-for="s in shapeList"
        :key="s.v"
        type="button"
        class="pocket-shape"
        role="menuitemradio"
        :aria-checked="shapes.shape.value === s.v"
        :class="{ on: shapes.shape.value === s.v }"
        :title="s.n + (s.note ? ' (' + s.note + ')' : '')"
        @click="pick(s.v)"
      >
        <Icon :n="s.icon" :t="16" />
        <span>{{ s.n }}</span>
      </button>
      <button
        v-if="inTauri"
        type="button"
        class="pocket-shape"
        role="menuitemcheckbox"
        :aria-checked="shapes.onTop.value"
        :class="{ on: shapes.onTop.value }"
        title="Siempre encima de las demás ventanas"
        @click="shapes.toggleOnTop()"
      >
        <Icon n="pin" :t="16" />
        <span>Siempre encima</span>
      </button>
      <button
        type="button"
        class="pocket-shape pocket-shapes-close"
        title="Cerrar (Escape)"
        @click="showShapes = false"
      >
        <Icon n="close" :t="16" />
        <span>Cerrar</span>
      </button>
    </div>
  </div>
</template>
