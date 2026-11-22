<script setup>
/**
 * Los marcadores del modo estudio (useStudyMarkers.js) y la lista de las
 * guías habladas. Una columna de StudyBar.vue.
 */
import Icon from './Icon.vue'
import SliderField from './ui/SliderField.vue'
import { usePlayback } from '../composables/usePlayback.js'
import { useStudyContext } from '../composables/studyContext.js'
import { isSpan } from '../composables/useStudyDoc.js'
import { isMobileApp } from '../platform.js'
import { formatTime } from '../utils/format.js'
import { pctText } from '../utils/num.js'

const { track } = usePlayback()
const ctx = useStudyContext()
const { study } = ctx.doc
const {
  selected,
  checked,
  markerLabel,
  loopSaved,
  saveMarker,
  selectMarker,
  playMarker,
  toggleChecked,
  checkedList,
  playChecked,
  removeMarker,
  renameMarker
} = ctx.markers
const { LOOP_ENDS, loopEndWanted, setLoopEnd, hasLoop } = ctx.loops
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
  <section class="study-col">
    <h5 class="study-col-title">
      <Icon n="list" :t="12" /> Marcadores
      <small v-if="study.markers.length">marca varios y tócalos seguidos</small>
    </h5>
    <div class="study-markers">
      <div
        v-for="m in study.markers"
        :key="m.t + ':' + (m.end || 0)"
        class="study-marker"
        :class="{ on: m === selected, span: isSpan(m), checked: checked.has(m) }"
      >
        <button
          v-if="isSpan(m)"
          type="button"
          role="checkbox"
          class="study-check"
          :class="{ on: checked.has(m) }"
          :aria-checked="checked.has(m) ? 'true' : 'false'"
          :aria-label="'Elegir «' + m.label + '» para tocarlo con otros'"
          :title="'Elegir «' + m.label + '» para tocarlo seguido con otros'"
          @click="toggleChecked(m)"
        >
          <Icon v-if="checked.has(m)" n="check" :t="10" />
        </button>
        <button
          type="button"
          class="study-pick"
          :title="
            isSpan(m) ? 'Repetir este tramo (' + markerLabel(m) + ')' : 'Ir a ' + markerLabel(m)
          "
          @click="selectMarker(m)"
        >
          <Icon :n="isSpan(m) ? 'repeat' : 'right'" :t="11" class="study-pick-ico" />
          <span class="mono study-pick-time">{{ markerLabel(m) }}</span>
          <span class="study-pick-name">{{ m.label }}</span>
        </button>
        <button
          v-if="isSpan(m)"
          type="button"
          class="field-btn"
          :title="'Tocar «' + m.label + '» ya'"
          @click="playMarker(m)"
        >
          <Icon n="play" :t="11" />
        </button>
        <button
          type="button"
          class="field-btn"
          :title="'Renombrar «' + m.label + '»'"
          @click="renameMarker(m)"
        >
          <Icon n="pencil" :t="11" />
        </button>
        <button type="button" class="field-btn" title="Quitar" @click="removeMarker(m)">
          <Icon n="close" :t="11" />
        </button>
      </div>
      <p v-if="!study.markers.length" class="study-empty">
        Elige un tramo en la onda y guárdalo: aquí quedan tus partes con nombre y notas.
      </p>
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
    </div>
    <div class="btn-row study-col-foot">
      <template v-if="checkedList.length">
        <button
          class="btn mini study-play-checked"
          type="button"
          :title="
            'Tocar seguidos los ' +
            checkedList.length +
            ' elegidos, saltándose lo de en medio (al acabar: ' +
            LOOP_ENDS.find((e) => e.v === loopEndWanted).n.toLowerCase() +
            ')'
          "
          @click="playChecked"
        >
          <Icon n="play" :t="11" /> Tocar {{ checkedList.length }}
          {{ checkedList.length === 1 ? 'elegido' : 'elegidos' }}
        </button>
        <span class="study-ends" role="radiogroup" aria-label="Los elegidos, al acabar">
          <button
            v-for="e in LOOP_ENDS"
            :key="e.v"
            type="button"
            role="radio"
            class="btn mini study-end"
            :class="{ on: loopEndWanted === e.v }"
            :aria-checked="loopEndWanted === e.v"
            :title="e.title"
            @click="setLoopEnd(e.v)"
          >
            <Icon :n="e.icon" :t="11" />
          </button>
        </span>
      </template>
      <button
        v-else-if="hasLoop && !loopSaved"
        class="btn mini"
        type="button"
        title="Guardar el tramo que se repite como marcador"
        @click="saveMarker"
      >
        <Icon n="save" :t="12" /> Guardar tramo
      </button>
      <button
        v-else-if="!hasLoop"
        class="btn mini"
        type="button"
        :disabled="!track"
        title="Marcar este instante (sin tramo elegido)"
        @click="saveMarker"
      >
        <Icon n="plus" :t="12" /> aquí
      </button>
    </div>
  </section>
</template>
