<script setup>
/**
 * Una guía: lo que suena en un punto de la canción («Coro», «Puente, cuatro
 * tiempos», una cuenta, un aviso grabado), como la pista «Guide» de una
 * secuencia. Se escribe y la dice una voz que se elige (la hace el núcleo,
 * en el equipo, sin internet salvo la primera vez que se baja esa voz), se
 * graba con el micrófono o se importa un audio corto del equipo (que no dure
 * más que la canción). Lo grabado o importado se ve con su onda y se arregla
 * aquí mismo, como en un editor: recortar los bordes, fundidos de entrada y
 * de salida, quitar el ruido de fondo y los graves, igualar el volumen (en un
 * archivo nuevo: el de antes no se toca). Suena en el motor justo ahí,
 * también a otra velocidad y en la vuelta de un tramo, por la salida de las
 * guías.
 */
import { ref, reactive, computed, watch, onMounted, onUnmounted } from 'vue'
import { api, app as tauriApp, errorMessage, pickAudio } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { usePlayback } from '../composables/usePlayback.js'
import { formatTime } from '../utils/format.js'
import { nearestBeat, stepBeat } from '../utils/beats.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'
import SliderField from './ui/SliderField.vue'

const props = defineProps({
  /**
   * {t, text, voice, cue, file?, seconds?}: `cue` es la guía que se cambia
   * (o null: nueva); `file`, un audio ya importado para arreglarlo y ponerlo.
   */
  draft: { type: Object, required: true },
  songId: { type: Number, default: null },
  /** Lo que dura la canción: un audio importado no puede durar más. */
  songLength: { type: Number, default: 0 },
  /** con el imán, los pulsos del clic: «antes» y «después» van de pulso en pulso */
  beats: { type: Array, default: () => [] }
})
const emit = defineEmits(['made', 'remove', 'close'])
const player = usePlayback()

const ownAudio =
  props.draft.file || (props.draft.cue && !props.draft.cue.text && props.draft.cue.file)
const at = ref(props.draft.t || 0)
const text = ref(props.draft.text || '')
const voice = ref(props.draft.voice || '')
const tab = ref(ownAudio ? 'audio' : 'write')
const voices = ref([])
const busy = ref(false)
const recording = ref(false)
const level = ref(0)
/** Lo que lleva grabado (segundos), para verlo mientras se habla. */
const recSeconds = ref(0)
let levelTimer = null

const SUGGESTIONS = [
  'Intro',
  'Verso',
  'Coro',
  'Puente',
  'Final',
  'Todos',
  'Solo voces',
  '4 tiempos',
  'Sube medio tono',
  'Baja la banda'
]
const HELP =
  'Suena justo en ese punto, por la salida de las guías (en «Salidas» puedes mandarla a tus cascos). ' +
  'Ponla un poco antes de donde entra la parte, como un director.'

/** Por qué no se pueden decir guías aquí (falta algo en la instalación). */
const unavailable = ref('')
let pollTimer = null
async function loadVoices() {
  try {
    const r = await api.cueVoices()
    voices.value = r?.voices || []
    unavailable.value = r?.available || ''
  } catch {
    voices.value = []
  }
  // mientras se baje alguna, se pregunta cada segundo por dónde va
  clearTimeout(pollTimer)
  if (voices.value.some((v) => v.progress != null && !v.installed && !v.error))
    pollTimer = setTimeout(loadVoices, 1000)
}
onMounted(async () => {
  await loadVoices()
  if (!voices.value.some((v) => v.id === voice.value))
    voice.value = voices.value.find((v) => v.installed)?.id || voices.value[0]?.id || ''
})
/** La voz elegida (con si está bajada y por dónde va). */
const chosen = computed(() => voices.value.find((v) => v.id === voice.value) || null)
const downloading = computed(
  () =>
    !!chosen.value &&
    !chosen.value.installed &&
    chosen.value.progress != null &&
    !chosen.value.error
)
const voiceReady = computed(() => !!chosen.value?.installed)
async function download() {
  if (!chosen.value) return
  try {
    await api.installCueVoice(chosen.value.id)
  } catch (e) {
    notify('No se pudo bajar la voz: ' + errorMessage(e))
  }
  await loadVoices()
}
const mb = (bytes) => Math.max(1, Math.round((bytes || 0) / 1e6))
function onKey(e) {
  if (e.key === 'Escape') {
    e.stopPropagation()
    emit('close')
  }
}
onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  clearInterval(levelTimer)
  clearTimeout(pollTimer)
  stopListening()
  if (recording.value) tauriApp.recordStop().catch(() => {})
})

