<script setup>
/**
 * Las herramientas para ajustar el clic de muchos pulsos a la vez. Salen bajo
 * la onda mientras se ajustan los pulsos (solo en el escritorio).
 *
 * Primera fila: deshacer y rehacer, qué hay elegido y cómo elegir más. La
 * segunda, lo que se hace con ello: correrlo, repartirlo parejo, doblarlo,
 * quitarlo… y «Tempo y onda…», que abre el panel para escribir un tempo y
 * pegarlo a la forma de la onda. Cada herramienta es UN paso de deshacer.
 *
 * Todo actúa sobre lo ELEGIDO o, sin elegir nada, sobre los pulsos del tramo
 * (A-B). Todo el estado y las teclas viven en `useBeatEdit`.
 */
import { computed, ref } from 'vue'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'
import BeatAlign from './BeatAlign.vue'
import { beatsWord, bpmText, timeText } from '../composables/useBeatEdit.js'
import { usePlayback } from '../composables/usePlayback.js'

const props = defineProps({
  /** Lo que devuelve `useBeatEdit` */
  edit: { type: Object, required: true },
  /** El estudio tiene un tramo (A-B) */
  hasLoop: Boolean
})

// por dónde va la canción (para «Hasta el final» sin nada elegido), leída al
// pulsar: como prop, las herramientas se repintaban con cada aviso de Rust
const player = usePlayback()
const info = computed(() => props.edit.targetInfo)
const count = computed(() => info.value?.count ?? 0)
const chosen = computed(() => props.edit.selection.length)

/** Lo que dice cuando no hay con qué trabajar: en el título de cada herramienta. */
const PICK = 'Elige pulsos (arrastra sobre la franja de arriba, o usa «Todos») o un tramo A-B'
const hint = (ok, text) => (ok ? text : `${text} · ${PICK}`)

/** «12 pulsos · 1:05,2 – 1:12,0 · 76,3 bpm» */
const summary = computed(() => {
  const i = info.value
  if (!i) return ''
  const bpm = i.bpm ? ` · ${bpmText(i.bpm)} bpm` : ''
  return `${beatsWord(i.count)} · ${timeText(i.from)} – ${timeText(i.to)}${bpm}`
})

// ---- correr
const shiftText = ref('')
function shiftTyped() {
  const v = Number(String(shiftText.value).trim().replace(',', '.'))
  if (!Number.isFinite(v) || v === 0) return
  props.edit.moveBy(v)
  shiftText.value = ''
}
const SHIFTS = [-10, -1, 1, 10]
const shiftTitle = (v) =>
  hint(
    count.value > 0,
    `${v < 0 ? 'Adelantar' : 'Retrasar'} ${count.value > 0 ? beatsWord(count.value) : 'los pulsos'} ${Math.abs(v)} ms`
  )

// ---- el «1» cada…
const BAR_CHOICES = [2, 3, 4, 5, 6, 7, 8].map((n) => ({ v: n, n: String(n) }))
const barsN = ref(4)

const isOn = (v) => !!v
</script>

