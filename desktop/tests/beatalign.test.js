import { describe, it, expect } from 'vitest'
import { REACH_DEFAULT, fitReport, alignBeats, estimateTempo } from '../src/utils/beatAlign.js'
import { MIN_GAP } from '../src/utils/beatEdit.js'
import sal22 from './fixtures/sal22-hits.json'

// ---------------------------------------------------------------- los datos
// Nadie puede ESCUCHAR aquí: los golpes del audio se fabrican a mano, con una
// semilla fija, y el clic se mide contra ellos en milisegundos.

const seeded = (seed) => () => {
  seed = (seed + 0x6d2b79f5) | 0
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}
const round3 = (t) => Math.round(t * 1000) / 1000

/** Un peine regular: `n` pulsos desde `start` a `bpm`. */
const comb = (start, bpm, n) => Array.from({ length: n }, (_, k) => round3(start + (k * 60) / bpm))

/** Un tramo que acelera de `b0` a `b1` bpm a lo largo de `n` pulsos. */
function accelerating(start, n, b0, b1) {
  const times = [start]
  for (let k = 1; k < n; k++) times.push(times[k - 1] + 60 / (b0 + ((b1 - b0) * (k - 1)) / (n - 2)))
  return times
}

/** Un golpe en cada tiempo, corrido `shift` ms y con ±`jitter` ms de ruido; fuerza ~4. */
function hitsAt(times, { shift = 0, jitter = 0, seed = 1, strength = 4 } = {}) {
  const rand = seeded(seed)
  return times.map((t) => [
    round3(t + shift / 1000 + (rand() * 2 - 1) * (jitter / 1000)),
    strength * (0.8 + 0.4 * rand())
  ])
}

/** Congela todo: si una función toca lo que recibe, revienta (los módulos son estrictos). */
function frozen(value) {
  if (value && typeof value === 'object') {
    for (const v of Object.values(value)) frozen(v)
    Object.freeze(value)
  }
  return value
}

/** El tramo que abarca unos pulsos, con un poco de margen. */
const spanOf = (beats) => [beats[0] - 0.01, beats.at(-1) + 0.01]
/** La raíz de la media de los cuadrados de lo que se aleja de la verdad, en ms. */
const rms = (beats, truth) =>
  Math.sqrt(beats.reduce((s, b, k) => s + ((b - truth[k]) * 1000) ** 2, 0) / beats.length)
/** Cuánto cambian de media los intervalos respecto de los que tenían, en ms. */
const intervalChange = (after, before) => {
  let sum = 0
  for (let k = 1; k < after.length; k++) {
    sum += Math.abs(after[k] - after[k - 1] - (before[k] - before[k - 1]))
  }
  return (sum / (after.length - 1)) * 1000
}

// ---------------------------------------------------------------- el informe

describe('how well the click falls on the audio (fitReport)', () => {
  const beats = comb(5, 100, 40)

  it('hits 17 ms after the click: offset +17, a small spread, every beat supported', () => {
    const hits = frozen(hitsAt(beats, { shift: 17, jitter: 3 }))
    const rep = fitReport(frozen(beats), hits, [5, 29])
    expect(rep.n).toBe(40)
    expect(rep.supported).toBe(1)
    expect(rep.offsetMs).toBeGreaterThan(15.5)
    expect(rep.offsetMs).toBeLessThan(18.5)
    expect(rep.spreadMs).toBeGreaterThan(0)
    expect(rep.spreadMs).toBeLessThan(4)
  })

  it('a click AFTER the hit gives a negative offset, as the server does', () => {
    // los golpes caen 30 ms antes que el clic: pasa de los 25 ms (no cuenta como
    // acierto) pero está al alcance (50 ms): se mide
    const hits = hitsAt(beats, { shift: -30, jitter: 2 })
    const rep = fitReport(beats, hits, [5, 29])
    expect(rep.offsetMs).toBeGreaterThan(-32)
    expect(rep.offsetMs).toBeLessThan(-28)
    expect(rep.supported).toBe(0)
  })

  it('only the beats inside the span count', () => {
    const hits = hitsAt(beats, { shift: 0, jitter: 1 })
    expect(fitReport(beats, hits, [5, 10.41]).n).toBe(10)
    expect(fitReport(beats, hits, [100, 200]).n).toBe(0)
    expect(fitReport(beats, hits, [100, 200])).toEqual({
      n: 0,
      supported: 0,
      offsetMs: 0,
      spreadMs: 0
    })
  })

  it('without a hit near, everything is zero', () => {
    const far = beats.map((t) => [t + 0.3, 4])
    expect(fitReport(beats, far, [5, 29])).toEqual({
      n: 40,
      supported: 0,
      offsetMs: 0,
      spreadMs: 0
    })
    expect(fitReport(beats, [], [5, 29]).supported).toBe(0)
  })

  it('the strength is normalised: a hit scale does not matter, and a weak one is ignored', () => {
    const base = hitsAt(beats, { shift: 12, jitter: 2 })
    const loud = base.map(([t, s]) => [t, s * 80])
    const a = fitReport(beats, base, [5, 29])
    const b = fitReport(beats, loud, [5, 29])
    expect(b.offsetMs).toBeCloseTo(a.offsetMs, 9)
    expect(b.supported).toBe(a.supported)
    // ruido flojo justo en el pulso (0,3 contra 4): no vale, y el golpe de verdad está a 20 ms
    const real = hitsAt(beats, { shift: 20 })
    const noise = beats.map((t) => [t, 0.3])
    const mixed = [...real, ...noise].sort((x, y) => x[0] - y[0])
    expect(fitReport(beats, mixed, [5, 29]).offsetMs).toBeCloseTo(20, 0)
  })

  it('takes the strong hit that is near over a weak one that is nearer', () => {
    // el pulso del 5.º tiene un golpe fuerte a +30 ms y uno flojo (fuerza 1 contra 4) a +2 ms
    const clean = hitsAt(beats, { shift: 0, strength: 4 }).filter((_, k) => k !== 4)
    const two = [...clean, [round3(beats[4] + 0.03), 4], [round3(beats[4] + 0.002), 1]]
    two.sort((a, b) => a[0] - b[0])
    const rep = fitReport(beats, two, [beats[4], beats[4]])
    expect(rep.n).toBe(1)
    expect(rep.offsetMs).toBeCloseTo(30, 0)
    // el flojo (0,25 de 1) pesa 0,25 · 0,96 = 0,24 contra 1 · 0,4 = 0,4 del fuerte
  })

  it('does not change what it receives', () => {
    fitReport(frozen(beats), frozen(hitsAt(beats, { jitter: 2 })), [5, 29])
  })

  it('REACH_DEFAULT is 50 ms', () => {
    expect(REACH_DEFAULT).toBe(0.05)
  })
})