const voiceOptions = computed(() =>
  voices.value.map((v) => ({
    v: v.id,
    n: v.name,
    note: [
      v.engine === 'supertonic' ? 'natural' : v.lang,
      v.installed ? 'lista' : v.progress != null ? 'bajándose…' : `${mb(v.size)} MB`
    ].join(' · ')
  }))
)
const canRecord = computed(() => typeof tauriApp.recordStart === 'function' && tauriApp.canRecord)

const round2 = (v) => Math.round(v * 100) / 100
/** Antes o después: medio segundo, o con el imán, al pulso de al lado. */
function nudge(delta) {
  const beat = props.beats.length ? stepBeat(props.beats, at.value, delta < 0 ? -1 : 1) : null
  at.value = Math.max(0, round2(beat ?? at.value + delta))
}
function here() {
  at.value = round2(nearestBeat(props.beats, player.position.value))
}
const stepTitle = (dir) =>
  props.beats.length
    ? dir < 0
      ? 'Al pulso de antes (imán puesto)'
      : 'Al pulso de después (imán puesto)'
    : dir < 0
      ? 'Medio segundo antes'
      : 'Medio segundo después'

// ---- escuchar lo que se va a guardar (sin guardarlo)
const listening = ref(false)
/** Por dónde va lo que se escucha, en la onda (segundos), o null. */
const headAt = ref(null)
let headFrame = 0
let listenTurn = 0
function stopListening() {
  if (!listening.value) return
  listening.value = false
  headAt.value = null
  cancelAnimationFrame(headFrame)
  api.stopPreview?.()?.catch?.(() => {})
}
/** La aguja recorre lo que suena (de `from` a `to`, en `secs`). */
function runHead(from, to, secs) {
  const turn = ++listenTurn
  const t0 = performance.now()
  const step = () => {
    if (turn !== listenTurn || !listening.value) return
    const k = (performance.now() - t0) / 1000 / Math.max(0.05, secs)
    if (k >= 1) {
      listening.value = false
      headAt.value = null
      return
    }
    headAt.value = from + (to - from) * k
    headFrame = requestAnimationFrame(step)
  }
  headFrame = requestAnimationFrame(step)
}
// ---- lo grabado o importado: su onda y lo que se le hace
/** El audio de la guía ({file, seconds}): lo grabado, lo importado o el que ya tenía. */
const clip = ref(ownAudio ? { file: ownAudio, seconds: props.draft.seconds || 0 } : null)
/** Su onda (de 0 a 1), para verla y recortarla. */
const peaks = ref([])
const fx = reactive({
  start: 0,
  end: 0,
  fadeIn: 0,
  fadeOut: 0,
  denoise: false,
  highpass: false,
  normalize: false
})
const clipLength = computed(() => clip.value?.seconds || 0)
/** Lo que queda (de `start` a `end`). */
const keptLength = computed(() => Math.max(0, fx.end - fx.start))
const fadeMax = computed(() => Math.max(0.05, Math.min(3, keptLength.value / 2)))
/** Si se le ha cambiado algo (si no, se usa tal cual). */
const changed = computed(
  () =>
    fx.start > 0.005 ||
    fx.end < clipLength.value - 0.005 ||
    fx.fadeIn > 0 ||
    fx.fadeOut > 0 ||
    fx.denoise ||
    fx.highpass ||
    fx.normalize
)
function resetFx() {
  Object.assign(fx, {
    start: 0,
    end: clipLength.value,
    fadeIn: 0,
    fadeOut: 0,
    denoise: false,
    highpass: false,
    normalize: false
  })
}
async function loadWave() {
  const file = clip.value?.file
  peaks.value = []
  if (!file) return
  try {
    const w = await api.cueWave(file, 240)
    if (clip.value?.file !== file) return
    peaks.value = w?.peaks || []
    if (w?.seconds) clip.value = { ...clip.value, seconds: w.seconds }
  } catch {
    peaks.value = []
  }
  resetFx()
}
watch(
  () => clip.value?.file,
  () => {
    stopListening()
    loadWave()
  },
  { immediate: true }
)
watch(
  () => [fx.fadeIn, fx.fadeOut, keptLength.value],
  () => {
    fx.fadeIn = Math.min(fx.fadeIn, fadeMax.value)
    fx.fadeOut = Math.min(fx.fadeOut, fadeMax.value)
  }
)
const pct = (t) => (clipLength.value ? (t / clipLength.value) * 100 : 0)
/** La onda, como una figura (arriba y abajo de la línea del medio). */
const wavePath = computed(() => {
  const p = peaks.value
  if (!p.length) return ''
  const n = p.length
  const x = (i) => ((i + 0.5) / n) * 1000
  const top = p.map((v, i) => `${x(i).toFixed(1)},${(50 - Math.max(0.6, v * 46)).toFixed(1)}`)
  const bottom = p
    .map((v, i) => `${x(i).toFixed(1)},${(50 + Math.max(0.6, v * 46)).toFixed(1)}`)
    .reverse()
  return `M0,50 L${top.join(' L')} L1000,50 L${bottom.join(' L')} Z`
})
/** El volumen que queda (los fundidos): una línea como en un editor. */
const envelope = computed(() => {
  if (!clipLength.value) return ''
  const sx = (t) => (pct(t) * 10).toFixed(1)
  return (
    `M${sx(fx.start)},96 L${sx(fx.start + fx.fadeIn)},4 ` +
    `L${sx(fx.end - fx.fadeOut)},4 L${sx(fx.end)},96`
  )
})
const fmt = (s) => (s < 60 ? s.toFixed(2).replace('.', ',') + ' s' : formatTime(s))

