import { describe, it, expect } from 'vitest'
import {
  cleanSegments,
  sameSegments,
  segmentAt,
  nextSegment,
  segmentsLength,
  snapToBeats
} from '../src/utils/segments.js'

describe('los tramos que se repiten', () => {
  it('se dejan en orden, sin los que no valen y juntando los que se pisan', () => {
    expect(cleanSegments([[30, 40], [10, 20], [35, 45], [-1, 3], [9, 2], ['a', 5], [50]])).toEqual([
      [10, 20],
      [30, 45]
    ])
    expect(cleanSegments(null)).toEqual([])
    expect(cleanSegments([[1.234, 2.345]])).toEqual([[1.23, 2.35]])
    // los que se tocan tambien se juntan: un tramo detras de otro sin hueco es uno
    expect(
      cleanSegments([
        [0, 5],
        [5, 8]
      ])
    ).toEqual([[0, 8]])
  })
  it('se comparan a la centesima, que es como se guardan', () => {
    expect(sameSegments([[10, 20]], [[10.004, 19.996]])).toBe(true)
    expect(sameSegments([[10, 20]], [[10, 21]])).toBe(false)
    expect(sameSegments([[10, 20]], [])).toBe(false)
  })
  it('saben en cual cae un segundo, a cual se va y cuanto duran', () => {
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
  it('se ajustan al pulso mas cercano sin quedarse cortos', () => {
    const beats = Array.from({ length: 40 }, (_, i) => 0.25 + 0.5 * i)
    expect(snapToBeats([[10.1, 12.4]], beats)).toEqual([[10.25, 12.25]])
    // uno que al ajustarse se quedaria en nada crece hasta el siguiente pulso
    expect(snapToBeats([[10.1, 10.3]], beats)).toEqual([[10.25, 10.75]])
    expect(snapToBeats([[3, 4]], [])).toEqual([[3, 4]])
  })
})