// ------------------------------------------------------------------- la fase

describe('aligning the phase of a comb (how: phase)', () => {
  const beats = comb(5, 100, 40)
  const span = [5, 5 + 39 * 0.6]

  it('finds a known shift to within 2 ms, for hits ahead and behind', () => {
    for (const shift of [-40, -12, 0, 9, 17, 33]) {
      const hits = hitsAt(beats, { shift, jitter: 3, seed: 7 })
      const res = alignBeats(frozen({ beats }), span, frozen(hits), { how: 'phase' })
      expect(res.applied, `shift ${shift}`).toBe(true)
      const got = (res.beats[20] - beats[20]) * 1000
      expect(Math.abs(got - shift), `shift ${shift}: got ${got}`).toBeLessThanOrEqual(2)
      expect(Math.abs(res.report.after.offsetMs), `after ${shift}`).toBeLessThanOrEqual(2.5)
      expect(res.report.after.supported).toBe(1)
    }
  })

  it('keeps the spacing of the span: every beat moves the same', () => {
    const hits = hitsAt(beats, { shift: 17, jitter: 3 })
    const res = alignBeats({ beats }, span, hits, { how: 'phase' })
    const moved = res.beats.map((t, k) => (t - beats[k]) * 1000)
    expect(Math.max(...moved) - Math.min(...moved)).toBeLessThanOrEqual(1.001)
    expect(res.report.n).toBe(40)
    expect(res.report.moved).toBe(40)
    expect(res.report.reason).toBe('')
    // y antes de moverlo, el informe ya decía los 17 ms
    expect(res.report.before.offsetMs).toBeGreaterThan(15.5)
    expect(res.report.before.offsetMs).toBeLessThan(18.5)
  })

  it('only the beats inside the span move; the others come out exactly as they were', () => {
    const hits = hitsAt(beats, { shift: 17, jitter: 2 })
    const res = alignBeats({ beats }, [beats[10] - 0.01, beats[19] + 0.01], hits, { how: 'phase' })
    expect(res.report.n).toBe(10)
    beats.forEach((t, k) => {
      if (k < 10 || k > 19) expect(res.beats[k]).toBe(t)
      else expect(res.beats[k]).not.toBe(t)
    })
  })

  it('never gets within 20 ms of the beats outside that it does not move', () => {
    // los pulsos 1, 2 y 3 s van a +45 ms; el de fuera está a 60 ms del último
    const own = { beats: [1, 2, 3, 3.06] }
    const hits = [
      [1.045, 4],
      [2.045, 4],
      [3.045, 4]
    ]
    const res = alignBeats(frozen(own), [0.5, 3], frozen(hits), { how: 'phase' })
    expect(res.applied).toBe(true)
    expect(res.beats[0]).toBe(1.045)
    expect(res.beats[2]).toBe(3.04)
    expect(res.beats[3]).toBe(3.06)
    expect(res.beats[3] - res.beats[2]).toBeGreaterThanOrEqual(MIN_GAP - 1e-9)
  })

  it('"no pega": hits that have nothing to do with the comb leave every beat as it was', () => {
    const rand = seeded(5)
    const random = Array.from({ length: 50 }, () => [5 + rand() * 24, 3 + rand() * 2]).sort(
      (a, b) => a[0] - b[0]
    )
    const res = alignBeats(frozen({ beats }), span, frozen(random), { how: 'phase' })
    expect(res.applied).toBe(false)
    expect(res.report.reason).toBe('no pega')
    expect(res.report.moved).toBe(0)
    expect(res.beats).toEqual(beats)
    expect(res.report.candidate.supported).toBeLessThan(0.5)
    expect(res.report.after).toEqual(res.report.before)
    // con `force` se mueve igual
    const forced = alignBeats({ beats }, span, random, { how: 'phase', force: true })
    expect(forced.applied).toBe(true)
    expect(forced.report.reason).toBe('')
    expect(forced.report.moved).toBeGreaterThan(0)
  })

  it('"sin golpes": a span with no usable hit is left alone', () => {
    const none = alignBeats({ beats }, span, [], { how: 'phase' })
    expect(none.applied).toBe(false)
    expect(none.report.reason).toBe('sin golpes')
    expect(none.beats).toEqual(beats)
    // golpes solo en otro sitio de la canción
    const elsewhere = alignBeats({ beats }, span, hitsAt(comb(100, 100, 20)), { how: 'phase' })
    expect(elsewhere.report.reason).toBe('sin golpes')
    // y ni con `force` hay nada que pegar
    expect(alignBeats({ beats }, span, [], { how: 'phase', force: true }).applied).toBe(false)
  })

  it('a span with no beats has nothing to align', () => {
    const res = alignBeats({ beats }, [100, 110], hitsAt(beats), { how: 'phase' })
    expect(res.applied).toBe(false)
    expect(res.report.reason).toBe('sin pulsos')
    expect(res.report.n).toBe(0)
  })

  it('hits only on beats 1 and 3 of every bar are enough (half of the beats supported)', () => {
    const odd = beats.filter((_, k) => k % 4 === 0 || k % 4 === 2)
    const hits = hitsAt(odd, { shift: 12, jitter: 2 })
    const res = alignBeats({ beats }, span, hits, { how: 'phase' })
    expect(res.applied).toBe(true)
    expect(res.report.after.supported).toBe(0.5)
    expect(Math.abs((res.beats[8] - beats[8]) * 1000 - 12)).toBeLessThanOrEqual(2)
    // pidiendo más, ya no pega
    const strict = alignBeats({ beats }, span, hits, { how: 'phase', minSupport: 0.6 })
    expect(strict.applied).toBe(false)
    expect(strict.report.reason).toBe('no pega')
    expect(strict.report.candidate.supported).toBe(0.5)
  })

  it('lead puts the click ahead of the hit (negative) or behind it', () => {
    const hits = hitsAt(beats, { shift: 15, jitter: 2, seed: 3 })
    const plain = alignBeats({ beats }, span, hits, { how: 'phase' })
    const ahead = alignBeats({ beats }, span, hits, { how: 'phase', lead: -20 })
    ahead.beats.forEach((t, k) => expect((t - plain.beats[k]) * 1000).toBeCloseTo(-20, 0))
    // el clic queda 20 ms delante del golpe: golpe − pulso = +20
    expect(ahead.report.after.offsetMs).toBeGreaterThan(18)
    expect(ahead.report.after.offsetMs).toBeLessThan(22)
    const behind = alignBeats({ beats }, span, hits, { how: 'phase', lead: 8 })
    expect((behind.beats[3] - plain.beats[3]) * 1000).toBeCloseTo(8, 0)
    // el lead no entra en la cuenta de si pega o no
    const tooAhead = alignBeats({ beats }, span, hits, { how: 'phase', lead: -45 })
    expect(tooAhead.applied).toBe(true)
  })

  it('the strength scale of the hits does not matter', () => {
    const hits = hitsAt(beats, { shift: 11, jitter: 3 })
    const a = alignBeats({ beats }, span, hits, { how: 'phase' })
    const b = alignBeats(
      { beats },
      span,
      hits.map(([t, s]) => [t, s * 37]),
      { how: 'phase' }
    )
    expect(b.beats).toEqual(a.beats)
  })

  it('does not change what it receives', () => {
    const hits = frozen(hitsAt(beats, { shift: 5, jitter: 2 }))
    for (const how of ['phase', 'line', 'follow']) {
      alignBeats(frozen({ beats }), span, hits, { how })
    }
  })
})

