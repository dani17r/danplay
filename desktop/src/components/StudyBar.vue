<script setup>
/**
 * El modo estudio: para machacar un trozo de la canción que suena.
 *
 * El tramo que se repite se elige sobre la forma de onda (StudyTimeline):
 * se arrastra de donde a donde, se cogen sus bordes, o se marca con las
 * teclas A y B mientras suena. Un tramo se guarda como **marcador**: un
 * nombre, el inicio Y el final, y sus propias notas; pulsarlo vuelve a
 * poner ese bucle y coloca la canción al principio del tramo (si sonaba,
 * sigue sonando desde ahí; si no, queda lista para play). Con el marcador
 * elegido, mover los bordes del tramo lo cambia a él. Hay además notas
 * generales de la canción, velocidad sin cambiar el tono (ffmpeg la
 * ralentiza; sin ffmpeg, Rust avisa de que el tono se mueve) y marcadores
 * de un instante suelto.
 *
 * Todo se guarda con la canción —en el índice y en una etiqueta del
 * archivo—, así que al volver a ella está como se dejó; y se va con ella si
 * se borra. Al abrir la barra se aplican el bucle y la velocidad guardados;
 * al cerrarla se quitan y la canción vuelve a sonar normal.
 *
 * La barra es una capa por encima de la lista: no recoloca nada de detrás.
 */
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { ask } from '../composables/useDialog.js'
import { usePlayback } from '../composables/usePlayback.js'
import { useHotkeys } from '../composables/useHotkeys.js'
import { formatTime } from '../utils/format.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import StudyTimeline from './StudyTimeline.vue'

const emit = defineEmits(['close'])
const player = usePlayback()
const { track, position, duration, speed, loopA, loopB, pitchPreserved } = player

const SPEEDS = [0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.25]
const blank = () => ({ loop: null, speed: 1, markers: [], notes: '' })
const study = ref(blank())
const loadedFor = ref(null)
const saving = ref(false)
/** El marcador elegido (uno de `study.markers`), o null. */
const selected = ref(null)
let saveTimer = null

const round2 = (v) => Math.round(v * 100) / 100
const near = (x, y) => Math.abs(x - y) < 0.011
const isSpan = (m) => m.end > m.t

/** Lo guardado con la canción, si lo hay. */
function parse (raw) {
  try { return raw ? JSON.parse(raw) : {} } catch { return {} }
}
async function loadFor (id) {
  loadedFor.value = id
  selected.value = null
  if (!id) { study.value = blank(); return }
  try {
    const s = parse((await api.song(id))?.study)
    if (loadedFor.value !== id) return
    study.value = { loop: Array.isArray(s.loop) ? s.loop : null, speed: s.speed || 1,
                    markers: (s.markers || []).map((m) => ({ ...m })), notes: s.notes || '' }
    // lo guardado se aplica al entrar: para eso se guardo
    if (study.value.loop) await player.setLoop(study.value.loop[0], study.value.loop[1])
    else await player.clearLoop()
    if (study.value.speed !== speed.value) await player.setSpeed(study.value.speed)
    // si el bucle guardado es el de un marcador, ese marcador queda elegido
    selected.value = markerOf(study.value.loop)
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
    const markers = s.markers.map((m) => ({
      t: m.t, ...(isSpan(m) ? { end: m.end } : {}), label: m.label,
      ...(m.notes?.trim() ? { notes: m.notes.trim() } : {})
    }))
    await api.setStudy(id, {
      ...(s.loop ? { loop: s.loop } : {}),
      ...(s.speed && s.speed !== 1 ? { speed: s.speed } : {}),
      ...(markers.length ? { markers } : {}),
      ...(s.notes.trim() ? { notes: s.notes.trim() } : {})
    })
  } catch (e) { notify('No se pudo guardar el estudio: ' + errorMessage(e)) } finally { saving.value = false }
}

