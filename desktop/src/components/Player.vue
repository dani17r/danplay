<script setup>
/**
 * La barra de reproducción de abajo.
 *
 * No tiene estado propio: todo sale de `usePlayback`, que escucha lo que
 * cuenta Rust. Antes preguntaba el estado cuatro veces por segundo con un
 * temporizador; ahora Rust avisa cuando algo cambia.
 */
import { ref, computed, nextTick } from 'vue'
import { onClickOutside } from '../composables/useClickOutside.js'
import { usePlayback } from '../composables/usePlayback.js'
import { useHotkeys } from '../composables/useHotkeys.js'
import { formatTime } from '../utils/format.js'
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import SliderField from './ui/SliderField.vue'

const emit = defineEmits(['goToOrigin', 'playSelected'])

const player = usePlayback()
const {
  track,
  queue,
  playing,
  position,
  duration,
  volume,
  speed,
  repeat,
  shuffle,
  error,
  hasOutput,
  origin
} = player

// Como se pinta cada modo. El icono dice «lista o canción» y la marca dice
// «para siempre o una vez».
const REPEAT_LOOK = {
  list: { icon: 'repeat', mark: '∞', on: true, title: 'Repetir la lista (infinito)' },
  one: { icon: 'repeatOne', mark: '∞', on: true, title: 'Repetir esta canción (infinito)' },
  once: { icon: 'repeatOne', mark: '1', on: true, title: 'Solo esta canción: al acabar, se para' },
  queue: { icon: 'repeat', mark: '1', on: false, title: 'La lista una vez y para' }
}
const repeatLook = computed(() => REPEAT_LOOK[repeat.value] || REPEAT_LOOK.list)

const muted = ref(false)
const showQueue = ref(false)
const fullQueue = ref(false)
const queuePanel = ref(null)
const queueButton = ref(null)
const currentRow = ref(null)
const SPEEDS = [0.5, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 2]

const failure = computed(() =>
  error.value || (hasOutput.value ? '' : 'Este equipo no tiene salida de audio')
)
const originLabel = computed(() => origin.value?.label || '')

// Solo tres: la anterior, la que suena y la siguiente. Para ver el resto,
// «Ver todo» lleva a la lista desde la que se puso a sonar.
const trio = computed(() => {
  const list = queue.value || []
  const i = list.findIndex((x) => x.id === track.value?.id)
  if (i < 0) return []
  return [
    { pos: 'before', song: list[i - 1] || null },
    { pos: 'now', song: list[i] },
    { pos: 'after', song: list[i + 1] || null }
  ]
})
const LABELS = { before: 'Sonaba antes', now: 'Sonando ahora', after: 'A continuación' }

onClickOutside([queuePanel, queueButton], () => {
  showQueue.value = false
})

function toggleQueue() {
  showQueue.value = !showQueue.value
  if (!showQueue.value) fullQueue.value = false
}
async function expandQueue() {
  fullQueue.value = !fullQueue.value
  if (fullQueue.value) {
    await nextTick()
    const el = currentRow.value
    if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'center' })
  }
}

// -------------------------------------------------------------- controles
async function togglePlay() {
  // Sin nada cargado, play reproduce lo que esté seleccionado en la lista:
  // tener que dar doble clic a la fila para empezar no es evidente.
  if (!track.value) return emit('playSelected')
  await player.toggle()
}
function seekTo(e) {
  const r = e.currentTarget.getBoundingClientRect()
  player.seek(((e.clientX - r.left) / r.width) * (duration.value || 0))
}
function applyVolume(value) {
  muted.value = false
  player.setVolume(value)
}
function bumpVolume(delta) {
  applyVolume(Math.min(1, Math.max(0, +(volume.value + delta).toFixed(2))))
}
let volumeBeforeMute = 0.9
function toggleMute() {
  if (muted.value || !volume.value) {
    muted.value = false
    player.setVolume(volumeBeforeMute || 0.9)
  } else {
    volumeBeforeMute = volume.value
    muted.value = true
    player.setVolume(0)
  }
}
function goToOrigin() {
  emit('goToOrigin')
  showQueue.value = false
}
function cycleSpeed() {
  const i = SPEEDS.indexOf(speed.value)
  player.setSpeed(SPEEDS[(i + 1) % SPEEDS.length])
}

// Atajos de teclado. `useHotkeys` los ignora cuando el foco está en un campo,
// en un botón o hay un diálogo abierto: antes el espacio con un botón
// enfocado pausaba la música en vez de pulsar el botón, y las flechas dentro
// de un desplegable movían el volumen.
useHotkeys({
  ' ': togglePlay,
  ArrowRight: (e) => player.nudge(e.shiftKey ? 30 : 10),
  ArrowLeft: (e) => player.nudge(e.shiftKey ? -30 : -10),
  ArrowUp: () => bumpVolume(0.05),
  ArrowDown: () => bumpVolume(-0.05),
  m: toggleMute,
  s: () => player.toggleShuffle(),
  r: () => player.cycleRepeat(),
  n: () => player.next(),
  p: () => player.previous()
})
</script>

