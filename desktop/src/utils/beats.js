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

/** A partir de cuántos pulsos sin nada es un hueco sin clic (`GAP` de beats.rs). */
const GAP = 1.4

/**
 * El doble de pulsos (a mitad de camino de cada par). El «1» se queda donde
 * estaba. Un hueco sin pulso sigue sin clic, y después del último de una
 * rejilla cerrada no hay nada (como `doubled` de beats.rs).
 * @param {BeatGrid} g
 * @returns {BeatGrid}
 */
export function doubled(g) {
  const period = 60 / Math.max(1, g.bpm)
  const beats = []
  const moved = []
  g.beats.forEach((b, i) => {
    moved.push(beats.length)
    beats.push(b)
    const next = g.beats[i + 1] ?? (g.closed ? null : b + period)
    if (next != null && next - b <= GAP * period) beats.push((b + next) / 2)
  })
  const at = (i) => moved[i] ?? i * 2
  return {
    ...g,
    bpm: g.bpm * 2,
    beats,
    first_downbeat: at(g.first_downbeat),
    phase3: g.phase3 * 2,
    phase4: g.phase4 * 2,
    ...(g.bars?.length ? { bars: g.bars.map(at) } : {})
  }
}

/**
 * La mitad de pulsos: uno de cada dos contando desde el «1» de cada compás
 * (el 1 y el 3 en un 4/4); en un compás impar, uno de cada dos desde el
 * primer «1» (como `halved` de beats.rs).
 * @param {BeatGrid} g
 * @returns {BeatGrid}
 */
export function halved(g) {
  const keep = []
  if (g.meter > 0 && g.meter % 2 === 0 && g.bars?.length) {
    g.beats.forEach((_, i) => {
      if (beatInBar(g, i) % 2 === 0) keep.push(i)
    })
  } else {
    for (let i = g.first_downbeat % 2; i < g.beats.length; i += 2) keep.push(i)
  }
  const position = (i) => {
    let k = 0
    while (k < keep.length && keep[k] < i) k++
    return k
  }
  const kept = new Set(keep)
  return {
    ...g,
    bpm: g.bpm / 2,
    beats: keep.map((i) => g.beats[i]),
    first_downbeat: position(g.first_downbeat),
    phase3: Math.floor(g.phase3 / 2),
    phase4: Math.floor(g.phase4 / 2),
    ...(g.bars?.length ? { bars: g.bars.filter((b) => kept.has(b)).map(position) } : {})
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
  // el mismo: se quedan los «1» como estaban (con Beat This!, compás a compás)
  if (m === g.meter) return g
  const { bars: _bars, ...regular } = g
  if (!m) return { ...regular, meter: 0 }
  const phase = m % 3 === 0 ? g.phase3 : g.phase4
  return { ...regular, meter: m, first_downbeat: phase % m }
}

/**
 * El «1» corrido `shift` pulsos.
 * @param {BeatGrid} g
 * @param {number} shift
 * @returns {BeatGrid}
 */
export function shifted(g, shift) {
  const out = { ...g, first_downbeat: mod(g.first_downbeat + (shift || 0), Math.max(1, g.meter)) }
  if (g.bars?.length && shift) {
    out.bars = g.bars.map((b) => b + shift).filter((b) => b >= 0 && b < g.beats.length)
  }
  return out
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
 * Qué tiempo del compás es el pulso `i` (0 = el «1»). Con `bars` (Beat
 * This!), contando desde el último «1» antes de `i`, como Rust (`beat_in_bar`).
 * @param {BeatGrid} g @param {number} i
 */
export function beatInBar(g, i) {
  let from = g.first_downbeat
  if (g.bars?.length) {
    from = g.bars[0]
    for (const b of g.bars) {
      if (b > i) break
      from = b
    }
  }
  return mod(i - from, Math.max(1, g.meter))
}

/**
 * ¿Es el «1» el pulso `i`? Sin acento, ninguno.
 * @param {BeatGrid} g @param {number} i
 */
export function isDownbeat(g, i) {
  return g.meter > 0 && beatInBar(g, i) === 0
}

/**
 * Los golpes del clic a lo largo de la canción, para guardarlos en una
 * mezcla: los de la rejilla tal como suena, o con tempo a mano, uno cada
 * tanto desde el primer pulso de la canción (o desde el principio). Y cuáles
 * son el «1».
 * @param {BeatGrid|null} grid  la rejilla con lo ajustado a mano (`effectiveGrid`)
 * @param {{ free: boolean, bpm: number, meter: number }} metro  el metrónomo como va
 * @param {number} duration  lo que dura la canción, en segundos
 * @returns {{ beats: number[], accents: boolean[] }}
 */
export function clickTimes(grid, metro, duration) {
  if (grid && !metro.free) {
    return { beats: [...grid.beats], accents: grid.beats.map((_, i) => isDownbeat(grid, i)) }
  }
  const period = 60 / Math.max(10, Number(metro.bpm) || 100)
  const meter = normalMeter(metro.meter)
  const start = grid?.beats?.[grid.first_downbeat] ?? 0
  const beats = []
  const accents = []
  let t = start
  // hacia atrás hasta el principio, y hacia delante hasta el final
  while (t - period >= 0) t -= period
  const offset = Math.round((start - t) / period)
  for (let i = 0; t <= duration && beats.length < 20000; i++, t += period) {
    beats.push(Math.round(t * 1000) / 1000)
    accents.push(meter > 0 && mod(i - offset, meter) === 0)
  }
  return { beats, accents }
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
