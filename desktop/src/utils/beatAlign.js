// @ts-check
// Pegar el clic a lo que se oye: la parte que sabe dónde golpea el audio.
//
// Los golpes (`hits`) salen del análisis, `[[segundos, fuerza], …]` en orden:
// los picos de energía de la canción. La fuerza va en unidades de «subida de
// energía en logaritmo»: un golpe claro de batería ronda 2–6 y el ruido 0,3.
// Como eso cambia de una canción (y de un tramo) a otra, lo primero de todo es
// normalizar: `w = min(1, fuerza / ref)`, con `ref` el percentil 85 de las
// fuerzas del tramo (más un segundo de margen por cada lado), y fuera los
// golpes con `w < 0,15`.
//
// Nada aquí sabe de la pantalla ni toca lo que recibe. Los tiempos que
// devuelven van al milisegundo, y ninguna función acerca un pulso a menos de
// `MIN_GAP` de los vecinos que no mueve.

import { MIN_GAP } from './beatEdit.js'

/** @typedef {[number, number]} Hit  [segundos, fuerza > 0] */
/** @typedef {{n: number, supported: number, offsetMs: number, spreadMs: number}} Fit */
/** @typedef {{t: number[], w: number[]}} Prepared */

/** Hasta dónde se va a buscar el golpe de un pulso, en segundos. */
export const REACH_DEFAULT = 0.05
/** Un pulso «tiene golpe» si lo hay a menos de esto (25 ms). */
const SUPPORT_WINDOW = 0.025
/** Por debajo de esta fuerza normalizada, un golpe no cuenta. */
const WEAK = 0.15
/** Lo que se mira más allá del tramo, para tomar la medida de la fuerza. */
const MARGIN = 1
const MIN_GAP_MS = Math.round(MIN_GAP * 1000)
/** La anchura de un golpe «a tiempo» al alinear la fase (8 ms). */
const SIGMA = 0.008

const toMs = (t) => Math.round(t * 1000)
const round3 = (t) => Math.round(t * 1000) / 1000 + 0
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

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

/** La mediana (0 si no hay nada). @param {number[]} list */
function median(list) {
  if (!list.length) return 0
  const sorted = [...list].sort((a, b) => a - b)
  const mid = sorted.length >> 1
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2
}

/** El percentil `q` (0..1) de una lista ya ordenada, entre dos vecinos. */
function percentile(sorted, q) {
  if (!sorted.length) return 0
  const at = q * (sorted.length - 1)
  const lo = Math.floor(at)
  const hi = Math.min(sorted.length - 1, lo + 1)
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (at - lo)
}

/**
 * Los golpes de alrededor del tramo, ya con la fuerza normalizada y sin los
 * débiles.
 * @param {ReadonlyArray<Hit>} hits @param {number} t0 @param {number} t1
 * @returns {Prepared}
 */
function prepare(hits, t0, t1) {
  const list = Array.isArray(hits) ? hits : []
  let ordered = true
  for (let i = 1; i < list.length && ordered; i++) ordered = list[i][0] >= list[i - 1][0]
  const src = ordered ? list : [...list].sort((a, b) => a[0] - b[0])
  const lo = t0 - MARGIN
  const hi = t1 + MARGIN
  const near = src.filter(
    (h) => Number.isFinite(h[0]) && Number.isFinite(h[1]) && h[1] > 0 && h[0] >= lo && h[0] <= hi
  )
  const ref = percentile(
    near.map((h) => h[1]).sort((a, b) => a - b),
    0.85
  )
  /** @type {Prepared} */
  const out = { t: [], w: [] }
  if (!(ref > 0)) return out
  for (const [t, s] of near) {
    const w = Math.min(1, s / ref)
    if (w >= WEAK) {
      out.t.push(t)
      out.w.push(w)
    }
  }
  return out
}

/**
 * El golpe de un pulso: el que más puntúa de los que caen a menos de `reach`
 * (`fuerza * (1 - distancia / reach)`: fuerte y cerca). null si no hay.
 * @param {Prepared} prep @param {number} x @param {number} reach
 * @returns {{t: number, w: number}|null}
 */
