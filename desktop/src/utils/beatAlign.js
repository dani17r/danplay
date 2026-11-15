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
/**
 * @typedef {{
 *   n: number, supported: number, offsetMs: number, spreadMs: number,
 *   chance: number, excess: number
 * }} Fit
 */
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
/** Cuánto `excess` se puede perder sin que se diga que «empeora» (3 puntos). */
const WORSE_EXCESS = 0.03
/** Dentro de esto (2 puntos) el `excess` cuenta como el mismo. */
const TIE_EXCESS = 0.02
/** Cuánto `excess` hay que ganar para aceptar un resultado por debajo de `minExcess`. */
const BETTER_EXCESS = 0.12
/** Lo que tiene que pegar de más sobre el azar un resultado para aceptarlo. */
const MIN_EXCESS = 0.4
/**
 * Y lo que, como mínimo (esta fracción de `minExcess`), ha de pegar el que se
 * acepta por ser «claramente mejor». Ajustar a la onda elige la mejor de
 * muchas posiciones, y eso solo ya sube el `excess` de un peine al azar de
 * 0,1 a 0,3: sin este suelo, entre un 3 y un 18 % de los peines al azar de
 * Sal 22 se aceptaban por ganar 12 puntos; con él, casi ninguno.
 */
const BETTER_FLOOR = 0.75
/** La dispersión es «peor» si pasa de `antes · 1,3 + 3 ms`. */
const SPREAD_SLACK = 1.3
const SPREAD_PAD = 3
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

/** ¿Hay un golpe a menos de 25 ms de `x`? */
function hasHitNear(prep, x) {
  const j = lowerBound(prep.t, x - SUPPORT_WINDOW)
  return j < prep.t.length && prep.t[j] <= x + SUPPORT_WINDOW
}

/**
 * Los corrimientos con que se mide lo que pegaría un peine «al azar», como
 * fracciones del intervalo entre pulsos: lejos de la fase de verdad (0) y de
 * la mitad del intervalo (0,5), que es donde pega el doble.
 */
const NULL_SHIFTS = [0.13, 0.29, 0.41, 0.59, 0.71, 0.87]
/** Con más de esto de azar no se mide nada: no se divide por casi cero. */
const CHANCE_CAP = 0.9

/**
 * Qué tal caen unos pulsos sobre los golpes.
 *
 * `chance` es cuánto pegarían estos mismos pulsos corridos a sitios que no
 * son suyos (la mediana de `supported` con seis corrimientos fijos, en
 * fracciones del intervalo): la música de verdad está llena de golpes
 * (11–15 por segundo), y un peine a un tempo cualquiera ya cae sobre uno de
 * cada dos. Como el corrimiento conserva la estructura de los golpes (la
 * rejilla del charles, las corcheas), es una vara de medir mejor que una
 * probabilidad. `excess` es lo que pega de más sobre el azar, de 0 a 1.
 * @param {number[]} times @param {Prepared} prep @param {number} reach
 * @returns {Fit}
 */