// --------------------------------------------------------------------- recta

describe('fitting a straight line (how: line)', () => {
  // las pistas grabadas a un clic de 100 bpm; el clic de aquí va a 99,7 y se desvía
  const truth = comb(5, 100, 40)
  const own = comb(5, 99.7, 40)
  const span = spanOf(own)

  it('recovers the tempo and the phase of a click that the beats drift away from', () => {
    const hits = hitsAt(truth, { jitter: 3, seed: 3 })
    expect(Math.abs(own.at(-1) - truth.at(-1)) * 1000, 'it drifts 70 ms').toBeGreaterThan(65)
    const res = alignBeats({ beats: own }, span, hits, { how: 'line' })
    expect(res.applied).toBe(true)
    const period = (res.beats.at(-1) - res.beats[0]) / 39
    expect(60 / period).toBeCloseTo(100, 1)
    expect(Math.abs(res.beats[0] - truth[0]) * 1000).toBeLessThanOrEqual(2)
    expect(rms(res.beats, truth)).toBeLessThan(2.5)
    expect(res.report.after.supported).toBe(1)
    // perfectamente regular, salvo el redondeo al milisegundo
    const gaps = res.beats.slice(1).map((t, k) => t - res.beats[k])
    expect(Math.max(...gaps) - Math.min(...gaps)).toBeLessThanOrEqual(0.0011)
  })

  it('a single shift cannot fix a drift; the line can', () => {
    const hits = hitsAt(truth, { jitter: 3, seed: 3 })
    const phase = alignBeats({ beats: own }, span, hits, { how: 'phase', force: true })
    const line = alignBeats({ beats: own }, span, hits, { how: 'line' })
    expect(phase.report.after.spreadMs).toBeGreaterThan(15)
    expect(line.report.after.spreadMs).toBeLessThan(4)
    expect(rms(phase.beats, truth)).toBeGreaterThan(15)
    expect(rms(line.beats, truth)).toBeLessThan(rms(phase.beats, truth) / 5)
  })

  it('is robust to missing hits and to wrong ones', () => {
    const rand = seeded(21)
    // sin el 30 % de los golpes y con cuatro golpes fuera de sitio (+/− 40 ms)
    let hits = hitsAt(truth, { jitter: 3, seed: 4 }).filter(() => rand() > 0.3)
    hits = hits.map(([t, s], k) => [k % 9 === 4 ? round3(t + (k % 2 ? 0.04 : -0.04)) : t, s])
    const res = alignBeats({ beats: own }, span, hits, { how: 'line' })
    expect(res.applied).toBe(true)
    expect(rms(res.beats, truth)).toBeLessThan(3)
    expect(60 / ((res.beats.at(-1) - res.beats[0]) / 39)).toBeCloseTo(100, 1)
  })

  it('a span with fewer than three beats falls back to a shift', () => {
    const two = { beats: [10, 10.6] }
    const hits = hitsAt([10.017, 10.617], { jitter: 0 })
    const res = alignBeats(two, [9.9, 10.7], hits, { how: 'line' })
    expect(res.applied).toBe(true)
    expect(res.beats[0]).toBeCloseTo(10.017, 3)
    expect(res.beats[1] - res.beats[0]).toBeCloseTo(0.6, 3)
  })

  it('"no pega" holds for a line too', () => {
    const rand = seeded(8)
    const random = Array.from({ length: 50 }, () => [5 + rand() * 24, 3 + rand() * 2]).sort(
      (a, b) => a[0] - b[0]
    )
    const res = alignBeats({ beats: own }, span, random, { how: 'line' })
    expect(res.applied).toBe(false)
    expect(res.beats).toEqual(own)
  })
})

