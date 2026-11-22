// @ts-check
// Editar el clic a mano, de muchos pulsos a la vez: la parte que no sabe nada
// de la pantalla.
//
// Trabaja sobre un clic propio (`own`: `{beats, bars, meter}`, los segundos de
// cada pulso al milisegundo, los ÍNDICES de los «1» y los pulsos por compás) y
// una selección (los índices de los pulsos elegidos, en orden). Todo son
// funciones puras: devuelven arrays nuevos y no tocan lo que reciben, así que
// la interfaz puede guardar el estado de antes para deshacer sin copiarlo.
//
// Reglas que se cumplen siempre:
//   · los pulsos salen en orden, sin negativos y a `MIN_GAP` como mínimo unos
//     de otros (y sin pasar de `duration` cuando se da);
//   · los «1» (`bars`) salen en orden, sin repetir y con índices que existen;
//   · los pulsos que no se tocan quedan EXACTAMENTE como estaban.
// Las cuentas de márgenes se hacen en milisegundos enteros: con segundos en
// coma flotante, `0.52 - 0.5 >= 0.02` dice que no.

import { normalMeter } from './beats.js'
import { clamp, round3 } from './num.js'

// (al milisegundo y sin devolver nunca un `-0`: ver num.js)
export { round3 }

/** @typedef {{beats: number[], bars: number[], meter: number}} Own */
/** @typedef {Own & {selection: number[]}} Edited */

/** Lo más cerca que pueden quedar dos pulsos, en segundos. */
export const MIN_GAP = 0.02
const MIN_GAP_MS = Math.round(MIN_GAP * 1000)
/** Más cerca que esto de otro pulso, no se deja añadir uno (`addBeatAt`). */
const ADD_GAP_MS = 30

const toMs = (t) => Math.round(t * 1000)
const mod = (a, n) => ((a % n) + n) % n

// -------------------------------------------------------------- lo de dentro

/** El primer índice de `list` (en orden) con `list[i] >= x`. */
function lowerBound(list, x) {
  let lo = 0
  let hi = list.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (list[mid] < x) lo = mid + 1
    else hi = mid
  }
  return lo
}

/** El primer índice de `list` (en orden) con `list[i] > x`. */
function upperBound(list, x) {
  let lo = 0
  let hi = list.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (list[mid] <= x) lo = mid + 1
    else hi = mid
  }
  return lo
}

/**
 * El clic en limpio y en copia: los «1» en orden, sin repetir y que existan;
 * el compás como lo entiende el resto (sin compás guardado, 4).
 * @param {any} own
 * @returns {Own}
 */
function normOwn(own) {
  const beats = Array.isArray(own?.beats) ? own.beats.map(Number) : []
  const bars = Array.isArray(own?.bars)
    ? [...new Set(own.bars.map(Number))]
        .filter((b) => Number.isInteger(b) && b >= 0 && b < beats.length)
        .sort((a, b) => a - b)
    : []
  return { beats, bars, meter: own?.meter == null ? 4 : normalMeter(own.meter) }
}

/** Una selección en limpio: índices que existen, sin repetir y en orden. */
function cleanSel(sel, n) {
  const list = Array.isArray(sel) ? sel : []
  return [...new Set(list.map(Number))]
    .filter((i) => Number.isInteger(i) && i >= 0 && i < n)
    .sort((a, b) => a - b)
}

/**
 * @param {Own} o @param {number[]} beats @param {number[]} bars @param {number[]} selection
 * @returns {Edited}
 */
const edited = (o, beats, bars, selection) => ({ beats, bars, meter: o.meter, selection })

/** Lo mismo que entró, en copia (para cuando no se puede hacer lo pedido). */
const same = (o, selection) => edited(o, [...o.beats], [...o.bars], [...selection])

/**
 * Lo que queda de unos «1» al quitar los pulsos `gone` (en orden): los
 * índices que siguen, cada uno en su sitio nuevo.
 * @param {number[]} bars @param {number[]} gone
 */