function measure(times, prep, reach) {
  const n = times.length
  if (!n) return { n: 0, supported: 0, offsetMs: 0, spreadMs: 0, chance: 0, excess: 0 }
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
  const supported = withHit / n

  let chance = 0
  if (prep.t.length) {
    const gaps = []
    for (let k = 1; k < n; k++) gaps.push(times[k] - times[k - 1])
    const interval = median(gaps) || 0.5
    chance = median(
      NULL_SHIFTS.map((f) => {
        let hit = 0
        for (const b of times) if (hasHitNear(prep, b + f * interval)) hit++
        return hit / n
      })
    )
  }
  const excess = clamp((supported - chance) / (1 - Math.min(chance, CHANCE_CAP)), 0, 1)
  return { n, supported, offsetMs, spreadMs, chance, excess }
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
 *   · `chance`: cuánto pegarían esos mismos pulsos corridos a sitios que no
 *     son suyos (ver `measure`), y `excess` lo que pegan de más sobre eso,
 *     `(supported − chance) / (1 − chance)` entre 0 y 1: la medida de si de
 *     verdad pegan o es la música, que está llena de golpes.
 * Sin golpes cerca, todo vale 0.
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
 *   lead?: number, minExcess?: number, minSupport?: number, force?: boolean
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
 * tramo: el clic por delante del golpe.
 *
 * SOLO donde mejora, y sin `force` no se mueve nada (`applied: false`) si:
 *   · `reason: 'empeora'`: el resultado pega 3 puntos menos de `excess` que
 *     lo de ahora, o tiene una dispersión de más de `antes · 1,3 + 3 ms` sin
 *     ganar `excess` (más de 2 puntos);
 *   · `reason: 'no pega'`: pega menos de `minExcess` (0,4) por encima del
 *     azar y tampoco es claramente mejor (12 puntos más de `excess`, sin más
 *     dispersión y con al menos las tres cuartas partes de `minExcess`: un
 *     tramo a medio tiempo solo apoya la mitad de un peine, y pasar de nada a
 *     algo claro sí se aplica, pero ajustar elige la mejor de muchas
 *     posiciones y eso sube el azar por sí solo). Quien pase `minSupport`
 *     pide además que caiga sobre un golpe al menos esa fracción de pulsos;
 *   · `reason: 'sin golpes'`: el tramo no tiene ninguno que valga;
 *   · `reason: 'sin pulsos'`: no tiene pulsos (y `'no cabe'`: no hay sitio a
 *     20 ms de los vecinos de fuera).
 * `excess` (ver `fitReport`) es lo que pega de más sobre el azar: la música
 * está llena de golpes y un peine cualquiera ya cae sobre la mitad.
 * El `lead` no entra en esas cuentas.
 *
 * `report.after` son los pulsos como quedan (si no se movió nada, como
 * estaban); `report.candidate`, lo que habría salido sin el `lead` (para
 * decir «solo pegaba un 30 %»). `moved` cuenta los pulsos que cambiaron.
 * `report.spreadReliable` es false con `follow`: cada pulso se pega a su
 * golpe y la dispersión de después sale casi 0 sin que eso diga nada; lo que
 * se mide de verdad es `supported` y `offsetMs`.
 * @param {{beats: number[]}} own @param {[number, number]} span
 * @param {ReadonlyArray<Hit>} hits @param {AlignOpts} [opts]
 * @returns {{
 *   beats: number[], applied: boolean,
 *   report: {n: number, before: Fit, after: Fit, candidate: Fit, moved: number, reason: string, spreadReliable: boolean}
 * }}
 */
export function alignBeats(own, [t0, t1], hits, opts = {}) {
  const { how = 'phase', rigidity = 0.5, lead = 0, force = false } = opts
  const minExcess = Number.isFinite(opts.minExcess)
    ? /** @type {number} */ (opts.minExcess)
    : MIN_EXCESS
  // `minSupport` ya no manda, pero quien lo pide a mano lo sigue teniendo
  const minSupport = Number.isFinite(opts.minSupport) ? /** @type {number} */ (opts.minSupport) : 0
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
    report: {
      n: span.length,
      before,
      after: before,
      candidate,
      moved: 0,
      reason,
      spreadReliable: how !== 'follow'
    }
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
  if (!force) {
    const gain = wouldBe.excess - before.excess
    // un resultado que deja las cosas peor que como están no se aplica, aunque
    // todavía «pegue» (el 93 % de antes era mejor que un 72 %)
    const lessSpread = wouldBe.spreadMs > before.spreadMs * SPREAD_SLACK + SPREAD_PAD
    if (gain < -WORSE_EXCESS - 1e-9 || (lessSpread && gain <= TIE_EXCESS + 1e-9)) {
      return untouched('empeora', wouldBe)
    }
    // y uno que pega poco sobre el azar se acepta si es claramente mejor que
    // lo que hay: un tramo a medio tiempo solo apoya la mitad de los pulsos
    const better =
      gain >= BETTER_EXCESS - 1e-9 &&
      !lessSpread &&
      wouldBe.excess >= BETTER_FLOOR * minExcess - 1e-9
    const weak = wouldBe.excess < minExcess - 1e-9 && !better
    if (weak || wouldBe.supported < minSupport - 1e-9) return untouched('no pega', wouldBe)
  }

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
      reason: '',
      spreadReliable: how !== 'follow'
    }
  }
}

// ----------------------------------------------------------------- el tempo

/** Cuánto se mira alrededor del tempo que se espera (±12 %). */
const WINDOW_DEFAULT = 0.12
/** Qué fracción del mejor `excess` debe tener un tempo más rápido de la misma familia. */
const FAMILY_KEEP = 0.85
/**
 * Donde se busca el pulso «de contar» cuando no se sabe el tempo: de 60 a 180
 * bpm. Los golpes de la corchea (el charles) apoyan el peine del doble tanto
 * como el del pulso, y sin este techo «el más rápido» sería siempre el 240.
 */