// -------------------------------------------------------------------- seguir

describe('following the hits beat by beat (how: follow)', () => {
  // un tramo que acelera de 100 a 110 bpm; el clic que tienes se desvía de él
  // hasta 35 ms a un lado y a otro (como lo haría la rejilla del análisis)
  const truth = accelerating(5, 48, 100, 110)
  const wrong = truth.map((t, k) => round3(t + 0.035 * Math.sin((2 * Math.PI * k) / 24)))
  const span = spanOf(wrong)

  it('tracks the accelerating section where a single shift cannot', () => {
    const hits = hitsAt(truth, { jitter: 2, seed: 5 })
    const phase = alignBeats({ beats: wrong }, span, hits, { how: 'phase', force: true })
    const follow = alignBeats({ beats: wrong }, span, hits, { how: 'follow' })
    expect(rms(wrong, truth), 'it starts 25 ms off').toBeGreaterThan(20)
    expect(rms(phase.beats, truth), 'a shift does not fix it').toBeGreaterThan(15)
    expect(follow.applied).toBe(true)
    expect(rms(follow.beats, truth)).toBeLessThan(3)
    expect(follow.report.after.supported).toBe(1)
    expect(follow.report.after.spreadMs).toBeLessThan(4)
    expect(follow.report.before.spreadMs).toBeGreaterThan(follow.report.after.spreadMs * 3)
  })

  it('the more rigid, the less the intervals change', () => {
    // un tempo fijo de 100 bpm y golpes con ±12 ms de ruido: sin rigidez los
    // sigue todos; con ella solo los que casan con la estructura
    const fixed = comb(5, 100, 60)
    const rand = seeded(9)
    const hits = fixed.map((t) => [round3(t + 0.01 + (rand() * 2 - 1) * 0.012), 3 + rand() * 2])
    const changes = [0, 0.5, 0.75, 1].map((rigidity) => {
      const res = alignBeats({ beats: fixed }, spanOf(fixed), hits, { how: 'follow', rigidity })
      expect(res.applied, `rigidity ${rigidity}`).toBe(true)
      return intervalChange(res.beats, fixed)
    })
    expect(changes[0], 'follows the hits').toBeGreaterThan(4)
    for (let i = 1; i < changes.length; i++) expect(changes[i]).toBeLessThan(changes[i - 1])
    expect(changes.at(-1), 'almost a shift').toBeLessThan(changes[0] / 10)
  })

  it('a very rigid follow cannot track a tempo change, and says "no pega" instead of making it worse', () => {
    const hits = hitsAt(truth, { jitter: 2, seed: 5 })
    const res = alignBeats({ beats: wrong }, span, hits, { how: 'follow', rigidity: 1 })
    expect(res.applied).toBe(false)
    expect(res.report.reason).toBe('no pega')
    expect(res.beats).toEqual(wrong)
  })

  it('a beat with no hits near keeps its interval with the one before: it does not jump back', () => {
    const fixed = comb(5, 100, 40)
    const hits = hitsAt(fixed, { shift: 20, jitter: 2, seed: 6 }).filter((_, k) => k < 15 || k > 20)
    const res = alignBeats({ beats: fixed }, spanOf(fixed), hits, { how: 'follow' })
    expect(res.applied).toBe(true)
    for (let k = 15; k <= 20; k++) {
      const off = (res.beats[k] - fixed[k]) * 1000
      expect(off, `beat ${k}`).toBeGreaterThan(16)
      expect(off, `beat ${k}`).toBeLessThan(24)
      expect(
        Math.abs(res.beats[k] - res.beats[k - 1] - (fixed[k] - fixed[k - 1])) * 1000
      ).toBeLessThan(5)
    }
    // y los de antes del primer golpe toman el desfase del primero que lo tuvo
    const late = alignBeats(
      { beats: fixed },
      spanOf(fixed),
      hitsAt(fixed, { shift: 20 }).slice(5),
      {
        how: 'follow'
      }
    )
    expect((late.beats[0] - fixed[0]) * 1000).toBeGreaterThan(18)
  })

  it('beats stay in order and 20 ms apart, whatever the hits do', () => {
    // golpes pegados unos a otros: dos pulsos querrían el mismo
    const fixed = comb(5, 100, 20)
    const crowded = fixed.flatMap((t) => [
      [round3(t + 0.004), 4],
      [round3(t + 0.012), 4]
    ])
    const res = alignBeats({ beats: fixed }, spanOf(fixed), crowded, { how: 'follow', rigidity: 0 })
    for (let k = 1; k < res.beats.length; k++) {
      expect(res.beats[k] - res.beats[k - 1]).toBeGreaterThanOrEqual(MIN_GAP - 1e-9)
    }
  })
})

