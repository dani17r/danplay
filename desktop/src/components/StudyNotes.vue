<script setup>
/**
 * Las notas del modo estudio: un solo cuadro, dos pestañas (las de la
 * canción y las del marcador elegido). Una columna de StudyBar.vue.
 */
import { watch } from 'vue'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import { usePlayback } from '../composables/usePlayback.js'
import { useStudyContext } from '../composables/studyContext.js'

const { track } = usePlayback()
const ctx = useStudyContext()
const { study, scheduleSave } = ctx.doc
const { notesTab } = ctx
const { selected } = ctx.markers

// sin marcador elegido, las de la canción
watch(selected, (m) => {
  if (!m) notesTab.value = 'song'
})
</script>

<template>
  <section class="study-col study-notes">
    <h5 class="study-col-title"><Icon n="pencil" :t="12" /> Notas</h5>
    <div class="study-tabs" role="tablist">
      <button
        type="button"
        role="tab"
        class="study-tab"
        :class="{ on: notesTab === 'song' }"
        :aria-selected="notesTab === 'song'"
        @click="notesTab = 'song'"
      >
        La canción
      </button>
      <button
        v-if="selected"
        type="button"
        role="tab"
        class="study-tab"
        :class="{ on: notesTab === 'marker' }"
        :aria-selected="notesTab === 'marker'"
        @click="notesTab = 'marker'"
      >
        «{{ selected.label }}»
      </button>
      <span v-else class="study-tab off" title="Elige un marcador para escribirle notas"
        >un marcador</span
      >
    </div>
    <!-- primero se apunta lo escrito y luego se programa el guardado, que
         copia la ficha en ese momento -->
    <TextField
      v-if="notesTab === 'marker' && selected"
      :key="'m' + selected.t"
      :model-value="selected.notes"
      multiline
      :rows="6"
      width="100%"
      :placeholder="'qué trabajar en «' + selected.label + '»…'"
      maxlength="2000"
      class="study-notes-field"
      @update:model-value="
        (v) => {
          selected.notes = v
          scheduleSave()
        }
      "
    />
    <TextField
      v-else
      :model-value="study.notes"
      multiline
      :rows="6"
      width="100%"
      placeholder="cejilla en 2, entrar tras el redoble…"
      :disabled="!track"
      maxlength="4000"
      class="study-notes-field"
      @update:model-value="
        (v) => {
          study.notes = v
          scheduleSave()
        }
      "
    />
  </section>
</template>
