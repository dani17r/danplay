<script setup>
/**
 * «Tempo y onda»: el panel para arreglar un tramo del clic de una vez.
 *
 * El recorrido que se hace con él: elegir un tramo, ESCRIBIR UN TEMPO para
 * él, y pegarlo a la forma de la onda solo si de verdad pega, probando de
 * una manera y de otra hasta que una sea la buena. Todo es una prueba hasta
 * pulsar «Aplicar»: lo que sale se ve en la onda como líneas fantasma y, si se
 * quiere, suena en el clic sin guardarse. Al cancelar (o cerrar, o salir de
 * Ajustar, o cambiar de canción) el clic suena como estaba guardado.
 *
 * El cálculo es `planAlign` (useBeatEdit.js): el tempo con `setTempoSpan` y el
 * pegado con `alignBeats`. Aquí solo están los mandos y lo que dicen.
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import SliderField from './ui/SliderField.vue'
import TextField from './ui/TextField.vue'
import CheckField from './ui/CheckField.vue'
import Icon from './Icon.vue'
import { filled, selectBetween, tempoOf } from '../utils/beatEdit.js'
import { parseBpm } from '../utils/beats.js'
import {
  beatsWord,
  bpmText,
  describePlan,
  findTempo,
  planAlign,
  ratioLabel,
  tempoChoices,
  timeText
} from '../composables/useBeatEdit.js'

const props = defineProps({
  /** Lo que devuelve `useBeatEdit` */
  edit: { type: Object, required: true }
})
const edit = props.edit

const TEMPOS = [
  { v: 'keep', n: 'Mantener', title: 'Dejar el tempo del tramo como está' },
  {
    v: 'set',
    n: 'Poner un tempo',
    title: 'Rehacer los pulsos del tramo a un tempo exacto que pones tú'
  }
]
const HOWS = [
  { v: 'none', n: 'No ajustar', title: 'Dejar los pulsos donde caen (solo el tempo, si lo pones)' },
  {
    v: 'phase',
    n: 'Solo la fase',
    title:
      'Mover todos los pulsos del tramo a la vez, sin torcer el tempo: el peine entero a su sitio'
  },
  {
    v: 'follow',
    n: 'Sigue la onda',
    title:
      'Cada pulso se va al golpe que tiene cerca (con la rigidez, el ritmo se resiste a torcerse)'
  },
  {
    v: 'line',
    n: 'Tempo fijo',
    title: 'La mejor recta: un tempo constante y su fase. Para lo grabado a un clic que se desvía'
  }
]

// ---- el tramo
const span = computed(() => edit.alignSpan)
const own = computed(() => edit.currentOwn())
const inSpan = computed(() => {
  const o = own.value
  const s = span.value
  if (!o || !s) return []
  return selectBetween(o.beats, s[0] - 0.0005, s[1] + 0.0005)
})
const spanText = computed(() => {
  const s = span.value
  if (!s) return ''
  return `${timeText(s[0])} – ${timeText(s[1])} · ${beatsWord(inSpan.value.length)}`
})
const nowBpm = computed(() => {
  const o = own.value
  return o && inSpan.value.length > 1 ? tempoOf(o.beats, filled(inSpan.value)) : 0
})

// ---- el tempo
const tempoMode = ref('keep')
const bpm = ref(0)
const bpmDraft = ref(null)
const anchor = ref(null)
const phaseMs = ref(0)
/**
 * Lo que dijo «Buscar en la onda»: `{main, picked}` (lo que encontró y lo que
 * está puesto: el mismo, o una de sus alternativas), o `{none: true}`.
 */