// ------------------------------------------------------ solo donde mejora

describe('only where it gets better (empeora / no pega)', () => {
  it('"empeora": a straight line over a good click that follows a tempo change is refused', () => {
    // el clic ya sigue el tramo que acelera: una recta de tempo fijo lo estropea
    const truth = accelerating(5, 48, 100, 110)
    const good = truth.map((t, k) => round3(t + ((k % 3) - 1) * 0.002))
    const hits = hitsAt(truth, { jitter: 2, seed: 5 })
    const res = alignBeats(frozen({ beats: good }), spanOf(good), frozen(hits), { how: 'line' })
    expect(res.report.before.supported).toBe(1)
    expect(res.applied).toBe(false)
    expect(res.report.reason).toBe('empeora')
    expect(res.beats).toEqual(good)
    expect(res.report.moved).toBe(0)
    expect(res.report.after, 'nothing moved: after is before').toEqual(res.report.before)
    // el informe cuenta lo que habría salido
    expect(res.report.candidate.supported).toBeLessThan(res.report.before.supported - 0.03)
    // y con `force` se hace igual
    const forced = alignBeats({ beats: good }, spanOf(good), hits, { how: 'line', force: true })
    expect(forced.applied).toBe(true)
    expect(forced.report.after.supported).toBeLessThan(1)
  })

  it('"empeora": the same support with a clearly wider spread is refused too', () => {
    // un tempo fijo de 100 bpm tocado por una persona: cada golpe se descuadra
    // hasta 12 ms. El clic que cae en cada golpe tiene dispersión 0; la mejor
    // recta pega con todos a 25 ms pero ya no con los 0 ms de antes
    const rand = seeded(13)
    const played = comb(5, 100, 40).map((t) => round3(t + (rand() * 2 - 1) * 0.012))
    const hits = played.map((t) => [t, 4])
    const res = alignBeats({ beats: played }, spanOf(played), hits, { how: 'line' })
    expect(res.report.before.spreadMs).toBe(0)
    expect(res.report.candidate.supported).toBe(1)
    expect(res.report.candidate.spreadMs).toBeGreaterThan(3)
    expect(res.applied).toBe(false)
    expect(res.report.reason).toBe('empeora')
  })

  it('a loss of up to 3 points of support is still accepted', () => {
    const fixed = comb(5, 100, 40)
    // un 5 % de los golpes (2 de 40) cae 30 ms fuera del pulso tras el ajuste
    const hits = hitsAt(fixed, { jitter: 2, seed: 2 }).map(([t, s], k) => [
      k === 7 || k === 23 ? round3(t + 0.04) : t,
      s
    ])
    const res = alignBeats({ beats: fixed }, spanOf(fixed), hits, { how: 'phase' })
    expect(res.report.before.supported).toBeCloseTo(0.95, 2)
    expect(res.report.reason).not.toBe('empeora')
  })

  it('a section that only supports a part of the comb is accepted when it clearly improves', () => {
    // golpes solo en uno de cada tres pulsos y 35 ms tarde (fuera de los 25 ms):
    // pegado, solo apoya un tercio (por debajo de `minSupport`), pero de 0 a 33 %
    const fixed = comb(5, 100, 42)
    const third = fixed.filter((_, k) => k % 3 === 0)
    const hits = hitsAt(third, { shift: 35, jitter: 2, seed: 4 })
    const res = alignBeats(frozen({ beats: fixed }), spanOf(fixed), frozen(hits), { how: 'phase' })
    expect(res.report.before.supported).toBe(0)
    expect(res.report.after.supported).toBeCloseTo(1 / 3, 2)
    expect(res.applied).toBe(true)
    expect(res.report.reason).toBe('')
    expect((res.beats[3] - fixed[3]) * 1000).toBeGreaterThan(32)
    // pero si ya estaban pegados, el mismo 33 % no es una mejora: «no pega»
    const already = alignBeats({ beats: fixed }, spanOf(fixed), hitsAt(third, { jitter: 2 }), {
      how: 'phase'
    })
    expect(already.report.before.supported).toBeCloseTo(1 / 3, 2)
    expect(already.applied).toBe(false)
    expect(already.report.reason).toBe('no pega')
    // ni una mejora de 7 puntos (de 11 pulsos con golpe a 25 ms, a 14)
    const nudged = third.map((t, k) => [round3(t + (k < 3 ? 0.027 : 0.02)), 4])
    const small = alignBeats({ beats: fixed }, spanOf(fixed), nudged, {
      how: 'phase',
      minSupport: 0.9
    })
    expect(small.report.before.supported).toBeCloseTo(11 / 42, 2)
    expect(small.report.candidate.supported).toBeCloseTo(14 / 42, 2)
    expect(small.report.reason).toBe('no pega')
  })

  it('a gain below minSupport is not accepted when it comes with a clearly wider spread', () => {
    // golpes cada tres pulsos, de 40 a 70 ms tarde: ahora solo los más cercanos
    // (≤ 50 ms) cuentan y se ven juntos; con la fase puesta caerían todos, pero
    // ya no tan juntos
    const fixed = comb(5, 100, 42)
    const rand = seeded(17)
    const late = fixed
      .filter((_, k) => k % 3 === 0)
      .map((t) => [round3(t + (40 + rand() * 30) / 1000), 4])
    const res = alignBeats({ beats: fixed }, spanOf(fixed), late, { how: 'phase' })
    expect(res.report.before.supported).toBe(0)
    expect(res.report.candidate.supported).toBeCloseTo(1 / 3, 2)
    expect(res.report.candidate.spreadMs).toBeGreaterThan(res.report.before.spreadMs * 1.3 + 3)
    expect(res.applied).toBe(false)
    expect(res.report.reason).toBe('no pega')
  })

  it('the spread of follow says nothing, and the report says so', () => {
    const truth = accelerating(5, 48, 100, 110)
    const wrong = truth.map((t, k) => round3(t + 0.035 * Math.sin((2 * Math.PI * k) / 24)))
    const hits = hitsAt(truth, { jitter: 2, seed: 5 })
    const follow = alignBeats({ beats: wrong }, spanOf(wrong), hits, { how: 'follow' })
    expect(follow.report.spreadReliable).toBe(false)
    expect(
      alignBeats({ beats: wrong }, spanOf(wrong), hits, { how: 'phase' }).report.spreadReliable
    ).toBe(true)
    expect(
      alignBeats({ beats: wrong }, [100, 101], hits, { how: 'follow' }).report.spreadReliable
    ).toBe(false)
  })
})

