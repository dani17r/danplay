<script setup>
/**
 * Las pistas separadas, en el modo estudio (useStudyStems.js): separarla, o
 * que suenen ellas con su mezcla. Va en dos sitios de StudyBar.vue: con
 * `head`, en la cabecera del escritorio (los botones junto a la canción); sin
 * él, la fila de debajo de la cabecera en el celular. Las dos son una sola
 * cadena de `v-if` para que el DOM sea el mismo que cuando estaban en
 * StudyBar (sin envoltorios ni marcas de más).
 */
import Icon from './Icon.vue'
import { usePlayback } from '../composables/usePlayback.js'
import { useStudyContext } from '../composables/studyContext.js'
import { isMobileApp } from '../platform.js'

defineProps({
  /** La de la cabecera del escritorio (si no, la fila del celular). */
  head: { type: Boolean, default: false }
})
const { track } = usePlayback()
const {
  separation,
  hasStems,
  mixerOn,
  plan,
  progress,
  canSeparate,
  busyWith,
  toggleStems,
  separateThis,
  progressText,
  stemsMenu
} = useStudyContext().stems
</script>

<template>
  <template v-if="head && track && hasStems">
    <button
      type="button"
      class="btn mini study-stems-toggle"
      :class="{ on: mixerOn }"
      :aria-pressed="mixerOn"
      :title="
        mixerOn
          ? 'Suenan las pistas separadas (M calla una, S la deja sola): pulsa para volver a la canción tal cual'
          : 'Que suenen las pistas separadas, cada una con su volumen'
      "
      @click="toggleStems"
    >
      <Icon n="mixer" :t="13" /> Pistas
      <span v-if="mixerOn" class="study-count mono"
        >{{ plan.filter((t) => t.on).length }}/{{ plan.length }}</span
      >
    </button>
    <span v-if="progress" class="study-stems-progress mono" :title="progressText"
      >{{ Math.round((progress.fraction || 0) * 100) }} %</span
    >
    <button
      type="button"
      class="btn mini icon-btn study-stems-more"
      aria-label="Opciones de las pistas"
      title="Que suenen todas, abrir la carpeta, separar otra vez, borrarlas…"
      @click="stemsMenu"
    >
      <Icon n="more" :t="13" />
    </button>
  </template>
  <template v-else-if="head && track && progress">
    <span class="study-stems-busy" :title="progressText">
      <Icon n="mixer" :t="13" />
      <span
        class="study-stems-bar"
        role="progressbar"
        :aria-valuenow="Math.round(progress.fraction * 100)"
        aria-valuemin="0"
        aria-valuemax="100"
        ><span :style="{ width: progress.fraction * 100 + '%' }"></span
      ></span>
      <span class="mono">{{ Math.round((progress.fraction || 0) * 100) }} %</span>
    </span>
    <button
      type="button"
      class="btn mini icon-btn"
      :aria-label="progress.waiting ? 'Quitar' : 'Parar'"
      :title="progress.waiting ? 'Quitarla de la cola' : 'Parar la separación'"
      @click="progress.waiting ? separation.unqueue(track.id) : separation.cancel()"
    >
      <Icon n="close" :t="12" />
    </button>
  </template>
  <button
    v-else-if="head && track && canSeparate"
    type="button"
    class="btn mini study-stems-separate"
    :title="
      busyWith
        ? `Ahora se separa «${busyWith.title}»${busyWith.pct ? ' (' + busyWith.pct + ' %)' : ''}: esta empieza cuando acabe`
        : 'Separar la canción en pistas: batería, voces, bajo… cada una aparte, para callarla o dejarla sola. En unos minutos las tienes, y luego se mejoran'
    "
    @click="separateThis"
  >
    <Icon n="mixer" :t="13" /> {{ busyWith ? 'Poner en la cola' : 'Separar en pistas' }}
  </button>
  <div
    v-else-if="!head && isMobileApp && track && (hasStems || canSeparate || progress)"
    class="study-stems"
  >
    <template v-if="hasStems">
      <button
        type="button"
        class="btn mini study-stems-toggle"
        :class="{ on: mixerOn }"
        :aria-pressed="mixerOn"
        :title="
          mixerOn
            ? 'Suenan las pistas separadas: pulsa para volver a la canción tal cual'
            : 'Que suenen las pistas separadas, cada una con su volumen'
        "
        @click="toggleStems"
      >
        <Icon n="mixer" :t="12" /> Pistas
      </button>
      <span class="study-stems-note">
        <template v-if="mixerOn">
          {{ plan.filter((t) => t.on).length }} de {{ plan.length }} suenan · M calla, S deja sola
        </template>
        <template v-else>{{ plan.length }} pistas separadas</template>
      </span>
      <span v-if="progress" class="study-stems-progress mono">{{ progressText }}</span>
      <button
        type="button"
        class="btn mini study-stems-more"
        :title="
          isMobileApp
            ? 'Que suenen todas, como vienen'
            : 'Que suenen todas, abrir la carpeta, separar otra vez, borrarlas…'
        "
        aria-label="Opciones de las pistas"
        @click="stemsMenu"
      >
        ⋯
      </button>
    </template>
    <template v-else-if="progress">
      <Icon n="mixer" :t="12" />
      <span class="study-stems-progress mono">{{ progressText }}</span>
      <span
        class="study-stems-bar"
        role="progressbar"
        :aria-valuenow="Math.round(progress.fraction * 100)"
        aria-valuemin="0"
        aria-valuemax="100"
        ><span :style="{ width: progress.fraction * 100 + '%' }"></span
      ></span>
      <button
        type="button"
        class="btn mini"
        :title="progress.waiting ? 'Quitarla de la cola' : 'Parar la separación'"
        @click="progress.waiting ? separation.unqueue(track.id) : separation.cancel()"
      >
        <Icon n="close" :t="11" /> {{ progress.waiting ? 'Quitar' : 'Parar' }}
      </button>
    </template>
    <template v-else>
      <button
        type="button"
        class="btn mini study-stems-separate"
        :title="
          busyWith
            ? 'Ponerla en la cola: se separa cuando acabe la que se está separando ahora'
            : 'Separar la canción en pistas (batería, voces, bajo…) para callar o dejar sola cada una'
        "
        @click="separateThis"
      >
        <Icon n="mixer" :t="12" /> {{ busyWith ? 'Poner en la cola' : 'Separar en pistas' }}
      </button>
      <span class="study-stems-note">
        <template v-if="busyWith">
          Ahora se separa «{{ busyWith.title }}»<template v-if="busyWith.pct">
            ({{ busyWith.pct }} %)</template
          >: esta empieza cuando acabe<template v-if="busyWith.waiting">
            (y las {{ busyWith.waiting }} que esperan)</template
          >
        </template>
        <template v-else>
          batería, voces, bajo… cada una aparte · en unos minutos, y luego se mejoran
        </template>
      </span>
    </template>
  </div>
</template>
