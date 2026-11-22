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
 * @property {ReturnType<typeof import('./useStudyLoops.js').useStudyLoops>} loops
 *   los tramos: uno o varios, qué pasa al acabar, el candado, las teclas A y B…
 * @property {ReturnType<typeof import('./useStudyCues.js').useStudyCues>} cues
 *   las guías habladas: sus carriles, ponerlas, moverlas, la guía automática…
 * @property {ReturnType<typeof import('./useStudyStems.js').useStudyStems>} stems
 *   las pistas separadas: las de la canción, su mezcla, separarla, exportar…
 * @property {ReturnType<typeof import('./useStudyClick.js').useStudyClick>} metro
 *   el clic: su rejilla tal como suena, el clic manual (`edit`), el tempo…
 * @property {ReturnType<typeof import('./useClick.js').useClick>} click
 *   el clic de la canción que suena: su rejilla, si carga, y el modo
 * @property {ReturnType<typeof import('./useRouting.js').useRouting>} routing
 *   por dónde suena cada cosa
 * @property {import('vue').Ref<any>} timeline
 *   la línea de tiempo (StudyTimeline): su zoom, ir a una guía…
 * @property {() => boolean} visible  si el estudio se ve (puede estar oculto y seguir puesto)
 * @property {() => Record<string, any>} waveStyleItem  el submenú «Estilo de la onda»
 * @property {StudyView} view  la vista de la onda (StudyBar)
 */

/**
 * @typedef {object} StudyView
 * @property {import('vue').Ref<boolean>} showMix     a la vista, la mezcla de cada carril
 * @property {import('vue').Ref<boolean>} showRoutes  a la vista, los canales de cada carril
 * @property {() => Promise<void>} tour               el recorrido por el modo estudio
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