// ------------------------------------------- golpes de verdad: Sal 22 Te Amo

describe('on the real hits of a live song (Sal 22 Te Amo, 120 bpm with a half-time section)', () => {
  const unflat = (flat) =>
    Array.from({ length: flat.length / 2 }, (_, i) => [flat[2 * i], flat[2 * i + 1]])
  const sources = { batería: unflat(sal22.drums), mezcla: unflat(sal22.mix) }
  const own = { beats: sal22.beats }

  it('the click already fits a clear section: line is refused, phase and follow are not worse', () => {
    for (const [name, hits] of Object.entries(sources)) {
      const line = alignBeats(frozen({ beats: sal22.beats }), [100, 160], hits, { how: 'line' })
      expect(line.applied, name).toBe(false)
      expect(line.report.reason, name).toBe('empeora')
      expect(line.beats).toEqual(sal22.beats)
      expect(line.report.candidate.supported, name).toBeLessThan(
        line.report.before.supported - 0.03
      )
      for (const how of ['phase', 'follow']) {
        const r = alignBeats(own, [100, 160], hits, { how })
        if (r.applied) {
          expect(r.report.after.supported, `${name} ${how}`).toBeGreaterThanOrEqual(
            r.report.before.supported - 0.03
          )
        }
      }
    }
    expect(fitReport(sal22.beats, sources.batería, [100, 160]).supported).toBeGreaterThan(0.9)
  })

  it('a half-time section (only half of the beats can have a hit): from 50 % to 64 % is applied', () => {
    const hits = sources.batería
    const before = fitReport(sal22.beats, hits, [241, 270])
    expect(before.supported).toBeLessThan(0.55)
    for (const how of ['phase', 'line']) {
      const r = alignBeats(own, [241, 270], hits, { how, minSupport: 0.7 })
      expect(r.applied, how).toBe(true)
      expect(r.report.after.supported - r.report.before.supported, how).toBeGreaterThanOrEqual(0.12)
      expect(r.report.moved).toBeGreaterThan(0)
    }
    // y no se pide más de lo que hay: con 0,9 de mínimo y sin mejora clara no se mueve
    const strict = alignBeats(own, [241, 270], sources.mezcla, { how: 'phase', minSupport: 0.9 })
    expect(strict.applied).toBe(false)
    expect(strict.report.reason).toBe('no pega')
  })

  it('a shift that only moves a mix that was already good is refused', () => {
    for (const how of ['phase', 'follow', 'line']) {
      const r = alignBeats(own, [270, 340], sources.mezcla, { how })
      expect(r.applied, how).toBe(false)
      expect(r.report.reason, how).toBe('empeora')
    }
    // con la batería, que está clara, el ajuste de fase sí se aplica
    expect(alignBeats(own, [270, 340], sources.batería, { how: 'phase' }).applied).toBe(true)
  })

  it('estimateTempo keeps the 120 the click has, from the drums and from the mix', () => {
    for (const [name, hits] of Object.entries(sources)) {
      for (const [span, around] of [
        [[100, 160], 119.8],
        [[270, 340], 120]
      ]) {
        const est = estimateTempo(hits, span, { around })
        expect(est.bpm, `${name} ${span}`).toBeGreaterThan(119.7)
        expect(est.bpm, `${name} ${span}`).toBeLessThan(120.3)
        // y con la mitad como alternativa a elegir, nunca elegida por él
        expect(est.alternatives.find((a) => a.ratio === 0.5).bpm).toBeCloseTo(60, 0)
      }
    }
  })

  it('the half-time stretch: 120 for the click, and 60 offered with its support', () => {
    const est = estimateTempo(sources.batería, [241, 270], { around: 120 })
    expect(est.bpm).toBeGreaterThan(119.7)
    expect(est.bpm).toBeLessThan(120.3)
    const half = est.alternatives.find((a) => a.ratio === 0.5)
    expect(half.bpm).toBeGreaterThan(59.7)
    expect(half.bpm).toBeLessThan(60.3)
    expect(half.support).toBeGreaterThanOrEqual(0.9)
  })

  it('with no tempo given it finds 120 over the clear section, not 60 nor 240', () => {
    for (const hits of Object.values(sources)) {
      const est = estimateTempo(hits, [100, 160])
      expect(est.bpm).toBeGreaterThan(119.5)
      expect(est.bpm).toBeLessThan(120.5)
    }
    // y en el tramo de después, donde el charles apoya también el 240
    expect(estimateTempo(sources.batería, [270, 340]).bpm).toBeCloseTo(120, 0)
    expect(estimateTempo(sources.mezcla, [270, 340]).bpm).toBeCloseTo(120, 0)
  })
})

