<script setup>
/**
 * El modo estudio: para machacar un trozo de la canción que suena.
 *
 * Es una capa que sube desde el reproductor y ocupa media pantalla, sin
 * recolocar la lista de detrás. Arriba, la forma de onda con la regla de
 * minutos: ahí se elige el tramo que se repite (arrastrando, cogiendo los
 * bordes, o con las teclas A y B mientras suena). Debajo, tres columnas:
 *
 * - Reproducción: el tramo, la velocidad (sin cambiar el tono), el tono
 *   corrido en semitonos, y el metrónomo, que detecta solo el pulso y el
 *   compás de la canción y entra en el «1».
 * - Marcadores: tramos guardados con nombre (inicio Y final) y sus notas;
 *   pulsar uno vuelve a poner ese bucle y coloca la canción al principio.
 * - Notas: un solo cuadro con dos pestañas, las de la canción y las del
 *   marcador elegido.
 *
 * Todo se guarda con la canción —en el índice y en una etiqueta del
 * archivo— y se va con ella si se borra. Al abrir se aplica lo guardado;
 * al cerrar se quita y la canción vuelve a sonar normal.
 */
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { ask } from '../composables/useDialog.js'
import { usePlayback } from '../composables/usePlayback.js'
import { useHotkeys } from '../composables/useHotkeys.js'
import { formatTime } from '../utils/format.js'
import { transposeKey, semitoneLabel } from '../utils/theory.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SliderField from './ui/SliderField.vue'
import StudyTimeline from './StudyTimeline.vue'

const emit = defineEmits(['close'])
const player = usePlayback()
const { track, position, duration, speed, pitch, metronome, loopA, loopB, pitchPreserved } = player

const SPEEDS = [0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.25]
const blank = () => ({ loop: null, speed: 1, pitch: 0, metronome: {}, markers: [], notes: '' })
const study = ref(blank())
const loadedFor = ref(null)
const saving = ref(false)
/** El marcador elegido (uno de `study.markers`), o null. */
const selected = ref(null)
/** La rejilla de pulsos de la cancion que suena (la pinta la onda), o null. */
const grid = ref(null)
const analyzing = ref(false)
const gridError = ref('')
let gridFor = null
/** La pestaña de notas: las de la cancion o las del marcador elegido. */
const notesTab = ref('song')
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
  grid.value = null
  if (!id) { study.value = blank(); return }
  try {
    const s = parse((await api.song(id))?.study)
    if (loadedFor.value !== id) return
    study.value = { loop: Array.isArray(s.loop) ? s.loop : null, speed: s.speed || 1,
                    pitch: Number.isInteger(s.pitch) ? s.pitch : 0,
                    metronome: typeof s.metronome === 'object' && s.metronome ? { ...s.metronome } : {},
                    markers: (s.markers || []).map((m) => ({ ...m })), notes: s.notes || '' }
    // lo guardado se aplica al entrar: para eso se guardo
    if (study.value.loop) await player.setLoop(study.value.loop[0], study.value.loop[1])
    else await player.clearLoop()
    if (study.value.speed !== speed.value) await player.setSpeed(study.value.speed)
    if (study.value.pitch !== pitch.value) await player.setPitch(study.value.pitch)
    await player.resetMetronomeOverrides(study.value.metronome)
    selected.value = markerOf(study.value.loop)
    // el compas se analiza ya, para que el clic entre al momento al pedirlo
    ensureGrid()
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
    const metro = Object.fromEntries(Object.entries(s.metronome || {}).filter(([, v]) => v != null && v !== 0))
    await api.setStudy(id, {
      ...(s.loop ? { loop: s.loop } : {}),
      ...(s.speed && s.speed !== 1 ? { speed: s.speed } : {}),
      ...(s.pitch ? { pitch: s.pitch } : {}),
      ...(Object.keys(metro).length ? { metronome: metro } : {}),
      ...(markers.length ? { markers } : {}),
      ...(s.notes.trim() ? { notes: s.notes.trim() } : {})
    })
  } catch (e) { notify('No se pudo guardar el estudio: ' + errorMessage(e)) } finally { saving.value = false }
}