function reindexAfterRemoval(bars, gone) {
  const goneSet = new Set(gone)
  return bars.filter((b) => !goneSet.has(b)).map((b) => b - lowerBound(gone, b))
}

// ------------------------------------------------------------- la selección

/**
 * Los pulsos que caen entre dos tiempos (los dos incluidos). Da igual el
 * orden en que se den: arrastrar de derecha a izquierda es lo mismo.
 * @param {ReadonlyArray<number>} beats @param {number} t0 @param {number} t1
 * @returns {number[]}
 */
export function selectBetween(beats, t0, t1) {
  const lo = Math.min(t0, t1)
  const hi = Math.max(t0, t1)
  const out = []
  for (let i = 0; i < beats.length; i++) if (beats[i] >= lo && beats[i] <= hi) out.push(i)
  return out
}

/**
 * Todos los índices de un extremo a otro (los dos incluidos).
 * @param {number} a @param {number} b
 * @returns {number[]}
 */
export function selectRange(a, b) {
  if (!Number.isFinite(a) || !Number.isFinite(b)) return []
  const lo = Math.round(Math.min(a, b))
  const hi = Math.round(Math.max(a, b))
  return Array.from({ length: hi - lo + 1 }, (_, k) => lo + k)
}

/**
 * Añade el índice `i` si no estaba y lo quita si estaba.
 * @param {ReadonlyArray<number>} sel @param {number} i
 * @returns {number[]}
 */
export function toggleIndex(sel, i) {
  const has = sel.includes(i)
  const out = has ? sel.filter((j) => j !== i) : [...sel, i]
  return out.sort((a, b) => a - b)
}

/**
 * El primer y el último índice de la selección; null si está vacía.
 * @param {ReadonlyArray<number>} sel
 * @returns {[number, number]|null}
 */
export function spanOf(sel) {
  if (!sel?.length) return null
  return [Math.min(...sel), Math.max(...sel)]
}

/**
 * La selección sin huecos: todos los índices del primero al último. Lo que
 * necesita un bloque seguido (estirar, doblar...) lo pide así.
 * @param {ReadonlyArray<number>} sel
 * @returns {number[]}
 */
export function filled(sel) {
  const span = spanOf(sel)
  return span ? selectRange(span[0], span[1]) : []
}

/**
 * El tempo de lo seleccionado (de los pulsos seguidos del primero al último):
 * los bpm del intervalo mediano. 0 si no llegan a haber dos pulsos.
 * @param {ReadonlyArray<number>} beats @param {ReadonlyArray<number>} sel
 */
export function tempoOf(beats, sel) {
  const f = filled(cleanSel(sel, beats.length))
  if (f.length < 2) return 0
  const gaps = []
  for (let k = 1; k < f.length; k++) gaps.push(beats[f[k]] - beats[f[k - 1]])
  gaps.sort((a, b) => a - b)
  const mid = gaps.length >> 1
  const median = gaps.length % 2 ? gaps[mid] : (gaps[mid - 1] + gaps[mid]) / 2
  return median > 0 ? 60 / median : 0
}

// --------------------------------------------------- mover y estirar bloques

/**
 * Corre los pulsos elegidos `dt` segundos, con la misma separación entre
 * ellos. El corrimiento se recorta para que ninguno quede a menos de
 * `MIN_GAP` de un pulso sin elegir (ni pase de 0 ni de `duration`): en una
 * selección con huecos cuenta el pulso sin elegir más cercano de cada lado
 * de CADA elegido. `applied` es lo que se corrió de verdad (al milisegundo).
 * @param {any} own @param {ReadonlyArray<number>} sel @param {number} dt
 * @param {{duration?: number}} [opts]
 * @returns {Edited & {applied: number}}
 */
