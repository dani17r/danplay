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

describe('la rejilla tal como suena, igual que en Rust', () => {
  it('el doble y la mitad de pulsos, con el «1» donde estaba', () => {
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

  it('otros compases sacan el «1» de donde caia en 3 o en 4', () => {
    expect(withMeter(grid(), 3)).toMatchObject({ meter: 3, first_downbeat: 2 })
    expect(withMeter(grid(), 2)).toMatchObject({ meter: 2, first_downbeat: 1 })
    expect(withMeter(grid(), 6)).toMatchObject({ meter: 6, first_downbeat: 2 })
    expect(withMeter(grid(), 1).meter, 'un acento en cada pulso es ninguno').toBe(0)
    expect(normalMeter(40)).toBe(12)
    const none = withMeter(grid(), 0)
    expect(none.beats.some((_, i) => isDownbeat(none, i))).toBe(false)
  })

  it('el «1» se corre dentro del compas', () => {
    expect(shifted(grid(), 1).first_downbeat).toBe(2)
    expect(shifted(grid(), -2).first_downbeat).toBe(3)
    expect(isDownbeat(grid(), 5) && !isDownbeat(grid(), 6)).toBe(true)
  })

  it('en el orden de Rust: doble o mitad, compas y «1» corrido', () => {
    expect(effectiveGrid(null)).toBeNull()
    const g = effectiveGrid(grid(), { mult: 1, meter: 2, shift: 1 })
    expect(g.beats).toHaveLength(32)
    expect(g.meter).toBe(2)
    // doble: phase4 pasa a 2; 2/4: 2 % 2 = 0; corrido uno: 1
    expect(g.first_downbeat).toBe(1)
    expect(effectiveGrid(grid())).toEqual(grid())
  })
})

describe('el tempo escrito a mano', () => {
  it('se enseña con un decimal solo si lo tiene', () => {
    expect(formatBpm(120)).toBe('120')
    expect(formatBpm(120.03)).toBe('120')
    expect(formatBpm(90.68)).toBe('90.7')
    expect(formatBpm(0)).toBe('0')
  })
  it('se lee con punto o con coma', () => {
    expect(parseBpm('90.7')).toBe(90.7)
    expect(parseBpm(' 90,7 ')).toBe(90.7)
    expect(parseBpm('')).toBeNull()
    expect(parseBpm('rapido')).toBeNull()
    expect(parseBpm('-3')).toBeNull()
    expect(parseBpm(null)).toBeNull()
  })
  it('el doble y la mitad', () => {
    expect(multFactor(1)).toBe(2)
    expect(multFactor(-1)).toBe(0.5)
    expect(multFactor(0)).toBe(1)
  })
})

describe('los golpes del clic para guardarlos en la mezcla', () => {
  it('siguiendo la canción: los de su rejilla, con el «1» donde suena', () => {
    const g = effectiveGrid(grid(), {})
    const { beats, accents } = clickTimes(g, { free: false, bpm: 120, meter: 4 }, 30)
    expect(beats).toEqual(grid().beats)
    expect(accents.slice(0, 6)).toEqual([false, true, false, false, false, true])
  })
  it('con tempo a mano: uno cada tanto, alineados con el «1» de la canción', () => {
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

describe('los «1» de Beat This!, compás a compás, como en Rust', () => {
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

  it('el compás irregular se respeta, y lo que sigue va con el compás de la canción', () => {
    expect(ones(irregular())).toEqual([0, 4, 8, 10, 14, 18])
  })
  it('correr el «1», el doble, la mitad y otro compás, igual que Rust', () => {
    const g = irregular()
    expect(shifted(g, 1).bars).toEqual([1, 5, 9, 11, 15])
    expect(doubled(g).bars).toEqual([0, 8, 16, 20, 28])
    expect(halved(g).bars).toEqual([0, 2, 4, 5, 7])
    expect(withMeter(g, 4)).toBe(g)
    const waltz = withMeter(g, 3)
    expect(waltz.bars).toBeUndefined()
    expect(ones(waltz, 9)).toHaveLength(3)
  })
})
