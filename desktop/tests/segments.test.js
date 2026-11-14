import { describe, it, expect } from 'vitest'
import {
  cleanSegments,
  sameSegments,
  segmentAt,
  nextSegment,
  segmentsLength,
  snapToBeats
} from '../src/utils/segments.js'

describe('loop segments', () => {
  it('are kept in order, dropping invalid ones and merging overlapping ones', () => {
    expect(cleanSegments([[30, 40], [10, 20], [35, 45], [-1, 3], [9, 2], ['a', 5], [50]])).toEqual([
      [10, 20],
      [30, 45]
    ])
    expect(cleanSegments(null)).toEqual([])
    // al milisegundo (más no hace falta: son 44 muestras)
    expect(cleanSegments([[1.2344, 2.3456]])).toEqual([[1.234, 2.346]])
    // los que se tocan tambien se juntan: un tramo detras de otro sin hueco es uno
    expect(
      cleanSegments([
        [0, 5],
        [5, 8]
      ])
    ).toEqual([[0, 8]])
  })
  it('are compared to the hundredth, which is how earlier versions stored them', () => {
    expect(sameSegments([[10, 20]], [[10.004, 19.996]])).toBe(true)
    expect(sameSegments([[10, 20]], [[10, 21]])).toBe(false)
    expect(sameSegments([[10, 20]], [])).toBe(false)
  })
  it('know which one a second falls in, which one to jump to and how long they last', () => {
    const list = [
      [10, 20],
      [30, 40]
    ]
    expect(segmentAt(list, 15)).toBe(0)
    expect(segmentAt(list, 25)).toBe(-1)
    expect(segmentAt(list, 40)).toBe(-1)
    expect(nextSegment(list, 5)).toBe(0)
    expect(nextSegment(list, 25)).toBe(1)
    expect(nextSegment(list, 45), 'pasados todos, el primero').toBe(0)
    expect(segmentsLength(list)).toBe(20)
  })
  it('snap to the nearest beat without falling short', () => {
    const beats = Array.from({ length: 40 }, (_, i) => 0.25 + 0.5 * i)
    expect(snapToBeats([[10.1, 12.4]], beats)).toEqual([[10.25, 12.25]])
    // uno que al ajustarse se quedaria en nada crece hasta el siguiente pulso
    expect(snapToBeats([[10.1, 10.3]], beats)).toEqual([[10.25, 10.75]])
    expect(snapToBeats([[3, 4]], [])).toEqual([[3, 4]])
  })
  it('snapped to the beats, each edge stays on its beat to the millisecond', () => {
    // a 130 bpm ningún pulso cae en una centésima: redondeando a ella, cada
    // borde quedaba hasta 5 ms fuera del pulso
    const beats = Array.from({ length: 400 }, (_, i) => 0.137 + (i * 60) / 130)
    const offBeat = (t) => Math.min(...beats.map((b) => Math.abs(b - t)))
    const loops = [
      [10.1, 14.9],
      [31.3, 47.2],
      [60.02, 75.5],
      [100.4, 130.7]
    ]
    const snapped = snapToBeats(loops, beats)
    expect(snapped).toHaveLength(4)
    const worst = Math.max(...snapped.flat().map(offBeat))
    expect(worst).toBeLessThanOrEqual(0.0005 + 1e-9)
    // y lo que se queda guardado no lleva más decimales que esos
    for (const t of snapped.flat())
      expect(Math.abs(t * 1000 - Math.round(t * 1000))).toBeLessThan(1e-6)
  })
})
