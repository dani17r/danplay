<script setup>
/**
 * El reproductor del celular.
 *
 * Abajo, una barra baja con lo justo (la carátula, el título, sonar y la
 * siguiente) que deja sitio a la lista. Deslizándola hacia arriba se abre el
 * modo estudio (lo más usado al ensayar); con un toque, el reproductor
 * entero, a pantalla completa y con todo grande: la carátula, la barra de
 * tiempo para el dedo, los botones de siempre y, debajo, el estudio, la
 * letra y la cola. Se cierra con la flecha, con «atrás» o deslizándolo hacia
 * abajo.
 *
 * Con el modo estudio abierto la barra de abajo cambia: lleva la hora, la
 * barra de tiempo y los saltos de 10 segundos, que es lo que se usa al
 * ensayar con el teléfono en el atril.
 *
 * Como el de la PC (Player.vue), no tiene estado propio: todo sale de
 * `usePlayback`, que escucha a Rust.
 */
import { ref, computed, watch } from 'vue'
import { usePlayback, MAX_VOLUME } from '../composables/usePlayback.js'
import { useScrub } from '../composables/useScrub.js'
import { openMenu } from '../composables/useContextMenu.js'
import { formatTime } from '../utils/format.js'
import Icon from '../components/Icon.vue'
import CoverArt from '../components/ui/CoverArt.vue'
import SliderField from '../components/ui/SliderField.vue'

const props = defineProps({
  study: Boolean,
  /** el estudio sigue puesto aunque no se vea */
  studyActive: Boolean,
  /** el reproductor entero, abierto */
  full: Boolean,
  /** la canción que suena, como está en la lista (favorita, tono, bpm) */
  song: { type: Object, default: null }
})
const emit = defineEmits(['update:full', 'toggleStudy', 'lyrics', 'favorite', 'goToOrigin'])

const player = usePlayback()
const { track, queue, playing, position, duration, speed, repeat, shuffle, volume, origin, loops } =
  player

const REPEAT_LOOK = {
  list: { icon: 'repeat', mark: '∞', on: true, title: 'Repetir la lista' },
  one: { icon: 'repeatOne', mark: '∞', on: true, title: 'Repetir esta canción' },
  once: { icon: 'repeatOne', mark: '1', on: true, title: 'Solo esta canción y para' },
  queue: { icon: 'repeat', mark: '1', on: false, title: 'La lista una vez y para' }
}
const repeatLook = computed(() => REPEAT_LOOK[repeat.value] || REPEAT_LOOK.list)
const SPEEDS = [0.5, 0.75, 0.9, 1, 1.1, 1.25, 1.5]
const VOLUMES = [0.25, 0.5, 0.75, 1, 1.25, MAX_VOLUME]

const { scrub, start: grab } = useScrub({ duration, seek: (s) => player.seek(s) })
const shown = computed(() => (scrub.active ? scrub.value : position.value))
const pct = computed(() =>
  duration.value ? Math.min(100, (shown.value / duration.value) * 100) + '%' : '0%'
)
const segments = computed(() => (duration.value ? loops.value : []))
const originLabel = computed(() => origin.value?.label || 'Mis pistas')
const info = computed(() => ({ ...(track.value || {}), ...(props.song || {}) }))
const showQueue = ref(false)

function close() {
  showQueue.value = false
  emit('update:full', false)
}
function open() {
  if (track.value) emit('update:full', true)
}

