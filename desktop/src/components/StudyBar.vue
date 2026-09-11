<script setup>
/**
 * El modo estudio: para machacar un trozo de la canción que suena.
 *
 * Bucle A-B (marcar A donde va, marcar B, y da vueltas), velocidad sin
 * cambiar el tono (ffmpeg la ralentiza; sin ffmpeg, Rust avisa de que el
 * tono se mueve), marcadores con nombre para saltar a una sección, y notas.
 * Todo se guarda con la canción —en el índice y en una etiqueta del
 * archivo—, así que al volver a ella está como se dejó.
 *
 * Al abrir la barra se aplican el bucle y la velocidad guardados; al
 * cerrarla se quitan y la canción vuelve a sonar normal.
 */
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { ask } from '../composables/useDialog.js'
import { usePlayback } from '../composables/usePlayback.js'
import { formatTime } from '../utils/format.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'

const emit = defineEmits(['close'])
const player = usePlayback()
const { track, position, speed, loopA, loopB, pitchPreserved } = player

const SPEEDS = [0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.25]
const study = ref({ loop: null, speed: 1, markers: [], notes: '' })
const loadedFor = ref(null)
const saving = ref(false)
let saveTimer = null

/** Lo guardado con la canción, si lo hay. */
function parse (raw) {
  try { return raw ? JSON.parse(raw) : {} } catch { return {} }
}
async function loadFor (id) {
  loadedFor.value = id
  if (!id) { study.value = { loop: null, speed: 1, markers: [], notes: '' }; return }
  try {
    const s = parse((await api.song(id))?.study)
    if (loadedFor.value !== id) return
    study.value = { loop: Array.isArray(s.loop) ? s.loop : null, speed: s.speed || 1,
                    markers: s.markers || [], notes: s.notes || '' }
    // lo guardado se aplica al entrar: para eso se guardo
    if (study.value.loop) await player.setLoop(study.value.loop[0], study.value.loop[1])
    else await player.clearLoop()
    if (study.value.speed !== speed.value) await player.setSpeed(study.value.speed)
  } catch (e) { notify(errorMessage(e)) }
}
watch(() => track.value?.id ?? null, (id) => loadFor(id), { immediate: true })

function scheduleSave () {
  clearTimeout(saveTimer)
  saveTimer = setTimeout(save, 600)
}
async function save () {
  const id = loadedFor.value
  if (!id) return
  saving.value = true
  try {
    const s = study.value
    await api.setStudy(id, {
      ...(s.loop ? { loop: s.loop } : {}),
      ...(s.speed && s.speed !== 1 ? { speed: s.speed } : {}),
      ...(s.markers.length ? { markers: s.markers } : {}),
      ...(s.notes.trim() ? { notes: s.notes.trim() } : {})
    })
  } catch (e) { notify('No se pudo guardar el estudio: ' + errorMessage(e)) } finally { saving.value = false }
}

// ---- bucle A-B: un boton marca A, el siguiente B, el tercero lo quita
const pendingA = ref(null)
const loopLabel = computed(() => {
  if (loopB.value > loopA.value) return `${formatTime(loopA.value)} – ${formatTime(loopB.value)}`
  if (pendingA.value != null) return `A en ${formatTime(pendingA.value)}, marca B`
  return 'sin bucle'
})
async function markLoop () {
  if (loopB.value > loopA.value) { await clearLoop(); return }
  const at = position.value
  if (pendingA.value == null) { pendingA.value = at; return }
  const a = Math.min(pendingA.value, at)
  const b = Math.max(pendingA.value, at)
  pendingA.value = null
  if (b - a < 0.5) { notify('El bucle tiene que durar al menos medio segundo'); return }
  study.value.loop = [Math.round(a * 100) / 100, Math.round(b * 100) / 100]
  await player.setLoop(study.value.loop[0], study.value.loop[1])
  await player.seek(a)
  scheduleSave()
}
async function clearLoop () {
  pendingA.value = null
  study.value.loop = null
  await player.clearLoop()
  scheduleSave()
}