function hitFor(prep, x, reach) {
  let best = null
  let bestScore = -1
  for (let j = lowerBound(prep.t, x - reach); j < prep.t.length && prep.t[j] <= x + reach; j++) {
    const score = prep.w[j] * (1 - Math.abs(prep.t[j] - x) / reach)
    if (score > bestScore) {
      bestScore = score
      best = { t: prep.t[j], w: prep.w[j] }
    }
  }
  return best
}

/**
 * Qué tal caen unos pulsos sobre los golpes.
 * @param {number[]} times @param {Prepared} prep @param {number} reach
 * @returns {Fit}
 */
function measure(times, prep, reach) {
  const n = times.length
  if (!n) return { n: 0, supported: 0, offsetMs: 0, spreadMs: 0 }
  const window = Math.max(reach, SUPPORT_WINDOW)
  let withHit = 0
  /** @type {number[]} */
  const offsets = []
  for (const b of times) {
    let closest = Infinity
    for (
      let j = lowerBound(prep.t, b - window);
      j < prep.t.length && prep.t[j] <= b + window;
      j++
    ) {
      closest = Math.min(closest, Math.abs(prep.t[j] - b))
    }
    if (closest <= SUPPORT_WINDOW) withHit++
    const hit = hitFor(prep, b, reach)
    if (hit) offsets.push((hit.t - b) * 1000)
  }
  const offsetMs = median(offsets)
  const spreadMs = offsets.length ? 1.4826 * median(offsets.map((o) => Math.abs(o - offsetMs))) : 0
  return { n, supported: withHit / n, offsetMs, spreadMs }
}

// ------------------------------------------------------------------ el informe

/**
 * Cómo caen los pulsos de `[t0, t1]` sobre los golpes del audio.
 *   · `supported`: la fracción (0..1) de pulsos con un golpe a menos de 25 ms.
 *   · `offsetMs`: la mediana de `(golpe - pulso)` en ms, sobre los pulsos con
 *     golpe a menos de `reach`. Negativo = el clic cae DESPUÉS del golpe (va
 *     tarde), igual que `offset_ms` del servidor.
 *   · `spreadMs`: cuánto se dispersan esos desfases (1,4826 × la desviación
 *     absoluta mediana).
 * Sin golpes cerca, `offsetMs`, `spreadMs` y `supported` valen 0.
 * @param {ReadonlyArray<number>} beats @param {ReadonlyArray<Hit>} hits
 * @param {[number, number]} span @param {{reach?: number}} [opts]
 * @returns {Fit}
 */
export function fitReport(beats, hits, [t0, t1], { reach = REACH_DEFAULT } = {}) {
  const inside = beats.filter((b) => b >= t0 && b <= t1)
  return measure(inside, prepare(hits, t0, t1), Math.max(0.001, reach))
}

// --------------------------------------------------------------------- fase

/** Lo que aporta un golpe a un pulso en `x`: su fuerza por lo cerca que cae. */
function bump(prep, x) {
  let best = 0
  const near = 4 * SIGMA
  for (let j = lowerBound(prep.t, x - near); j < prep.t.length && prep.t[j] <= x + near; j++) {
    const z = (prep.t[j] - x) / SIGMA
    best = Math.max(best, prep.w[j] * Math.exp(-0.5 * z * z))
  }
  return best
}

/**
 * El único corrimiento (de -reach a +reach, de milisegundo en milisegundo y
 * afinado con una parábola) que más pega los pulsos a los golpes. Sin golpes
 * cerca de ninguna posición, 0.
 * @param {number[]} times @param {Prepared} prep @param {number} reach
 * @returns {number} segundos
 */
function phaseShift(times, prep, reach) {
  const steps = Math.round(reach * 1000)
  /** @type {number[]} */
  const scores = []
  for (let s = -steps; s <= steps; s++) {
    let total = 0
    for (const b of times) total += bump(prep, b + s / 1000)
    scores.push(total)
  }
  let best = 0
  for (let k = 1; k < scores.length; k++) {
    const better = scores[k] > scores[best] + 1e-12
    const tie =
      Math.abs(scores[k] - scores[best]) <= 1e-12 && Math.abs(k - steps) < Math.abs(best - steps)
    if (better || tie) best = k
  }
  if (!(scores[best] > 1e-9)) return 0
  let frac = 0
  if (best > 0 && best < scores.length - 1) {
    const lo = scores[best - 1]
    const hi = scores[best + 1]
    const curve = lo - 2 * scores[best] + hi
    if (curve < 0) frac = clamp((0.5 * (lo - hi)) / curve, -1, 1)
  }
  return (best - steps + frac) / 1000
}