// ---------------------------------------------------------------------- tempo

describe('estimating the tempo of a stretch from the hits', () => {
  it('finds 76 bpm from the hits of a 76 bpm section, and where its comb starts', () => {
    const beats = comb(10.3, 76, 30)
    const hits = hitsAt(beats, { jitter: 3, seed: 2 })
    const est = estimateTempo(frozen(hits), [10, 33])
    expect(est).not.toBeNull()
    expect(est.bpm).toBeGreaterThan(75.8)
    expect(est.bpm).toBeLessThan(76.2)
    expect(est.support).toBeGreaterThan(0.9)
    expect(Math.abs(est.phase - 10.3)).toBeLessThanOrEqual(0.006)
  })

  it('only the hits of the span count: the song around it can be at another tempo', () => {
    const before = hitsAt(comb(0.2, 120, 20), { jitter: 2, seed: 1 })
    const inside = hitsAt(comb(10.3, 76, 30), { jitter: 2, seed: 2 })
    const after = hitsAt(comb(34, 140, 40), { jitter: 2, seed: 3 })
    const all = [...before, ...inside, ...after].sort((a, b) => a[0] - b[0])
    const est = estimateTempo(all, [10, 33])
    expect(est.bpm).toBeGreaterThan(75.8)
    expect(est.bpm).toBeLessThan(76.2)
  })

  it('the phase can be used as the anchor of the comb', () => {
    const beats = comb(10.3, 76, 30)
    const est = estimateTempo(hitsAt(beats, { jitter: 2, seed: 2 }), [10.5, 33])
    // el primer punto del peine desde 10,5 es el segundo pulso (11,089)
    expect(est.phase).toBeGreaterThanOrEqual(10.5)
    expect(Math.abs(est.phase - beats[1])).toBeLessThanOrEqual(0.006)
  })

  it('with hits on the eighth notes, around decides between the beat and its double', () => {
    // el mismo golpe en cada corchea: el peine de la negra pega con todos los
    // golpes que pega el del doble; solo `around` dice cuál se quiere
    const eighths = hitsAt(comb(10.3, 152, 59), { jitter: 2, seed: 2 })
    const span = [10, 33]
    expect(estimateTempo(eighths, span).bpm, 'sin pista, el más rápido').toBeCloseTo(152, 0)
    expect(estimateTempo(eighths, span, { around: 76 }).bpm).toBeCloseTo(76, 0)
    expect(estimateTempo(eighths, span, { around: 152 }).bpm).toBeCloseTo(152, 0)
    // «cerca de» vale: alguien que escribió 72 está hablando del 76
    expect(estimateTempo(eighths, span, { around: 72 }).bpm).toBeCloseTo(76, 0)
  })

  it('with around it never jumps to another octave, but offers it with its real support', () => {
    // golpes solo en las negras (76): quien tiene el clic a 152 recibe 152, con
    // la mitad de los puntos apoyados, y el 76 como alternativa con todos
    const quarters = hitsAt(comb(10.3, 76, 30), { jitter: 2, seed: 2 })
    const est = estimateTempo(quarters, [10, 33], { around: 152 })
    expect(est.bpm).toBeGreaterThan(151.5)
    expect(est.bpm).toBeLessThan(152.5)
    expect(est.support).toBeCloseTo(0.5, 1)
    const half = est.alternatives.find((a) => a.ratio === 0.5)
    expect(half.bpm).toBeGreaterThan(75.8)
    expect(half.bpm).toBeLessThan(76.2)
    expect(half.support).toBeGreaterThan(0.9)
    expect(Math.abs(half.phase - 10.3)).toBeLessThanOrEqual(0.006)
    // el doble (304) se sale de 40..240: no se ofrece
    expect(est.alternatives.some((a) => a.ratio === 2)).toBe(false)
    // ni la mitad de 76 (38), aunque la ventana de ±12 % llegue a rozar el mínimo
    const mid = estimateTempo(quarters, [10, 33], { around: 76 })
    expect(mid.alternatives.map((a) => a.ratio)).toEqual([2])
    // y la ventana es de ±12 % (se puede cambiar): con un 76 escrito no se ve el 152
    expect(estimateTempo(quarters, [10, 33], { around: 120, window: 0.12 }).bpm).toBeLessThan(135)
  })

  it('alternatives: the double and the half, each searched near its octave', () => {
    const est = estimateTempo(hitsAt(comb(10.3, 100, 40), { jitter: 2, seed: 4 }), [10, 33], {
      around: 100
    })
    expect(est.alternatives.map((a) => a.ratio)).toEqual([2, 0.5])
    const [double, half] = est.alternatives
    expect(double.bpm).toBeGreaterThan(199)
    expect(double.bpm).toBeLessThan(201)
    expect(double.support, 'only the beats are hit: half of the double').toBeCloseTo(0.5, 1)
    expect(half.bpm).toBeGreaterThan(49.5)
    expect(half.bpm).toBeLessThan(50.5)
    expect(half.support).toBeGreaterThan(0.95)
    // la mitad que se sale de `min..max` no se ofrece (30 bpm con el mínimo en 40)
    const slow = hitsAt(comb(10.3, 60, 30), { jitter: 2, seed: 4 })
    expect(estimateTempo(slow, [10, 38], { around: 60 }).alternatives.map((a) => a.ratio)).toEqual([
      2
    ])
    expect(
      estimateTempo(slow, [10, 38], { around: 60, min: 25 }).alternatives.map((a) => a.ratio)
    ).toEqual([2, 0.5])
  })

  describe('a live drum groove: strong kick and snare on beats 1 and 3, weak hi-hat in between', () => {
    // 120 bpm; en las negras 2 y 4 solo suena el charles, flojo, y en las
    // corcheas también: como en una canción de banda de verdad
    function groove({ half = false, seed = 3 } = {}) {
      const rand = seeded(seed)
      const jitter = () => (rand() * 2 - 1) * 0.003
      const hits = []
      for (let b = 0; b < 120; b++) {
        const t = 10 + b * 0.5
        if (b % 4 === 0 || b % 4 === 2) hits.push([round3(t + jitter()), 4.5 + rand()])
        else if (!half) hits.push([round3(t + jitter()), 1.2 + rand() * 0.3])
        if (!half) hits.push([round3(t + 0.25 + jitter()), 0.9 + rand() * 0.3])
      }
      return hits.sort((a, b) => a[0] - b[0])
    }
    const span = [10.2, 69.3]

    it('with the tempo the click has, it finds 120 and not its half (the strong beats alone)', () => {
      const est = estimateTempo(frozen(groove()), span, { around: 119.8 })
      expect(est.bpm).toBeGreaterThan(119.7)
      expect(est.bpm).toBeLessThan(120.3)
      expect(est.support).toBeGreaterThan(0.95)
      expect(est.alternatives.find((a) => a.ratio === 0.5).bpm).toBeCloseTo(60, 0)
    })

    it('with no tempo given, it chooses the fastest level that is as well supported', () => {
      // el 60 es un subconjunto del 120: empatan, y gana el 120; el 240 de las
      // corcheas también apoya, pero no se cuenta por encima de 180
      const est = estimateTempo(groove(), span)
      expect(est.bpm).toBeGreaterThan(119.7)
      expect(est.bpm).toBeLessThan(120.3)
      expect(est.support).toBeGreaterThan(0.95)
    })

    it('a half-time section (hits every second beat): around keeps the 120, 60 is the alternative', () => {
      const hits = groove({ half: true })
      const est = estimateTempo(hits, span, { around: 120 })
      expect(est.bpm).toBeGreaterThan(119.7)
      expect(est.bpm).toBeLessThan(120.3)
      expect(est.support).toBeCloseTo(0.5, 1)
      const half = est.alternatives.find((a) => a.ratio === 0.5)
      expect(half.bpm).toBeGreaterThan(59.8)
      expect(half.bpm).toBeLessThan(60.2)
      expect(half.support).toBeGreaterThanOrEqual(0.9)
      // sin pista, el 120 solo apoya la mitad: gana el 60
      const free = estimateTempo(hits, span)
      expect(free.bpm).toBeGreaterThan(59.8)
      expect(free.bpm).toBeLessThan(60.2)
      expect(free.alternatives.find((a) => a.ratio === 2).support).toBeCloseTo(0.5, 1)
    })
  })

  it('gives nothing when there are not enough hits or the span is too short', () => {
    expect(estimateTempo(hitsAt(comb(10, 76, 3)), [9, 20])).toBeNull()
    expect(estimateTempo([], [0, 30])).toBeNull()
    // menos de 4 pulsos al tempo más lento (40 bpm → 6 s)
    expect(estimateTempo(hitsAt(comb(1, 120, 10)), [0.9, 5])).toBeNull()
    expect(estimateTempo(hitsAt(comb(1, 120, 10)), [5, 5])).toBeNull()
    // tres golpes fuertes entre siete flojos: los flojos no cuentan, y con tres no hay tempo
    const weak = comb(10, 100, 7).map((t) => [t, 0.2])
    const strong = [
      [10.9, 4],
      [12.1, 4],
      [13.3, 4]
    ]
    const mixed = [...weak, ...strong].sort((a, b) => a[0] - b[0])
    expect(estimateTempo(mixed, [9, 24])).toBeNull()
  })

  it('a 60 s span with 600 hits is estimated in well under a second', () => {
    const rand = seeded(11)
    const hits = hitsAt(comb(2, 100, 100), { jitter: 3, seed: 2 })
    while (hits.length < 600) hits.push([rand() * 62, 0.5 + rand() * 3])
    hits.sort((a, b) => a[0] - b[0])
    estimateTempo(hits, [1, 61]) // para que el motor caliente
    const t0 = performance.now()
    const est = estimateTempo(hits, [1, 61])
    const took = performance.now() - t0
    expect(est.bpm).toBeGreaterThan(99.8)
    expect(est.bpm).toBeLessThan(100.2)
    expect(took).toBeLessThan(250)
  })

  it('does not change what it receives', () => {
    estimateTempo(frozen(hitsAt(comb(10.3, 76, 30), { jitter: 2 })), [10, 33])
  })
})