<template>
  <transition name="dropdown">
    <div v-if="showQueue" ref="queuePanel" class="queue" :class="{ full: fullQueue }">
      <h4>
        Cola
        <span style="margin-left: auto; color: var(--muted2); text-transform: none; letter-spacing: 0">
          {{ (queue || []).length }} en total</span
        >
        <button title="Cerrar la cola" @click="showQueue = false"><Icon n="close" :t="14" /></button>
      </h4>

      <div v-if="!trio.length && !(queue || []).length" class="queue-empty">Nada sonando</div>

      <!-- toda la cola -->
      <template v-else-if="fullQueue">
        <div
          v-for="(song, i) in queue"
          :key="song.id"
          :ref="(el) => { if (el && track?.id === song.id) currentRow = el }"
          class="queue-row flat"
          :class="{ current: track?.id === song.id }"
          @click="player.jump(song.id)"
        >
          <span class="queue-n">
            <Icon v-if="track?.id === song.id" n="play" :t="11" /><template v-else>{{
              i + 1
            }}</template>
          </span>
          <span class="queue-title"
            >{{ song.title }}<span class="sub"> · {{ song.artist || '—' }}</span></span
          >
          <span class="mono sub queue-dur">{{ formatTime(song.duration) }}</span>
        </div>
      </template>

      <!-- solo anterior, actual y siguiente -->
      <template v-else>
        <div
          v-for="t in trio"
          :key="t.pos"
          class="queue-row"
          :class="[t.pos, { act: t.pos === 'now' }]"
          @click="t.song && player.jump(t.song.id)"
        >
          <span class="queue-label">{{ LABELS[t.pos] }}</span>
          <template v-if="t.song">
            <span class="queue-n">
              <Icon v-if="t.pos === 'now'" n="play" :t="11" />
              <Icon v-else-if="t.pos === 'before'" n="previous" :t="11" />
              <Icon v-else n="next" :t="11" />
            </span>
            <span class="queue-title">
              {{ t.song.title }}<span class="sub"> · {{ t.song.artist || '—' }}</span></span
            >
            <span class="mono sub queue-dur">{{
              t.song.duration ? formatTime(t.song.duration) : ''
            }}</span>
          </template>
          <span v-else class="queue-none">—</span>
        </div>
      </template>

      <div class="queue-foot">
        <button class="queue-more" @click="expandQueue">
          <Icon :n="fullQueue ? 'viewCompact' : 'queue'" :t="14" />
          {{ fullQueue ? 'Ver solo 3' : 'Ver la cola entera (' + (queue || []).length + ')' }}
        </button>
        <button class="queue-more" @click="goToOrigin">
          <Icon n="right" :t="14" />
          Ir a{{ originLabel ? ' ' + originLabel : ' la lista' }}
        </button>
      </div>
    </div>
  </transition>

  <div class="player">
    <CoverArt
      :id="track?.id"
      :blur="!!track?.blur"
      class="pl-cover"
      :icon-size="20"
      :alt="track?.title || ''"
    />

    <div class="pl-info">
      <div class="title" style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap">
        {{ track?.title || 'Nada sonando' }}
      </div>
      <div class="sub" style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap">
        <span v-if="failure" style="color: var(--red)">{{ failure }}</span>
        <template v-else>
          {{ track?.artist || '—' }}
          <span v-if="track?.key" class="mono"> · {{ track.key }}</span>
          <span v-if="track?.bpm" class="mono"> · {{ Math.round(track.bpm) }} bpm</span>
        </template>
      </div>
    </div>

    <div class="pl-controls">
      <button
        class="pl-btn"
        :class="{ on: shuffle }"
        title="Aleatorio (S)"
        @click="player.toggleShuffle()"
      >
        <Icon n="shuffle" :t="16" />
      </button>
      <button class="pl-btn" title="Anterior (P)" @click="player.previous()">
        <Icon n="previous" :t="16" />
      </button>
      <button class="pl-btn" title="Retroceder 10 s (←)" @click="player.nudge(-10)">
        <Icon n="back10" :t="15" />
      </button>
      <button class="pl-btn pl-play" title="Reproducir / pausar (espacio)" @click="togglePlay">
        <Icon :n="playing ? 'pause' : 'play'" :t="16" />
      </button>
      <button class="pl-btn" title="Avanzar 10 s (→)" @click="player.nudge(10)">
        <Icon n="forward10" :t="15" />
      </button>
      <button class="pl-btn" title="Siguiente (N)" @click="player.next()">
        <Icon n="next" :t="16" />
      </button>
      <button
        class="pl-btn repeat-btn"
        :class="{ on: repeatLook.on }"
        :title="repeatLook.title + '  (R)'"
        @click="player.cycleRepeat()"
      >
        <Icon :n="repeatLook.icon" :t="16" />
        <span class="repeat-mark">{{ repeatLook.mark }}</span>
      </button>
    </div>

    <div class="pl-bar">
      <span class="time">{{ formatTime(position) }}</span>
      <div class="track" @click="seekTo">
        <div
          class="track-fill"
          :style="{ width: duration ? (position / duration) * 100 + '%' : '0%' }"
        ></div>
      </div>
      <span class="time">{{ formatTime(duration) }}</span>
    </div>

    <button class="speed pl-speed" title="Velocidad de reproducción" @click="cycleSpeed">
      {{ speed }}×
    </button>

    <div class="pl-volume">
      <button
        class="pl-btn"
        style="width: 24px; height: 24px; font-size: 12px"
        :title="muted ? 'Quitar silencio (M)' : 'Silenciar (M)'"
        @click="toggleMute"
      >
        <Icon :n="muted || !volume ? 'mute' : 'volume'" :t="15" />
      </button>
      <SliderField
        :model-value="volume"
        :min="0"
        :max="1"
        :step="0.01"
        width="100%"
        @update:model-value="applyVolume"
      />
    </div>

    <button
      ref="queueButton"
      class="pl-btn"
      :class="{ on: showQueue }"
      title="Cola de reproducción"
      @click="toggleQueue"
    >
      <Icon n="queue" :t="16" />
    </button>
  </div>
</template>
