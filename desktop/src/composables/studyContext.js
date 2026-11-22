// @ts-check
// Lo que comparten las partes del modo estudio. StudyBar.vue arma un solo
// objeto con sus composables (useStudyDoc, useStudyMarkers…) y lo da con
// `provide(STUDY, ctx)`; cada parte (StudyNotes, StudyTempo, StudyClick…) lo
// toma con `useStudyContext()`. Con props serían más de cuarenta, y cada
// cambio de una repintaba la parte entera.
//
// Las partes se arman en el mismo orden que cuando todo estaba en StudyBar
// (el orden de los `watch` importa: dos que miran lo mismo avisan por orden
// de creación). Por eso algunas usan cosas de otras que se arman después: esas
// se leen de `ctx` al usarlas, nunca al crearse.
import { inject } from 'vue'

/**
 * @typedef {object} StudyContext
 * @property {ReturnType<typeof import('./useStudyDoc.js').useStudyDoc>} doc
 *   el estudio de la canción y su guardado
 * @property {import('vue').Ref<string>} notesTab
 *   la pestaña de notas: 'song' (las de la canción) o 'marker' (las del elegido)
 * @property {{ selected: import('vue').Ref<any> }} markers
 *   los marcadores: el elegido
 */

/** @type {import('vue').InjectionKey<StudyContext>} */
export const STUDY = Symbol('estudio')

/**
 * El estudio, para una de sus partes (tiene que estar dentro de StudyBar).
 * @returns {StudyContext}
 */
export function useStudyContext() {
  const ctx = inject(STUDY, null)
  if (!ctx) throw new Error('Esta parte del modo estudio va dentro de StudyBar')
  return ctx
}