// ---------------------------------- deslizar la barra hacia arriba: estudio
const LIFT_OPENS = 44
const lift = ref(0)
let gesture = null
let swallowClick = false
function liftDown(e) {
  if (!track.value) return
  gesture = { x: e.clientX, y: e.clientY, t: Date.now(), vertical: false }
}
function liftMove(e) {
  if (!gesture) return
  const dx = e.clientX - gesture.x
  const dy = e.clientY - gesture.y
  if (!gesture.vertical && Math.abs(dy) > 8 && Math.abs(dy) > Math.abs(dx)) {
    gesture.vertical = true
    e.currentTarget.setPointerCapture?.(e.pointerId)
  }
  if (gesture.vertical) lift.value = Math.max(-90, Math.min(0, dy))
}
function liftUp() {
  if (!gesture) return
  const { vertical, t } = gesture
  const went = -lift.value
  gesture = null
  lift.value = 0
  if (!vertical) return
  // lo que fue un arrastre no cuenta además como toque
  swallowClick = true
  setTimeout(() => (swallowClick = false), 400)
  const fast = went / Math.max(1, Date.now() - t) > 0.5
  if (went > LIFT_OPENS || (fast && went > 20)) {
    navigator.vibrate?.(10)
    emit('toggleStudy')
  }
}
function tapBar() {
  if (swallowClick) return
  open()
}
const liftStyle = computed(() =>
  lift.value ? { transform: `translateY(${lift.value * 0.35}px)`, transition: 'none' } : {}
)
watch(track, (t) => {
  if (!t) close()
})

// ------------------------------------------------ deslizar hacia abajo
const dragY = ref(0)
const dragging = ref(false)
let y0 = 0
let t0 = 0
function sheetDown(e) {
  dragging.value = true
  y0 = e.clientY
  t0 = Date.now()
}
function sheetMove(e) {
  if (dragging.value) dragY.value = Math.max(0, e.clientY - y0)
}
function sheetUp() {
  if (!dragging.value) return
  dragging.value = false
  const fast = dragY.value / Math.max(1, Date.now() - t0) > 0.6
  if (dragY.value > 130 || (fast && dragY.value > 40)) close()
  dragY.value = 0
}
const sheetStyle = computed(() =>
  dragY.value
    ? { transform: `translateY(${dragY.value}px)`, transition: dragging.value ? 'none' : '' }
    : {}
)

// ------------------------------------------------------------- el volumen
// A mano, siempre a la vista en el reproductor entero (antes solo en un
// menú): bajar la app un poco sin tocar el volumen del teléfono, o subirla
// de lo que viene cuando la pista está baja.
const vol = computed({
  get: () => volume.value,
  set: (v) => player.setVolume(Math.round(v * 100) / 100)
})
let beforeMute = 1
function toggleMute() {
  if (volume.value > 0) {
    beforeMute = volume.value
    player.setVolume(0)
  } else player.setVolume(beforeMute || 1)
}

// ------------------------------------------------------------- menús
const pctText = (v) => Math.round(v * 100) + ' %'
function more(e) {
  openMenu(
    e,
    [
      {
        label: 'Velocidad',
        icon: 'forward10',
        note: speed.value + '×',
        children: SPEEDS.map((s) => ({
          label: s + '×',
          icon: s === speed.value ? 'check' : undefined,
          note: s === 1 ? 'normal' : undefined,
          action: () => player.setSpeed(s)
        }))
      },
      {
        label: 'Volumen de la app',
        icon: volume.value ? 'volume' : 'mute',
        note: pctText(volume.value),
        children: VOLUMES.map((v) => ({
          label: pctText(v),
          icon: Math.abs(v - volume.value) < 0.005 ? 'check' : undefined,
          note: v > 1 ? 'más alto de como viene' : undefined,
          action: () => player.setVolume(v)
        }))
      },
      { label: 'Desde el principio', icon: 'restart', action: () => player.restart() },
      { separator: true },
      { label: 'Cola de reproducción', icon: 'queue', action: () => (showQueue.value = true) },
      {
        label: 'Ir a ' + originLabel.value,
        icon: 'right',
        action: () => {
          close()
          emit('goToOrigin')
        }
      }
    ],
    track.value?.title || 'Reproductor'
  )
}
function studyNow() {
  close()
  emit('toggleStudy')
}
function lyricsNow() {
  close()
  emit('lyrics')
}
</script>

