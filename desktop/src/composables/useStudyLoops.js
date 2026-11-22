// @ts-check
// Los tramos del modo estudio (StudyBar.vue, StudyWaveBar.vue): uno (el bucle
// A-B) o varios seguidos, qué pasa al acabar, el candado de la onda, los
// mandos de la canción de encima de la onda, las teclas A y B y las opciones
// de un tramo (el botón ⋯ de la onda, o el clic derecho).
import { ref, computed, watch } from 'vue'
import { usePlayback } from './usePlayback.js'
import { notify } from './useNotices.js'
import { openMenu } from './useContextMenu.js'
import { useHotkeys } from './useHotkeys.js'
import { isMobileApp } from '../platform.js'
import { formatTime } from '../utils/format.js'
import { clamp, round2 } from '../utils/num.js'
import { readPref, savePref } from '../utils/prefs.js'
import {
  cleanSegments,
  segmentAt,
  nextSegment,
  segmentsLength,
  snapToBeats
} from '../utils/segments.js'
import { markerParts as partsOf } from '../utils/cues.js'

/**
 * Se arma después de leer la canción y antes del clic y de las guías: la
 * rejilla que suena, el clic manual (`ctx.metro`) y poner una guía
 * (`ctx.cues`) se leen al usarlos.
 * @param {import('./studyContext.js').StudyContext} ctx
 */
