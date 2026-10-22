import { describe, it, expect } from 'vitest'
import {
  doubled,
  halved,
  withMeter,
  shifted,
  effectiveGrid,
  isDownbeat,
  normalMeter,
  formatBpm,
  parseBpm,
  multFactor,
  clickTimes
} from '../src/utils/beats.js'

// La misma rejilla que las pruebas de Rust (`beats.rs`): 120 bpm, un pulso
// cada medio segundo desde 0,25, el «1» en el pulso 1.
const grid = () => ({
  bpm: 120,
  meter: 4,
  beats: Array.from({ length: 16 }, (_, i) => 0.5 * i + 0.25),
  first_downbeat: 1,
  phase3: 2,
  phase4: 1,
  confidence: 1
})

describe('the beat grid as it plays, same as in Rust', () => {
  it('double and half the beats, with the «1» where it was', () => {
    const d = doubled(grid())
    expect(d.beats).toHaveLength(32)
    expect(d.beats[1]).toBeCloseTo(0.5)
    expect(d.first_downbeat).toBe(2)
    expect(d.bpm).toBe(240)
    const h = halved(grid())
    expect(h.beats).toHaveLength(8)
    expect(h.beats[0], 'empieza en el 1').toBeCloseTo(0.75)
    expect(h.first_downbeat).toBe(0)
    expect(h.bpm).toBe(60)
  })

  it('other meters take the «1» from where it fell in 3 or in 4', () => {
    expect(withMeter(grid(), 3)).toMatchObject({ meter: 3, first_downbeat: 2 })
    expect(withMeter(grid(), 2)).toMatchObject({ meter: 2, first_downbeat: 1 })
    expect(withMeter(grid(), 6)).toMatchObject({ meter: 6, first_downbeat: 2 })
    expect(withMeter(grid(), 1).meter, 'un acento en cada pulso es ninguno').toBe(0)
    expect(normalMeter(40)).toBe(12)
    const none = withMeter(grid(), 0)
    expect(none.beats.some((_, i) => isDownbeat(none, i))).toBe(false)
  })

  it('the «1» shifts within the bar', () => {
    expect(shifted(grid(), 1).first_downbeat).toBe(2)
    expect(shifted(grid(), -2).first_downbeat).toBe(3)
    expect(isDownbeat(grid(), 5) && !isDownbeat(grid(), 6)).toBe(true)
  })

  it('in the same order as Rust: double or half, meter and shifted «1»', () => {
    expect(effectiveGrid(null)).toBeNull()
    const g = effectiveGrid(grid(), { mult: 1, meter: 2, shift: 1 })
    expect(g.beats).toHaveLength(32)
    expect(g.meter).toBe(2)
    // doble: phase4 pasa a 2; 2/4: 2 % 2 = 0; corrido uno: 1
    expect(g.first_downbeat).toBe(1)
    expect(effectiveGrid(grid())).toEqual(grid())
  })
})

describe('the manually typed tempo', () => {
  it('is shown with a decimal only if it has one', () => {
    expect(formatBpm(120)).toBe('120')
    expect(formatBpm(120.03)).toBe('120')
    expect(formatBpm(90.68)).toBe('90.7')
    expect(formatBpm(0)).toBe('0')
  })
  it('is read with a dot or a comma', () => {
    expect(parseBpm('90.7')).toBe(90.7)
    expect(parseBpm(' 90,7 ')).toBe(90.7)
    expect(parseBpm('')).toBeNull()
    expect(parseBpm('rapido')).toBeNull()
    expect(parseBpm('-3')).toBeNull()
    expect(parseBpm(null)).toBeNull()
  })
  it('double and half', () => {
    expect(multFactor(1)).toBe(2)
    expect(multFactor(-1)).toBe(0.5)
    expect(multFactor(0)).toBe(1)
  })
})

describe('the click hits to save in the mix', () => {
  it('following the song: those of its grid, with the «1» where it plays', () => {
    const g = effectiveGrid(grid(), {})
    const { beats, accents } = clickTimes(g, { free: false, bpm: 120, meter: 4 }, 30)
    expect(beats).toEqual(grid().beats)
    expect(accents.slice(0, 6)).toEqual([false, true, false, false, false, true])
  })
  it('with manual tempo: one every interval, aligned with the «1» of the song', () => {
    const { beats, accents } = clickTimes(grid(), { free: true, bpm: 60, meter: 3 }, 5)
    // el «1» de la canción cae a 0,75: de ahí hacia atrás y hacia delante
    expect(beats).toEqual([0.75, 1.75, 2.75, 3.75, 4.75])
    expect(accents).toEqual([true, false, false, true, false])
    // sin rejilla, desde el principio; sin acento, ninguno
    const free = clickTimes(null, { free: true, bpm: 120, meter: 0 }, 1)
    expect(free.beats).toEqual([0, 0.5, 1])
    expect(free.accents).toEqual([false, false, false])
  })
})

describe('Beat This! downbeats, bar by bar, as in Rust', () => {
  // la misma que la prueba de Rust `the_bars_of_beat_this_are_followed_as_heard`
  const irregular = () => ({
    bpm: 120,
    meter: 4,
    beats: Array.from({ length: 16 }, (_, i) => 0.5 * i),
    first_downbeat: 0,
    phase3: 0,
    phase4: 0,
    confidence: 0.9,
    bars: [0, 4, 8, 10, 14]
  })
  const ones = (g, n = 20) => Array.from({ length: n }, (_, i) => i).filter((i) => isDownbeat(g, i))

  it('the irregular bar is respected, and what follows uses the song meter', () => {
    expect(ones(irregular())).toEqual([0, 4, 8, 10, 14, 18])
  })
  it('shifting the «1», double, half and another meter, same as Rust', () => {
    const g = irregular()
    expect(shifted(g, 1).bars).toEqual([1, 5, 9, 11, 15])
    expect(doubled(g).bars).toEqual([0, 8, 16, 20, 28])
    expect(halved(g).bars).toEqual([0, 2, 4, 5, 7])
    expect(withMeter(g, 4)).toBe(g)
    const waltz = withMeter(g, 3)
    expect(waltz.bars).toBeUndefined()
    expect(ones(waltz, 9)).toHaveLength(3)
  })
  // las mismas que `a_hole_and_a_closed_end_stay_silent` y `halving_keeps_every_one`
  it('double adds no hits in a gap without beats nor after the end', () => {
    const beats = [
      ...Array.from({ length: 8 }, (_, i) => 0.5 * i),
      ...Array.from({ length: 8 }, (_, i) => 6.2 + 0.5 * i)
    ]
    const d = doubled({ ...irregular(), beats, bars: [0, 4, 8, 12], closed: true })
    expect(d.beats.some((b) => b > 3.6 && b < 6.1)).toBe(false)
    expect(d.beats).toHaveLength(30)
    expect(d.bars).toEqual([0, 8, 15, 23])
  })
  it('half keeps every «1», even with a bar of three', () => {
    const g = { ...irregular(), bars: [0, 4, 7, 11] }
    const h = halved(g)
    expect(h.bars.map((b) => h.beats[b])).toEqual(g.bars.map((b) => g.beats[b]))
  })
})