export function moveBlock(own, sel, dt, { duration } = {}) {
  const o = normOwn(own)
  const s = cleanSel(sel, o.beats.length)
  if (!s.length || !Number.isFinite(dt)) return { ...same(o, s), applied: 0 }
  const chosen = new Set(s)
  const ms = o.beats.map(toMs)
  // los límites del corrimiento, en ms: los pulsos que van juntos solo chocan
  // por los extremos del bloque, pero mirarlos todos da lo mismo y vale para
  // las selecciones con huecos
  let lo = -ms[s[0]]
  let hi = Infinity
  if (Number.isFinite(duration)) {
    hi =
      Math.floor(/** @type {number} */ (duration) * 1000 + 1e-6) -
      ms[/** @type {number} */ (s.at(-1))]
  }
  for (const i of s) {
    if (i > 0 && !chosen.has(i - 1)) lo = Math.max(lo, ms[i - 1] + MIN_GAP_MS - ms[i])
    if (i < ms.length - 1 && !chosen.has(i + 1)) hi = Math.min(hi, ms[i + 1] - MIN_GAP_MS - ms[i])
  }
  // lo que ya estaba mal (un pulso más cerca de lo debido) no se agrava, pero
  // quedarse donde estaba siempre se puede
  lo = Math.min(lo, 0)
  hi = Math.max(hi, 0)
  const applied = clamp(Math.round(dt * 1000), lo, hi)
  const beats = [...o.beats]
  if (applied) for (const i of s) beats[i] = round3(o.beats[i] + applied / 1000)
  return { ...edited(o, beats, [...o.bars], s), applied: applied / 1000 + 0 }
}

/**
 * Estira (o encoge) el bloque seguido de lo elegido: las distancias de dentro
 * se escalan desde el extremo que se queda quieto (`fixed`) hasta que el otro
 * cae en `to`. Los de en medio van en proporción. El extremo que se mueve no
 * pasa del pulso sin elegir de su lado, ni del fijo, ni deja ningún intervalo
 * a menos de `MIN_GAP`.
 * @param {any} own @param {ReadonlyArray<number>} sel
 * @param {{fixed: 'first'|'last', to: number}} target
 * @param {{duration?: number}} [opts]
 * @returns {Edited}
 */
export function stretchBlock(own, sel, { fixed, to }, { duration } = {}) {
  const o = normOwn(own)
  const s = cleanSel(sel, o.beats.length)
  const block = filled(s)
  if (block.length < 2 || !Number.isFinite(to) || (fixed !== 'first' && fixed !== 'last')) {
    return same(o, s)
  }
  const ms = o.beats.map(toMs)
  const a = block[0]
  const z = /** @type {number} */ (block.at(-1))
  const fromFirst = fixed === 'first'
  // lo que se mide siempre desde el extremo quieto hacia el que se mueve
  const sign = fromFirst ? 1 : -1
  const anchor = fromFirst ? ms[a] : ms[z]
  const origEnd = fromFirst ? ms[z] : ms[a]
  const span = Math.abs(origEnd - anchor)
  if (!span) return same(o, s)
  // el intervalo más corto fija lo que se puede encoger sin juntar dos pulsos
  let shortest = Infinity
  for (let k = a + 1; k <= z; k++) shortest = Math.min(shortest, ms[k] - ms[k - 1])
  const minScale = shortest > 0 ? Math.min(1, MIN_GAP_MS / shortest) : 1
  const minSpan = Math.ceil(span * minScale)
  // el rango en que puede caer el extremo que se mueve (tiempos en ms): lo
  // que ya estaba fuera de él no se agrava, pero quedarse siempre se puede
  let lo
  let hi
  if (fromFirst) {
    lo = anchor + minSpan
    hi = z < ms.length - 1 ? ms[z + 1] - MIN_GAP_MS : Infinity
    if (Number.isFinite(duration)) {
      hi = Math.min(hi, Math.floor(/** @type {number} */ (duration) * 1000 + 1e-6))
    }
    hi = Math.max(hi, origEnd)
  } else {
    hi = anchor - minSpan
    lo = Math.min(a > 0 ? ms[a - 1] + MIN_GAP_MS : 0, origEnd)
  }
  const spanTo = Math.abs(clamp(toMs(to), lo, hi) - anchor)
  const scale = spanTo / span
  const out = [...o.beats]
  /** @type {number[]} */
  const placed = []
  for (let k = a; k <= z; k++)
    placed.push(anchor + sign * Math.round(sign * (ms[k] - anchor) * scale))
  // el redondeo al milisegundo puede juntar dos pulsos en el recorte máximo:
  // se separan desde el extremo quieto hacia fuera
  if (fromFirst) {
    placed[placed.length - 1] = anchor + spanTo
    for (let k = 1; k < placed.length; k++) {
      const floor = Math.min(MIN_GAP_MS, ms[a + k] - ms[a + k - 1])
      placed[k] = Math.max(placed[k], placed[k - 1] + floor)
    }
  } else {
    placed[0] = anchor - spanTo
    for (let k = placed.length - 2; k >= 0; k--) {
      const floor = Math.min(MIN_GAP_MS, ms[a + k + 1] - ms[a + k])
      placed[k] = Math.min(placed[k], placed[k + 1] - floor)
    }
  }
  placed.forEach((p, k) => {
    if (p !== ms[a + k]) out[a + k] = p / 1000
  })
  return edited(o, out, [...o.bars], s)
}

