// @ts-check
// El metrónomo visto desde la interfaz: la rejilla tal como suena y el tempo
// escrito a mano.
//
// La rejilla que se pinta sobre la onda es la del análisis con lo ajustado a
// mano encima (doble o mitad, compás, el «1» corrido), igual que la rehace
// Rust para el clic (`beats.rs`, `player/metro.rs`). Antes se pintaba la del
// análisis tal cual, y pulsar ×2 o cambiar el compás no cambiaba nada en la
// onda: parecía que esos botones no hacían nada.

import { formatTime } from './format.js'

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
    ...(g.phase2 != null ? { phase2: g.phase2 * 2 } : {}),
    ...(g.phase6 != null ? { phase6: g.phase6 * 2 } : {}),
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
    ...(g.phase2 != null ? { phase2: Math.floor(g.phase2 / 2) } : {}),
    ...(g.phase6 != null ? { phase6: Math.floor(g.phase6 / 2) } : {}),
    ...(g.bars?.length ? { bars: g.bars.filter((b) => kept.has(b)).map(position) } : {})
  }
}

/**
 * Otro compás: el «1» sale de donde caía en ese compás (el análisis sabe el
 * de 2, 3, 4 y 6). Sin eso (una rejilla de antes), un 2/4 es medio 4/4 y un
 * 6/8 son dos grupos de tres.
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
  // (como `with_meter` de beats.rs)
  const phase =
    m === 2
      ? (g.phase2 ?? g.phase4)
      : m === 6
        ? (g.phase6 ?? g.phase3)
        : m % 3 === 0
          ? g.phase3
          : g.phase4
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

// ------------------------------------------------------------ el clic propio
// La rejilla que uno ajusta a mano (moviendo pulsos sobre la onda) se guarda
// con la canción, en su estudio: `metronome.own = {beats, bars, meter,
// touched}`, con los segundos a la milésima. De ahí sale una rejilla como las
// del análisis, que es lo que entiende Rust. `touched` son las zonas de la
// canción que movió él, `[[desde, hasta], …]` en segundos: al re-analizar,
// lo de dentro se respeta y lo demás se rehace con el análisis nuevo. Son
// tiempos y no índices de pulso: añadir o quitar un pulso antes no las mueve.

/**
 * La mediana de los intervalos, en bpm.
 * @param {ReadonlyArray<number>} beats
 */
export function medianBpm(beats) {
  const gaps = []
  for (let i = 1; i < beats.length; i++) gaps.push(beats[i] - beats[i - 1])
  if (!gaps.length) return 120
  gaps.sort((a, b) => a - b)
  const mid = gaps[Math.floor(gaps.length / 2)]
  return mid > 0 ? Math.min(600, Math.max(10, 60 / mid)) : 120
}

/**
 * La rejilla de un clic propio guardado, o null si no vale.
 * @param {any} own  `{beats, bars?, meter?}`
 * @returns {BeatGrid|null}
 */
export function ownToGrid(own) {
  const beats = Array.isArray(own?.beats)
    ? own.beats.map(Number).filter((t) => Number.isFinite(t) && t >= 0)
    : []
  const clean = []
  for (const t of beats) if (!clean.length || t > clean.at(-1) + 0.01) clean.push(t)
  if (clean.length < 2) return null
  const bars = (Array.isArray(own?.bars) ? own.bars : [])
    .map(Number)
    .filter((i) => Number.isInteger(i) && i >= 0 && i < clean.length)
  const meter = own?.meter == null ? 4 : normalMeter(own.meter)
  return {
    bpm: Math.round(medianBpm(clean) * 100) / 100,
    meter,
    beats: clean,
    first_downbeat: bars[0] ?? 0,
    phase3: 0,
    phase4: 0,
    confidence: 1,
    bars: [...new Set(bars)].sort((a, b) => a - b),
    closed: true
  }
}

/**
 * Lo que se guarda de una rejilla como clic propio (con su «1» compás a
 * compás, tal como suena) y, si se dan, las zonas que se tocaron a mano.
 * @param {BeatGrid} grid
 * @param {Array<[number, number]>} [touched]  sin ellas, el clic no las lleva
 * @returns {{beats: number[], bars: number[], meter: number, touched?: Array<[number, number]>}}
 */
export function gridToOwn(grid, touched) {
  const beats = (grid?.beats || []).map((t) => Math.round(t * 1000) / 1000)
  const bars = []
  for (let i = 0; i < beats.length; i++) if (isDownbeat(grid, i)) bars.push(i)
  return {
    beats,
    bars,
    meter: grid?.meter ?? 4,
    ...(Array.isArray(touched) ? { touched: mergeWindows(touched) } : {})
  }
}

/**
 * El pulso más cercano a `t` (los pulsos, en orden): para pegar algo al
 * clic, también si la canción cambia de tempo. Sin pulsos, `t` tal cual.
 * @param {number[]} beats @param {number} t
 */
export function nearestBeat(beats, t) {
  if (!beats?.length) return t
  let lo = 0
  let hi = beats.length - 1
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (beats[mid] < t) lo = mid + 1
    else hi = mid
  }
  return lo > 0 && Math.abs(beats[lo - 1] - t) <= Math.abs(beats[lo] - t)
    ? beats[lo - 1]
    : beats[lo]
}

/**
 * El pulso de antes (`dir` -1) o el de después (1) de `t`, sin contar uno
 * que caiga justo en `t`. Sin pulsos por ese lado, null.
 * @param {number[]} beats @param {number} t @param {-1|1} dir
 */
export function stepBeat(beats, t, dir) {
  if (!beats?.length) return null
  const eps = 0.005
  if (dir < 0) {
    for (let i = beats.length - 1; i >= 0; i--) if (beats[i] < t - eps) return beats[i]
    return null
  }
  for (const b of beats) if (b > t + eps) return b
  return null
}