// -------------------------------------------------------------------- recta

/**
 * Mínimos cuadrados ponderados de `y = a + b·k`. null si no se puede.
 * @param {number[]} ks @param {number[]} ys @param {number[]} ws
 * @returns {{a: number, b: number}|null}
 */
function weightedLine(ks, ys, ws) {
  let sw = 0
  let sk = 0
  let sy = 0
  let skk = 0
  let sky = 0
  for (let i = 0; i < ks.length; i++) {
    sw += ws[i]
    sk += ws[i] * ks[i]
    sy += ws[i] * ys[i]
    skk += ws[i] * ks[i] * ks[i]
    sky += ws[i] * ks[i] * ys[i]
  }
  const det = sw * skk - sk * sk
  if (!(sw > 1e-9) || !(Math.abs(det) > 1e-9)) return null
  const b = (sw * sky - sk * sy) / det
  return { a: (sy - b * sk) / sw, b }
}

/**
 * La recta `t_k = a + b·k` (tempo constante y fase) que mejor pega al audio,
 * por mínimos cuadrados reponderados con pesos de Tukey: los pulsos sin golpe
 * cerca, o con uno que no cuadra con los demás, casi no cuentan. Arranca de
 * donde están los pulsos (corridos a la mejor fase) y va estrechando la
 * tolerancia. null si no hay recta que valga (o el tempo se va a otro).
 * @param {number[]} times @param {Prepared} prep @param {number} reach
 * @returns {number[]|null}
 */
function lineFit(times, prep, reach) {
  const m = times.length
  const ks = times.map((_, k) => k)
  const start = weightedLine(
    ks,
    times,
    times.map(() => 1)
  )
  if (!start || !(start.b > 0)) return null
  let a = start.a + phaseShift(times, prep, reach)
  let b = start.b
  for (let it = 0; it < 12; it++) {
    const cut = Math.max(0.012, reach * 0.7 ** it)
    /** @type {number[]} */
    const hitTimes = []
    /** @type {number[]} */
    const hitKs = []
    /** @type {number[]} */
    const weights = []
    for (let k = 0; k < m; k++) {
      const at = a + b * k
      const hit = hitFor(prep, at, reach)
      if (!hit) continue
      const u = (hit.t - at) / cut
      const tukey = Math.abs(u) < 1 ? (1 - u * u) ** 2 : 0
      if (tukey <= 0) continue
      hitTimes.push(hit.t)
      hitKs.push(k)
      weights.push(hit.w * tukey)
    }
    const next = weightedLine(hitKs, hitTimes, weights)
    if (!next) break
    a = next.a
    b = next.b
  }
  if (!(b > 0) || Math.abs(b / start.b - 1) > 0.25) return null
  return ks.map((k) => a + b * k)
}

// ------------------------------------------------------------------- seguir

/** Lo que cuesta cambiar un intervalo: de casi nada (sigue todo) a muchísimo. */
const LAMBDA_FREE = 20
const LAMBDA_STIFF = 100000
/** Los candidatos que se guardan por pulso (los que más puntúan). */
const CANDIDATES = 5
/** El castigo por alejarse del sitio que tenía, a `reach` de distancia. */
const DISTANCE_COST = 0.2

/** De la rigidez (0..1) a lo que se paga por torcer el ritmo. */
function lambdaOf(rigidity) {
  const r = clamp(Number.isFinite(rigidity) ? rigidity : 0.5, 0, 1)
  return LAMBDA_FREE * (LAMBDA_STIFF / LAMBDA_FREE) ** r
}