/**
 * Reparte los pulsos de lo elegido (del primero al último, sin huecos) a
 * partes iguales: el primero y el último se quedan donde están.
 * @param {any} own @param {ReadonlyArray<number>} sel
 * @returns {Edited}
 */
export function spreadEven(own, sel) {
  const o = normOwn(own)
  const s = cleanSel(sel, o.beats.length)
  const block = filled(s)
  if (block.length < 2) return same(o, s)
  const a = block[0]
  const z = /** @type {number} */ (block.at(-1))
  const step = (toMs(o.beats[z]) - toMs(o.beats[a])) / (z - a)
  // sin sitio para ellos a `MIN_GAP`, se dejan como estaban
  if (step < MIN_GAP_MS) return same(o, s)
  const beats = [...o.beats]
  for (let k = 1; k < z - a; k++) beats[a + k] = round3(o.beats[a] + (k * step) / 1000)
  return edited(o, beats, [...o.bars], s)
}

// ------------------------------------------------------ el tempo de un tramo

/**
 * LA herramienta: cambia todos los pulsos de `[t0, t1)` por un peine regular
 * a `bpm`. La fase sale de `opts.anchor` (un tiempo en segundos que ha de
 * caer en un pulso); sin él, del primer pulso que había en el tramo, y si no
 * había, de `t0`. El peine se genera hacia delante y hacia atrás desde ahí,
 * y el primero que se deja es el menor `>= t0 - 5 ms`.
 *
 * Fuera del tramo no cambia nada. Para no dejar dos pulsos casi encima, se
 * descarta el generado que quede a menos de 0,4 periodos del primer pulso de
 * `t1` en adelante o del último de antes de `t0`.
 *
 * Los «1» de dentro pasan a ser uno de cada `meter` pulsos generados, de
 * modo que el generado más cercano al primer «1» viejo del tramo lo sea; si
 * el tramo no tenía ninguno, sigue la cuenta del compás del pulso anterior
 * (contando cuántos pulsos hay desde su último «1»). Como el número de pulsos
 * suele cambiar, los índices de los «1» de después se corren.
 *
 * `selection` son los pulsos generados.
 * @param {any} own @param {[number, number]} span @param {number} bpm
 * @param {{anchor?: number, duration?: number}} [opts]
 * @returns {Edited & {added: number, removed: number}}
 */