export function useStudyLoops(ctx) {
  const player = usePlayback()
  const { track, position, duration, playing } = player
  const { study, scheduleSave } = ctx.doc
  const { selected, loopSaved, saveMarker } = ctx.markers

  // ---- el candado de la onda
  // Puesto (lo normal), mientras suena un clic en la onda no mueve la cancion
  // y elegir un tramo no salta a el: el tramo entra cuando la cancion llega
  // (Rust lo espera). En pausa, la onda coloca como siempre.
  const LOCK_KEY = 'danplay.study.lock'
  const locked = ref(readPref(LOCK_KEY, true) !== false)
  watch(locked, (v) => savePref(LOCK_KEY, v))
  /** La onda no puede mover la cancion ahora. */
  const guarded = computed(() => locked.value && playing.value)

  // ---- los tramos: uno (el bucle A-B) o varios seguidos
  /**
   * Qué pasa al acabar el último tramo: 'repeat' (vuelta al primero),
   * 'stop' (se para al principio del primero: play lo vuelve a tocar) o
   * 'continue' (suenan una vez y la canción sigue). Es de quien toca: se
   * recuerda. Antes se repetían siempre, y el botón «Al acabar» quería decir
   * otra cosa (esperar al final de la canción): parecía no funcionar.
   */
  const END_KEY = 'danplay.study.atEnd'
  const LOOP_ENDS = [
    {
      v: 'repeat',
      n: 'Repetir',
      icon: 'repeat',
      title: 'Al acabar el último tramo, vuelve al primero: en bucle hasta que lo quites'
    },
    {
      v: 'stop',
      n: 'Parar',
      icon: 'stop',
      title: 'Suena cada tramo una vez y se para al principio del primero (play, otra vez)'
    },
    {
      v: 'continue',
      n: 'Seguir',
      icon: 'right',
      title: 'Suena cada tramo una vez y la canción sigue desde ahí'
    }
  ]
  const savedEnd = readPref(END_KEY, 'stop')
  const loopEndWanted = ref(LOOP_ENDS.some((e) => e.v === savedEnd) ? savedEnd : 'stop')
  watch(loopEndWanted, (v) => savePref(END_KEY, v))
  async function setLoopEnd(v) {
    loopEndWanted.value = v
    await player.setLoops(study.value.loops, { end: v })
  }

  /** Varios tramos a la vez: cada arrastre en la onda añade otro. */
  const multi = ref(false)
  /** El último tramo que se tocó (al dejar «varios», se queda ese). */
  const lastTouched = ref(-1)
  const pendingA = ref(null)
  const hasLoop = computed(() => study.value.loops.length > 0)
  // ---- los mandos de la canción, encima de la onda (sin bajar al reproductor)
  /** Al principio: del tramo, si hay, o de la canción. */
  function toStart() {
    player.seek(hasLoop.value ? study.value.loops[0][0] : 0)
  }
  /** Atrás o adelante unos segundos. @param {number} secs */
  function nudgeTime(secs) {
    const d = duration.value || 0
    player.seek(clamp(position.value + secs, 0, Math.max(0, d - 0.05)))
  }
  /** Detener: se para y vuelve al principio (del tramo, si hay). */
  async function stopHere() {
    if (playing.value) await player.toggle()
    toStart()
  }
  /**
   * «Al acabar: parar» (lo de fábrica) para la canción al final del tramo y la
   * deja al principio de él. Sin verlo, parece que se pausa sola: se dice.
   */
  let soundingAt = 0
  watch(position, (p) => {
    if (playing.value) soundingAt = p
  })
  watch(playing, (now, before) => {
    if (now || !before || !hasLoop.value || loopEndWanted.value !== 'stop') return
    const list = study.value.loops
    if (soundingAt < list[list.length - 1][1] - 1.5) return
    notify(
      'Se paró al acabar el tramo («Al acabar: parar»). Para que vuelva a empezar solo, elige «Repetir».',
      'info',
      6
    )
  })
  /**
   * Lo que se puede hacer con el ratón sobre la onda: en el botón de ayuda de
   * la barra (antes, una línea de texto siempre a la vista).
   */
  const waveHelp = computed(() => {
    if (ctx.metro.editingBeats.value) {
      return 'Ajustando tu clic: clic en un pulso lo elige (Ctrl suma, Mayús elige hasta ahí) · arrastra sobre la franja de arriba para elegir una zona · arrastrar un pulso elegido los lleva todos · doble clic añade uno · clic derecho lo quita o lo hace el «1» · flechas: 5 ms · Ctrl + Z deshace · Ctrl + rueda acerca'
    }
    const pick = multi.value
      ? 'Arrastra para añadir tramos (suenan seguidos, saltándose lo de en medio) · clic sobre uno para quitarlo'
      : 'Arrastra sobre la onda para elegir un tramo · ' +
        (locked.value
          ? 'con el candado, sonando, un clic no mueve la canción'
          : 'clic para ir a un punto')
    return (
      pick +
      ' · Ctrl + rueda: acercar · Mayús + rueda: recorrer · clic derecho: opciones · teclas A y B · en las guías: doble clic pone una, arrástralas para moverlas'
    )
  })

  const loopLabel = computed(() => {
    const list = study.value.loops
    if (list.length === 1) return `${formatTime(list[0][0])} – ${formatTime(list[0][1])}`
    if (list.length > 1) return `${list.length} tramos · ${formatTime(segmentsLength(list))}`
    if (pendingA.value != null) return `A en ${formatTime(pendingA.value)} · pulsa B donde acabe`
    return 'sin tramo: arrastra sobre la onda'
  })
  // al dejar «varios» se queda un solo tramo: el último que se tocó
  watch(multi, (on) => {
    const list = study.value.loops
    if (on || list.length <= 1) return
    const keep = list[clamp(lastTouched.value, 0, list.length - 1)] || list.at(-1)
    lastTouched.value = 0
    applyLoops([keep], { jump: 'never' })
  })

  /**
   * Deja los tramos. `jump` dice si la cancion va a ellos: 'outside' si va
   * por fuera de todos (lo de siempre: al siguiente que venga), 'never' para
   * dejarla donde va (la onda con candado), o un segundo concreto (la tecla
   * B vuelve a su A).
   */
  async function applyLoops(list, { jump = 'outside' } = {}) {
    const clean = cleanSegments(list)
    if (clean.some(([a, b]) => b - a < 0.5)) {
      notify('El bucle tiene que durar al menos medio segundo')
      return
    }
    pendingA.value = null
    study.value.loops = clean
    await player.setLoops(clean, { end: loopEndWanted.value })
    if (typeof jump === 'number') await player.seek(jump)
    else if (jump === 'outside' && clean.length && segmentAt(clean, position.value) < 0) {
      await player.seek(clean[nextSegment(clean, position.value)][0])
    }
    scheduleSave()
  }
  async function clearLoop() {
    pendingA.value = null
    study.value.loops = []
    lastTouched.value = -1
    await player.clearLoop()
    scheduleSave()
  }
  /** Un marcador pasa a ser esos tramos (uno solo, o varios como partes). */
  function setMarkerParts(m, list) {
    m.t = list[0][0]
    m.end = list.at(-1)[1]
    if (list.length > 1) m.parts = list.map((x) => [...x])
    else delete m.parts
    study.value.markers = [...study.value.markers].sort((x, y) => x.t - y.t)
  }
  /** El índice en `after` del tramo que no estaba en `before` (el que se añadió). */
  function addedIndex(before, after) {
    const i = after.findIndex(
      ([a, b]) => !before.some(([x, y]) => Math.abs(x - a) < 0.011 && Math.abs(y - b) < 0.011)
    )
    return i < 0 ? after.length - 1 : i
  }
  /**
   * Lo que manda la linea de tiempo. Un tramo nuevo dibujado de cero (o uno
   * que se añade o se quita) deja de apuntar al marcador que hubiera elegido;
   * mover un borde con un marcador elegido cambia el tramo DEL marcador, que es
   * lo que uno espera al estirarlo.
   * @param {Array<[number, number]>} list
   * @param {{ mode?: string, index?: number }} [opts]
   */
  function onLoops(list, { mode, index } = {}) {
    if (!list.length) return clearLoop()
    const m = selected.value
    const clean = cleanSegments(list)
    if (mode === 'edit' && m && partsOf(m).length) setMarkerParts(m, clean)
    else if (mode === 'select' || mode === 'add' || mode === 'remove') selected.value = null
    if (mode === 'add') lastTouched.value = addedIndex(study.value.loops, clean)
    else if (mode === 'edit' && Number.isInteger(index)) lastTouched.value = index
    else if (mode === 'select') lastTouched.value = 0
    else lastTouched.value = Math.min(lastTouched.value, clean.length - 1)
    return applyLoops(list, { jump: guarded.value || mode === 'remove' ? 'never' : 'outside' })
  }
  function markA() {
    if (!track.value || !ctx.visible()) return
    const at = round2(position.value)
    selected.value = null
    const list = study.value.loops
    // con un solo tramo, A lo acorta por delante
    if (!multi.value && list.length === 1 && at < list[0][1] - 0.5) {
      return applyLoops([[at, list[0][1]]])
    }
    if (!multi.value && list.length) clearLoop()
    pendingA.value = at
  }
  function markB() {
    if (!track.value || !ctx.visible()) return
    const at = round2(position.value)
    const list = study.value.loops
    const single = !multi.value && list.length === 1
    // B sin A (y sin un tramo que alargar) hacía el tramo desde 0:00 y mandaba
    // la canción al principio: un dedo en la tecla de al lado, a media
    // práctica, y se perdía por dónde se iba
    if (pendingA.value == null && !single) {
      notify('Marca primero dónde empieza el tramo, con A')
      return
    }
    const from = pendingA.value ?? list[0][0]
    if (at - from < 0.5) {
      notify('El bucle tiene que durar al menos medio segundo')
      return
    }
    selected.value = null
    // B cierra el tramo donde va la cancion y vuelve a A: es la repeticion
    // A-B. Con «varios», el tramo se añade a los que hubiera.
    const next = multi.value ? [...list, [from, at]] : [[from, at]]
    if (multi.value) lastTouched.value = addedIndex(list, cleanSegments(next))
    return applyLoops(next, { jump: from })
  }

  // ---- las opciones de un tramo (el boton ⋯ de la onda, o el clic derecho)
  /** Ir ya a ese tramo y que suene. */
  async function playNow(index = 0) {
    const seg = study.value.loops[index]
    if (!seg) return
    await player.seek(seg[0])
    if (!playing.value) await player.toggle()
  }
  /** Los bordes de los tramos, al pulso mas cercano: para que entren a tiempo. */
  function snapLoops() {
    const beats = ctx.metro.soundingGrid.value?.beats
    if (!beats?.length || !hasLoop.value) return
    const snapped = snapToBeats(study.value.loops, beats)
    const m = selected.value
    if (m && partsOf(m).length) setMarkerParts(m, snapped)
    return applyLoops(snapped, { jump: 'never' })
  }
  function removeSegment(index) {
    return onLoops(
      study.value.loops.filter((_, i) => i !== index),
      { mode: 'remove' }
    )
  }
  /**
   * Lo que se puede hacer con un tramo de la onda (o, con `index` -1, en ese
   * punto de ella). Las que mueven la cancion son a proposito, asi que valen
   * tambien con el candado puesto.
   */
  function showOptions({ index, t, x, y, event }) {
    const list = study.value.loops
    const seg = list[index]
    const items = []
    if (seg) {
      items.push({
        label: 'Reproducir ahora',
        icon: 'play',
        note: formatTime(seg[0]),
        action: () => playNow(index)
      })
      for (const e of LOOP_ENDS) {
        items.push({
          label: `Al acabar: ${e.n.toLowerCase()}`,
          icon: e.icon,
          active: loopEndWanted.value === e.v,
          action: () => setLoopEnd(e.v)
        })
      }
    }
    items.push({
      label: 'Ir aquí',
      icon: 'right',
      note: formatTime(t),
      action: () => player.seek(t)
    })
    if (!isMobileApp) {
      items.push({
        label: 'Guía aquí…',
        icon: 'mic',
        note: playing.value ? 'para la canción' : formatTime(t),
        disabled: playing.value,
        action: () => ctx.cues.addCue(t)
      })
    }
    if (seg) {
      items.push({
        label: 'Ajustar a los pulsos',
        icon: 'note',
        disabled: !ctx.metro.soundingGrid.value?.beats?.length,
        action: snapLoops
      })
      if (!loopSaved.value)
        items.push({ label: 'Guardar como marcador', icon: 'save', action: saveMarker })
    }
    items.push(
      multi.value
        ? { label: 'Volver a un solo tramo', icon: 'repeat', action: () => (multi.value = false) }
        : { label: 'Añadir más tramos', icon: 'plus', action: () => (multi.value = true) }
    )
    if (list.length) {
      items.push({ separator: true })
      if (seg && list.length > 1) {
        items.push({
          label: 'Quitar este tramo',
          icon: 'close',
          action: () => removeSegment(index)
        })
      }
      items.push({
        label: list.length > 1 ? 'Quitar todos los tramos' : 'Quitar la selección',
        icon: 'trash',
        danger: true,
        action: clearLoop
      })
    }
    const title = seg ? (list.length > 1 ? `Tramo ${index + 1}` : 'El tramo') : 'La onda'
    if (!isMobileApp) items.push({ separator: true }, ctx.waveStyleItem())
    openMenu(event ?? { clientX: x, clientY: y }, items, title)
  }
  useHotkeys({ a: markA, b: markB })

  return {
    locked,
    guarded,
    LOOP_ENDS,
    loopEndWanted,
    setLoopEnd,
    multi,
    lastTouched,
    pendingA,
    hasLoop,
    toStart,
    nudgeTime,
    stopHere,
    waveHelp,
    loopLabel,
    clearLoop,
    onLoops,
    playNow,
    showOptions
  }
}