/** Arrastrar un borde (o pulsar en la onda: el más cercano va ahí). */
const waveEl = ref(null)
let dragging = null
function timeAt(clientX) {
  const r = waveEl.value?.getBoundingClientRect()
  if (!r || !r.width) return 0
  return Math.max(0, Math.min(1, (clientX - r.left) / r.width)) * clipLength.value
}
function setEdge(edge, t) {
  const gap = Math.min(0.1, clipLength.value / 4)
  if (edge === 'start') fx.start = Math.round(Math.max(0, Math.min(t, fx.end - gap)) * 100) / 100
  else fx.end = Math.round(Math.min(clipLength.value, Math.max(t, fx.start + gap)) * 100) / 100
}
function grab(edge, e) {
  if (!clipLength.value) return
  dragging = edge
  e.currentTarget?.setPointerCapture?.(e.pointerId)
}
function onWaveDown(e) {
  if (!clipLength.value || e.button > 0) return
  const t = timeAt(e.clientX)
  dragging = Math.abs(t - fx.start) <= Math.abs(t - fx.end) ? 'start' : 'end'
  setEdge(dragging, t)
  waveEl.value?.setPointerCapture?.(e.pointerId)
}
function onWaveMove(e) {
  if (dragging) setEdge(dragging, timeAt(e.clientX))
}
function onWaveUp() {
  dragging = null
}
/** Con el teclado: flechas (Mayús, de medio segundo). */
function edgeKey(edge, e) {
  const step = e.shiftKey ? 0.5 : 0.05
  const now = edge === 'start' ? fx.start : fx.end
  if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') setEdge(edge, now - step)
  else if (e.key === 'ArrowRight' || e.key === 'ArrowUp') setEdge(edge, now + step)
  else if (e.key === 'Home') setEdge(edge, 0)
  else if (e.key === 'End') setEdge(edge, clipLength.value)
  else return
  e.preventDefault()
}