/**
 * Cada pulso puede irse a un golpe de `reach` a la redonda, o quedarse (sin
 * pruebas). Se elige el camino de toda la tanda con Viterbi: suma la fuerza de
 * los golpes, resta un poco por alejarse y resta `λ·(Δintervalo/periodo)²`,
 * donde `Δintervalo` es lo que cambia cada intervalo respecto del que tenía.
 * Un pulso que se queda sin pruebas conserva el intervalo que tenía con el
 * anterior (no da saltos).
 * @param {number[]} times @param {Prepared} prep @param {number} reach
 * @param {number} rigidity
 * @returns {number[]}
 */
function followFit(times, prep, reach, rigidity) {
  const m = times.length
  const gaps = []
  for (let k = 1; k < m; k++) gaps.push(times[k] - times[k - 1])
  const period = median(gaps) || 0.5
  const lambda = lambdaOf(rigidity)
  /** @typedef {{score: number, off: number|null, from: number}} Cell */
  /** @type {Cell[][]} */
  const layers = []
  for (let k = 0; k < m; k++) {
    const found = []
    for (
      let j = lowerBound(prep.t, times[k] - reach);
      j < prep.t.length && prep.t[j] <= times[k] + reach;
      j++
    ) {
      const off = prep.t[j] - times[k]
      found.push({
        off,
        gain: prep.w[j] - (DISTANCE_COST * Math.abs(off)) / reach,
        rank: prep.w[j] * (1 - Math.abs(off) / reach)
      })
    }
    found.sort((x, y) => y.rank - x.rank)
    const prev = layers[k - 1]
    /** @type {Cell[]} */
    const cells = []
    // sin pruebas: se queda con el desfase que traía el mejor de antes
    if (!prev) cells.push({ score: 0, off: null, from: -1 })
    else {
      let from = 0
      prev.forEach((cell, i) => {
        if (cell.score > prev[from].score) from = i
      })
      cells.push({ score: prev[from].score, off: prev[from].off, from })
    }
    for (const c of found.slice(0, CANDIDATES)) {
      if (!prev) {
        cells.push({ score: c.gain, off: c.off, from: -1 })
        continue
      }
      let bestScore = -Infinity
      let from = 0
      prev.forEach((cell, i) => {
        const bend = cell.off === null ? 0 : lambda * ((c.off - cell.off) / period) ** 2
        const score = cell.score + c.gain - bend
        if (score > bestScore) {
          bestScore = score
          from = i
        }
      })
      cells.push({ score: bestScore, off: c.off, from })
    }
    layers.push(cells)
  }
  // el camino, de atrás hacia delante
  const offsets = new Array(m)
  let at = 0
  layers[m - 1].forEach((cell, i) => {
    if (cell.score > layers[m - 1][at].score) at = i
  })
  for (let k = m - 1; k >= 0; k--) {
    offsets[k] = layers[k][at].off
    at = layers[k][at].from
  }
  // los primeros, sin pruebas, toman el desfase del primero que las tuvo
  const known = offsets.find((o) => o !== null) ?? 0
  return times.map((t, k) => t + (offsets[k] ?? known))
}

// -------------------------------------------------------------- alinear todo

/**
 * @typedef {{
 *   how?: 'phase'|'follow'|'line', reach?: number, rigidity?: number,
 *   lead?: number, minSupport?: number, force?: boolean
 * }} AlignOpts
 */

/**
 * Los pulsos del tramo que se pueden poner en `P` (ms) sin pisar a los
 * vecinos de fuera ni dejar dos a menos de `MIN_GAP`. null si no caben.
 * @param {number[]} want ms que se quieren @param {number[]} was ms que tenían
 * @param {number|null} before @param {number|null} after los vecinos de fuera, en ms
 */
function fitBetween(want, was, before, after) {
  const placed = [...want]
  const floorAt = (k) => Math.min(MIN_GAP_MS, was[k] - was[k - 1])
  const low = before === null ? 0 : before + MIN_GAP_MS
  placed[0] = Math.max(placed[0], low)
  for (let k = 1; k < placed.length; k++)
    placed[k] = Math.max(placed[k], placed[k - 1] + floorAt(k))
  const last = placed.length - 1
  if (after !== null) placed[last] = Math.min(placed[last], after - MIN_GAP_MS)
  for (let k = last - 1; k >= 0; k--)
    placed[k] = Math.min(placed[k], placed[k + 1] - floorAt(k + 1))
  return placed[0] < low ? null : placed
}