const found = ref(null)
/** Si cerca de lo escrito no hay golpes: lo que salió buscando sin pista, para ofrecerlo. */
const suggestion = ref(null)
/** Menos de esto de los puntos del peine con golpe es que ahí no hay nada que encontrar. */
const CLEAR = 0.5
const round2 = (v) => Math.round(v * 100) / 100
/** «+12 ms», «−3 ms», «0 ms» */
const signed = (v) => `${v > 0 ? '+' : v < 0 ? '−' : ''}${Math.abs(v)} ms`
const clampBpm = (v) => Math.min(300, Math.max(20, v))
function pickTempo(mode) {
  tempoMode.value = mode
  if (mode === 'set' && !(bpm.value > 0)) bpm.value = Math.round((nowBpm.value || 100) * 10) / 10
}
function setBpm(v) {
  bpm.value = round2(clampBpm(v))
  bpmDraft.value = null
}
function onBpmInput(text) {
  bpmDraft.value = text
  const v = parseBpm(text)
  // vale al momento (se ve la prueba mientras se escribe), con los topes de siempre
  if (v != null && v >= 20 && v <= 300) bpm.value = v
}
function onBpmKey(e) {
  if (e.key === 'Enter') {
    e.preventDefault()
    bpmDraft.value = null
    e.target.blur()
  } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
    e.preventDefault()
    const step = (e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 1 : 0.1)
    setBpm(Math.round((bpm.value + step) * 10) / 10)
  }
}
const bpmShown = computed(() => bpmDraft.value ?? (bpm.value > 0 ? bpmText(bpm.value) : ''))
/** Pone ese tempo y su fase en el campo. */
function take(c) {
  tempoMode.value = 'set'
  bpm.value = c.bpm
  bpmDraft.value = null
  anchor.value = c.phase
  phaseMs.value = 0
}
/** Cerca de lo escrito (o de lo que tiene) se busca; si ahí no hay golpes, a ciegas, y se ofrece. */
async function search() {
  const s = span.value
  if (!s) return
  suggestion.value = null
  await edit.loadHits()
  // lo que se escribió es lo que se espera: lo que se quiere es su tempo exacto y su fase
  const typed = tempoMode.value === 'set' && bpm.value > 0 ? bpm.value : nowBpm.value
  const near = findTempo(edit.hits, s, typed)
  if (near && (near.support >= CLEAR || !(typed > 0))) {
    take(near)
    found.value = { main: near, picked: near }
    return
  }
  const free = typed > 0 ? findTempo(edit.hits, s, 0) : null
  if (!free) {
    found.value = { none: true }
    return
  }
  found.value = null
  suggestion.value = { around: typed, result: free }
}
function useSuggestion() {
  const r = suggestion.value?.result
  if (!r) return
  take(r)
  found.value = { main: r, picked: r }
  suggestion.value = null
}
function pick(c) {
  take(c)
  found.value = { ...found.value, picked: c }
}
const pct = (c) => Math.round((c?.support || 0) * 100)
/** «÷2 · 60 bpm · pegan 97 %» */
const chipText = (c) =>
  `${ratioLabel(foundNow.value.picked.bpm, c.bpm)} · ${bpmText(c.bpm)} bpm · pegan ${pct(c)} %`
/** Lo que se encontró, mientras el campo siga con ese tempo. */
const foundNow = computed(() => {
  const f = found.value
  return f?.picked && Math.abs(bpm.value - f.picked.bpm) < 0.06 ? f : null
})
/** El doble y la mitad (y lo que se encontró, si se saltó a otro) que pegan casi tanto. */
const chips = computed(() => {
  const f = foundNow.value
  return f ? tempoChoices(f.main).filter((c) => c !== f.picked) : []
})
// otro tramo: lo que se había buscado ya no vale
watch(
  () => (span.value ? span.value.map((t) => Math.round(t * 1000)).join() : ''),
  () => {
    anchor.value = null
    phaseMs.value = 0
    found.value = null
    suggestion.value = null
  }
)

// ---- pegarlo a la onda
const how = ref('none')
const reach = ref(50)
const lead = ref(0)
const rigidity = ref(50)
const force = ref(false)

const plan = computed(() => {
  const o = own.value
  const s = span.value
  if (!o || !s || !(s[1] > s[0])) return null
  return planAlign(
    o,
    s,
    {
      tempo: {
        mode: tempoMode.value,
        bpm: bpm.value,
        anchor: anchor.value,
        phaseMs: phaseMs.value
      },
      fit: {
        how: how.value,
        reach: reach.value / 1000,
        rigidity: rigidity.value / 100,
        lead: lead.value,
        force: force.value
      },
      duration: edit.duration
    },
    edit.hits
  )
})
/** La prueba: lo que quedaría, o nada si no cambia nada. */
const trial = computed(() => (plan.value?.changed ? plan.value.own : null))
const loadingHits = computed(
  () => how.value !== 'none' && ['idle', 'loading'].includes(edit.hitsStatus)
)
const result = computed(() =>
  plan.value
    ? describePlan(plan.value, { loading: loadingHits.value, error: edit.hitsError })
    : { text: '', kind: 'info' }
)

// la prueba: se ve siempre, y suena si se quiere. Sin sonar, el motor vuelve a lo guardado
watch(
  [trial, () => edit.hearTrial],
  () => {
    edit.setPreview(trial.value)
    if (trial.value && edit.hearTrial) edit.hearOwn(trial.value)
    else edit.stopSound()
  },
  { immediate: true }
)
onBeforeUnmount(() => edit.endPreview())

function apply() {
  const p = plan.value
  if (!p?.changed) return
  edit.commitPreview(p.own, p.label, p.selection)
}
function cancel() {
  edit.closePanel()
}
</script>

