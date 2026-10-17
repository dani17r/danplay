<script setup>
/**
 * Elegir la voz de las guías: para la guía automática (la primera vez, si no
 * hay ninguna elegida) o para cambiar la de las guías que ya están. Cada voz
 * con lo que es (natural, de dónde) y, si no está bajada, lo que pesa y su
 * botón para bajarla (una sola vez; luego va sin internet).
 */
import { computed, onUnmounted, ref, watch } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import Icon from './Icon.vue'
import WideDialog from './ui/WideDialog.vue'

const props = defineProps({
  open: Boolean,
  title: { type: String, default: 'La voz de las guías' },
  /** lo que pasa al elegir («Hacer la guía», «Cambiar la voz») */
  okLabel: { type: String, default: 'Usar esta voz' },
  /** la que está puesta (sale elegida) */
  current: { type: String, default: '' }
})
const emit = defineEmits(['close', 'pick'])

const voices = ref([])
const unavailable = ref('')
const chosen = ref('')
let poll = null

async function load() {
  try {
    const r = await api.cueVoices()
    voices.value = r?.voices || []
    unavailable.value = r?.available || ''
  } catch (e) {
    voices.value = []
    unavailable.value = errorMessage(e)
  }
  clearTimeout(poll)
  if (props.open && voices.value.some((v) => v.progress != null && !v.installed && !v.error))
    poll = setTimeout(load, 1000)
}
watch(
  () => props.open,
  async (v) => {
    if (!v) return clearTimeout(poll)
    await load()
    const ready = voices.value.filter((x) => x.installed)
    chosen.value =
      voices.value.find((x) => x.id === props.current)?.id ||
      ready[0]?.id ||
      voices.value[0]?.id ||
      ''
  },
  { immediate: true }
)
onUnmounted(() => clearTimeout(poll))

const mb = (bytes) => Math.max(1, Math.round((bytes || 0) / 1e6))
const pick = computed(() => voices.value.find((v) => v.id === chosen.value) || null)
const ready = computed(() => !!pick.value?.installed)
async function download(v) {
  try {
    await api.installCueVoice(v.id)
  } catch (e) {
    notify('No se pudo bajar la voz: ' + errorMessage(e))
  }
  await load()
}
function go() {
  if (!ready.value) return
  emit('pick', chosen.value)
}
</script>

<template>
  <WideDialog :open="open" :title="title" icon="speech" width="560px" @close="emit('close')">
    <p v-if="unavailable" class="vp-warn">No se pueden decir guías aquí: {{ unavailable }}</p>
    <div v-else class="vp-list" role="radiogroup" aria-label="Las voces">
      <div v-for="v in voices" :key="v.id" class="vp-voice" :class="{ on: chosen === v.id }">
        <button
          type="button"
          role="radio"
          class="vp-pick"
          :aria-checked="chosen === v.id"
          @click="chosen = v.id"
        >
          <span class="vp-dot" aria-hidden="true"
            ><Icon v-if="chosen === v.id" n="check" :t="12"
          /></span>
          <span class="vp-name">{{ v.name }}</span>
          <span class="vp-meta">{{ v.engine === 'supertonic' ? 'natural' : v.lang }}</span>
        </button>
        <span v-if="v.installed" class="vp-state ok"><Icon n="checkCircle" :t="13" /> lista</span>
        <span v-else-if="v.progress != null && !v.error" class="vp-state">
          <span class="vp-bar"
            ><span :style="{ width: Math.round(v.progress * 100) + '%' }"></span
          ></span>
          {{ Math.round(v.progress * 100) }} %
        </span>
        <button
          v-else
          type="button"
          class="btn mini vp-get"
          :title="
            'Bajarla una sola vez (' +
            mb(v.size) +
            ' MB' +
            (v.engine === 'supertonic' ? ', y con ella las diez voces naturales' : '') +
            '). Luego funciona sin internet.'
          "
          @click="download(v)"
        >
          <Icon n="download" :t="12" /> {{ mb(v.size) }} MB
        </button>
      </div>
    </div>
    <template #foot>
      <button type="button" class="btn" @click="emit('close')">Cancelar</button>
      <button
        type="button"
        class="btn primary vp-go"
        :disabled="!ready"
        :title="ready ? '' : 'Baja antes esa voz'"
        @click="go"
      >
        <Icon n="sparkles" :t="13" /> {{ okLabel }}
      </button>
    </template>
  </WideDialog>
</template>
