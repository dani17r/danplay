<script setup>
/**
 * El clic del modo estudio (useStudyClick.js): el metrónomo, su tempo, de
 * dónde sale (automático, manual o tempo fijo), ajustar los pulsos sobre la
 * onda, re-analizarlo, los volúmenes, el compás y el sonido. Una columna de
 * StudyBar.vue.
 */
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SliderField from './ui/SliderField.vue'
import { usePlayback, MAX_VOLUME } from '../composables/usePlayback.js'
import { useStudyContext } from '../composables/studyContext.js'
import { isMobileApp } from '../platform.js'
import { METERS } from '../utils/beats.js'
import { pctText } from '../utils/num.js'

const { track, metronome, volume } = usePlayback()
const ctx = useStudyContext()
const clickAuto = ctx.click.auto
const clickMode = ctx.click.mode
const clickWanted = ctx.click.wanted
const grid = ctx.click.grid
const {
  CLICK_MODES,
  modeTitle,
  chooseMode,
  bpmDraft,
  bpmShown,
  clickPercent,
  clickLoading,
  hasOwn,
  fitTitle,
  metroStatus,
  toggleMetronome,
  bumpBpm,
  onBpmFocus,
  onBpmInput,
  commitBpm,
  onBpmKey,
  followSong,
  setMeter,
  shiftOne,
  setMult,
  multTitle,
  setMetroVolume,
  editingBeats,
  startEditing,
  reanalyze,
  resetOwn,
  CLICK_MAX,
  setSongVolume,
  clickMore,
  COUNT_INS,
  setCountIn,
  CLICK_SOUNDS,
  setClickSound
} = ctx.metro
</script>