<template>
  <section class="beat-align" role="group" aria-label="Tempo y onda">
    <header class="ba-head">
      <h5 class="ba-title"><Icon n="waveform" :t="12" /> Tempo y onda</h5>
      <span v-if="span" class="ba-span mono" title="El tramo sobre el que se trabaja">
        {{ spanText }}
      </span>
      <span v-else class="ba-span none">
        Elige pulsos (arrastra sobre la franja de arriba) o un tramo A-B para empezar
      </span>
      <span class="ba-fill"></span>
      <button
        class="btn mini icon-btn"
        type="button"
        aria-label="Cerrar"
        title="Cerrar sin aplicar (Esc)"
        @click="cancel"
      >
        <Icon n="close" :t="12" />
      </button>
    </header>

    <div class="ba-grid" :class="{ off: !span }">
      <!-- ============================================== el tempo -->
      <section class="ba-col ba-tempo" aria-label="Tempo">
        <h6 class="ba-legend">Tempo</h6>
        <div class="ba-picks" role="radiogroup" aria-label="Qué tempo">
          <button
            v-for="o in TEMPOS"
            :key="o.v"
            type="button"
            role="radio"
            class="btn mini ba-pick"
            :class="{ on: tempoMode === o.v }"
            :aria-checked="tempoMode === o.v"
            :disabled="!span"
            :title="o.title"
            @click="pickTempo(o.v)"
          >
            {{ o.n }}
          </button>
        </div>
        <div class="ba-row" :class="{ 'ba-dim': tempoMode !== 'set' }">
          <button
            class="btn mini bt-step"
            type="button"
            title="Una décima menos"
            aria-label="Una décima menos"
            :disabled="tempoMode !== 'set'"
            @click="setBpm(bpm - 0.1)"
          >
            −
          </button>
          <TextField
            class="study-bpm-input mono"
            compact
            :model-value="bpmShown"
            :disabled="tempoMode !== 'set'"
            aria-label="Tempo del tramo, en pulsos por minuto"
            title="El tempo exacto del tramo (admite decimales: 76,3) · ↑ ↓ lo mueven de décima en décima, con Mayús de uno en uno"
            @update:model-value="onBpmInput"
            @keydown="onBpmKey"
            @blur="bpmDraft = null"
          />
          <small>bpm</small>
          <button
            class="btn mini bt-step"
            type="button"
            title="Una décima más"
            aria-label="Una décima más"
            :disabled="tempoMode !== 'set'"
            @click="setBpm(bpm + 0.1)"
          >
            +
          </button>
          <button
            class="btn mini bt-step"
            type="button"
            title="La mitad de tempo"
            :disabled="tempoMode !== 'set'"
            @click="setBpm(bpm / 2)"
          >
            ÷2
          </button>
          <button
            class="btn mini bt-step"
            type="button"
            title="El doble de tempo"
            :disabled="tempoMode !== 'set'"
            @click="setBpm(bpm * 2)"
          >
            ×2
          </button>
          <small v-if="nowBpm" class="ba-now mono" title="El tempo que tiene ahora el tramo">
            ahora {{ bpmText(nowBpm) }}
          </small>
        </div>
        <div class="ba-row">
          <button
            class="btn mini"
            type="button"
            :disabled="!span"
            title="Buscar en el audio el tempo que mejor encaja con los golpes de este tramo (y su fase)"
            @click="search"
          >
            <Icon n="search" :t="11" /> Buscar en la onda
          </button>
          <small v-if="found && found.none" class="ba-found warn">
            Sin golpes claros en este tramo
          </small>
          <small
            v-else-if="foundNow"
            class="ba-found mono"
            title="Cuántos puntos del peine caen sobre un golpe"
          >
            {{ bpmText(foundNow.picked.bpm) }} bpm · pegan {{ foundNow.picked.pegan }} de
            {{ foundNow.picked.points }}
          </small>
        </div>
        <div v-if="chips.length" class="ba-chips" aria-label="Otros tempos que también pegan">
          <button
            v-for="c in chips"
            :key="c.bpm"
            type="button"
            class="btn mini ba-chip mono"
            :title="`Poner ${bpmText(c.bpm)} bpm, con su fase (pegan ${c.pegan} de ${c.points} puntos del peine)`"
            @click="pick(c)"
          >
            {{ chipText(c) }}
          </button>
        </div>
        <p v-if="suggestion" class="ba-suggest" aria-live="polite">
          Cerca de {{ bpmText(suggestion.around) }} bpm no hay golpes claros. Buscando sin pista:
          {{ bpmText(suggestion.result.bpm) }} bpm (pegan {{ pct(suggestion.result) }} %)
          <button
            type="button"
            class="btn mini"
            title="Poner ese tempo y su fase"
            @click="useSuggestion"
          >
            Usar este
          </button>
        </p>
        <div :class="{ 'ba-dim': tempoMode !== 'set' }">
          <SliderField
            v-model="phaseMs"
            :min="-60"
            :max="60"
            :step="1"
            label="Fase"
            :value-text="signed(phaseMs)"
            title="Correr el peine entero hacia delante o hacia atrás, para que caiga sobre el golpe"
          />
        </div>
      </section>

      <!-- ====================================== el ajuste a la onda -->
      <section class="ba-col ba-fit" aria-label="Ajuste a la onda">
        <h6 class="ba-legend">Ajuste a la onda</h6>
        <div class="ba-picks" role="radiogroup" aria-label="Cómo pegarlo a la onda">
          <button
            v-for="h in HOWS"
            :key="h.v"
            type="button"
            role="radio"
            class="btn mini ba-pick"
            :class="{ on: how === h.v }"
            :aria-checked="how === h.v"
            :disabled="!span"
            :title="h.title"
            @click="how = h.v"
          >
            {{ h.n }}
          </button>
        </div>
        <div class="ba-sliders" :class="{ 'ba-dim': how === 'none' }">
          <SliderField
            v-model="reach"
            :min="20"
            :max="120"
            :step="5"
            label="Margen"
            :value-text="'±' + reach + ' ms'"
            title="Hasta dónde se busca el golpe de cada pulso"
          />
          <SliderField
            v-model="lead"
            :min="-30"
            :max="30"
            :step="1"
            label="Adelanto"
            :value-text="signed(lead)"
            title="El clic por delante del golpe (menos) o por detrás (más), tras ajustar"
          />
          <SliderField
            v-model="rigidity"
            :min="0"
            :max="100"
            :step="5"
            label="Rigidez"
            :value-text="rigidity + ' %'"
            title="Solo en «Sigue la onda»: cuánto se resiste el ritmo a torcerse (0: sigue todos los golpes)"
          />
        </div>
        <div class="ba-row">
          <span class="ba-lab">Fuente</span>
          <span class="ba-picks" role="radiogroup" aria-label="De dónde salen los golpes">
            <button
              type="button"
              role="radio"
              class="btn mini ba-pick"
              :class="{ on: edit.hitsSource === 'mix' }"
              :aria-checked="edit.hitsSource === 'mix'"
              title="Los golpes de toda la canción"
              @click="edit.setHitsSource('mix')"
            >
              Mezcla
            </button>
            <button
              type="button"
              role="radio"
              class="btn mini ba-pick"
              :class="{ on: edit.hitsSource === 'drums' }"
              :aria-checked="edit.hitsSource === 'drums'"
              :disabled="!edit.hasDrums"
              :title="
                edit.hasDrums
                  ? 'Solo los golpes de la batería separada: más limpios'
                  : 'Esta canción no tiene la batería separada: sepárala en «Pistas»'
              "
              @click="edit.setHitsSource('drums')"
            >
              Batería separada
            </button>
          </span>
          <CheckField
            v-model="force"
            class="ba-check"
            title="Ajustar aunque casi no pegue con la onda"
          >
            Forzar el ajuste
          </CheckField>
        </div>
      </section>

      <!-- ===================================================== el resultado -->
      <section class="ba-col ba-result" aria-label="Resultado">
        <h6 class="ba-legend">Resultado</h6>
        <p class="ba-text" :class="result.kind" aria-live="polite">
          <span v-if="loadingHits" class="ba-spin" aria-hidden="true"></span>
          {{ result.text }}
        </p>
        <CheckField
          class="ba-check"
          :model-value="edit.hearTrial"
          title="Que suene la prueba en el clic, sin guardarla (el clic tiene que estar puesto)"
          @update:model-value="edit.toggle('hearTrial')"
        >
          Oír la prueba
        </CheckField>
        <div class="ba-actions">
          <button
            class="btn mini"
            type="button"
            :disabled="!edit.targetInfo"
            title="Poner el tramo en bucle y que suene"
            @click="edit.hearTarget()"
          >
            <Icon n="play" :t="10" /> Oír el tramo
          </button>
          <span class="ba-fill"></span>
          <button
            class="btn mini"
            type="button"
            title="Dejar el clic como estaba (Esc)"
            @click="cancel"
          >
            Cancelar
          </button>
          <button
            class="btn mini primary"
            type="button"
            :disabled="!plan?.changed"
            title="Guardar esto como tu clic (Ctrl + Z lo deshace)"
            @click="apply"
          >
            Aplicar
          </button>
        </div>
      </section>
    </div>
  </section>
</template>