<template>
  <!-- ===================================================== la barra baja -->
  <div
    class="m-player"
    :class="{ 'm-player-study': study, 'm-idle': !track }"
    role="region"
    aria-label="Reproductor"
  >
    <template v-if="!study">
      <div class="m-mini-progress" aria-hidden="true">
        <span :style="{ width: pct }"></span>
      </div>
      <!-- el asa dice que la barra se puede subir -->
      <span v-if="track" class="m-mini-handle" aria-hidden="true"></span>
      <div
        v-if="track"
        class="m-lift-hint"
        :class="{ ready: -lift > LIFT_OPENS }"
        :style="{ opacity: Math.min(1, -lift / LIFT_OPENS) }"
        aria-hidden="true"
      >
        <Icon n="academic" :t="16" />
        {{ -lift > LIFT_OPENS ? 'Suelta para abrir el estudio' : 'Sube para el modo estudio' }}
      </div>
      <div class="m-mini" :style="liftStyle">
        <button
          type="button"
          class="m-mini-open"
          :disabled="!track"
          :aria-label="
            track
              ? 'Abrir el reproductor: ' + track.title + '. Desliza hacia arriba para el estudio'
              : 'Nada sonando'
          "
          @click="tapBar"
          @pointerdown="liftDown"
          @pointermove="liftMove"
          @pointerup="liftUp"
          @pointercancel="liftUp"
        >
          <CoverArt :id="track?.id" :size="96" :icon-size="20" class="m-mini-cover" alt="" />
          <span class="m-mini-text">
            <span class="m-mini-title">{{ track?.title || 'Nada sonando' }}</span>
            <span class="m-mini-sub">{{
              track ? track.artist || '—' : 'Doble toque en una canción para ponerla'
            }}</span>
          </span>
        </button>
        <button
          type="button"
          class="m-round m-play-mini"
          :disabled="!track"
          :aria-label="playing ? 'Pausar' : 'Reproducir'"
          @click="player.toggle()"
        >
          <Icon :n="playing ? 'pause' : 'play'" :t="22" />
        </button>
        <button
          type="button"
          class="m-icon"
          :disabled="!track"
          aria-label="Siguiente"
          @click="player.next()"
        >
          <Icon n="next" :t="22" />
        </button>
      </div>
    </template>

    <!-- con el estudio abierto: la hora, la barra y los saltos -->
    <template v-else>
      <div class="m-seek-row">
        <span class="m-time">{{ formatTime(shown) }}</span>
        <div
          class="m-seek"
          :class="{ active: scrub.active }"
          role="slider"
          aria-label="Posición"
          :aria-valuenow="Math.round(shown)"
          aria-valuemin="0"
          :aria-valuemax="Math.round(duration)"
          tabindex="0"
          @pointerdown="grab"
        >
          <div class="m-seek-track">
            <div
              v-for="[a, b] in segments"
              :key="a + ':' + b"
              class="m-seek-loop"
              :style="{ left: (a / duration) * 100 + '%', width: ((b - a) / duration) * 100 + '%' }"
            ></div>
            <div class="m-seek-fill" :style="{ width: pct }"></div>
          </div>
          <div class="m-seek-thumb" :style="{ left: pct }"></div>
        </div>
        <span class="m-time">{{ formatTime(duration) }}</span>
      </div>
      <div class="m-transport m-transport-study">
        <button
          type="button"
          class="m-icon"
          aria-label="Desde el principio"
          @click="player.restart()"
        >
          <Icon n="restart" :t="20" />
        </button>
        <button
          type="button"
          class="m-icon m-skip"
          aria-label="Retroceder 10 segundos"
          @click="player.nudge(-10)"
        >
          <Icon n="back10" :t="22" /><span>10</span>
        </button>
        <button
          type="button"
          class="m-round m-play"
          :aria-label="playing ? 'Pausar' : 'Reproducir'"
          @click="player.toggle()"
        >
          <Icon :n="playing ? 'pause' : 'play'" :t="26" />
        </button>
        <button
          type="button"
          class="m-icon m-skip"
          aria-label="Avanzar 10 segundos"
          @click="player.nudge(10)"
        >
          <Icon n="forward10" :t="22" /><span>10</span>
        </button>
        <button type="button" class="m-icon" aria-label="Siguiente" @click="player.next()">
          <Icon n="next" :t="20" />
        </button>
      </div>
    </template>
  </div>

  <!-- ============================================ el reproductor entero -->
  <transition name="m-sheet">
    <div
      v-if="full && track"
      class="m-full"
      :style="sheetStyle"
      role="dialog"
      aria-modal="true"
      aria-label="Reproductor"
    >
      <!-- la cabecera y la carátula se arrastran hacia abajo para cerrar -->
      <div
        class="m-full-grab"
        @pointerdown="sheetDown"
        @pointermove="sheetMove"
        @pointerup="sheetUp"
        @pointercancel="sheetUp"
      >
        <div class="m-full-head">
          <button type="button" class="m-icon" aria-label="Cerrar el reproductor" @click="close">
            <Icon n="down" :t="26" />
          </button>
          <div class="m-full-from">
            <span>Sonando desde</span>
            <strong>{{ originLabel }}</strong>
          </div>
          <button type="button" class="m-icon" aria-label="Más opciones" @click="more">
            <Icon n="moreVertical" :t="22" />
          </button>
        </div>
        <div class="m-full-art">
          <CoverArt :id="track.id" :size="320" :icon-size="64" :alt="track.title" />
        </div>
      </div>

      <div class="m-full-body">
        <div class="m-full-titles">
          <div class="m-full-text">
            <div class="m-full-title">{{ track.title }}</div>
            <div class="m-full-artist">{{ track.artist || '—' }}</div>
          </div>
          <button
            type="button"
            class="m-icon m-heart"
            :class="{ on: info.favorite }"
            :aria-pressed="!!info.favorite"
            :aria-label="info.favorite ? 'Quitar de favoritas' : 'Marcar como favorita'"
            @click="emit('favorite', info)"
          >
            <Icon :n="info.favorite ? 'heartFull' : 'heart'" :t="24" />
          </button>
        </div>
        <div v-if="info.key || info.bpm || speed !== 1" class="m-full-chips">
          <span v-if="info.key" class="m-tag">Tono {{ info.key }}</span>
          <span v-if="info.bpm" class="m-tag">{{ Math.round(info.bpm) }} bpm</span>
          <span v-if="speed !== 1" class="m-tag m-tag-on">{{ speed }}×</span>
        </div>

        <div
          class="m-seek m-seek-big"
          :class="{ active: scrub.active }"
          role="slider"
          aria-label="Posición"
          :aria-valuenow="Math.round(shown)"
          aria-valuemin="0"
          :aria-valuemax="Math.round(duration)"
          tabindex="0"
          @pointerdown="grab"
        >
          <div class="m-seek-track">
            <div
              v-for="[a, b] in segments"
              :key="a + ':' + b"
              class="m-seek-loop"
              :style="{ left: (a / duration) * 100 + '%', width: ((b - a) / duration) * 100 + '%' }"
            ></div>
            <div class="m-seek-fill" :style="{ width: pct }"></div>
          </div>
          <div class="m-seek-thumb" :style="{ left: pct }"></div>
        </div>
        <div class="m-full-times">
          <span>{{ formatTime(shown) }}</span
          ><span>{{ formatTime(duration) }}</span>
        </div>

        <div class="m-transport">
          <button
            type="button"
            class="m-icon m-skip"
            aria-label="Retroceder 10 segundos"
            @click="player.nudge(-10)"
          >
            <Icon n="back10" :t="24" /><span>10</span>
          </button>
          <button
            type="button"
            class="m-icon m-big"
            aria-label="Anterior"
            @click="player.previous()"
          >
            <Icon n="previous" :t="30" />
          </button>
          <button
            type="button"
            class="m-round m-play m-play-big"
            :aria-label="playing ? 'Pausar' : 'Reproducir'"
            @click="player.toggle()"
          >
            <Icon :n="playing ? 'pause' : 'play'" :t="34" />
          </button>
          <button type="button" class="m-icon m-big" aria-label="Siguiente" @click="player.next()">
            <Icon n="next" :t="30" />
          </button>
          <button
            type="button"
            class="m-icon m-skip"
            aria-label="Avanzar 10 segundos"
            @click="player.nudge(10)"
          >
            <Icon n="forward10" :t="24" /><span>10</span>
          </button>
        </div>

        <div class="m-volume">
          <button
            type="button"
            class="m-icon"
            :aria-label="volume > 0 ? 'Silenciar la app' : 'Volver a oírla'"
            @click="toggleMute"
          >
            <Icon :n="volume > 0 ? 'volume' : 'mute'" :t="22" />
          </button>
          <SliderField
            v-model="vol"
            class="m-volume-slider"
            :min="0"
            :max="MAX_VOLUME"
            :step="0.01"
            :mark="1"
            aria-label="Volumen de la app"
            :value-text="pctText(volume)"
          />
          <span class="m-volume-pct mono">{{ pctText(volume) }}</span>
        </div>

        <div class="m-modes">
          <button
            type="button"
            class="m-mode"
            :class="{ on: shuffle }"
            :aria-pressed="shuffle"
            @click="player.toggleShuffle()"
          >
            <Icon n="shuffle" :t="18" /> Aleatorio
          </button>
          <button
            type="button"
            class="m-mode"
            :class="{ on: repeatLook.on }"
            :aria-label="repeatLook.title"
            @click="player.cycleRepeat()"
          >
            <Icon :n="repeatLook.icon" :t="18" /> {{ repeatLook.title }}
          </button>
        </div>

        <div class="m-tiles">
          <button
            type="button"
            class="m-tile"
            :class="{ on: study, active: studyActive && !study }"
            @click="studyNow"
          >
            <Icon n="academic" :t="22" /><span>Estudio</span>
          </button>
          <button type="button" class="m-tile" @click="lyricsNow">
            <Icon n="lyrics" :t="22" /><span>Letra y acordes</span>
          </button>
          <button type="button" class="m-tile" @click="showQueue = true">
            <Icon n="queue" :t="22" /><span>Cola</span>
          </button>
        </div>
      </div>

      <!-- la cola, encima -->
      <transition name="m-sheet">
        <div v-if="showQueue" class="m-queue" role="region" aria-label="Cola de reproducción">
          <div class="m-queue-head">
            <strong>Cola</strong>
            <span>{{ queue.length }} canciones · {{ originLabel }}</span>
            <button
              type="button"
              class="m-icon"
              aria-label="Cerrar la cola"
              @click="showQueue = false"
            >
              <Icon n="close" :t="22" />
            </button>
          </div>
          <ol class="m-queue-list">
            <li v-for="(s, i) in queue" :key="s.id">
              <button
                type="button"
                class="m-queue-row"
                :class="{ now: track?.id === s.id }"
                :aria-current="track?.id === s.id ? 'true' : undefined"
                @click="player.jump(s.id)"
              >
                <span class="m-queue-n">
                  <span v-if="track?.id === s.id && playing" class="m-eq"
                    ><i></i><i></i><i></i
                  ></span>
                  <template v-else>{{ i + 1 }}</template>
                </span>
                <span class="m-queue-text">
                  <span class="m-queue-title">{{ s.title }}</span>
                  <span class="m-queue-sub">{{ s.artist || '—' }}</span>
                </span>
                <span class="m-queue-dur">{{ s.duration ? formatTime(s.duration) : '' }}</span>
              </button>
            </li>
          </ol>
        </div>
      </transition>
    </div>
  </transition>
</template>
