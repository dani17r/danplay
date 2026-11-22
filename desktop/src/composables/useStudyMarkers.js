// @ts-check
// Los marcadores del modo estudio (StudyBar.vue, StudyMarkers.vue): tramos
// con nombre y notas (uno, o varios seguidos), o un instante suelto. Elegir
// uno pone sus tramos; con la casilla puesta, varios suenan seguidos.
import { ref, computed } from 'vue'
import { usePlayback } from './usePlayback.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'
import { isSpan } from './useStudyDoc.js'
import { formatTime } from '../utils/format.js'
import { round2 } from '../utils/num.js'
import { cleanSegments, sameSegments } from '../utils/segments.js'
import { markerParts as partsOf } from '../utils/cues.js'

/**
 * Se arma antes de leer la canción (`loadFor` quita el elegido nada más
 * empezar). Los tramos (`ctx.loops`) se arman después: se leen al usarlos.
 * @param {import('./studyContext.js').StudyContext} ctx
 */
export function useStudyMarkers(ctx) {
  const player = usePlayback()
  const { track, position, playing } = player
  const { study, scheduleSave } = ctx.doc
  const { notesTab } = ctx
  /** El marcador elegido (uno de `study.markers`), o null. */
  const selected = ref(null)
  /** Los marcadores con la casilla puesta: para tocar varios seguidos. */
  const checked = ref(/** @type {Set<any>} */ (new Set()))

  /** El marcador que son exactamente esos tramos, si lo hay. @param {any} list */
  function markerOf(list) {
    if (!list?.length) return null
    return (
      study.value.markers.find((m) => {
        const parts = partsOf(m)
        return parts.length > 0 && sameSegments(parts, list)
      }) || null
    )
  }
  /** @param {any} m */
  function markerLabel(m) {
    const parts = partsOf(m)
    if (parts.length > 1) return `${parts.length} tramos · ${formatTime(parts[0][0])}…`
    return isSpan(m) ? `${formatTime(m.t)} – ${formatTime(m.end)}` : formatTime(m.t)
  }
  const loopSaved = computed(() => ctx.loops.hasLoop.value && !!markerOf(study.value.loops))

  function saveMarker() {
    if (!track.value) return
    /** @type {Record<string, any>} */
    let m
    const list = study.value.loops
    if (list.length) {
      const dup = markerOf(list)
      if (dup) {
        selected.value = dup
        return notify(`Ese tramo ya es «${dup.label}»`, 'info')
      }
      const n = study.value.markers.filter(isSpan).length + 1
      m = { t: list[0][0], end: list.at(-1)[1], label: `Tramo ${n}`, notes: '' }
      // varios tramos se guardan juntos: un solo marcador que los repite seguidos
      if (list.length > 1) m.parts = list.map((x) => [...x])
    } else {
      const n = study.value.markers.filter((x) => !isSpan(x)).length + 1
      m = { t: round2(position.value), label: `Marca ${n}`, notes: '' }
    }
    study.value.markers = [...study.value.markers, m].sort((a, b) => a.t - b.t)
    selected.value = m
    notesTab.value = 'marker'
    scheduleSave()
  }
  /**
   * Elige un marcador: pone sus tramos y, si `jump`, coloca la cancion al
   * principio. Desde la onda con candado, sonando, no salta: el tramo entra
   * cuando la cancion llega.
   * @param {any} m
   */
  async function selectMarker(m, { jump = true } = {}) {
    const { pendingA, multi, lastTouched, loopEndWanted } = ctx.loops
    selected.value = m
    notesTab.value = 'marker'
    const parts = partsOf(m)
    if (parts.length) {
      pendingA.value = null
      if (parts.length > 1) multi.value = true
      study.value.loops = cleanSegments(parts)
      lastTouched.value = study.value.loops.length - 1
      await player.setLoops(study.value.loops, { end: loopEndWanted.value })
      scheduleSave()
    }
    if (jump) await player.seek(m.t)
  }
  /** Un marcador, a sonar ya (desde su principio). @param {any} m */
  async function playMarker(m) {
    await selectMarker(m)
    if (!playing.value) await player.toggle()
  }
  /** @param {any} m */
  function toggleChecked(m) {
    const next = new Set(checked.value)
    if (next.has(m)) next.delete(m)
    else next.add(m)
    checked.value = next
  }
  const checkedList = computed(() =>
    study.value.markers.filter((m) => checked.value.has(m) && partsOf(m).length)
  )
  /**
   * Los marcadores con la casilla puesta, seguidos (saltándose lo de entre
   * medias), desde el primero: una vez o en bucle, según «Al acabar».
   */
  async function playChecked() {
    const { pendingA, multi, lastTouched, loopEndWanted } = ctx.loops
    const list = checkedList.value
    if (!list.length) return
    const parts = cleanSegments(list.flatMap((m) => partsOf(m)))
    selected.value = list.length === 1 ? list[0] : null
    pendingA.value = null
    multi.value = parts.length > 1
    study.value.loops = parts
    lastTouched.value = parts.length - 1
    await player.setLoops(parts, { end: loopEndWanted.value })
    scheduleSave()
    await player.seek(parts[0][0])
    if (!playing.value) await player.toggle()
  }
  /** @param {any} m */
  function removeMarker(m) {
    if (selected.value === m) {
      selected.value = null
      notesTab.value = 'song'
    }
    if (checked.value.has(m)) toggleChecked(m)
    study.value.markers = study.value.markers.filter((x) => x !== m)
    scheduleSave()
  }
  /** @param {any} m */
  async function renameMarker(m) {
    const label = await ask({
      kind: 'prompt',
      title: 'Nombre del marcador',
      value: m.label,
      okLabel: 'Guardar'
    })
    if (!label) return
    m.label = label.slice(0, 60)
    scheduleSave()
  }
  /** @param {number} t */
  const jump = (t) => player.seek(t)

  return {
    selected,
    checked,
    markerOf,
    markerLabel,
    loopSaved,
    saveMarker,
    selectMarker,
    playMarker,
    toggleChecked,
    checkedList,
    playChecked,
    removeMarker,
    renameMarker,
    jump
  }
}