<template>
  <section class="study-col study-click">
    <h5 class="study-col-title"><Icon n="note" :t="12" /> Clic</h5>
    <div class="study-block study-metro" :class="{ on: clickWanted }">
      <div class="study-metro-row">
        <button
          class="btn study-metro-toggle"
          type="button"
          :class="{ on: clickWanted, loading: clickLoading }"
          :aria-pressed="clickWanted ? 'true' : 'false'"
          :disabled="!track || clickLoading"
          :title="
            clickLoading
              ? 'Cargando el clic de esta canción: en cuanto esté, se puede poner'
              : clickWanted
                ? 'Parar el metrónomo (la canción sigue)'
                : clickAuto
                  ? 'Arrancar el metrónomo, al compás de la canción'
                  : 'Arrancar el metrónomo, al tempo puesto'
          "
          @click="toggleMetronome"
        >
          <Icon :n="clickWanted ? 'pause' : 'play'" :t="12" />
          {{ clickLoading ? `Cargando… ${clickPercent} %` : clickWanted ? 'Parar' : 'Clic' }}
        </button>
        <span class="study-bpm">
          <button
            class="btn mini study-bpm-step"
            type="button"
            title="Un pulso menos por minuto (el clic pasa a tempo a mano)"
            :disabled="!track"
            @click="bumpBpm(-1)"
          >
            −
          </button>
          <!-- la «x» (con tempo a mano) lo vacia: vuelve al de la cancion -->
          <TextField
            class="study-bpm-input mono"
            compact
            :clearable="clickAuto && metronome.free && metronome.has_grid"
            :model-value="bpmDraft ?? bpmShown"
            :disabled="!track"
            aria-label="Tempo del clic, en pulsos por minuto"
            title="Escribe el tempo (admite decimales: 90.7) · ↑ ↓ lo mueven de décima en décima, con Mayús de uno en uno"
            @update:model-value="onBpmInput"
            @focus="onBpmFocus"
            @keydown="onBpmKey"
            @blur="commitBpm"
          />
          <small>bpm</small>
          <button
            class="btn mini study-bpm-step"
            type="button"
            title="Un pulso más por minuto (el clic pasa a tempo a mano)"
            :disabled="!track"
            @click="bumpBpm(1)"
          >
            +
          </button>
        </span>
      </div>
      <p class="study-metro-status" aria-live="polite" :title="fitTitle || null">
        {{ metroStatus }}
      </p>
      <div class="study-metro-row study-modes" role="radiogroup" aria-label="De dónde sale el clic">
        <button
          v-for="m in CLICK_MODES"
          :key="m.v"
          type="button"
          role="radio"
          class="btn mini study-mode"
          :class="{ on: clickMode === m.v, has: m.v === 'own' && hasOwn }"
          :aria-checked="clickMode === m.v"
          :title="modeTitle(m)"
          @click="chooseMode(m.v)"
        >
          {{ m.n }}
        </button>
      </div>
      <div v-if="clickAuto" class="study-metro-row">
        <button
          class="btn mini study-mult"
          type="button"
          :class="{ on: metronome.mult === 1 }"
          :aria-pressed="metronome.mult === 1"
          :title="multTitle(1)"
          :disabled="!track"
          @click="setMult(1)"
        >
          ×2
        </button>
        <button
          class="btn mini study-mult"
          type="button"
          :class="{ on: metronome.mult === -1 }"
          :aria-pressed="metronome.mult === -1"
          :title="multTitle(-1)"
          :disabled="!track"
          @click="setMult(-1)"
        >
          ÷2
        </button>
        <button
          class="btn mini"
          type="button"
          title="Si el acento no cae en el 1: el siguiente pulso pasa a ser el 1"
          :disabled="!metronome.has_grid || metronome.meter === 0"
          @click="shiftOne"
        >
          el 1 es el siguiente
        </button>
        <button
          v-if="metronome.free && metronome.has_grid"
          class="btn mini"
          type="button"
          title="Volver al tempo de la canción y seguirla"
          @click="followSong"
        >
          <Icon n="refresh" :t="11" /> el de la canción
        </button>
      </div>
      <div v-if="clickAuto && !isMobileApp" class="study-metro-row">
        <button
          class="btn mini study-edit-beats"
          type="button"
          :class="{ on: editingBeats }"
          :aria-pressed="editingBeats"
          :disabled="!track || !grid"
          :title="
            hasOwn
              ? 'Mover los pulsos de esta canción sobre la onda (Manual): lo que muevas se guarda con ella'
              : 'Mover los pulsos sobre la onda para hacer tu clic manual de esta canción: pasa a Manual y parte del automático'
          "
          @click="startEditing"
        >
          <Icon n="pencil" :t="11" />
          {{
            editingBeats
              ? 'Listo'
              : clickMode === 'own' && !hasOwn
                ? 'Empezar el mío'
                : 'Ajustar pulsos'
          }}
        </button>
        <button
          class="btn mini"
          type="button"
          :disabled="!track"
          :title="
            clickMode === 'own' && hasOwn
              ? 'Volver a analizar el clic respetando lo que moviste tú: lo demás se rehace con el análisis nuevo'
              : hasOwn
                ? 'Volver a analizar el clic automático (tu clic manual no se toca)'
                : 'Volver a analizar el clic: arreglar huecos y saltos, tempo fijo, con la batería separada, solo el tramo…'
          "
          @click="reanalyze"
        >
          <Icon n="sparkles" :t="11" /> Re-analizar…
        </button>
        <button
          v-if="hasOwn"
          class="btn mini"
          type="button"
          title="Borrar los pulsos que moviste a mano en esta canción: queda el automático"
          @click="resetOwn"
        >
          <Icon n="close" :t="11" /> Borrar el manual
        </button>
      </div>
      <div class="study-metro-row study-metro-vol">
        <span class="study-metro-label">Clic</span>
        <SliderField
          :model-value="metronome.volume"
          :min="0"
          :max="CLICK_MAX"
          :step="0.05"
          :mark="1"
          width="100%"
          aria-label="Volumen del clic"
          :value-text="pctText(metronome.volume)"
          title="El volumen del clic: hasta el triple de fuerte"
          @update:model-value="setMetroVolume"
        />
        <span class="study-vol-n mono">{{ pctText(metronome.volume) }}</span>
      </div>
      <div class="study-metro-row study-song-vol">
        <span class="study-metro-label">Canción</span>
        <SliderField
          :model-value="volume"
          :min="0"
          :max="MAX_VOLUME"
          :step="0.01"
          :mark="1"
          width="100%"
          aria-label="Volumen de la canción"
          :value-text="pctText(volume)"
          title="El volumen de la canción: hasta un 50 % más de como viene"
          @update:model-value="setSongVolume"
        />
        <span class="study-vol-n mono">{{ pctText(volume) }}</span>
      </div>
      <button
        type="button"
        class="study-more-toggle"
        :aria-expanded="clickMore"
        @click="clickMore = !clickMore"
      >
        <Icon :n="clickMore ? 'up' : 'down'" :t="11" />
        {{ clickMore ? 'Menos' : 'Compás, cuenta y sonido' }}
      </button>
      <template v-if="clickMore">
        <div class="study-metro-row study-meters" role="group" aria-label="Compás">
          <span class="study-metro-label">Compás</span>
          <button
            v-for="m in METERS"
            :key="m.v"
            class="btn mini"
            type="button"
            :class="{ on: metronome.meter === m.v }"
            :aria-pressed="metronome.meter === m.v"
            :title="m.title + (grid && grid.meter === m.v ? ' · el que detectó DanPlay' : '')"
            :disabled="!track"
            @click="setMeter(m.v)"
          >
            {{ m.n }}
          </button>
        </div>
        <div
          v-if="isMobileApp"
          class="study-metro-row study-meters"
          role="group"
          aria-label="Cuenta al empezar"
        >
          <span class="study-metro-label">Cuenta</span>
          <button
            v-for="c in COUNT_INS"
            :key="c.v"
            class="btn mini"
            type="button"
            :class="{ on: (metronome.count_in || 0) === c.v }"
            :aria-pressed="(metronome.count_in || 0) === c.v"
            :title="c.title"
            @click="setCountIn(c.v)"
          >
            {{ c.n }}
          </button>
        </div>
        <div class="study-metro-row study-meters" role="group" aria-label="Sonido del clic">
          <span class="study-metro-label">Sonido</span>
          <button
            v-for="c in CLICK_SOUNDS"
            :key="c.v"
            class="btn mini"
            type="button"
            :class="{ on: (metronome.sound || 'clasico') === c.v }"
            :aria-pressed="(metronome.sound || 'clasico') === c.v"
            :title="c.title"
            @click="setClickSound(c.v)"
          >
            {{ c.n }}
          </button>
        </div>
      </template>
    </div>
  </section>
</template>
