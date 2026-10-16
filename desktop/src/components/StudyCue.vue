<script setup>
/**
 * Una guía hablada: lo que se dice en un punto de la canción («Coro»,
 * «Puente, cuatro tiempos», «Sube medio tono»), como la pista «Guide» de una
 * secuencia. Se escribe y la dice una voz que se elige (la hace el núcleo,
 * en el equipo, sin internet salvo la primera vez que se baja esa voz), o se
 * graba con el micrófono. Queda en un archivo y suena en el motor justo ahí,
 * también a otra velocidad y en la vuelta de un tramo, por la salida de las
 * guías.
 */
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { api, app as tauriApp, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { usePlayback } from '../composables/usePlayback.js'
import { formatTime } from '../utils/format.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'

const props = defineProps({
  /** {t, text, voice, cue}: `cue` es la guía que se cambia (o null: nueva) */
  draft: { type: Object, required: true },
  songId: { type: Number, default: null }
})
const emit = defineEmits(['made', 'remove', 'close'])
const player = usePlayback()

const at = ref(props.draft.t || 0)
const text = ref(props.draft.text || '')
const voice = ref(props.draft.voice || '')
const tab = ref(props.draft.cue && !props.draft.cue.text ? 'record' : 'write')
const voices = ref([])
const busy = ref(false)
/** Lo grabado (la ruta del archivo) y si se está grabando. */
const recorded = ref('')
const recording = ref(false)
const level = ref(0)
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

function nudge(delta) {
  at.value = Math.max(0, Math.round((at.value + delta) * 100) / 100)
}
function here() {
  at.value = Math.round(player.position.value * 100) / 100
}

/** Escuchar lo que se va a guardar (sin guardarlo). */
async function preview() {
  busy.value = true
  try {
    const r =
      tab.value === 'write'
        ? await api.makeCue(props.songId, { text: text.value, voice: voice.value, preview: true })
        : { file: recorded.value }
    if (r?.file) await api.previewCue(r.file)
  } catch (e) {
    notify(errorMessage(e))
  } finally {
    busy.value = false
  }
}
async function save() {
  busy.value = true
  try {
    let r
    if (tab.value === 'write') {
      if (!text.value.trim()) return notify('Escribe lo que se dirá', 'info')
      if (!voiceReady.value) return notify('Baja antes la voz', 'info')
      r = await api.makeCue(props.songId, { text: text.value.trim(), voice: voice.value })
    } else {
      if (!recorded.value) return notify('Graba antes lo que se dirá', 'info')
      r = await api.importCue(props.songId, recorded.value)
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
async function toggleRecord() {
  try {
    if (!recording.value) {
      await tauriApp.recordStart()
      recording.value = true
      recorded.value = ''
      levelTimer = setInterval(async () => {
        level.value = (await tauriApp.recordLevel().catch(() => 0)) || 0
      }, 80)
    } else {
      clearInterval(levelTimer)
      recording.value = false
      level.value = 0
      recorded.value = await tauriApp.recordStop()
    }
  } catch (e) {
    recording.value = false
    clearInterval(levelTimer)
    notify('No se pudo grabar: ' + errorMessage(e))
  }
}
</script>

<template>
  <!-- fuera de la tarjeta, cierra (con el teclado, Escape: ver onKey) -->
  <!-- eslint-disable-next-line vuejs-accessibility/click-events-have-key-events, vuejs-accessibility/no-static-element-interactions -->
  <div class="cue-veil" @click.self="emit('close')">
    <div class="cue-card" role="dialog" aria-label="Guía hablada">
      <header class="cue-head">
        <Icon n="mic" :t="15" />
        <strong>Guía hablada</strong>
        <span class="cue-at mono">
          <button type="button" class="btn mini" title="Medio segundo antes" @click="nudge(-0.5)">
            −
          </button>
          {{ formatTime(at) }}
          <button type="button" class="btn mini" title="Medio segundo después" @click="nudge(0.5)">
            +
          </button>
          <button type="button" class="btn mini" title="Donde va la canción" @click="here">
            aquí
          </button>
        </span>
        <button type="button" class="btn mini cue-close" aria-label="Cerrar" @click="emit('close')">
          <Icon n="close" :t="12" />
        </button>
      </header>
      <p class="cue-lead">
        Suena justo en ese punto, por la salida de las guías (en «Salidas» puedes mandarla a tus
        cascos). Ponla un poco antes de donde entra la parte, como un director.
      </p>

      <div class="study-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          class="study-tab"
          :class="{ on: tab === 'write' }"
          :aria-selected="tab === 'write'"
          @click="tab = 'write'"
        >
          Escribirla
        </button>
        <button
          type="button"
          role="tab"
          class="study-tab"
          :class="{ on: tab === 'record' }"
          :aria-selected="tab === 'record'"
          @click="tab = 'record'"
        >
          Grabar mi voz
        </button>
      </div>

      <div v-if="tab === 'write'" class="cue-body">
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
      <div v-else class="cue-body">
        <button
          type="button"
          class="btn cue-rec"
          :class="{ on: recording }"
          :disabled="!canRecord"
          @click="toggleRecord"
        >
          <Icon :n="recording ? 'stop' : 'mic'" :t="14" />
          {{ recording ? 'Parar' : recorded ? 'Grabar otra vez' : 'Grabar' }}
        </button>
        <span class="cue-meter" aria-hidden="true"
          ><span :style="{ width: Math.min(100, Math.round(level * 140)) + '%' }"></span
        ></span>
        <p class="cue-note">
          <template v-if="!canRecord"
            >Grabar solo se puede en la aplicación de escritorio.</template
          >
          <template v-else-if="recording">Habla ahora… y pulsa «Parar» al acabar.</template>
          <template v-else-if="recorded">Listo: escúchala o guárdala.</template>
          <template v-else>Con el micrófono del equipo: di la parte y para.</template>
        </p>
      </div>

      <footer class="cue-foot">
        <button
          v-if="draft.cue"
          type="button"
          class="btn mini danger"
          @click="emit('remove', draft.cue)"
        >
          <Icon n="trash" :t="12" /> Quitar
        </button>
        <span class="cue-gap"></span>
        <button
          type="button"
          class="btn mini"
          :disabled="busy || (tab === 'write' ? !text.trim() || !voiceReady : !recorded)"
          @click="preview"
        >
          <Icon n="play" :t="11" /> Escuchar
        </button>
        <button
          type="button"
          class="btn mini primary"
          :disabled="busy || (tab === 'write' ? !text.trim() || !voiceReady : !recorded)"
          @click="save"
        >
          <Icon n="save" :t="12" /> {{ busy ? 'Haciéndola…' : 'Guardar' }}
        </button>
      </footer>
    </div>
  </div>
</template>