// ---- velocidad
async function setSpeed (v) {
  study.value.speed = v
  await player.setSpeed(v)
  scheduleSave()
}

// ---- marcadores
function addMarker () {
  const t = Math.round(position.value * 100) / 100
  const n = study.value.markers.length + 1
  study.value.markers = [...study.value.markers, { t, label: `Marca ${n}` }].sort((a, b) => a.t - b.t)
  scheduleSave()
}
function removeMarker (m) {
  study.value.markers = study.value.markers.filter((x) => x !== m)
  scheduleSave()
}
async function renameMarker (m) {
  const label = await ask({ kind: 'prompt', title: 'Nombre del marcador', value: m.label, okLabel: 'Guardar' })
  if (!label) return
  m.label = label.slice(0, 60)
  scheduleSave()
}
const jump = (t) => player.seek(t)

/** Al cerrar, la cancion vuelve a sonar normal. */
async function close () {
  clearTimeout(saveTimer)
  await save()
  await player.clearLoop()
  if (speed.value !== 1) await player.setSpeed(1)
  emit('close')
}
onMounted(() => { if (track.value?.id) loadFor(track.value.id) })
onUnmounted(() => clearTimeout(saveTimer))
</script>

<template>
  <div class="study">
    <div class="study-head">
      <Icon n="academic" :t="14" /> <strong>Modo estudio</strong>
      <span class="study-song" v-if="track">{{ track.artist }} — {{ track.title }}</span>
      <span class="study-song" v-else>Pon una canción para estudiarla</span>
      <span v-if="saving" class="study-saving">guardando…</span>
      <button class="btn mini" type="button" title="Cerrar el modo estudio (vuelve a sonar normal)" @click="close">
        <Icon n="close" :t="13" /></button>
    </div>
    <div class="study-row">
      <div class="study-group">
        <span class="field-label">Bucle A-B</span>
        <div class="btn-row">
          <button class="btn mini" type="button" :class="{on: loopB > loopA || pendingA != null}" :disabled="!track" @click="markLoop">
            <Icon n="repeat" :t="12" />
            {{ loopB > loopA ? 'Quitar bucle' : pendingA != null ? 'Marcar B' : 'Marcar A' }}</button>
          <span class="study-loop mono">{{ loopLabel }}</span>
        </div>
      </div>
      <div class="study-group">
        <span class="field-label">Velocidad <em v-if="speed !== 1 && !pitchPreserved" class="model-warn">cambia el tono: falta ffmpeg</em>
          <em v-else-if="speed !== 1">mismo tono</em></span>
        <div class="btn-row">
          <button v-for="v in SPEEDS" :key="v" class="btn mini" type="button" :class="{on: speed === v}"
                  :disabled="!track" @click="setSpeed(v)">{{ v }}×</button>
        </div>
      </div>
    </div>
    <div class="study-row">
      <div class="study-group" style="flex:1">
        <span class="field-label">Marcadores</span>
        <div class="study-markers">
          <span v-for="m in study.markers" :key="m.t" class="chip study-marker">
            <button type="button" class="study-jump mono" :title="'Ir a ' + formatTime(m.t)" @click="jump(m.t)">{{ formatTime(m.t) }}</button>
            <button type="button" class="study-marker-name" :title="'Renombrar «' + m.label + '»'" @click="renameMarker(m)">{{ m.label }}</button>
            <button type="button" class="field-btn" title="Quitar" @click="removeMarker(m)"><Icon n="close" :t="11" /></button>
          </span>
          <button class="btn mini" type="button" :disabled="!track" title="Marcar este momento" @click="addMarker">
            <Icon n="plus" :t="12" /> aquí</button>
        </div>
      </div>
      <div class="study-group" style="flex:1">
        <TextField v-model="study.notes" multiline :rows="2" width="100%" label="Notas"
                   placeholder="cejilla en 2, entrar tras el redoble…" :disabled="!track"
                   @update:modelValue="scheduleSave" />
      </div>
    </div>
  </div>
</template>