// ------------------------------------------------- lo que se movió a mano
const ms = (t) => Math.round(t * 1000)
const round3 = (t) => Math.round(t * 1000) / 1000

/**
 * Unas zonas `[[desde, hasta], …]` en limpio: solo pares de segundos que
 * valgan, al milisegundo, en orden, y las que se pisan o quedan a menos de
 * `gap` segundos una de otra, hechas una sola.
 * @param {any} list @param {number} [gap]
 * @returns {Array<[number, number]>}
 */
export function mergeWindows(list, gap = 0) {
  /** @type {Array<[number, number]>} */
  const out = []
  const clean = (Array.isArray(list) ? list : [])
    .map((w) => (Array.isArray(w) ? [Number(w[0]), Number(w[1])] : [NaN, NaN]))
    .filter(([a, b]) => Number.isFinite(a) && Number.isFinite(b) && a >= 0 && b > a)
    .map(([a, b]) => [round3(a), round3(b)])
    .sort((x, y) => x[0] - y[0] || x[1] - y[1])
  for (const [a, b] of clean) {
    const last = out.at(-1)
    if (last && a <= last[1] + gap + 1e-9) last[1] = Math.max(last[1], b)
    else out.push([a, b])
  }
  return out
}

/**
 * Qué pulsos cambiaron de una rejilla a otra: los que están en `after` y no
 * estaban (movidos o nuevos) y los que estaban y ya no (movidos o quitados).
 * Dos pulsos son el mismo si coinciden al milisegundo. `changed` cuenta un
 * pulso movido una vez (no como uno quitado y uno nuevo).
 * @param {ReadonlyArray<number>} before @param {ReadonlyArray<number>} after
 * @returns {{added: number[], removed: number[], changed: number}}
 */
export function beatChanges(before, after) {
  const was = new Set(before.map(ms))
  const now = new Set(after.map(ms))
  const added = after.filter((t) => !was.has(ms(t)))
  const removed = before.filter((t) => !now.has(ms(t)))
  return { added, removed, changed: Math.max(added.length, removed.length) }
}

/**
 * Los pulsos de `beats` (en orden) que caen justo antes y justo después de
 * `t` (sin contar uno que esté en `t`); donde no hay, el propio `t`.
 * @param {ReadonlyArray<number>} beats @param {number} t
 * @returns {[number, number]}
 */
function around(beats, t) {
  let lo = 0
  let hi = beats.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (ms(beats[mid]) <= ms(t)) lo = mid + 1
    else hi = mid
  }
  // `lo`: el primero que cae después de t
  let before = lo - 1
  if (before >= 0 && ms(beats[before]) === ms(t)) before--
  return [before >= 0 ? beats[before] : t, lo < beats.length ? beats[lo] : t]
}

/**
 * Las zonas que movió él al pasar de `before` a `after`: cada pulso movido,
 * nuevo o quitado (y cada «1» que cambió de sitio sin moverse el pulso) con
 * un pulso de margen por cada lado, y las que quedan juntas, hechas una. Se
 * suman a las que ya llevaba (`prev`), que son tiempos: mover o añadir
 * pulsos antes no las desplaza.
 * @param {BeatGrid} before  lo que se veía al empezar a ajustar
 * @param {BeatGrid} after  lo que quedó
 * @param {Array<[number, number]>} [prev]  las zonas de antes
 * @returns {Array<[number, number]>}
 */
export function touchedByEdit(before, after, prev = []) {
  const kept = mergeWindows(prev)
  const bb = before?.beats || []
  const ab = after?.beats || []
  const { added, removed } = beatChanges(bb, ab)
  /** @type {number[]} */
  const points = [...added, ...removed]
  // un «1» que cambia de sitio sin que se mueva el pulso (clic derecho: «este es el 1»)
  const wasOne = new Map(bb.map((t, i) => [ms(t), isDownbeat(before, i)]))
  ab.forEach((t, i) => {
    const was = wasOne.get(ms(t))
    if (was !== undefined && was !== isDownbeat(after, i)) points.push(t)
  })
  if (!points.length) return kept
  const windows = points.map((t) => around(ab, t))
  // «a menos de un pulso y medio» de otra, es la misma zona
  const period = 60 / medianBpm(ab.length > 1 ? ab : bb)
  const gap = Math.min(3, Math.max(0.1, period * 1.5))
  return mergeWindows([...kept, ...windows], gap)
}

/**
 * Las zonas que movió él, guardadas con su clic propio. `null`: es de antes
 * de que se guardaran (no se sabe cuáles).
 * @param {any} own
 * @returns {Array<[number, number]>|null}
 */
export function ownTouched(own) {
  return Array.isArray(own?.touched) ? mergeWindows(own.touched) : null
}

/**
 * Un clic manual como llega del núcleo, en limpio: pulsos válidos, sus «1» y
 * las zonas tocadas (si no las trae, las `fallback`). null si no vale.
 * @param {any} own
 * @param {Array<[number, number]>} [fallback]
 */
export function cleanOwn(own, fallback = []) {
  const g = ownToGrid(own)
  if (!g) return null
  return gridToOwn(g, ownTouched(own) ?? fallback)
}

/**
 * Unas zonas para leerlas: «1:05–1:20, 2:10–2:30 y 2 más» (hasta `max`).
 * @param {ReadonlyArray<[number, number]>} list @param {number} [max]
 */
export function windowsText(list, max = 3) {
  const shown = list.slice(0, max).map(([a, b]) => `${formatTime(a)}–${formatTime(b)}`)
  const rest = list.length - shown.length
  if (rest <= 0) return shown.join(', ')
  return `${shown.join(', ')} y ${rest} más`
}
