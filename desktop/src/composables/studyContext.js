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
 * @property {ReturnType<typeof import('./useStudyMarkers.js').useStudyMarkers>} markers
 *   los marcadores: el elegido, los de la casilla, elegir, tocar, guardar…
 * @property {StudyLoops} loops  los tramos (StudyBar)
 * @property {ReturnType<typeof import('./useStudyCues.js').useStudyCues>} cues
 *   las guías habladas: sus carriles, ponerlas, moverlas, la guía automática…
 * @property {ReturnType<typeof import('./useStudyStems.js').useStudyStems>} stems
 *   las pistas separadas: las de la canción, su mezcla, separarla, exportar…
 * @property {StudyMetro} metro  el clic (StudyBar)
 * @property {ReturnType<typeof import('./useClick.js').useClick>} click
 *   el clic de la canción que suena: su rejilla, si carga, y el modo
 * @property {ReturnType<typeof import('./useRouting.js').useRouting>} routing
 *   por dónde suena cada cosa
 * @property {import('vue').Ref<any>} timeline
 *   la línea de tiempo (StudyTimeline): su zoom, ir a una guía…
 */

/**
 * @typedef {object} StudyMetro
 * @property {import('vue').ComputedRef<any>} soundingGrid  la rejilla del clic tal como suena
 * @property {import('vue').ComputedRef<any>} shownGrid  la de la onda (con el doble o la mitad)
 * @property {import('vue').ComputedRef<boolean>} hasOwn   la canción tiene su clic manual
 * @property {import('vue').ComputedRef<number[]>} clickBeats  los pulsos del clic (los del imán)
 */

/**
 * @typedef {object} StudyLoops
 * @property {Array<{ v: string, n: string, icon: string, title: string }>} LOOP_ENDS
 * @property {import('vue').Ref<import('../api.js').LoopEnd>} loopEndWanted  qué pasa al acabar el último tramo
 * @property {(v: import('../api.js').LoopEnd) => Promise<void>} setLoopEnd
 * @property {import('vue').Ref<boolean>} multi           varios tramos a la vez
 * @property {import('vue').Ref<number>} lastTouched      el último tramo que se tocó
 * @property {import('vue').Ref<number | null>} pendingA  la A puesta, esperando a la B
 * @property {import('vue').ComputedRef<boolean>} hasLoop
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