// ---- el tramo que se repite
// Lo elige la linea de tiempo (arrastrando), un marcador, o las teclas A y B.
// `pendingA` es una A marcada con la tecla a la que aun le falta su B.
const pendingA = ref(null)
const hasLoop = computed(() => loopB.value > loopA.value)
const loopLabel = computed(() => {
  if (hasLoop.value) return `${formatTime(loopA.value)} – ${formatTime(loopB.value)}`
  if (pendingA.value != null) return `A en ${formatTime(pendingA.value)} · pulsa B donde acabe`
  return 'arrastra sobre la onda para elegir el tramo, o marca con A y B mientras suena'
})

/** Deja el bucle en [a, b]. Si la cancion va por fuera del tramo, salta a A. */
async function applyLoop (a, b) {
  if (!(b - a >= 0.5)) { notify('El bucle tiene que durar al menos medio segundo'); return }
  pendingA.value = null
  study.value.loop = [round2(a), round2(b)]
  await player.setLoop(study.value.loop[0], study.value.loop[1])
  if (position.value < a || position.value > b) await player.seek(a)
  scheduleSave()
}
async function clearLoop () {
  pendingA.value = null
  study.value.loop = null
  await player.clearLoop()
  scheduleSave()
}
/**
 * Lo que manda la linea de tiempo. Un tramo nuevo dibujado de cero deja de
 * apuntar al marcador que hubiera elegido; mover un borde con un marcador
 * elegido cambia el tramo DEL marcador, que es lo que uno espera al
 * estirarlo.
 */
function onLoop (range, { mode } = {}) {
  if (!range) return clearLoop()
  const m = selected.value
  if (mode === 'edit' && m && isSpan(m)) {
    m.t = range[0]; m.end = range[1]
    study.value.markers = [...study.value.markers].sort((x, y) => x.t - y.t)
  } else if (mode === 'select') {
    selected.value = null
  }
  return applyLoop(range[0], range[1])
}
/** Tecla A: aqui empieza. Con bucle puesto, mueve su A si cabe. */
function markA () {
  if (!track.value) return
  const at = round2(position.value)
  selected.value = null
  if (hasLoop.value && at < loopB.value - 0.5) return applyLoop(at, loopB.value)
  if (hasLoop.value) clearLoop()
  pendingA.value = at
}
/** Tecla B: aqui acaba. Cierra la A pendiente, mueve la B del bucle, o va desde 0. */
function markB () {
  if (!track.value) return
  const at = round2(position.value)
  const from = pendingA.value ?? (hasLoop.value ? loopA.value : 0)
  if (at - from < 0.5) { notify('El bucle tiene que durar al menos medio segundo'); return }
  selected.value = null
  return applyLoop(from, at)
}
useHotkeys({ a: markA, b: markB })

// ---- velocidad
async function setSpeed (v) {
  study.value.speed = v
  await player.setSpeed(v)
  scheduleSave()
}

// ---- marcadores: tramos con nombre y notas, o un instante suelto
/** El marcador cuyo tramo es exactamente ese, si lo hay. */
function markerOf (range) {
  if (!range) return null
  return study.value.markers.find((m) => isSpan(m) && near(m.t, range[0]) && near(m.end, range[1])) || null
}
const markerLabel = (m) => isSpan(m) ? `${formatTime(m.t)} – ${formatTime(m.end)}` : formatTime(m.t)
/** ¿El tramo que suena ya esta guardado como marcador? */
const loopSaved = computed(() => hasLoop.value && !!markerOf([loopA.value, loopB.value]))

/**
 * Guarda el tramo que se repite como marcador (inicio y fin); sin tramo,
 * marca el instante por el que va la cancion.
 */
function saveMarker () {
  if (!track.value) return
  let m
  if (hasLoop.value) {
    const dup = markerOf([loopA.value, loopB.value])
    if (dup) { selected.value = dup; return notify(`Ese tramo ya es «${dup.label}»`, 'info') }
    const n = study.value.markers.filter(isSpan).length + 1
    m = { t: round2(loopA.value), end: round2(loopB.value), label: `Tramo ${n}`, notes: '' }
  } else {
    const n = study.value.markers.filter((x) => !isSpan(x)).length + 1
    m = { t: round2(position.value), label: `Marca ${n}`, notes: '' }
  }
  study.value.markers = [...study.value.markers, m].sort((a, b) => a.t - b.t)
  selected.value = m
  scheduleSave()
}
/**
 * Pulsar un marcador lo elige y pone su tramo a repetir, con la cancion al
 * principio del tramo: si sonaba sigue sonando desde ahi, y si no, queda
 * lista para darle a play. Un instante suelto solo lleva alli.
 */