/**
 * Pega los pulsos de `[t0, t1]` a los golpes del audio, SOLO si de verdad
 * pegan. `opts.how`:
 *   · `phase`: un único corrimiento (de ±`reach`), que mantiene la separación
 *     de los pulsos tal cual. Lo que se usa tras escribir un tempo: encuentra
 *     la fase del peine sin torcer el tempo.
 *   · `line`: la mejor recta (tempo constante y fase) con mínimos cuadrados
 *     robustos. Para canciones grabadas a un clic que se desvían. Sale
 *     perfectamente regular.
 *   · `follow`: cada pulso puede irse a un golpe de `reach` a la redonda (o
 *     quedarse). `rigidity` (0..1) es cuánto se resiste el ritmo a torcerse:
 *     con 0 sigue todos los golpes, con 1 es casi `phase`.
 * `lead` (ms, negativo = antes) se suma al final a todos los pulsos del
 * tramo: el clic por delante del golpe. Si, con el resultado, menos de
 * `minSupport` de los pulsos tiene golpe a 25 ms, y no hay `force`, no se
 * mueve nada: `applied: false`, `reason: 'no pega'` (`'sin golpes'` si el
 * tramo no tiene ninguno que valga, `'sin pulsos'` si no tiene pulsos). El
 * `lead` no entra en esa cuenta.
 *
 * `report.after` son los pulsos como quedan (si no se movió nada, como
 * estaban); `report.candidate`, lo que habría salido sin el `lead` (para
 * decir «solo pegaba un 30 %»). `moved` cuenta los pulsos que cambiaron.
 * @param {{beats: number[]}} own @param {[number, number]} span
 * @param {ReadonlyArray<Hit>} hits @param {AlignOpts} [opts]
 * @returns {{
 *   beats: number[], applied: boolean,
 *   report: {n: number, before: Fit, after: Fit, candidate: Fit, moved: number, reason: string}
 * }}
 */
export function alignBeats(own, [t0, t1], hits, opts = {}) {
  const { how = 'phase', rigidity = 0.5, lead = 0, minSupport = 0.5, force = false } = opts
  const reach = Math.max(
    0.001,
    Number.isFinite(opts.reach) ? /** @type {number} */ (opts.reach) : REACH_DEFAULT
  )
  const beats = Array.isArray(own?.beats) ? own.beats.map(Number) : []
  const i0 = lowerBound(beats, t0)
  const i1 = upperBound(beats, t1)
  const span = beats.slice(i0, i1)
  const prep = prepare(hits, t0, t1)
  const before = measure(span, prep, reach)
  /** @param {string} reason @param {Fit} [candidate] */
  const untouched = (reason, candidate = before) => ({
    beats: [...beats],
    applied: false,
    report: { n: span.length, before, after: before, candidate, moved: 0, reason }
  })
  if (!span.length) return untouched('sin pulsos')
  const hasHits = lowerBound(prep.t, t0 - reach) < upperBound(prep.t, t1 + reach)
  if (!hasHits) return untouched('sin golpes')

  /** @type {number[]|null} */
  let candidate
  if (how === 'line' && span.length >= 3) candidate = lineFit(span, prep, reach)
  else if (how === 'follow') candidate = followFit(span, prep, reach, rigidity)
  else {
    const shift = phaseShift(span, prep, reach)
    candidate = span.map((t) => t + shift)
  }
  if (!candidate) return untouched('no pega')
  candidate = candidate.map(round3)
  const wouldBe = measure(candidate, prep, reach)
  if (!force && wouldBe.supported < minSupport - 1e-9) return untouched('no pega', wouldBe)

  const leadMs = Number.isFinite(lead) ? lead : 0
  const placed = fitBetween(
    candidate.map((t) => toMs(t + leadMs / 1000)),
    span.map(toMs),
    i0 > 0 ? toMs(beats[i0 - 1]) : null,
    i1 < beats.length ? toMs(beats[i1]) : null
  )
  if (!placed) return untouched('no cabe', wouldBe)
  const out = [...beats]
  let moved = 0
  placed.forEach((p, k) => {
    if (p === toMs(span[k])) return
    out[i0 + k] = p / 1000
    moved++
  })
  return {
    beats: out,
    applied: true,
    report: {
      n: span.length,
      before,
      after: measure(out.slice(i0, i1), prep, reach),
      candidate: wouldBe,
      moved,
      reason: ''
    }
  }
}