export function setTempoSpan(own, [t0, t1], bpm, opts = {}) {
  const o = normOwn(own)
  const nothing = () => ({ ...same(o, []), added: 0, removed: 0 })
  const tempo = Number(bpm)
  if (!(t1 > t0) || !Number.isFinite(t0) || !Number.isFinite(t1) || !Number.isFinite(tempo)) {
    return nothing()
  }
  const period = 60 / clamp(tempo, 20, 300)
  const i0 = lowerBound(o.beats, t0)
  const i1 = lowerBound(o.beats, t1)
  const before = i0 > 0 ? o.beats[i0 - 1] : null
  const after = i1 < o.beats.length ? o.beats[i1] : null
  const anchor = Number.isFinite(opts.anchor)
    ? /** @type {number} */ (opts.anchor)
    : i1 > i0
      ? o.beats[i0]
      : t0

  // el peine: el menor `anchor - m * period` que no baje de `t0 - 5 ms`
  const m = Math.floor((anchor - (t0 - 0.005)) / period + 1e-9)
  /** @type {Array<{k: number, t: number}>} */
  const comb = []
  for (let k = -m; comb.length < 50000; k++) {
    const raw = anchor + k * period
    if (raw >= t1) break
    comb.push({ k, t: round3(raw) })
  }

  // qué generados son «1» (por su número de orden `k`, para que los que se
  // descarten por estar pegados a un vecino no corran la cuenta)
  let isOne = (_k) => false
  if (o.meter >= 2 && comb.length) {
    const firstOne = o.bars.find((b) => b >= i0 && b < i1)
    const lastBefore = o.bars.filter((b) => b < i0).at(-1)
    if (firstOne !== undefined) {
      // el generado más cercano al primer «1» viejo (el de antes si empatan)
      const at = o.beats[firstOne]
      let best = comb[0]
      for (const c of comb) if (Math.abs(c.t - at) < Math.abs(best.t - at)) best = c
      isOne = (k) => mod(k - best.k, o.meter) === 0
    } else if (lastBefore !== undefined) {
      // sigue la cuenta del pulso anterior: el primero generado es el
      // siguiente a él
      const since = i0 - 1 - lastBefore
      isOne = (k) => mod(since + 1 + (k - comb[0].k), o.meter) === 0
    } else {
      isOne = (k) => mod(k - comb[0].k, o.meter) === 0
    }
  }

  const guard = 0.4 * period - 1e-9
  const limit = Number.isFinite(opts.duration) ? /** @type {number} */ (opts.duration) : Infinity
  const kept = comb.filter(
    ({ t }) =>
      t >= 0 &&
      t <= limit &&
      (before === null || t - before >= guard) &&
      (after === null || after - t >= guard)
  )
  // un tramo que dejaría la canción casi sin pulsos no se toca
  if (i0 + kept.length + (o.beats.length - i1) < 2) return nothing()

  const beats = [...o.beats.slice(0, i0), ...kept.map((c) => c.t), ...o.beats.slice(i1)]
  const shift = kept.length - (i1 - i0)
  const bars = [
    ...o.bars.filter((b) => b < i0),
    ...kept.flatMap((c, j) => (isOne(c.k) ? [i0 + j] : [])),
    ...o.bars.filter((b) => b >= i1).map((b) => b + shift)
  ]
  return {
    ...edited(
      o,
      beats,
      bars,
      kept.map((_, j) => i0 + j)
    ),
    added: kept.length,
    removed: i1 - i0
  }
}

// --------------------------------------------- doblar, partir y los compases

/**
 * Pone un pulso a mitad de cada hueco entre dos pulsos seguidos de lo
 * elegido (sin huecos): el último no lleva ninguno detrás. Los «1» se quedan
 * en sus pulsos. Un hueco que no deja `MIN_GAP` a cada lado se salta.
 * La selección que sale es el bloque de antes más los nuevos.
 * @param {any} own @param {ReadonlyArray<number>} sel
 * @returns {Edited}
 */
