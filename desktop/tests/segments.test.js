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
    expect(cleanSegments([[1.234, 2.345]])).toEqual([[1.23, 2.35]])
    // los que se tocan tambien se juntan: un tramo detras de otro sin hueco es uno
    expect(
      cleanSegments([
        [0, 5],
        [5, 8]
      ])
    ).toEqual([[0, 8]])
  })
  it('are compared to the hundredth, which is how they are stored', () => {
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
})