// ---- el tramo que se repite
const pendingA = ref(null)
const hasLoop = computed(() => loopB.value > loopA.value)
const loopLabel = computed(() => {
  if (hasLoop.value) return `${formatTime(loopA.value)} – ${formatTime(loopB.value)}`
  if (pendingA.value != null) return `A en ${formatTime(pendingA.value)} · pulsa B donde acabe`
  return 'sin tramo'
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
 * elegido cambia el tramo DEL marcador, que es lo que uno espera al estirarlo.
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
function markA () {
  if (!track.value) return
  const at = round2(position.value)
  selected.value = null
  if (hasLoop.value && at < loopB.value - 0.5) return applyLoop(at, loopB.value)
  if (hasLoop.value) clearLoop()
  pendingA.value = at
}
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
  const s = Math.round(Math.max(0.25, Math.min(3, Number(v) || 1)) * 100) / 100
  study.value.speed = s
  await player.setSpeed(s)
  scheduleSave()
}
const speedPercent = computed(() => Math.round(speed.value * 100))

// ---- tono
const pitchLabel = computed(() => semitoneLabel(pitch.value))
const keyNow = computed(() => (track.value?.key ? transposeKey(track.value.key, pitch.value) : ''))
async function setPitch (n) {
  const p = Math.max(-12, Math.min(12, Math.round(n)))
  study.value.pitch = p
  await player.setPitch(p)
  scheduleSave()
}

// ---- metronomo
// El pulso y el compas los detecta Rust (`analyzeBeats`); la rejilla se
// pinta sobre la onda y el clic la sigue. Lo que uno ajuste a mano (tempo,
// compas, «1», doble/mitad) se guarda con la cancion en `study.metronome`.
async function ensureGrid () {
  const path = player.state.path
  const id = loadedFor.value
  if (!path || !id || gridFor === path) return grid.value
  gridFor = path
  analyzing.value = true
  gridError.value = ''
  try {
    const g = await player.analyzeBeats(track.value?.bpm || null)
    if (loadedFor.value !== id) return null
    grid.value = g
    // con la rejilla ya en Rust, se vuelven a mandar los ajustes para que
    // el clic la coja (si esta sonando, entra en el siguiente pulso)
    await player.setMetronome({})
    return g
  } catch (e) {
    if (loadedFor.value === id) gridError.value = errorMessage(e)
    return null
  } finally {
    if (loadedFor.value === id) analyzing.value = false
  }
}
// la ruta que suena puede llegar despues de la ficha: se vuelve a intentar
watch(() => player.state.path, () => { if (loadedFor.value && !grid.value) { gridFor = null; ensureGrid() } })

/** El tempo que se oye: el nominal por la velocidad, si sigue la cancion. */
const bpmShown = computed(() => {
  const m = metronome.value
  const base = m.bpm || 0
  return Math.round(m.free ? base : base * speed.value)
})
const meterShown = computed(() => (metronome.value.meter === 3 ? '3/4' : '4/4'))
const metroStatus = computed(() => {
  const m = metronome.value
  if (analyzing.value) return 'buscando el pulso y el compás…'
  if (gridError.value) return 'sin compás detectado: va libre'
  if (!m.has_grid) return 'sin compás: va libre'
  if (m.free) return 'tempo a mano: va libre (vuelve al de la canción para que la siga)'
  const sure = m.confidence >= 0.6 ? 'seguro' : m.confidence >= 0.3 ? 'probable' : 'dudoso'
  return `sigue la canción · el «1» ${sure}`
})
function metroPatch (patch) {
  const m = { ...(study.value.metronome || {}) }
  for (const [k, v] of Object.entries(patch)) {
    if (v == null || v === 0) delete m[k]
    else m[k] = v
  }
  study.value.metronome = m
  scheduleSave()
  return player.setMetronome(patch)
}
async function toggleMetronome () {
  if (!track.value) return
  const on = !metronome.value.on
  if (on && !grid.value && !gridError.value) await ensureGrid()
  await player.setMetronome({ on })
}
function bumpBpm (delta) {
  const now = bpmShown.value || 100
  metroPatch({ bpm: Math.max(20, Math.min(300, now + delta)) })
}
const followSong = () => metroPatch({ bpm: null })
const toggleMeter = () => metroPatch({ meter: metronome.value.meter === 3 ? 4 : 3 })
const shiftOne = () => metroPatch({ shift: ((metronome.value.shift || 0) + 1) % (metronome.value.meter || 4) })
const setMult = (mult) => metroPatch({ mult: metronome.value.mult === mult ? 0 : mult })
const setMetroVolume = (v) => player.setMetronome({ volume: v })

// ---- marcadores: tramos con nombre y notas, o un instante suelto
function markerOf (range) {
  if (!range) return null
  return study.value.markers.find((m) => isSpan(m) && near(m.t, range[0]) && near(m.end, range[1])) || null
}
const markerLabel = (m) => isSpan(m) ? `${formatTime(m.t)} – ${formatTime(m.end)}` : formatTime(m.t)
const loopSaved = computed(() => hasLoop.value && !!markerOf([loopA.value, loopB.value]))

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
  notesTab.value = 'marker'
  scheduleSave()
}
async function selectMarker (m) {
  selected.value = m
  notesTab.value = 'marker'
  if (isSpan(m)) {
    pendingA.value = null
    study.value.loop = [m.t, m.end]
    await player.setLoop(m.t, m.end)
    scheduleSave()
  }
  await player.seek(m.t)
}
function removeMarker (m) {
  if (selected.value === m) { selected.value = null; notesTab.value = 'song' }
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

// ---- notas: un solo cuadro, dos pestañas
watch(selected, (m) => { if (!m) notesTab.value = 'song' })

/** Al cerrar, la cancion vuelve a sonar normal. */
async function close () {
  clearTimeout(saveTimer)
  await save()
  await player.clearLoop()
  if (speed.value !== 1) await player.setSpeed(1)
  if (pitch.value !== 0) await player.setPitch(0)
  if (metronome.value.on) await player.setMetronome({ on: false })
  emit('close')
}
onMounted(() => { if (track.value?.id) loadFor(track.value.id) })
onUnmounted(() => clearTimeout(saveTimer))
</script>

<template>
  <div class="study" role="region" aria-label="Modo estudio">
    <div class="study-head">
      <Icon n="academic" :t="15" /> <strong>Modo estudio</strong>
      <span class="study-song" v-if="track">{{ track.artist }} — {{ track.title }}
        <span v-if="track.key" class="mono study-key">· {{ track.key }}</span></span>
      <span class="study-song" v-else>Pon una canción para estudiarla</span>
      <span v-if="saving" class="study-saving">guardando…</span>
      <button class="btn mini" type="button" title="Cerrar el modo estudio (vuelve a sonar normal)" @click="close">
        <Icon n="close" :t="13" /> Cerrar</button>
    </div>

    <!-- la onda, la regla, el tramo, los marcadores y la rejilla del compas -->
    <StudyTimeline :song-id="track?.id ?? null" :duration="duration" :position="position"
                   :loop="study.loop" :markers="study.markers" :selected="selected" :grid="grid" :height="80"
                   @update:loop="onLoop" @seek="jump" @marker="selectMarker" />
    <p class="study-hint">Arrastra sobre la onda para elegir el tramo que se repite · clic para ir a un punto · teclas <b>A</b> y <b>B</b> mientras suena</p>

    <div class="study-body">
      <!-- ================================================== reproduccion -->
      <section class="study-col">
        <h5 class="study-col-title"><Icon n="play" :t="12" /> Reproducción</h5>

        <div class="study-block">
          <div class="study-block-head">
            <span class="study-block-name"><Icon n="repeat" :t="12" /> Tramo que se repite</span>
            <span class="study-loop mono" :class="{ off: !hasLoop && pendingA == null }">{{ loopLabel }}</span>
          </div>
          <div class="btn-row" v-if="hasLoop">
            <button v-if="!loopSaved" class="btn mini" type="button"
                    title="Guardar este tramo como marcador, con nombre y notas" @click="saveMarker">
              <Icon n="save" :t="12" /> Guardar como marcador</button>
            <button class="btn mini study-clear" type="button" title="Quitar el bucle" @click="clearLoop">
              <Icon n="close" :t="12" /> Quitar</button>
          </div>
        </div>

        <div class="study-block">
          <div class="study-block-head">
            <span class="study-block-name"><Icon n="forward10" :t="12" /> Velocidad</span>
            <span class="mono study-value">{{ speedPercent }} %
              <em v-if="speed !== 1 && !pitchPreserved" class="model-warn">cambia el tono: falta ffmpeg</em>
              <em v-else-if="speed !== 1">mismo tono</em></span>
          </div>
          <div class="btn-row study-speeds">
            <button v-for="v in SPEEDS" :key="v" class="btn mini" type="button" :class="{on: Math.abs(speed - v) < 0.005}"
                    :disabled="!track" @click="setSpeed(v)">{{ v }}×</button>
          </div>
          <SliderField :model-value="speed" :min="0.5" :max="1.25" :step="0.01" width="100%"
                       @update:model-value="setSpeed" />
        </div>

        <div class="study-block">
          <div class="study-block-head">
            <span class="study-block-name"><Icon n="chords" :t="12" /> Tono</span>
            <span class="mono study-value">
              <template v-if="track?.key">{{ track.key }}<template v-if="pitch"> → <b>{{ keyNow }}</b></template></template>
              <template v-else>{{ pitchLabel }} semitonos</template>
            </span>
          </div>
          <div class="btn-row study-pitch">
            <button class="btn mini" type="button" title="Bajar un semitono" :disabled="!track || pitch <= -12" @click="setPitch(pitch - 1)">−</button>
            <span class="study-pitch-n mono" :class="{ on: pitch !== 0 }">{{ pitchLabel }}</span>
            <button class="btn mini" type="button" title="Subir un semitono" :disabled="!track || pitch >= 12" @click="setPitch(pitch + 1)">+</button>
            <button v-if="pitch !== 0" class="btn mini" type="button" title="Volver al tono original" @click="setPitch(0)">original</button>
            <em v-if="!pitchPreserved && speed === 1" class="model-warn">hace falta ffmpeg</em>
          </div>
        </div>

      </section>

      <!-- ===================================================== metronomo -->
      <section class="study-col">
        <h5 class="study-col-title"><Icon n="note" :t="12" /> Metrónomo</h5>
        <div class="study-block study-metro" :class="{ on: metronome.on }">
          <p class="study-metro-status">{{ metroStatus }}</p>
          <div class="study-metro-row">
            <button class="btn mini study-metro-toggle" type="button" :class="{ on: metronome.on }" :disabled="!track"
                    :title="metronome.on ? 'Parar el metrónomo (la canción sigue)' : 'Arrancar el metrónomo, al compás de la canción'"
                    @click="toggleMetronome">
              <Icon :n="metronome.on ? 'pause' : 'play'" :t="12" /> {{ metronome.on ? 'Parar' : 'Clic' }}</button>
            <span class="study-bpm">
              <button class="btn mini" type="button" title="Un pulso menos por minuto (el clic pasa a ir libre)" :disabled="!track" @click="bumpBpm(-1)">−</button>
              <b class="mono">{{ bpmShown || '—' }}</b> <small>bpm</small>
              <button class="btn mini" type="button" title="Un pulso más por minuto (el clic pasa a ir libre)" :disabled="!track" @click="bumpBpm(1)">+</button>
            </span>
            <button v-if="metronome.free && metronome.has_grid" class="btn mini" type="button" title="Volver al tempo de la canción y seguirla" @click="followSong">
              <Icon n="refresh" :t="11" /> el de la canción</button>
          </div>
          <div class="study-metro-row">
            <button class="btn mini" type="button" :title="'Compás: ' + meterShown + ' (pulsa para cambiar)'" :disabled="!track" @click="toggleMeter">{{ meterShown }}</button>
            <button class="btn mini" type="button" title="Si el acento no cae en el 1: el siguiente pulso pasa a ser el 1" :disabled="!metronome.has_grid" @click="shiftOne">
              el 1 es el siguiente</button>
            <button class="btn mini" type="button" :class="{ on: metronome.mult === 1 }" title="El doble de pulsos (si el tempo salió a la mitad)" :disabled="!metronome.has_grid" @click="setMult(1)">×2</button>
            <button class="btn mini" type="button" :class="{ on: metronome.mult === -1 }" title="La mitad de pulsos" :disabled="!metronome.has_grid" @click="setMult(-1)">÷2</button>
          </div>
          <div class="study-metro-row study-metro-vol">
            <Icon n="volume" :t="12" />
            <SliderField :model-value="metronome.volume" :min="0" :max="1" :step="0.05" width="100%" @update:model-value="setMetroVolume" />
          </div>
        </div>
        <p class="study-empty">El pulso y el «1» los detecta DanPlay al abrir la canción; el clic entra en el compás y sigue la velocidad del estudio. Se puede parar y arrancar sin tocar la canción.</p>
      </section>

      <!-- ==================================================== marcadores -->
      <section class="study-col">
        <h5 class="study-col-title"><Icon n="list" :t="12" /> Marcadores
          <small v-if="study.markers.length">pulsa uno para repetir su tramo</small></h5>
        <div class="study-markers">
          <div v-for="m in study.markers" :key="m.t + ':' + (m.end || 0)" class="study-marker"
               :class="{ on: m === selected, span: isSpan(m) }">
            <button type="button" class="study-pick"
                    :title="isSpan(m) ? 'Repetir este tramo (' + markerLabel(m) + ')' : 'Ir a ' + markerLabel(m)"
                    @click="selectMarker(m)">
              <Icon :n="isSpan(m) ? 'repeat' : 'right'" :t="11" class="study-pick-ico" />
              <span class="mono study-pick-time">{{ markerLabel(m) }}</span>
              <span class="study-pick-name">{{ m.label }}</span>
            </button>
            <button type="button" class="field-btn" :title="'Renombrar «' + m.label + '»'" @click="renameMarker(m)">
              <Icon n="pencil" :t="11" /></button>
            <button type="button" class="field-btn" title="Quitar" @click="removeMarker(m)"><Icon n="close" :t="11" /></button>
          </div>
          <p v-if="!study.markers.length" class="study-empty">Elige un tramo en la onda y guárdalo: aquí quedan tus partes con nombre y notas.</p>
        </div>
        <div class="btn-row study-col-foot">
          <button v-if="hasLoop && !loopSaved" class="btn mini" type="button" title="Guardar el tramo que se repite como marcador" @click="saveMarker">
            <Icon n="save" :t="12" /> Guardar tramo</button>
          <button v-else-if="!hasLoop" class="btn mini" type="button" :disabled="!track"
                  title="Marcar este instante (sin tramo elegido)" @click="saveMarker">
            <Icon n="plus" :t="12" /> aquí</button>
        </div>
      </section>

      <!-- ========================================================= notas -->
      <section class="study-col study-notes">
        <h5 class="study-col-title"><Icon n="pencil" :t="12" /> Notas</h5>
        <div class="study-tabs" role="tablist">
          <button type="button" role="tab" class="study-tab" :class="{ on: notesTab === 'song' }" :aria-selected="notesTab === 'song'"
                  @click="notesTab = 'song'">La canción</button>
          <button v-if="selected" type="button" role="tab" class="study-tab" :class="{ on: notesTab === 'marker' }" :aria-selected="notesTab === 'marker'"
                  @click="notesTab = 'marker'">«{{ selected.label }}»</button>
          <span v-else class="study-tab off" title="Elige un marcador para escribirle notas">un marcador</span>
        </div>
        <TextField v-if="notesTab === 'marker' && selected" :key="'m' + selected.t" v-model="selected.notes" multiline :rows="6" width="100%"
                   :placeholder="'qué trabajar en «' + selected.label + '»…'" class="study-notes-field" @update:modelValue="scheduleSave" />
        <TextField v-else v-model="study.notes" multiline :rows="6" width="100%"
                   placeholder="cejilla en 2, entrar tras el redoble…" :disabled="!track" class="study-notes-field"
                   @update:modelValue="scheduleSave" />
      </section>
    </div>
  </div>
</template>