<template>
  <div class="beat-tools" role="group" aria-label="Herramientas para ajustar los pulsos">
    <div class="bt-row">
      <button
        class="btn mini icon-btn"
        type="button"
        :disabled="!edit.canUndo"
        :title="edit.canUndo ? `Deshacer: ${edit.undoLabel} (Ctrl+Z)` : 'No hay nada que deshacer'"
        aria-label="Deshacer"
        @click="edit.undo()"
      >
        <Icon n="undo" :t="13" />
      </button>
      <button
        class="btn mini icon-btn"
        type="button"
        :disabled="!edit.canRedo"
        :title="
          edit.canRedo ? `Rehacer: ${edit.redoLabel} (Ctrl+Mayús+Z)` : 'No hay nada que rehacer'
        "
        aria-label="Rehacer"
        @click="edit.redo()"
      >
        <Icon n="redo" :t="13" />
      </button>
      <span class="bt-summary" :class="{ none: !chosen }" aria-live="polite">
        <template v-if="chosen">{{ summary }}</template>
        <template v-else-if="info"> Sin elegir: se usa el tramo · {{ summary }} </template>
        <template v-else>Ningún pulso elegido: arrastra sobre la franja de arriba</template>
      </span>
      <span class="bt-group" role="group" aria-label="Elegir pulsos">
        <button
          class="btn mini"
          type="button"
          title="Elegir todos los pulsos de la canción (Ctrl + A)"
          @click="edit.selectAll()"
        >
          Todos
        </button>
        <button
          class="btn mini"
          type="button"
          :disabled="!chosen"
          title="No elegir ninguno (Esc)"
          @click="edit.selectNone()"
        >
          Ninguno
        </button>
        <button
          class="btn mini"
          type="button"
          :disabled="!hasLoop"
          :title="
            hasLoop
              ? 'Elegir los pulsos que caen dentro del tramo A-B'
              : 'Elige antes un tramo (arrastra sobre la onda, o las teclas A y B)'
          "
          @click="edit.selectLoop()"
        >
          El tramo
        </button>
        <button
          class="btn mini"
          type="button"
          title="Elegir desde el primero elegido (o desde donde va la canción) hasta el final"
          @click="edit.selectToEnd(player.position.value)"
        >
          Hasta el final
        </button>
        <button
          class="btn mini"
          type="button"
          :disabled="!count"
          :title="hint(count > 0, 'Poner estos pulsos en bucle y que suene el clic sobre ellos')"
          @click="edit.hearTarget()"
        >
          <Icon n="play" :t="10" /> Oír el tramo
        </button>
      </span>
      <span class="bt-group bt-views" role="group" aria-label="Qué se ve en la onda">
        <button
          class="btn mini"
          type="button"
          :class="{ on: edit.showHits }"
          :aria-pressed="isOn(edit.showHits)"
          title="Marcar en la onda dónde golpea el audio (para ver si el clic cae sobre el golpe)"
          @click="edit.toggle('showHits')"
        >
          <Icon n="waveform" :t="11" /> Golpes
        </button>
        <button
          class="btn mini"
          type="button"
          :class="{ on: edit.snapHits }"
          :aria-pressed="isOn(edit.snapHits)"
          title="Imán: al arrastrar un pulso, se pega al golpe del audio más cercano"
          @click="edit.toggle('snapHits')"
        >
          <Icon n="magnet" :t="11" /> Imán
        </button>
        <button
          class="btn mini"
          type="button"
          :class="{ on: edit.ghostAuto }"
          :aria-pressed="isOn(edit.ghostAuto)"
          title="Ver, tenue, dónde ponía los pulsos el clic automático"
          @click="edit.toggle('ghostAuto')"
        >
          <Icon n="eye" :t="11" /> Ver el automático
        </button>
      </span>
    </div>

    <div class="bt-row">
      <span class="bt-group bt-shift" role="group" aria-label="Correr los pulsos">
        <span class="bt-label">Desplazar</span>
        <TextField
          v-model="shiftText"
          class="bt-ms mono"
          compact
          placeholder="ms"
          aria-label="Milisegundos que correr (negativo: antes)"
          :disabled="!count"
          :title="
            hint(count > 0, 'Escribe los milisegundos y pulsa Intro (con signo menos, antes)')
          "
          @keydown.enter.prevent="shiftTyped"
        />
        <button
          v-for="v in SHIFTS"
          :key="v"
          class="btn mini bt-step mono"
          type="button"
          :disabled="!count"
          :title="shiftTitle(v)"
          :aria-label="`${v < 0 ? 'Adelantar' : 'Retrasar'} ${Math.abs(v)} ms`"
          @click="edit.moveBy(v)"
        >
          {{ v > 0 ? '+' + v : '−' + Math.abs(v) }}
        </button>
      </span>
      <span class="bt-group" role="group" aria-label="Cambiar el reparto de los pulsos">
        <button
          class="btn mini"
          type="button"
          :disabled="count < 3"
          :title="
            hint(
              count >= 3,
              'Repartir parejo: la misma distancia entre todos, del primero al último'
            )
          "
          @click="edit.spread()"
        >
          Repartir parejo
        </button>
        <button
          class="btn mini"
          type="button"
          :disabled="count < 2"
          :title="hint(count >= 2, 'Doble de pulsos: uno más en medio de cada dos')"
          @click="edit.doubleUp()"
        >
          Doble de pulsos
        </button>
        <button
          class="btn mini"
          type="button"
          :disabled="count < 3"
          :title="hint(count >= 3, 'Mitad: uno de cada dos (se queda el que tiene el «1»)')"
          @click="edit.halve()"
        >
          Mitad
        </button>
      </span>
      <span class="bt-group" role="group" aria-label="El «1» de cada compás">
        <button
          class="btn mini"
          type="button"
          :disabled="!count"
          :title="
            hint(count > 0, `Poner el «1» en el primer pulso y en uno de cada ${barsN} desde ahí`)
          "
          @click="edit.everyN(barsN)"
        >
          El «1» cada
        </button>
        <span class="bt-bars" title="Cada cuántos pulsos va el «1»">
          <SelectField v-model="barsN" :options="BAR_CHOICES" width="62px" />
        </span>
      </span>
      <span class="bt-group" role="group" aria-label="Quitar o deshacer lo ajustado">
        <button
          class="btn mini bt-danger"
          type="button"
          :disabled="!count"
          :title="hint(count > 0, 'Quitar los pulsos elegidos (Supr): Ctrl + Z los devuelve')"
          @click="edit.remove()"
        >
          Quitar pulsos
        </button>
        <button
          class="btn mini"
          type="button"
          :disabled="!edit.alignSpan"
          :title="
            hint(
              !!edit.alignSpan,
              'Volver al automático en el tramo: lo que ajustaste ahí se deshace'
            )
          "
          @click="edit.revertTarget()"
        >
          Volver al automático en el tramo
        </button>
      </span>
      <button
        class="btn mini bt-align"
        type="button"
        :class="{ on: edit.panelOpen }"
        :aria-pressed="isOn(edit.panelOpen)"
        :aria-expanded="isOn(edit.panelOpen)"
        title="Escribir un tempo para el tramo y pegarlo a la forma de la onda, probando antes de aplicar"
        @click="edit.togglePanel()"
      >
        <Icon n="waveform" :t="12" /> Tempo y onda…
      </button>
    </div>

    <p class="study-hint bt-hint">
      <b>Clic</b> en un pulso lo elige · <b>Ctrl + clic</b> suma o quita · <b>Mayús + clic</b> elige
      hasta ahí · <b>arrastra sobre la franja de arriba</b> para elegir una zona · arrastrar un
      pulso elegido los lleva todos · las asas de los extremos estiran · <b>doble clic</b> añade ·
      <b>clic derecho</b> quita o hace el «1» · <b>flechas</b> los corren 5 ms (Mayús 1, Ctrl 25) ·
      <b>Ctrl + Z</b> deshace
    </p>

    <BeatAlign v-if="edit.panelOpen" :edit="edit" />
  </div>
</template>