// ----------------------------------------------------------------- el tempo

/** Lo que se reparte entre octavas con `around`: 1 la cercana, menos las demás. */
const OCTAVE_NEAR = 0.06
const OCTAVE_SIBLING = 0.93
const OCTAVE_OTHER = 0.85
/** El ancho del golpe «a tiempo» en la búsqueda de tempo (10 ms). */
const TEMPO_SIGMA = 0.01
/** El ancho de los cajones en que se pliegan los golpes (5 ms). */
const BIN = 0.005
const KERNEL_RADIUS = Math.ceil((3 * TEMPO_SIGMA) / BIN)
const KERNEL = Array.from({ length: 2 * KERNEL_RADIUS + 1 }, (_, i) => {
  const z = ((i - KERNEL_RADIUS) * BIN) / TEMPO_SIGMA
  return Math.exp(-0.5 * z * z)
})

/**
 * Cuánto pesa un tempo según el que se espera: el que está cerca de `around`,
 * entero; el doble o la mitad, un poco menos; cualquier otro, bastante menos.
 * Sin `around`, todos igual.
 */
function octaveWeight(bpm, around) {
  if (!(around > 0)) return 1
  const near = (x) => Math.abs(bpm / x - 1) <= OCTAVE_NEAR
  if (near(around)) return 1
  return near(around * 2) || near(around / 2) ? OCTAVE_SIBLING : OCTAVE_OTHER
}

/**
 * Pliega los golpes sobre un periodo `p` y devuelve cuánto suman en el mejor
 * sitio del compás: cuánto pega un peine de ese periodo con la mejor fase.
 * @param {number[]} ts @param {number[]} ws @param {number} t0 @param {number} p
 * @returns {{score: number, phase: number}} `phase`: segundos desde `t0`
 */
function foldHits(ts, ws, t0, p) {
  const bins = Math.max(2 * KERNEL_RADIUS + 1, Math.round(p / BIN))
  const width = p / bins
  const wrapped = new Float64Array(bins + 2 * KERNEL_RADIUS)
  for (let i = 0; i < ts.length; i++) {
    const at = Math.min(bins - 1, Math.floor(((ts[i] - t0) % p) / width))
    wrapped[KERNEL_RADIUS + at] += ws[i]
  }
  // el principio y el final se envuelven, que el peine es circular
  for (let i = 0; i < KERNEL_RADIUS; i++) {
    wrapped[i] = wrapped[bins + i]
    wrapped[KERNEL_RADIUS + bins + i] = wrapped[KERNEL_RADIUS + i]
  }
  let best = -1
  let bestBin = 0
  for (let i = 0; i < bins; i++) {
    let sum = 0
    for (let j = 0; j < KERNEL.length; j++) sum += KERNEL[j] * wrapped[i + j]
    if (sum > best) {
      best = sum
      bestBin = i
    }
  }
  return { score: best, phase: (bestBin + 0.5) * width }
}

/**
 * El tempo cuyo peine regular mejor pega con los golpes de `[t0, t1]`.
 * Busca por todos los bpm de `min` a `max` con un paso que se estrecha en
 * cuanto el peine se desvía unos milisegundos a lo largo del tramo (hace
 * falta más fino cuanto más largo es), y afina alrededor de lo mejor.
 *
 * La puntuación es lo que suman los golpes sobre el peine, con mejor fase,
 * dividido por los puntos del peine; a igualdad gana el tempo más rápido
 * (el peine de la mitad también pega con todo lo que pega el entero).
 * Con `around` (los bpm que se esperan), el octavo se resuelve a su favor:
 * ese tempo cuenta entero, su doble y su mitad al 93 % y los demás al 85 %.
 *
 *   · `support`: la fracción de puntos del peine con un golpe a 25 ms.
 *   · `phase`: el primer punto del peine desde `t0` (segundos), por si se
 *     quiere usar de `anchor` en `setTempoSpan`.
 * null con menos de 4 golpes que valgan, o si el tramo no da para 4 pulsos al
 * tempo más lento.
 * @param {ReadonlyArray<Hit>} hits @param {[number, number]} span
 * @param {{around?: number, min?: number, max?: number}} [opts]
 * @returns {{bpm: number, support: number, phase: number}|null}
 */
