<script setup>
/**
 * La lista de las guías habladas (useStudyCues.js), por orden, con su
 * volumen: debajo de los marcadores, en su columna (StudyMarkers.vue).
 */
import Icon from './Icon.vue'
import SliderField from './ui/SliderField.vue'
import { useStudyContext } from '../composables/studyContext.js'
import { isMobileApp } from '../platform.js'
import { formatTime } from '../utils/format.js'
import { pctText } from '../utils/num.js'

const ctx = useStudyContext()
const { study } = ctx.doc
const {
  cueList,
  goToCue,
  previewCue,
  editCue,
  removeCue,
  guideLanes,
  laneOfCue,
  cueEdit,
  STOP_FIRST,
  cueVolume
} = ctx.cues
</script>

<template>
  <template v-if="study.cues?.length">
    <h6 class="study-sub"><Icon n="waveform" :t="11" /> Guías</h6>
    <TransitionGroup name="study-list" tag="div" class="study-cue-list">
      <div v-for="c in cueList" :key="'c' + c.t + c.file" class="study-marker study-cue">
        <button
          type="button"
          class="study-pick"
          :title="'Ir a ' + formatTime(c.t) + ' y verla en su carril'"
          @click="goToCue(c)"
        >
          <Icon :n="c.text ? 'speech' : 'mic'" :t="11" class="study-pick-ico" />
          <span class="mono study-pick-time">{{ formatTime(c.t) }}</span>
          <span class="study-pick-name">{{ c.text || 'audio' }}</span>
          <span
            v-if="guideLanes.length > 1"
            class="study-cue-lane"
            :title="
              'En el carril «' +
              (guideLanes.find((l) => l.key === laneOfCue(c))?.name || 'Guías') +
              '»'
            "
            >{{ guideLanes.find((l) => l.key === laneOfCue(c))?.name || 'Guías' }}</span
          >
        </button>
        <button
          type="button"
          class="field-btn"
          aria-label="Escucharla"
          title="Escucharla"
          @click="previewCue(c)"
        >
          <Icon n="play" :t="11" />
        </button>
        <button
          v-if="!isMobileApp"
          type="button"
          class="field-btn"
          aria-label="Cambiarla"
          :title="cueEdit ? 'Cambiarla' : STOP_FIRST"
          :disabled="!cueEdit"
          @click="editCue(c)"
        >
          <Icon :n="c.text ? 'pencil' : 'cut'" :t="11" />
        </button>
        <button
          v-if="!isMobileApp"
          type="button"
          class="field-btn"
          aria-label="Quitarla"
          :title="cueEdit ? 'Quitarla' : STOP_FIRST"
          :disabled="!cueEdit"
          @click="removeCue(c)"
        >
          <Icon n="close" :t="11" />
        </button>
      </div>
    </TransitionGroup>
    <div class="study-metro-row study-metro-vol">
      <span class="study-metro-label">Guías</span>
      <SliderField
        v-model="cueVolume"
        :min="0"
        :max="3"
        :step="0.05"
        :mark="1"
        width="100%"
        aria-label="Volumen de las guías"
        :value-text="pctText(cueVolume)"
      />
      <span class="study-vol-n mono">{{ pctText(cueVolume) }}</span>
    </div>
  </template>
</template>