/** Lo que se le pide al núcleo para hacerla nueva. */
const editBody = () => ({
  file: clip.value.file,
  start: fx.start,
  end: fx.end,
  fade_in: fx.fadeIn,
  fade_out: fx.fadeOut,
  denoise: fx.denoise,
  highpass: fx.highpass,
  normalize: fx.normalize
})
const FX = [
  {
    k: 'denoise',
    n: 'Quitar ruido',
    icon: 'filter',
    title: 'Quitar el ruido de fondo (ventilador, aire, zumbido) que se oye entre palabras'
  },
  {
    k: 'highpass',
    n: 'Quitar graves',
    icon: 'adjust',
    title:
      'Quitar los graves y los golpes del micrófono (por debajo de 100 Hz): la voz se oye más clara'
  },
  {
    k: 'normalize',
    n: 'Igualar volumen',
    icon: 'volume',
    title: 'Subirla hasta su máximo sin que sature (como las demás guías)'
  }
]

// ---- grabar o importar
const longest = computed(() => Math.min(props.songLength || 60, 300))
async function toggleRecord() {
  try {
    if (!recording.value) {
      stopListening()
      await tauriApp.recordStart()
      recording.value = true
      recSeconds.value = 0
      const t0 = Date.now()
      levelTimer = setInterval(async () => {
        recSeconds.value = (Date.now() - t0) / 1000
        level.value = (await tauriApp.recordLevel().catch(() => 0)) || 0
      }, 80)
    } else {
      clearInterval(levelTimer)
      recording.value = false
      level.value = 0
      const path = await tauriApp.recordStop()
      if (path) await useAudio(path, 'No se pudo usar lo grabado: ')
    }
  } catch (e) {
    recording.value = false
    clearInterval(levelTimer)
    notify('No se pudo grabar: ' + errorMessage(e))
  }
}
/** Un audio del equipo (corto: hasta lo que dura la canción). */
async function importAudio() {
  const path = await pickAudio()
  if (path) await useAudio(path, 'No se pudo importar: ')
}
/** Lo grabado o importado, a la carpeta de las guías (recortado y a su volumen). */
async function useAudio(path, fail) {
  busy.value = true
  try {
    clip.value = await api.importCue(props.songId, path, props.songLength || undefined)
  } catch (e) {
    notify(fail + errorMessage(e))
  } finally {
    busy.value = false
  }
}