const TACTUS = /** @type {[number, number]} */ ([60, 180])
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
 * Las búsquedas de tempo sobre los golpes de un tramo.
 * @param {number[]} ts @param {number[]} ws @param {number} t0 @param {number} t1
 * @param {Prepared} prep  los mismos golpes con su margen, para medir cada peine
 */
function tempoSearch(ts, ws, t0, t1, prep) {
  const length = t1 - t0

  /** Lo que vale un tempo: el peine que más pega, por punto del peine. */
  const rate = (bpm) => {
    const p = 60 / bpm
    const fold = foldHits(ts, ws, t0, p)
    // los puntos que caen de verdad en el tramo con esa fase (no `length / p`:
    // con pocos puntos, uno de más o de menos cambia la cuenta)
    const points = Math.floor((length - fold.phase) / p + 1e-9) + 1
    return { score: fold.score / points, phase: fold.phase }
  }

  /**
   * El mejor peine con un tempo entre `lo` y `hi`: una pasada con un paso que
   * se desvía a lo sumo ~8 ms al final del tramo, y se afinan los mejores
   * picos de esa pasada.
   * @param {number} lo @param {number} hi @param {number} [keep] cuántos picos se afinan
   * @returns {{bpm: number, score: number, phase: number}|null}
   */
  const band = (lo, hi, keep = 3) => {
    /** @type {Array<{bpm: number, score: number}>} */
    const coarse = []
    for (let bpm = lo; bpm <= hi + 1e-9; bpm += Math.max(0.03, (0.016 * bpm) / length)) {
      coarse.push({ bpm, score: rate(bpm).score })
    }
    if (!coarse.length) return null
    const peaks = coarse
      .map((c, i) => ({ ...c, i }))
      .filter(
        (c) =>
          (c.i === 0 || c.score >= coarse[c.i - 1].score) &&
          (c.i === coarse.length - 1 || c.score > coarse[c.i + 1].score)
      )
      .sort((x, y) => y.score - x.score)
      .slice(0, keep)
    let best = { bpm: peaks[0].bpm, score: -1, phase: 0 }
    for (const peak of peaks) {
      const from = coarse[Math.max(0, peak.i - 1)].bpm
      const to = coarse[Math.min(coarse.length - 1, peak.i + 1)].bpm
      const step = Math.max(1e-4, (to - from) / 16)
      for (let bpm = from; bpm <= to + 1e-9; bpm += step) {
        const r = rate(bpm)
        if (r.score > best.score) best = { bpm, score: r.score, phase: r.phase }
      }
    }
    return best.score > 0 ? best : null
  }

  /**
   * Un peine ya hallado, en limpio: la fase afinada con el centro de los
   * golpes que caen junto a él y cómo cae sobre los golpes (`support`, y
   * `chance` y `excess` como en `fitReport`: sus puntos son los pulsos).
   * @param {{bpm: number, phase: number}} best
   * @returns {{bpm: number, support: number, phase: number, chance: number, excess: number}}
   */
  const finish = (best) => {
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
    const phase = (((best.phase + (weight ? sum / weight : 0)) % p) + p) % p
    const points = []
    for (let c = t0 + phase; c <= t1 + 1e-9; c += p) points.push(c)
    const fit = measure(points, prep, REACH_DEFAULT)
    return {
      bpm: Math.round(best.bpm * 100) / 100,
      support: fit.supported,
      phase: round3(t0 + phase),
      chance: fit.chance,
      excess: fit.excess
    }
  }

  return { band, finish }
}

