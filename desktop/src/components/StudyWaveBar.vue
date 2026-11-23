<script setup>
/**
 * La barra de encima de la onda del modo estudio: la canción (sin bajar al
 * reproductor), los tramos (useStudyLoops.js) y qué pasa al acabar, las guías
 * y la vista de la onda (el imán, la mezcla y los canales de los carriles, el
 * zoom). Una parte de StudyBar.vue.
 */
import Icon from './Icon.vue'
import StudyClock from './StudyClock.vue'
import { usePlayback } from '../composables/usePlayback.js'
import { useStudyContext } from '../composables/studyContext.js'
import { isMobileApp } from '../platform.js'

const player = usePlayback()
const { track, playing } = player
const ctx = useStudyContext()
const { timeline } = ctx
const {
  LOOP_ENDS,
  loopEndWanted,
  setLoopEnd,
  pendingA,
  hasLoop,
  toStart,
  nudgeTime,
  stopHere,
  waveHelp,
  loopLabel,
  clearLoop,
  playNow
} = ctx.loops
const { loopSaved, saveMarker } = ctx.markers
const { cueEdit, STOP_FIRST, addCue, autoBusy, autoStage, autoGuide, snapCues, magnetTitle } =
  ctx.cues
const { showMix, showRoutes, tour } = ctx.view
</script>