async function preview() {
  if (listening.value) return stopListening()
  busy.value = true
  try {
    if (tab.value === 'write') {
      const r = await api.makeCue(props.songId, {
        text: text.value,
        voice: voice.value,
        preview: true
      })
      if (!r?.file) return
      const secs = await api.previewCue(r.file)
      listening.value = true
      runHead(0, 0, typeof secs === 'number' && secs > 0 ? secs : r.seconds || 1)
      return
    }
    if (!clip.value) return
    const file = changed.value
      ? (await api.editCue({ ...editBody(), preview: true }))?.file
      : clip.value.file
    if (!file) return
    const secs = await api.previewCue(file)
    listening.value = true
    runHead(fx.start, fx.end, typeof secs === 'number' && secs > 0 ? secs : keptLength.value)
  } catch (e) {
    notify(errorMessage(e))
  } finally {
    busy.value = false
  }
}
const ready = computed(() =>
  tab.value === 'write' ? !!text.value.trim() && voiceReady.value : !!clip.value && !recording.value
)
async function save() {
  busy.value = true
  try {
    let r
    if (tab.value === 'write') {
      if (!text.value.trim()) return notify('Escribe lo que se dirá', 'info')
      if (!voiceReady.value) return notify('Baja antes la voz', 'info')
      r = await api.makeCue(props.songId, { text: text.value.trim(), voice: voice.value })
    } else {
      if (!clip.value) return notify('Graba o importa antes un audio', 'info')
      stopListening()
      r = changed.value ? await api.editCue(editBody()) : clip.value
    }
    emit('made', {
      t: at.value,
      text: tab.value === 'write' ? text.value.trim() : '',
      voice: tab.value === 'write' ? voice.value : '',
      file: r.file,
      cue: props.draft.cue
    })
  } catch (e) {
    notify('No se pudo hacer la guía: ' + errorMessage(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <!-- fuera de la tarjeta, cierra (con el teclado, Escape: ver onKey) -->
  <!-- eslint-disable-next-line vuejs-accessibility/click-events-have-key-events, vuejs-accessibility/no-static-element-interactions -->
  <div class="cue-veil" @click.self="emit('close')">
    <div
      class="cue-card"
      :class="{ wide: tab === 'audio' && clip }"
      role="dialog"
      aria-label="Guía"
    >
      <header class="cue-head">
        <span class="cue-badge"><Icon :n="tab === 'write' ? 'pencil' : 'waveform'" :t="14" /></span>
        <strong>Guía</strong>
        <span class="cue-at mono" role="group" aria-label="Dónde suena">
          <button
            type="button"
            class="btn mini icon-btn"
            aria-label="Antes"
            :title="stepTitle(-1)"
            @click="nudge(-0.5)"
          >
            <Icon n="left" :t="12" />
          </button>
          <span class="cue-at-time">{{ formatTime(at) }}</span>
          <button
            type="button"
            class="btn mini icon-btn"
            aria-label="Después"
            :title="stepTitle(1)"
            @click="nudge(0.5)"
          >
            <Icon n="right" :t="12" />
          </button>
          <button
            type="button"
            class="btn mini icon-btn"
            aria-label="Aquí"
            title="Donde va la canción"
            @click="here"
          >
            <Icon n="crosshair" :t="13" />
          </button>
        </span>
        <button
          type="button"
          class="btn mini icon-btn cue-help"
          aria-label="Para qué sirve"
          :title="HELP"
        >
          <Icon n="help" :t="14" />
        </button>
        <button
          type="button"
          class="btn mini icon-btn cue-close"
          aria-label="Cerrar"
          title="Cerrar (Esc)"
          @click="emit('close')"
        >
          <Icon n="close" :t="12" />
        </button>
      </header>

      <div class="study-tabs cue-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          class="study-tab"
          :class="{ on: tab === 'write' }"
          :aria-selected="tab === 'write'"
          title="Escribe lo que se dirá y elige la voz que lo dice"
          @click="tab = 'write'"
        >
          <Icon n="pencil" :t="12" /> Escribir
        </button>
        <button
          type="button"
          role="tab"
          class="study-tab"
          :class="{ on: tab === 'audio' }"
          :aria-selected="tab === 'audio'"
          title="Graba tu voz con el micrófono o importa un audio corto del equipo, y arréglalo aquí"
          @click="tab = 'audio'"
        >
          <Icon n="mic" :t="12" /> Grabar o importar
        </button>
      </div>

      <Transition name="cue-swap" mode="out-in">
        <div v-if="tab === 'write'" key="write" class="cue-body">
          <TextField
            v-model="text"
            width="100%"
            placeholder="Coro · Puente, cuatro tiempos · Sube medio tono…"
            aria-label="Lo que se dirá"
            @keydown.enter="save"
          />
          <div class="cue-chips">
            <button v-for="s in SUGGESTIONS" :key="s" type="button" class="chip" @click="text = s">
              {{ s }}
            </button>
          </div>
          <SelectField
            v-if="voiceOptions.length"
            v-model="voice"
            :options="voiceOptions"
            label="La voz"
            width="100%"
          />
          <p v-if="unavailable" class="cue-note warn">
            No se pueden decir guías aquí: {{ unavailable }}
          </p>
          <div v-else-if="chosen && !chosen.installed" class="cue-get">
            <template v-if="downloading">
              <span class="cue-meter"
                ><span :style="{ width: Math.round((chosen.progress || 0) * 100) + '%' }"></span
              ></span>
              <span class="cue-note"
                >Bajando la voz… {{ Math.round((chosen.progress || 0) * 100) }} %</span
              >
            </template>
            <template v-else>
              <span class="cue-note">
                <template v-if="chosen.error">{{ chosen.error }}. </template>
                Esta voz se baja una sola vez ({{ mb(chosen.size) }} MB<template
                  v-if="chosen.engine === 'supertonic'"
                  >, y con ella las diez voces naturales</template
                >). Luego funciona sin internet.
              </span>
              <button type="button" class="btn mini primary" @click="download">
                <Icon n="download" :t="12" /> Bajar
              </button>
            </template>
          </div>
        </div>

        <div v-else key="audio" class="cue-body">
          <!-- grabar o importar: los dos a la vista; grabando, el medidor -->
          <div class="cue-source">
            <button
              type="button"
              class="btn cue-rec"
              :class="{ on: recording }"
              :disabled="!canRecord || busy"
              :title="
                canRecord
                  ? recording
                    ? 'Parar y usar lo grabado'
                    : 'Grabar con el micrófono del equipo: di la parte y para'
                  : 'Grabar solo se puede en la aplicación de escritorio'
              "
              @click="toggleRecord"
            >
              <!-- en un span: Icon lleva comentarios y la transición no lo animaría -->
              <Transition name="icon-swap" mode="out-in">
                <span :key="recording ? 'r' : 'm'" class="icon-swap"
                  ><Icon :n="recording ? 'stop' : 'mic'" :t="14"
                /></span>
              </Transition>
              {{ recording ? 'Parar' : clip ? 'Grabar otra vez' : 'Grabar' }}
            </button>
            <button
              type="button"
              class="btn cue-import"
              :disabled="recording || busy"
              :title="
                'Un audio corto del equipo (WAV, MP3, FLAC, OGG, M4A…): como mucho ' +
                formatTime(longest) +
                ', lo que dura la canción'
              "
              @click="importAudio"
            >
              <Icon n="import" :t="14" /> {{ clip ? 'Importar otro' : 'Importar un audio' }}
            </button>
            <Transition name="fade">
              <span v-if="recording" class="cue-rec-live">
                <span class="cue-rec-dot" aria-hidden="true"></span>
                <span class="mono">{{ formatTime(recSeconds) }}</span>
                <span class="cue-meter" aria-hidden="true"
                  ><span :style="{ width: Math.min(100, Math.round(level * 140)) + '%' }"></span
                ></span>
              </span>
            </Transition>
          </div>
          <p v-if="!clip && !recording" class="cue-note">
            <template v-if="!canRecord"
              >Grabar solo se puede en la aplicación de escritorio.</template
            >
            <template v-else
              >Graba tu voz o trae un audio corto: luego lo recortas y lo limpias aquí.</template
            >
          </p>
          <p v-else-if="recording" class="cue-note">Habla ahora… y pulsa «Parar» al acabar.</p>

          <!-- el editor: la onda con sus bordes, los fundidos y la limpieza -->
          <Transition name="cue-pop">
            <div v-if="clip && !recording" class="cue-editor">
              <!-- eslint-disable-next-line vuejs-accessibility/no-static-element-interactions -->
              <div
                ref="waveEl"
                class="cue-wave"
                :class="{ loading: !peaks.length }"
                title="Arrastra los bordes para recortar (o pulsa donde quieras que empiece o acabe)"
                @pointerdown="onWaveDown"
                @pointermove="onWaveMove"
                @pointerup="onWaveUp"
                @pointercancel="onWaveUp"
              >
                <svg
                  class="cue-wave-svg"
                  viewBox="0 0 1000 100"
                  preserveAspectRatio="none"
                  aria-hidden="true"
                >
                  <path class="cue-wave-shape" :d="wavePath" />
                  <path class="cue-wave-env" :d="envelope" />
                </svg>
                <span class="cue-cut" :style="{ left: 0, width: pct(fx.start) + '%' }"></span>
                <span class="cue-cut" :style="{ left: pct(fx.end) + '%', right: 0 }"></span>
                <button
                  type="button"
                  role="slider"
                  class="cue-edge start"
                  :style="{ left: pct(fx.start) + '%' }"
                  aria-label="Desde"
                  :aria-valuenow="fx.start"
                  aria-valuemin="0"
                  :aria-valuemax="clipLength"
                  :aria-valuetext="fmt(fx.start)"
                  title="Desde aquí (arrástralo, o con las flechas)"
                  @pointerdown.stop="grab('start', $event)"
                  @pointermove="onWaveMove"
                  @pointerup="onWaveUp"
                  @keydown="edgeKey('start', $event)"
                ></button>
                <button
                  type="button"
                  role="slider"
                  class="cue-edge end"
                  :style="{ left: pct(fx.end) + '%' }"
                  aria-label="Hasta"
                  :aria-valuenow="fx.end"
                  aria-valuemin="0"
                  :aria-valuemax="clipLength"
                  :aria-valuetext="fmt(fx.end)"
                  title="Hasta aquí (arrástralo, o con las flechas)"
                  @pointerdown.stop="grab('end', $event)"
                  @pointermove="onWaveMove"
                  @pointerup="onWaveUp"
                  @keydown="edgeKey('end', $event)"
                ></button>
                <span
                  v-if="headAt != null"
                  class="cue-wave-head"
                  :style="{ left: pct(headAt) + '%' }"
                ></span>
              </div>
              <div class="cue-times mono">
                <span title="Desde">{{ fmt(fx.start) }}</span>
                <span class="cue-len" title="Lo que dura la guía"
                  ><Icon n="cut" :t="11" /> {{ fmt(keptLength) }}</span
                >
                <span title="Hasta">{{ fmt(fx.end) }}</span>
              </div>
              <div class="cue-fades">
                <span
                  class="cue-fade"
                  title="Entrada suave: empieza en silencio y sube (fundido de entrada)"
                >
                  <Icon n="fadeIn" :t="15" />
                  <SliderField
                    v-model="fx.fadeIn"
                    :min="0"
                    :max="fadeMax"
                    :step="0.05"
                    width="100%"
                    aria-label="Entrada suave"
                    :value-text="fmt(fx.fadeIn)"
                  />
                  <span class="mono cue-fade-n"
                    >{{ fx.fadeIn.toFixed(1).replace('.', ',') }} s</span
                  >
                </span>
                <span
                  class="cue-fade"
                  title="Salida suave: baja hasta el silencio al final (fundido de salida)"
                >
                  <Icon n="fadeOut" :t="15" />
                  <SliderField
                    v-model="fx.fadeOut"
                    :min="0"
                    :max="fadeMax"
                    :step="0.05"
                    width="100%"
                    aria-label="Salida suave"
                    :value-text="fmt(fx.fadeOut)"
                  />
                  <span class="mono cue-fade-n"
                    >{{ fx.fadeOut.toFixed(1).replace('.', ',') }} s</span
                  >
                </span>
              </div>
              <div class="cue-fx" role="group" aria-label="Limpiarla">
                <button
                  v-for="f in FX"
                  :key="f.k"
                  type="button"
                  class="chip cue-fx-chip"
                  :class="{ on: fx[f.k] }"
                  :aria-pressed="fx[f.k]"
                  :title="f.title"
                  @click="fx[f.k] = !fx[f.k]"
                >
                  <Icon :n="fx[f.k] ? 'check' : f.icon" :t="12" /> {{ f.n }}
                </button>
                <span class="cue-gap"></span>
                <Transition name="fade">
                  <button
                    v-if="changed"
                    type="button"
                    class="btn mini icon-btn"
                    aria-label="Deshacer los cambios"
                    title="Deshacer los cambios (como estaba al grabarla o importarla)"
                    @click="resetFx"
                  >
                    <Icon n="refresh" :t="13" />
                  </button>
                </Transition>
              </div>
            </div>
          </Transition>
        </div>
      </Transition>

      <footer class="cue-foot">
        <button
          v-if="draft.cue"
          type="button"
          class="btn mini icon-btn danger cue-remove"
          aria-label="Quitar"
          title="Quitar esta guía"
          @click="emit('remove', draft.cue)"
        >
          <Icon n="trash" :t="13" />
        </button>
        <span class="cue-gap"></span>
        <button type="button" class="btn mini" :disabled="busy || !ready" @click="preview">
          <!-- en un span: Icon lleva comentarios y la transición no lo animaría -->
          <Transition name="icon-swap" mode="out-in">
            <span :key="listening ? 's' : 'p'" class="icon-swap"
              ><Icon :n="listening ? 'stop' : 'play'" :t="11"
            /></span>
          </Transition>
          {{ listening ? 'Parar' : 'Escuchar' }}
        </button>
        <button type="button" class="btn mini primary" :disabled="busy || !ready" @click="save">
          <Icon n="save" :t="12" /> {{ busy ? 'Un momento…' : 'Guardar' }}
        </button>
      </footer>
    </div>
  </div>
</template>