export function doubleBeats(own, sel) {
  const o = normOwn(own)
  const s = cleanSel(sel, o.beats.length)
  const block = filled(s)
  if (block.length < 2) return same(o, s)
  const a = block[0]
  const z = /** @type {number} */ (block.at(-1))
  const beats = []
  /** el sitio nuevo de cada pulso viejo */
  const moved = new Array(o.beats.length)
  for (let i = 0; i < o.beats.length; i++) {
    moved[i] = beats.length
    beats.push(o.beats[i])
    if (i >= a && i < z) {
      const mid = round3((o.beats[i] + o.beats[i + 1]) / 2)
      if (
        toMs(mid) - toMs(o.beats[i]) >= MIN_GAP_MS &&
        toMs(o.beats[i + 1]) - toMs(mid) >= MIN_GAP_MS
      ) {
        beats.push(mid)
      }
    }
  }
  return edited(
    o,
    beats,
    o.bars.map((b) => moved[b]),
    selectRange(moved[a], moved[z])
  )
}

/**
 * Quita uno de cada dos pulsos de lo elegido (sin huecos), de modo que se
 * quede el primer «1» de dentro (o, si no hay, el primer pulso). Los «1» de
 * los pulsos que se quitan se pierden; los que se quedan, lo siguen siendo.
 * La canción nunca se queda con menos de dos pulsos.
 * @param {any} own @param {ReadonlyArray<number>} sel
 * @returns {Edited}
 */
export function halveBeats(own, sel) {
  const o = normOwn(own)
  const s = cleanSel(sel, o.beats.length)
  const block = filled(s)
  if (block.length < 2) return same(o, s)
  const a = block[0]
  const z = /** @type {number} */ (block.at(-1))
  const origin = o.bars.find((b) => b >= a && b <= z) ?? a
  const gone = block.filter((i) => mod(i - origin, 2) === 1)
  if (o.beats.length - gone.length < 2) return same(o, s)
  const goneSet = new Set(gone)
  const beats = o.beats.filter((_, i) => !goneSet.has(i))
  const bars = reindexAfterRemoval(o.bars, gone)
  const selection = block.filter((i) => !goneSet.has(i)).map((i) => i - lowerBound(gone, i))
  return edited(o, beats, bars, selection)
}

/**
 * Un «1» en el primer pulso elegido y otro cada `n` pulsos (de 2 a 8) hasta
 * el último de lo elegido (sin huecos), quitando los «1» de antes que había
 * dentro. Fuera no se toca. Ojo: el compás de la canción (`meter`) sigue
 * siendo el que era; con un `n` mayor que él, los pulsos que van pasado el
 * compás desde el último «1» se acentúan igual, como en cualquier tramo sin
 * «1». Con `setMeter` el compás pasa a ser `n`.
 * @param {any} own @param {ReadonlyArray<number>} sel @param {number} n
 * @param {{setMeter?: boolean}} [opts]
 * @returns {Edited}
 */
export function barsEvery(own, sel, n, { setMeter = false } = {}) {
  const o = normOwn(own)
  const s = cleanSel(sel, o.beats.length)
  const block = filled(s)
  const every = Math.round(Number(n))
  if (!block.length || !Number.isFinite(every) || every < 2 || every > 8) return same(o, s)
  const a = block[0]
  const z = /** @type {number} */ (block.at(-1))
  const bars = o.bars.filter((b) => b < a || b > z)
  for (let i = a; i <= z; i += every) bars.push(i)
  bars.sort((x, y) => x - y)
  return { ...edited(o, [...o.beats], bars, s), ...(setMeter ? { meter: every } : {}) }
}

/**
 * Quita los pulsos elegidos y recoloca los «1» de los que quedan. Si la
 * canción se quedara con menos de dos pulsos, no se hace nada. La selección
 * que sale es vacía.
 * @param {any} own @param {ReadonlyArray<number>} sel
 * @returns {Edited}
 */
