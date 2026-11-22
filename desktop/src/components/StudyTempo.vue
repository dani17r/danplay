<script setup>
/**
 * La velocidad y el tono del modo estudio. Una columna de StudyBar.vue: lo
 * que se pone aquí se guarda con la canción.
 */
import { ref, computed, watch } from 'vue'
import Icon from './Icon.vue'
import SliderField from './ui/SliderField.vue'
import { usePlayback } from '../composables/usePlayback.js'
import { useStudyContext } from '../composables/studyContext.js'
import { transposeKey, toneWords, semitoneWords } from '../utils/theory.js'
import { clamp, round2 } from '../utils/num.js'
import { readPref, savePref } from '../utils/prefs.js'

const player = usePlayback()
const { track, speed, pitch } = player
const { study, scheduleSave } = useStudyContext().doc

// Las velocidades de ensayar, a la vista en el estudio. Son menos que las del
// reproductor (usePlayback.js) y todas están en aquella lista.
const SPEEDS = [0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.25]

// ---- velocidad
async function setSpeed(v) {
  const s = Math.round(Math.max(0.25, Math.min(3, Number(v) || 1)) * 100) / 100
  study.value.speed = s
  await player.setSpeed(s)
  scheduleSave()
}
const speedPercent = computed(() => Math.round(speed.value * 100))

// ---- tono, contado como lo cuenta un musico: medio tono es un semitono.
// El cuarto de tono (medio semitono) sirve para ponerse a la par de una
// grabacion que no esta afinada a 440.
const PITCH_STEPS = [
  { v: 0.5, n: '¼ de tono', name: 'un cuarto de tono', title: 'De cuarto en cuarto de tono' },
  { v: 1, n: '½ tono', name: 'medio tono', title: 'De medio en medio tono (un semitono)' },
  { v: 2, n: '1 tono', name: 'un tono', title: 'De tono en tono (dos semitonos)' }
]
const PITCH_STEP_KEY = 'danplay.study.pitchStep'
const savedStep = readPref(PITCH_STEP_KEY, 1)
/** De cuánto en cuánto se mueve el tono, en semitonos. */
const pitchStep = ref(PITCH_STEPS.some((s) => s.v === savedStep) ? savedStep : 1)
watch(pitchStep, (v) => savePref(PITCH_STEP_KEY, v))
const stepName = computed(() => PITCH_STEPS.find((s) => s.v === pitchStep.value)?.name || '')
/** «medio tono arriba», «1 tono abajo», «original». */
const pitchWords = computed(() => toneWords(pitch.value))
const pitchTitle = computed(() =>
  pitch.value ? `${toneWords(pitch.value)} (${semitoneWords(pitch.value)})` : 'Como está grabada'
)
const keyNow = computed(() => (track.value?.key ? transposeKey(track.value.key, pitch.value) : ''))
async function setPitch(n) {
  const p = round2(clamp(Number(n) || 0, -12, 12))
  study.value.pitch = p
  await player.setPitch(p)
  scheduleSave()
}
</script>

<template>
  <section class="study-col">
    <h5 class="study-col-title"><Icon n="forward10" :t="12" /> Velocidad y tono</h5>

    <div class="study-block">
      <div class="study-block-head">
        <span class="study-block-name"><Icon n="forward10" :t="12" /> Velocidad</span>
        <span class="mono study-value"
          >{{ speedPercent }} % <em v-if="speed !== 1">mismo tono</em></span
        >
      </div>
      <div class="btn-row study-speeds">
        <button
          v-for="v in SPEEDS"
          :key="v"
          class="btn mini"
          type="button"
          :class="{ on: Math.abs(speed - v) < 0.005 }"
          :aria-pressed="Math.abs(speed - v) < 0.005"
          :disabled="!track"
          @click="setSpeed(v)"
        >
          {{ v }}×
        </button>
      </div>
      <SliderField
        :model-value="speed"
        :min="0.5"
        :max="1.25"
        :step="0.01"
        width="100%"
        aria-label="Velocidad"
        @update:model-value="setSpeed"
      />
    </div>

    <div class="study-block">
      <div class="study-block-head">
        <span class="study-block-name"><Icon n="chords" :t="12" /> Tono</span>
        <span class="mono study-value">
          <template v-if="track?.key"
            >{{ track.key
            }}<template v-if="pitch">
              → <b>{{ keyNow }}</b></template
            ></template
          >
        </span>
      </div>
      <div class="study-pitch-now" :class="{ on: pitch !== 0 }" :title="pitchTitle">
        <button
          class="btn mini study-pitch-down"
          type="button"
          :title="'Bajar ' + stepName"
          :aria-label="'Bajar ' + stepName"
          :disabled="!track || pitch <= -12"
          @click="setPitch(pitch - pitchStep)"
        >
          −
        </button>
        <span class="study-pitch-words">
          <span class="study-pitch-n">{{ pitch ? pitchWords : 'Como está grabada' }}</span>
          <small v-if="pitch" class="mono">{{ semitoneWords(pitch) }}</small>
        </span>
        <button
          class="btn mini study-pitch-up"
          type="button"
          :title="'Subir ' + stepName"
          :aria-label="'Subir ' + stepName"
          :disabled="!track || pitch >= 12"
          @click="setPitch(pitch + pitchStep)"
        >
          +
        </button>
      </div>
      <div class="btn-row study-pitch">
        <span class="study-steps" role="group" aria-label="De cuánto en cuánto se corre el tono">
          <span class="study-metro-label">Pasos de</span>
          <button
            v-for="s in PITCH_STEPS"
            :key="s.v"
            type="button"
            class="btn mini study-step"
            :class="{ on: pitchStep === s.v }"
            :aria-pressed="pitchStep === s.v"
            :title="s.title"
            @click="pitchStep = s.v"
          >
            {{ s.n }}
          </button>
        </span>
        <button
          v-if="pitch !== 0"
          class="btn mini"
          type="button"
          title="Volver al tono original"
          @click="setPitch(0)"
        >
          original
        </button>
      </div>
    </div>
  </section>
</template>