<template>
  <div
    v-if="!isMobileApp || hasLoop"
    class="study-bar"
    role="toolbar"
    aria-label="La canción, los tramos y la vista de la onda"
  >
    <span v-if="!isMobileApp" class="study-bar-group study-transport">
      <button
        class="btn mini icon-btn"
        type="button"
        aria-label="Al principio"
        :title="hasLoop ? 'Al principio del tramo' : 'Al principio de la canción'"
        :disabled="!track"
        @click="toStart"
      >
        <Icon n="previous" :t="13" />
      </button>
      <button
        class="btn mini icon-btn"
        type="button"
        aria-label="5 segundos atrás"
        title="5 segundos atrás"
        :disabled="!track"
        @click="nudgeTime(-5)"
      >
        <Icon n="back10" :t="13" />
      </button>
      <button
        class="btn mini icon-btn study-play"
        type="button"
        :class="{ on: playing }"
        :aria-label="playing ? 'Pausar' : 'Reproducir'"
        :title="playing ? 'Pausar (espacio)' : 'Reproducir (espacio)'"
        :disabled="!track"
        @click="player.toggle()"
      >
        <!-- en un span: Icon lleva comentarios y la transición no lo animaría -->
        <Transition name="icon-swap" mode="out-in">
          <span :key="playing ? 'pause' : 'play'" class="icon-swap"
            ><Icon :n="playing ? 'pause' : 'play'" :t="14"
          /></span>
        </Transition>
      </button>
      <button
        class="btn mini icon-btn"
        type="button"
        aria-label="Detener"
        :title="
          hasLoop ? 'Detener y volver al principio del tramo' : 'Detener y volver al principio'
        "
        :disabled="!track"
        @click="stopHere"
      >
        <Icon n="stop" :t="13" />
      </button>
      <button
        class="btn mini icon-btn"
        type="button"
        aria-label="5 segundos adelante"
        title="5 segundos adelante"
        :disabled="!track"
        @click="nudgeTime(5)"
      >
        <Icon n="forward10" :t="13" />
      </button>
      <StudyClock />
    </span>
    <span class="study-bar-group study-loop-group">
      <span
        class="study-loop mono"
        :class="{ off: !hasLoop && pendingA == null }"
        :title="hasLoop || pendingA != null ? '' : 'Arrastra sobre la onda para elegir un tramo'"
        ><Icon v-if="!isMobileApp" n="repeat" :t="11" /> {{ loopLabel }}</span
      >
      <Transition name="fade">
        <span v-if="hasLoop" class="study-loop-tools">
          <span class="study-ends" role="radiogroup" aria-label="Al acabar el tramo">
            <span v-if="isMobileApp" class="study-ends-label">Al acabar el tramo</span>
            <button
              v-for="e in LOOP_ENDS"
              :key="e.v"
              type="button"
              role="radio"
              class="btn mini study-end"
              :class="{ on: loopEndWanted === e.v, 'icon-btn': !isMobileApp }"
              :aria-checked="loopEndWanted === e.v"
              :aria-label="e.n"
              :title="'Al acabar: ' + e.n.toLowerCase() + ' · ' + e.title"
              @click="setLoopEnd(e.v)"
            >
              <Icon :n="e.icon" :t="12" /><template v-if="isMobileApp"> {{ e.n }}</template>
            </button>
          </span>
          <button
            class="btn mini"
            :class="{ 'icon-btn': !isMobileApp }"
            type="button"
            aria-label="Ahora"
            title="Ir ya al tramo (al primero, si hay varios) y ponerlo a sonar"
            @click="playNow(0)"
          >
            <Icon n="play" :t="12" /><template v-if="isMobileApp"> Ahora</template>
          </button>
          <button
            v-if="!loopSaved"
            class="btn mini icon-btn study-save-loop"
            type="button"
            aria-label="Guardar"
            title="Guardar como marcador, con nombre y notas (varios tramos, juntos en uno)"
            @click="saveMarker"
          >
            <Icon n="save" :t="12" />
          </button>
          <button
            class="btn mini icon-btn study-clear"
            type="button"
            aria-label="Quitar"
            title="Quitar el tramo (la canción sigue de largo)"
            @click="clearLoop"
          >
            <Icon n="close" :t="12" />
          </button>
        </span>
      </Transition>
    </span>
    <span v-if="!isMobileApp" class="study-bar-group study-view">
      <button
        class="btn mini study-add-cue"
        type="button"
        :disabled="!cueEdit"
        aria-label="Guía"
        :title="
          !track
            ? ''
            : cueEdit
              ? 'Una guía aquí: escríbela (la dice una voz), grábala o importa un audio, y sonará al pasar'
              : STOP_FIRST
        "
        @click="addCue()"
      >
        <Icon n="mic" :t="13" /> Guía
      </button>
      <button
        class="btn mini icon-btn study-auto-guide"
        type="button"
        :class="{ busy: autoBusy }"
        :disabled="!cueEdit || autoBusy"
        aria-label="Guía automática"
        :title="
          !track
            ? ''
            : autoBusy
              ? autoStage || 'Buscando las partes de la canción…'
              : cueEdit
                ? 'Guía automática: DanPlay busca las partes (intro, estrofas, coros, puente, solos…) y las dice justo antes de cada una'
                : STOP_FIRST
        "
        @click="autoGuide"
      >
        <Icon n="sparkles" :t="13" />
      </button>
      <button
        class="btn mini icon-btn study-magnet"
        type="button"
        :class="{ on: snapCues }"
        :aria-pressed="snapCues"
        aria-label="Imán"
        :title="magnetTitle"
        @click="snapCues = !snapCues"
      >
        <Icon n="magnet" :t="14" />
      </button>
      <span class="study-head-sep" aria-hidden="true"></span>
      <button
        class="btn mini icon-btn study-show-mix"
        type="button"
        :class="{ on: showMix }"
        :aria-pressed="showMix"
        aria-label="Mezcla"
        :title="
          showMix
            ? 'Ocultar la mezcla de cada carril (M, S, volumen): más sitio para la onda'
            : 'Ver la mezcla de cada carril: callar, dejar solo y el volumen'
        "
        @click="showMix = !showMix"
      >
        <Icon n="adjust" :t="13" />
      </button>
      <button
        class="btn mini icon-btn study-show-routes"
        type="button"
        :class="{ on: showRoutes }"
        :aria-pressed="showRoutes"
        aria-label="Canales"
        :title="
          showRoutes
            ? 'Ocultar los canales de cada carril (L, R, estéreo o mono)'
            : 'Ver los canales de cada carril: por qué lado sale cada cosa, en estéreo o en mono'
        "
        @click="showRoutes = !showRoutes"
      >
        <Icon n="outputs" :t="13" />
      </button>
      <span class="study-head-sep" aria-hidden="true"></span>
      <button
        class="btn mini icon-btn study-zoom"
        type="button"
        title="Alejar (o Ctrl + rueda sobre la onda)"
        aria-label="Alejar"
        @click="timeline?.zoomBy?.(0.5)"
      >
        <Icon n="zoomOut" :t="13" />
      </button>
      <button
        class="btn mini icon-btn study-zoom"
        type="button"
        title="Acercar (o Ctrl + rueda sobre la onda)"
        aria-label="Acercar"
        @click="timeline?.zoomBy?.(2)"
      >
        <Icon n="zoomIn" :t="13" />
      </button>
      <button
        class="btn mini icon-btn"
        type="button"
        title="Toda la canción a la vista"
        aria-label="Ver toda la canción"
        @click="timeline?.zoomFit?.()"
      >
        <Icon n="fit" :t="13" />
      </button>
      <button
        class="btn mini icon-btn study-help"
        type="button"
        aria-label="Cómo se usa"
        :title="waveHelp + ' · Pulsa para un recorrido por el modo estudio'"
        @click="tour"
      >
        <Icon n="help" :t="14" />
      </button>
    </span>
  </div>
</template>