export function removeBeats(own, sel) {
  const o = normOwn(own)
  const s = cleanSel(sel, o.beats.length)
  if (!s.length || o.beats.length - s.length < 2) return same(o, s)
  const goneSet = new Set(s)
  return edited(
    o,
    o.beats.filter((_, i) => !goneSet.has(i)),
    reindexAfterRemoval(o.bars, s),
    []
  )
}

/**
 * Añade un pulso en `t`, salvo que haya otro a menos de 30 ms (entonces no se
 * hace nada y sale `refused: true`). Los «1» de los pulsos de después corren
 * un puesto, como ya hacía la pantalla. La selección que sale es el pulso
 * nuevo.
 * @param {any} own @param {number} t
 * @param {{duration?: number}} [opts]
 * @returns {Edited & {refused?: boolean}}
 */
export function addBeatAt(own, t, { duration } = {}) {
  const o = normOwn(own)
  const at = toMs(t)
  const refused =
    !Number.isFinite(t) ||
    at < 0 ||
    (Number.isFinite(duration) && at > toMs(/** @type {number} */ (duration))) ||
    o.beats.some((b) => Math.abs(toMs(b) - at) < ADD_GAP_MS)
  if (refused) return { ...same(o, []), refused: true }
  const index = lowerBound(o.beats, at / 1000)
  const beats = [...o.beats]
  beats.splice(index, 0, round3(t))
  return edited(
    o,
    beats,
    o.bars.map((b) => (b >= index ? b + 1 : b)),
    [index]
  )
}

/**
 * Devuelve a lo que decía el análisis un tramo: los pulsos de `own` con
 * `t0 <= t <= t1` pasan a ser los de `auto` en ese mismo tramo, con sus «1».
 * Si un extremo dejara dos pulsos a menos de `MIN_GAP` de los vecinos de
 * fuera, se descarta el de `auto` que quede de ese lado. La selección que
 * sale son los pulsos que vienen de `auto`.
 * @param {any} own  el clic de ahora
 * @param {any} auto  el clic hecho con la rejilla automática (`gridToOwn`)
 * @param {[number, number]} span
 * @returns {Edited}
 */
export function revertToAuto(own, auto, [t0, t1]) {
  const o = normOwn(own)
  const a = normOwn(auto)
  if (!(t1 >= t0)) return same(o, [])
  const i0 = lowerBound(o.beats, t0)
  const i1 = upperBound(o.beats, t1)
  const a0 = lowerBound(a.beats, t0)
  const a1 = upperBound(a.beats, t1)
  const autoOnes = new Set(a.bars)
  let incoming = []
  for (let j = a0; j < a1; j++) incoming.push({ t: a.beats[j], one: autoOnes.has(j) })
  const before = i0 > 0 ? toMs(o.beats[i0 - 1]) : null
  const after = i1 < o.beats.length ? toMs(o.beats[i1]) : null
  if (before !== null) {
    while (incoming.length && toMs(incoming[0].t) - before < MIN_GAP_MS) incoming.shift()
  }
  if (after !== null) {
    while (incoming.length && after - toMs(/** @type {any} */ (incoming.at(-1)).t) < MIN_GAP_MS) {
      incoming.pop()
    }
  }
  if (i0 + incoming.length + (o.beats.length - i1) < 2) return same(o, [])
  const beats = [...o.beats.slice(0, i0), ...incoming.map((c) => c.t), ...o.beats.slice(i1)]
  const shift = incoming.length - (i1 - i0)
  const bars = [
    ...o.bars.filter((b) => b < i0),
    ...incoming.flatMap((c, j) => (c.one ? [i0 + j] : [])),
    ...o.bars.filter((b) => b >= i1).map((b) => b + shift)
  ]
  return edited(
    o,
    beats,
    bars,
    incoming.map((_, j) => i0 + j)
  )
}
