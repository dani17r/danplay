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
  multFactor
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