async function selectMarker (m) {
  selected.value = m
  if (isSpan(m)) {
    pendingA.value = null
    study.value.loop = [m.t, m.end]
    await player.setLoop(m.t, m.end)
    scheduleSave()
  }
  await player.seek(m.t)
}
function removeMarker (m) {
  if (selected.value === m) selected.value = null
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
  <div class="study" role="region" aria-label="Modo estudio">
    <div class="study-head">
      <Icon n="academic" :t="14" /> <strong>Modo estudio</strong>
      <span class="study-song" v-if="track">{{ track.artist }} — {{ track.title }}</span>
      <span class="study-song" v-else>Pon una canción para estudiarla</span>
      <span v-if="saving" class="study-saving">guardando…</span>
      <button class="btn mini" type="button" title="Cerrar el modo estudio (vuelve a sonar normal)" @click="close">
        <Icon n="close" :t="13" /></button>
    </div>
    <!-- la onda, la regla, el tramo que se repite y los marcadores -->
    <StudyTimeline :song-id="track?.id ?? null" :duration="duration" :position="position"
                   :loop="study.loop" :markers="study.markers" :selected="selected"
                   @update:loop="onLoop" @seek="jump" @marker="selectMarker" />
    <div class="study-row">
      <div class="study-group">
        <span class="field-label">Tramo que se repite</span>
        <div class="btn-row">
          <span class="study-loop" :class="{ mono: hasLoop || pendingA != null }">{{ loopLabel }}</span>
          <button v-if="hasLoop && !loopSaved" class="btn mini" type="button"
                  title="Guardar este tramo como marcador, con su nombre y sus notas" @click="saveMarker">
            <Icon n="save" :t="12" /> Guardar tramo</button>
          <button v-if="hasLoop" class="chip x study-clear" type="button" title="Quitar el bucle" @click="clearLoop">
            quitar ×</button>
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
        <span class="field-label">Marcadores <em v-if="study.markers.length">pulsa uno para repetir ese tramo</em></span>
        <div class="study-markers">
          <span v-for="m in study.markers" :key="m.t + ':' + (m.end || 0)" class="chip study-marker"
                :class="{ on: m === selected, span: isSpan(m) }">
            <button type="button" class="study-pick"
                    :title="isSpan(m) ? 'Repetir este tramo (' + markerLabel(m) + ')' : 'Ir a ' + markerLabel(m)"
                    @click="selectMarker(m)">
              <span class="mono study-pick-time">{{ markerLabel(m) }}</span>
              <span class="study-pick-name">{{ m.label }}</span>
            </button>
            <button type="button" class="field-btn" :title="'Renombrar «' + m.label + '»'" @click="renameMarker(m)">
              <Icon n="pencil" :t="11" /></button>
            <button type="button" class="field-btn" title="Quitar" @click="removeMarker(m)"><Icon n="close" :t="11" /></button>
          </span>
          <button v-if="!hasLoop" class="btn mini" type="button" :disabled="!track"
                  title="Marcar este instante (sin tramo elegido)" @click="saveMarker">
            <Icon n="plus" :t="12" /> aquí</button>
        </div>
      </div>
      <div v-if="selected" class="study-group" style="flex:1">
        <TextField v-model="selected.notes" multiline :rows="2" width="100%"
                   :label="'Notas de «' + selected.label + '»'"
                   placeholder="qué trabajar en este tramo…" @update:modelValue="scheduleSave" />
      </div>
      <div class="study-group" style="flex:1">
        <TextField v-model="study.notes" multiline :rows="2" width="100%" label="Notas de la canción"
                   placeholder="cejilla en 2, entrar tras el redoble…" :disabled="!track"
                   @update:modelValue="scheduleSave" />
      </div>
    </div>
  </div>
</template>
