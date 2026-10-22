<script setup>
/**
 * La lista de canciones del celular, hecha para los dedos.
 *
 * - Doble toque en la canción: suena (o, con «Un toque la pone», un toque).
 * - Un toque: la elige y abre debajo sus botones grandes (sonar, estudiar,
 *   letra), para no tener que acertar en nada pequeño.
 * - La carátula es un botón de sonar.
 * - Mantener pulsado o el ⋮: el resto de opciones.
 *
 * El doble toque se cuenta a mano con los `click`: el WebView de Android no
 * siempre manda `dblclick` al tocar, y por eso en 1.26 no sonaba nada.
 *
 * Con `editing` (ordenar una lista) cada fila lleva subir, bajar y quitar.
 */
import { onUnmounted } from 'vue'
import Icon from '../components/Icon.vue'
import CoverArt from '../components/ui/CoverArt.vue'
import { formatDuration } from '../utils/format.js'

const props = defineProps({
  songs: { type: Array, required: true },
  selectedId: { type: Number, default: null },
  playingId: { type: Number, default: null },
  /** suena de verdad (no en pausa) */
  playing: Boolean,
  /** un toque pone la canción (si no, la elige y el doble toque la pone) */
  tapPlays: Boolean,
  editing: Boolean,
  label: { type: String, default: 'Canciones' }
})
const emit = defineEmits(['select', 'play', 'menu', 'study', 'lyrics', 'move', 'remove'])

const DOUBLE_MS = 330
const HOLD_MS = 480
let lastTap = { id: null, t: 0 }
let hold = null
let held = false
let startXY = null

function tap(song) {
  if (held) {
    held = false
    return
  }
  if (props.editing) return
  const now = Date.now()
  const double = lastTap.id === song.id && now - lastTap.t < DOUBLE_MS
  lastTap = double ? { id: null, t: 0 } : { id: song.id, t: now }
  if (double) {
    // el primer toque ya la eligió: el segundo la pone a sonar
    if (props.playingId !== song.id || !props.playing) emit('play', song)
    return
  }
  if (props.tapPlays && props.playingId !== song.id) emit('play', song)
  emit('select', song)
}

/** La carátula: sonar (o pausar), salvo que se acabe de mantener pulsada. */
function coverTap(song) {
  if (held) {
    held = false
    return
  }
  if (!props.editing) emit('play', song)
}

// mantener pulsado: las opciones (el WebView también manda `contextmenu`,
// pero no en todos los teléfonos, y a veces después de soltar)
function down(song, e) {
  if (props.editing) return
  clearTimeout(hold)
  held = false
  startXY = [e.clientX, e.clientY]
  hold = setTimeout(() => {
    held = true
    navigator.vibrate?.(12)
    emit('menu', { clientX: startXY[0], clientY: startXY[1] }, song)
  }, HOLD_MS)
}
function move(e) {
  if (!startXY) return
  if (Math.abs(e.clientX - startXY[0]) > 10 || Math.abs(e.clientY - startXY[1]) > 10) {
    clearTimeout(hold)
  }
}
function up() {
  clearTimeout(hold)
  startXY = null
}
function context(song, e) {
  if (held || props.editing) return
  clearTimeout(hold)
  held = true
  emit('menu', e, song)
}
onUnmounted(() => clearTimeout(hold))

function sub(c) {
  return [c.artist || '—', c.key, c.duration ? formatDuration(c.duration) : '']
    .filter(Boolean)
    .join(' · ')
}
</script>

<template>
  <ul class="m-songs" :aria-label="label">
    <li
      v-for="(c, i) in songs"
      :key="c.id"
      class="m-song-item"
      :class="{ sel: selectedId === c.id && !editing, now: playingId === c.id }"
    >
      <!-- la fila entera es la zona de toque; los botones de dentro paran el suyo -->
      <!-- eslint-disable-next-line vuejs-accessibility/click-events-have-key-events, vuejs-accessibility/no-static-element-interactions -->
      <div
        class="m-song"
        :aria-current="playingId === c.id ? 'true' : undefined"
        @click="tap(c)"
        @pointerdown="down(c, $event)"
        @pointermove="move"
        @pointerup="up"
        @pointercancel="up"
        @pointerleave="up"
        @contextmenu.prevent="context(c, $event)"
      >
        <button
          type="button"
          class="m-song-cover"
          :aria-label="
            (playingId === c.id && playing ? 'Pausar ' : 'Reproducir ') + (c.title || c.file)
          "
          @click.stop="coverTap(c)"
        >
          <CoverArt :id="c.id" :size="96" :icon-size="20" :alt="''" />
          <span v-if="!editing" class="m-song-cover-veil" aria-hidden="true">
            <span v-if="playingId === c.id && playing" class="m-eq"><i></i><i></i><i></i></span>
            <Icon v-else :n="playingId === c.id ? 'pause' : 'play'" :t="18" />
          </span>
        </button>
        <div class="m-song-text">
          <div class="m-song-title">{{ c.title || c.file }}</div>
          <div class="m-song-sub">
            <Icon v-if="c.favorite" n="heartFull" :t="12" class="m-song-fav" />{{ sub(c) }}
          </div>
        </div>
        <template v-if="editing">
          <button
            type="button"
            class="m-icon"
            :disabled="i === 0"
            :aria-label="'Subir ' + (c.title || c.file)"
            @click.stop="emit('move', i, i - 1)"
          >
            <Icon n="up" :t="20" />
          </button>
          <button
            type="button"
            class="m-icon"
            :disabled="i === songs.length - 1"
            :aria-label="'Bajar ' + (c.title || c.file)"
            @click.stop="emit('move', i, i + 1)"
          >
            <Icon n="down" :t="20" />
          </button>
          <button
            type="button"
            class="m-icon m-danger"
            :aria-label="'Quitar de la lista ' + (c.title || c.file)"
            @click.stop="emit('remove', c)"
          >
            <Icon n="close" :t="18" />
          </button>
        </template>
        <button
          v-else
          type="button"
          class="m-icon m-song-more"
          :aria-label="'Opciones de ' + (c.title || c.file)"
          @click.stop="emit('menu', $event, c)"
        >
          <Icon n="moreVertical" :t="20" />
        </button>
      </div>
      <transition name="m-expand">
        <div v-if="selectedId === c.id && !editing" class="m-song-actions">
          <button type="button" class="m-chip-btn m-primary" @click="emit('play', c)">
            <Icon :n="playingId === c.id && playing ? 'pause' : 'play'" :t="16" />
            {{ playingId === c.id && playing ? 'Pausar' : 'Reproducir' }}
          </button>
          <button type="button" class="m-chip-btn" @click="emit('study', c)">
            <Icon n="academic" :t="16" /> Estudiar
          </button>
          <button
            type="button"
            class="m-chip-btn"
            :disabled="!c.has_lyrics && !c.has_chords"
            @click="emit('lyrics', c)"
          >
            <Icon n="lyrics" :t="16" /> Letra
          </button>
        </div>
      </transition>
    </li>
  </ul>
</template>
