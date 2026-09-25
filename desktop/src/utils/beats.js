// @ts-check
// El metrónomo visto desde la interfaz: la rejilla tal como suena y el tempo
// escrito a mano.
//
// La rejilla que se pinta sobre la onda es la del análisis con lo ajustado a
// mano encima (doble o mitad, compás, el «1» corrido), igual que la rehace
// Rust para el clic (`beats.rs`, `player/metro.rs`). Antes se pintaba la del
// análisis tal cual, y pulsar ×2 o cambiar el compás no cambiaba nada en la
// onda: parecía que esos botones no hacían nada.

/** @typedef {import('../api.js').BeatGrid} BeatGrid */

/** Los compases que se ofrecen. 0: sin acento, todos los clics iguales. */
export const METERS = [
  { v: 0, n: 'sin acento', title: 'Sin acento: todos los clics iguales' },
  { v: 2, n: '2/4', title: 'Dos por compás: acento en el 1 de cada dos' },
  { v: 3, n: '3/4', title: 'Tres por compás (vals)' },
  { v: 4, n: '4/4', title: 'Cuatro por compás' },
  { v: 6, n: '6/8', title: 'Seis por compás: dos grupos de tres' }
]

/** Un compás que se pueda tocar: de 2 a 12, o 0 (sin acento). */
export function normalMeter(meter) {
  const m = Math.round(Number(meter) || 0)
  return m < 2 ? 0 : Math.min(12, m)
}

const mod = (a, n) => ((a % n) + n) % n

/**
 * El doble de pulsos (a mitad de camino de cada par). El «1» se queda donde
 * estaba.
 * @param {BeatGrid} g
 * @returns {BeatGrid}
 */
export function doubled(g) {
  const period = 60 / Math.max(1, g.bpm)
  const beats = []
  g.beats.forEach((b, i) => {
    beats.push(b)
    const next = g.beats[i + 1] ?? b + period
    beats.push((b + next) / 2)
  })
  return {
    ...g,
    bpm: g.bpm * 2,
    beats,
    first_downbeat: g.first_downbeat * 2,
    phase3: g.phase3 * 2,
    phase4: g.phase4 * 2
  }
}

/**
 * La mitad de pulsos: uno de cada dos, empezando por el «1».
 * @param {BeatGrid} g
 * @returns {BeatGrid}
 */
export function halved(g) {
  const start = g.first_downbeat % 2
  return {
    ...g,
    bpm: g.bpm / 2,
    beats: g.beats.filter((_, i) => i >= start && (i - start) % 2 === 0),
    first_downbeat: (g.first_downbeat - start) / 2,
    phase3: Math.floor(Math.max(0, g.phase3 - start) / 2),
    phase4: Math.floor(Math.max(0, g.phase4 - start) / 2)
  }
}

/**
 * Otro compás: el «1» sale de donde caía en 3 o en 4 (el análisis solo sabe
 * esos dos). Un 2/4 es medio 4/4, un 6/8 son dos grupos de tres.
 * @param {BeatGrid} g
 * @param {number} meter
 * @returns {BeatGrid}
 */
export function withMeter(g, meter) {
  const m = normalMeter(meter)
  if (!m) return { ...g, meter: 0 }
  const phase = m % 3 === 0 ? g.phase3 : g.phase4
  return { ...g, meter: m, first_downbeat: phase % m }
}

/**
 * El «1» corrido `shift` pulsos.
 * @param {BeatGrid} g
 * @param {number} shift
 * @returns {BeatGrid}
 */
export function shifted(g, shift) {
  return { ...g, first_downbeat: mod(g.first_downbeat + (shift || 0), Math.max(1, g.meter)) }
}

/**
 * La rejilla con lo ajustado a mano, en el mismo orden que Rust: doble o
 * mitad, compás y el «1» corrido.
 * @param {BeatGrid|null} base
 * @param {{ mult?: number, meter?: number|null, shift?: number }} [settings]
 * @returns {BeatGrid|null}
 */
export function effectiveGrid(base, { mult = 0, meter = null, shift = 0 } = {}) {
  if (!base || !Array.isArray(base.beats)) return null
  let g = mult === 1 ? doubled(base) : mult === -1 ? halved(base) : base
  if (meter != null) g = withMeter(g, meter)
  return shifted(g, shift)
}

/**
 * ¿Es el «1» el pulso `i`? Sin acento, ninguno.
 * @param {BeatGrid} g @param {number} i
 */
export function isDownbeat(g, i) {
  return g.meter > 0 && mod(i - g.first_downbeat, g.meter) === 0
}

/** El factor del doble o la mitad. */
export function multFactor(mult) {
  return mult === 1 ? 2 : mult === -1 ? 0.5 : 1
}

/**
 * Los bpm como se enseñan: con un decimal solo si lo tiene («90.7», «120»).
 * @param {number} bpm
 */
export function formatBpm(bpm) {
  const v = Math.round((Number(bpm) || 0) * 10) / 10
  return Number.isInteger(v) ? String(v) : v.toFixed(1)
}

/**
 * Lo que se escribe como tempo: «90.7» o «90,7». null si no es un número.
 * @param {string|number} text
 * @returns {number|null}
 */
export function parseBpm(text) {
  const v = Number(
    String(text ?? '')
      .trim()
      .replace(',', '.')
  )
  return String(text ?? '').trim() && Number.isFinite(v) && v > 0 ? v : null
}
