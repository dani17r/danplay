// @ts-check
// La aguja de lo que suena, pintada por fotogramas y sin pasar por Vue.
//
// Rust (o el <audio> del navegador) cuenta la posición cuatro veces por
// segundo. Leerla en una plantilla volvía a ejecutar la plantilla entera con
// cada aviso: el estudio son unas 2 000 líneas entre StudyBar y la línea de
// tiempo (carriles, cientos de pulsos y marcas de la regla), y seguía
// haciéndolo con el estudio oculto. Además la aguja avanzaba a saltos de
// 250 ms.
//
// Aquí se guarda la última posición y el instante en que llegó, y mientras
// suena se pinta en cada fotograma lo que va de canción desde entonces (a la
// velocidad puesta). `paint(t)` toca el DOM a mano (un `style.left`); nadie
// más se entera.
import { onScopeDispose, watch } from 'vue'
import { usePlayback } from './usePlayback.js'
import { segmentAt } from '../utils/segments.js'

/**
 * Lo más que se adelanta la aguja sin noticias de Rust: si deja de contar
 * (una salida que se cae, el hilo que se atasca), se queda ahí en vez de
 * seguir corriendo sola.
 */
const MAX_AHEAD = 0.3

// sin fotogramas (un entorno sin pintar), a 60 por segundo con un temporizador
const nextFrame = (/** @type {() => void} */ fn) =>
  typeof requestAnimationFrame === 'function'
    ? requestAnimationFrame(fn)
    : /** @type {number} */ (/** @type {unknown} */ (setTimeout(fn, 16)))
const cancelFrame = (/** @type {number} */ id) =>
  typeof cancelAnimationFrame === 'function' ? cancelAnimationFrame(id) : clearTimeout(id)

/**
 * @param {(t: number) => void} paint  pinta la aguja en el segundo `t`
 * @param {() => boolean} [active]     si se ve (oculto no se pinta)
 * @returns {{ now: () => number, repaint: () => void }}
 */
export function usePlayhead(paint, active = () => true) {
  const { state } = usePlayback()
  let base = 0
  let stamp = 0
  let frame = 0

  /** Por dónde va ahora, contando lo que pasó desde el último aviso. */
  function now() {
    // contando para entrar o preparando la canción, la posición no avanza
    if (!state.playing || state.counting_in || state.preparing) return base
    const ahead = Math.min(MAX_AHEAD, (performance.now() - stamp) / 1000)
    let t = base + ahead * (Number(state.speed) || 1)
    // dentro de un tramo, no más allá de su final: Rust la devuelve al
    // principio y la aguja no tiene que asomarse fuera
    const loops = state.loops || []
    const i = segmentAt(loops, base)
    if (i >= 0) t = Math.min(t, loops[i][1])
    if (state.duration > 0) t = Math.min(t, state.duration)
    return t
  }

  function step() {
    frame = 0
    if (!active()) return
    paint(now())
    if (state.playing) frame = nextFrame(step)
  }
  function kick() {
    if (!frame) frame = nextFrame(step)
  }

  watch(
    () => [state.position, state.playing],
    () => {
      base = Number(state.position) || 0
      stamp = performance.now()
      kick()
    },
    { immediate: true, flush: 'sync' }
  )
  // al volver a verse, a su sitio (y a moverse, si suena)
  watch(active, (on) => {
    if (on) kick()
  })
  onScopeDispose(() => {
    if (frame) cancelFrame(frame)
    frame = 0
  })
  return { now, repaint: () => paint(now()) }
}