/**
 * El tempo cuyo peine regular mejor pega con los golpes de `[t0, t1]`.
 *
 * La puntuación de un peine es lo que suman los golpes sobre él (con mejor
 * fase, y cada golpe pesa lo que su fuerza normalizada) dividido por los
 * puntos que tiene en el tramo. Con ella, un peine de la mitad (que se queda
 * con lo fuerte: el bombo y la caja) se parece mucho al entero: el octavo
 * es lo que no se puede decidir con la puntuación.
 *
 * Con `around` (el tempo que ya tiene el clic: lo que se espera), solo se
 * busca entre `around·(1 ± window)` y se devuelve lo mejor de ahí; no se
 * salta nunca a otro octavo. Sin `around`, se busca entre `min` y `max` y el
 * octavo se decide por el nivel métrico: se toma el mejor peine entre 60 y 180
 * bpm (`tactus`: el pulso de contar), y de su familia (el doble y la mitad,
 * también dentro de `tactus`) gana el MÁS RÁPIDO cuyo `excess` llegue al 85 %
 * del mejor (un peine de la mitad es un subconjunto del entero: con un 120
 * verdadero, el 60 empata; con un 60 verdadero, el 120 solo pega la mitad y
 * pierde).
 *
 * `alternatives` ofrece, sin elegirlos, el mejor peine alrededor del doble
 * (`ratio: 2`) y de la mitad (`ratio: 0.5`) del tempo devuelto, buscado igual
 * (±`window`) y con lo que pega de verdad; los que caen fuera de `min..max` no
 * salen, ni un doble por encima del techo de `tactus` (180 bpm: una
 * subdivisión, no un tempo de contar).
 *
 *   · `support`: la fracción de puntos del peine con un golpe a 25 ms.
 *   · `chance` y `excess`: cuánto pegaría ese mismo peine corrido a sitios que
 *     no son suyos y cuánto pega de más (como en `fitReport`). La música está
 *     llena de golpes y un peine a un tempo cualquiera ya apoya la mitad:
 *     `excess` bajo (menos de 0,4) es que ahí no hay nada que encontrar, por
 *     alto que salga `support`.
 *   · `phase`: el primer punto del peine desde `t0` (segundos), por si se
 *     quiere usar de `anchor` en `setTempoSpan`.
 * null con menos de 4 golpes que valgan, o si el tramo no da para 4 pulsos al
 * tempo más lento.
 * @param {ReadonlyArray<Hit>} hits @param {[number, number]} span
 * @param {{around?: number, min?: number, max?: number, window?: number, tactus?: [number, number]}} [opts]
 * @returns {{
 *   bpm: number, support: number, phase: number, chance: number, excess: number,
 *   alternatives: Array<{
 *     bpm: number, support: number, phase: number, chance: number, excess: number, ratio: number
 *   }>
 * }|null}
 */
export function estimateTempo(
  hits,
  [t0, t1],
  { around = 0, min = 40, max = 240, window = WINDOW_DEFAULT, tactus = TACTUS } = {}
) {
  if (!(t1 > t0) || !(min > 0) || !(max > min)) return null
  if (t1 - t0 < (4 * 60) / min) return null
  const prep = prepare(hits, t0, t1)
  const from = lowerBound(prep.t, t0)
  const to = upperBound(prep.t, t1)
  const ts = prep.t.slice(from, to)
  const ws = prep.w.slice(from, to)
  if (ts.length < 4) return null
  const search = tempoSearch(ts, ws, t0, t1, prep)
  const side = clamp(Number.isFinite(window) ? window : WINDOW_DEFAULT, 0.01, 0.5)

  /** El mejor peine alrededor de `centre` (±window), dentro de `min..max`. */
  const near = (centre) => {
    if (centre < min || centre > max) return null
    const best = search.band(Math.max(min, centre * (1 - side)), Math.min(max, centre * (1 + side)))
    return best && search.finish(best)
  }

  let chosen
  if (around > 0) {
    const best = search.band(Math.max(1, around * (1 - side)), around * (1 + side))
    chosen = best && search.finish(best)
  } else {
    // el pulso de contar se busca entre 60 y 180 bpm (o lo que dejen `min`/`max`)
    const lo = Math.max(min, tactus[0])
    const hi = Math.min(max, tactus[1])
    const [from, to] = lo < hi ? [lo, hi] : [min, max]
    const top = search.band(from, to, 6)
    if (!top) return null
    // la familia de octavos del mejor, cada uno con lo que pega su mejor peine
    // por encima del azar (el apoyo a secas no distingue: el charles de las
    // corcheas apoya el peine del doble tanto como el de la negra)
    const family = [search.finish(top)]
    for (const k of [-2, -1, 1, 2]) {
      const centre = top.bpm * 2 ** k
      const other = centre >= from * 0.98 && centre <= to * 1.02 ? near(centre) : null
      if (other) family.push(other)
    }
    const bestExcess = Math.max(...family.map((c) => c.excess))
    chosen = family
      .filter((c) => c.excess >= FAMILY_KEEP * bestExcess)
      .sort((a, b) => b.bpm - a.bpm)[0]
  }
  if (!chosen) return null

  // el doble solo se ofrece si sigue siendo un tempo de contar: por encima de
  // 180 es la corchea (el charles la apoya tanto como al pulso, y `excess` no
  // sabe que es una subdivisión)
  const alternatives = []
  for (const ratio of [2, 0.5]) {
    const centre = chosen.bpm * ratio
    const other = centre <= tactus[1] * 1.02 ? near(centre) : null
    if (other) alternatives.push({ ...other, ratio })
  }
  return { ...chosen, alternatives }
}
