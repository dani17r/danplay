<script setup>
/**
 * Los marcadores del modo estudio (useStudyMarkers.js) y, debajo, la lista
 * de las guías habladas (StudyCueList.vue). Una columna de StudyBar.vue.
 */
import Icon from './Icon.vue'
import StudyCueList from './StudyCueList.vue'
import { usePlayback } from '../composables/usePlayback.js'
import { useStudyContext } from '../composables/studyContext.js'
import { isSpan } from '../composables/useStudyDoc.js'

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
</script>

<template>
  <section class="study-col study-marks">
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
      <StudyCueList />
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