export function estimateTempo(hits, [t0, t1], { around = 0, min = 40, max = 240 } = {}) {
  if (!(t1 > t0) || !(min > 0) || !(max > min)) return null
  if (t1 - t0 < (4 * 60) / min) return null
  const prep = prepare(hits, t0, t1)
  const from = lowerBound(prep.t, t0)
  const to = upperBound(prep.t, t1)
  const ts = prep.t.slice(from, to)
  const ws = prep.w.slice(from, to)
  if (ts.length < 4) return null
  const length = t1 - t0

  /** Lo que vale un tempo: el peine que más pega, por punto del peine. */
  const rate = (bpm) => {
    const p = 60 / bpm
    const fold = foldHits(ts, ws, t0, p)
    // los puntos que caen de verdad en el tramo con esa fase (no `length / p`:
    // con pocos puntos, uno de más o de menos cambia la cuenta)
    const points = Math.floor((length - fold.phase) / p + 1e-9) + 1
    // (un 4 % por octava) para que a igualdad gane el más rápido: el peine
    // de la mitad pega con todo lo que pega el entero, y con ruido le sobra
    const faster = 1 + 0.04 * Math.log2(bpm / min)
    return { score: (fold.score / points) * octaveWeight(bpm, around) * faster, phase: fold.phase }
  }

  // la primera pasada: cada paso se desvía a lo sumo ~8 ms al final del tramo
  /** @type {Array<{bpm: number, score: number}>} */
  const coarse = []
  for (let bpm = min; bpm <= max;) {
    coarse.push({ bpm, score: rate(bpm).score })
    bpm += Math.max(0.03, (0.016 * bpm) / length)
  }
  // los picos de esa pasada: los mejores, cada uno con su entorno
  const peaks = coarse
    .map((c, i) => ({ ...c, i }))
    .filter(
      (c) =>
        (c.i === 0 || c.score >= coarse[c.i - 1].score) &&
        (c.i === coarse.length - 1 || c.score > coarse[c.i + 1].score)
    )
    .sort((x, y) => y.score - x.score)
    .slice(0, 6)
  let best = { bpm: peaks[0].bpm, score: -1, phase: 0 }
  for (const peak of peaks) {
    const lo = coarse[Math.max(0, peak.i - 1)].bpm
    const hi = coarse[Math.min(coarse.length - 1, peak.i + 1)].bpm
    const step = Math.max(1e-4, (hi - lo) / 16)
    for (let bpm = lo; bpm <= hi + 1e-9; bpm += step) {
      const r = rate(bpm)
      if (r.score > best.score) best = { bpm, score: r.score, phase: r.phase }
    }
  }
  if (!(best.score > 0)) return null

  // la fase en limpio: el centro de los golpes que caen junto al peine
  const p = 60 / best.bpm
  let sum = 0
  let weight = 0
  for (let i = 0; i < ts.length; i++) {
    const rel = ((((ts[i] - t0 - best.phase + p / 2) % p) + p) % p) - p / 2
    if (Math.abs(rel) <= 2 * TEMPO_SIGMA) {
      sum += ws[i] * rel
      weight += ws[i]
    }
  }
  let phase = best.phase + (weight ? sum / weight : 0)
  phase = ((phase % p) + p) % p
  // cuántos puntos del peine tienen un golpe cerca
  let points = 0
  let hit = 0
  for (let c = t0 + phase; c <= t1 + 1e-9; c += p) {
    points++
    const j = lowerBound(ts, c - SUPPORT_WINDOW)
    if (j < ts.length && ts[j] <= c + SUPPORT_WINDOW) hit++
  }
  return {
    bpm: Math.round(best.bpm * 100) / 100,
    support: points ? hit / points : 0,
    phase: round3(t0 + phase)
  }
}
