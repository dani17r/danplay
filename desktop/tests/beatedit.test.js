import { describe, it, expect } from 'vitest'
import {
  MIN_GAP,
  round3,
  selectBetween,
  selectRange,
  toggleIndex,
  spanOf,
  filled,
  tempoOf,
  moveBlock,
  stretchBlock,
  spreadEven,
  setTempoSpan,
  doubleBeats,
  halveBeats,
  barsEvery,
  removeBeats,
  addBeatAt,
  revertToAuto
} from '../src/utils/beatEdit.js'

// Un clic propio de 16 pulsos a 120 bpm desde 0,25 s, el «1» en el 1, 5, 9 y 13
// (como la rejilla de las pruebas de beats.test.js).
const grid = (n = 16) => ({
  beats: Array.from({ length: n }, (_, i) => round3(0.25 + 0.5 * i)),
  bars: Array.from({ length: n }, (_, i) => i).filter((i) => i % 4 === 1),
  meter: 4
})

/** Una canción larga a 120 bpm desde 0, con el «1» en cada cuarto pulso. */
const song = (n = 80) => ({
  beats: Array.from({ length: n }, (_, i) => round3(0.5 * i)),
  bars: Array.from({ length: n }, (_, i) => i).filter((i) => i % 4 === 0),
  meter: 4
})

/** Congela todo: si una función toca lo que recibe, revienta (los módulos son estrictos). */
function frozen(value) {
  if (value && typeof value === 'object') {
    for (const v of Object.values(value)) frozen(v)
    Object.freeze(value)
  }
  return value
}

/** Lo que se cumple siempre, salga lo que salga de una edición. */
function expectValid(res, { duration = Infinity } = {}) {
  const { beats, bars, selection } = res
  beats.forEach((t, i) => {
    expect(t, `pulso ${i} en ms`).toBeCloseTo(round3(t), 9)
    expect(t).toBeGreaterThanOrEqual(0)
    expect(t).toBeLessThanOrEqual(duration + 1e-9)
    if (i) expect(t - beats[i - 1], `intervalo ${i}`).toBeGreaterThanOrEqual(MIN_GAP - 1e-9)
  })
  expect(bars).toEqual([...new Set(bars)].sort((a, b) => a - b))
  for (const b of bars) {
    expect(Number.isInteger(b) && b >= 0 && b < beats.length, `«1» ${b}`).toBe(true)
  }
  expect(selection).toEqual([...new Set(selection)].sort((a, b) => a - b))
  for (const i of selection) expect(i >= 0 && i < beats.length).toBe(true)
}

describe('selecting beats', () => {
  it('selectBetween takes the beats inside a time range, ends included', () => {
    const { beats } = grid()
    expect(selectBetween(beats, 1.25, 2.25)).toEqual([2, 3, 4])
    expect(selectBetween(beats, 1.3, 2.2)).toEqual([3])
    expect(selectBetween(beats, 2.25, 1.25), 'dragging backwards is the same').toEqual([2, 3, 4])
    expect(selectBetween(beats, 20, 30)).toEqual([])
  })

  it('selectRange covers every index between two, whichever comes first', () => {
    expect(selectRange(3, 6)).toEqual([3, 4, 5, 6])
    expect(selectRange(6, 3)).toEqual([3, 4, 5, 6])
    expect(selectRange(2, 2)).toEqual([2])
    expect(selectRange(NaN, 2)).toEqual([])
  })

  it('toggleIndex adds or removes one index and keeps the list sorted', () => {
    const sel = Object.freeze([2, 5, 9])
    expect(toggleIndex(sel, 7)).toEqual([2, 5, 7, 9])
    expect(toggleIndex(sel, 5)).toEqual([2, 9])
    expect(toggleIndex([], 0)).toEqual([0])
    expect(toggleIndex([4], 4)).toEqual([])
  })

  it('spanOf and filled give the ends and the block without holes', () => {
    expect(spanOf([2, 5, 9])).toEqual([2, 9])
    expect(spanOf([])).toBeNull()
    expect(filled([2, 5, 9])).toEqual([2, 3, 4, 5, 6, 7, 8, 9])
    expect(filled([4])).toEqual([4])
    expect(filled([])).toEqual([])
  })

  it('tempoOf is the bpm of the median interval of the block', () => {
    expect(tempoOf(grid().beats, [3, 4, 5, 6])).toBeCloseTo(120, 6)
    // un intervalo raro no mueve la mediana
    const odd = [0, 0.5, 1, 1.5, 3, 3.5]
    expect(tempoOf(odd, [0, 1, 2, 3, 4, 5])).toBeCloseTo(120, 6)
    // la selección con huecos cuenta como el bloque seguido
    expect(tempoOf(grid().beats, [3, 6])).toBeCloseTo(120, 6)
    expect(tempoOf(grid().beats, [3])).toBe(0)
    expect(tempoOf(grid().beats, [])).toBe(0)
  })
})

describe('moving a block of beats', () => {
  it('shifts the chosen beats together, to the millisecond, and tells how much', () => {
    const own = frozen(grid())
    const res = moveBlock(own, frozen([3, 4]), 0.1234)
    expect(res.applied).toBe(0.123)
    expect(res.beats.slice(2, 6)).toEqual([1.25, 1.873, 2.373, 2.75])
    expect(res.selection).toEqual([3, 4])
    expect(res.bars, 'the «1» stay with their indices').toEqual(own.bars)
    expect(res.meter).toBe(4)
    expectValid(res)
  })

  it('the unchosen beats come out exactly as they were', () => {
    const own = grid()
    const res = moveBlock(own, [6], 0.2)
    own.beats.forEach((t, i) => {
      if (i !== 6) expect(res.beats[i]).toBe(t)
    })
  })

  it('stops 20 ms short of the neighbours it would run into', () => {
    const own = grid()
    // los pulsos 3 y 4 (1,75 y 2,25) tienen al 5 a las 2,75
    expect(moveBlock(own, [3, 4], 0.9).applied).toBe(0.48)
    expect(moveBlock(own, [3, 4], 0.9).beats[4]).toBe(2.73)
    // y al 2 a la 1,25 por detrás
    expect(moveBlock(own, [3, 4], -0.9).applied).toBe(-0.48)
    expect(moveBlock(own, [3, 4], -0.9).beats[3]).toBe(1.27)
  })

  it('does not go below zero nor past the duration', () => {
    const own = grid()
    expect(moveBlock(own, [0], -1).applied).toBe(-0.25)
    expect(moveBlock(own, [0], -1).beats[0]).toBe(0)
    const res = moveBlock(own, [15], 2, { duration: 8 })
    expect(res.applied).toBe(0.25)
    expect(res.beats[15]).toBe(8)
    expectValid(res, { duration: 8 })
  })

  it('a selection with holes moves as one and is held back by the nearest unchosen of each beat', () => {
    // el 0 y el 2 van juntos; el 1 se queda entre los dos
    const own = { beats: [0.5, 1, 1.1, 2, 3], bars: [0], meter: 4 }
    const right = moveBlock(own, [0, 2], 5)
    // el 0 chocaría con el 1 a 0,98 más; el 2, con el 3 (2 s) a 0,88 más
    expect(right.applied).toBe(0.48)
    expect(right.beats).toEqual([0.98, 1, 1.58, 2, 3])
    const second = { beats: [0.2, 1, 1.1, 2, 3], bars: [], meter: 4 }
    expect(moveBlock(second, [0, 2], 5).applied).toBe(0.78)
    // por detrás: el 2 (1,1) no puede pasar del 1 (1,0 + 20 ms)
    const back = moveBlock(own, [0, 2], -5)
    expect(back.applied).toBe(-0.08)
    expect(back.beats[0]).toBe(0.42)
    expect(back.beats[2]).toBe(1.02)
    expectValid(right)
    expectValid(back)
  })

  it('a pair already closer than 20 ms is not made worse, and can still stay put', () => {
    const own = { beats: [0.5, 0.51, 1.5], bars: [], meter: 4 }
    expect(moveBlock(own, [1], -0.2).applied).toBe(0)
    expect(moveBlock(own, [1], 0.3).applied).toBe(0.3)
  })

  it('with nothing chosen or no distance, nothing moves', () => {
    const own = grid()
    expect(moveBlock(own, [], 0.3)).toMatchObject({ beats: own.beats, applied: 0 })
    expect(moveBlock(own, [3], NaN).applied).toBe(0)
    expect(moveBlock(own, [3], 0).beats).toEqual(own.beats)
    expect(moveBlock(own, [99], 0.3).applied, 'an index that does not exist is ignored').toBe(0)
  })
})

describe('stretching a block of beats', () => {
  it('scales the distances from the fixed end: the interior goes in proportion', () => {
    const own = frozen({ beats: [0, 1, 1.5, 3, 10], bars: [1], meter: 4 })
    const first = stretchBlock(own, frozen([1, 2, 3]), { fixed: 'first', to: 5 })
    // el bloque de 1 a 3 pasa a medir el doble desde el 1; el 1,5 va a 2
    expect(first.beats).toEqual([0, 1, 2, 5, 10])
    const last = stretchBlock(own, [1, 2, 3], { fixed: 'last', to: -1 })
    // ... y desde el 3 hacia atrás: el 1 va a -1 (se queda en 0,02 tras el 0)
    expect(last.beats[1]).toBe(0.02)
    const back = stretchBlock(own, [1, 2, 3], { fixed: 'last', to: 0.5 })
    expect(back.beats).toEqual([0, 0.5, 1.125, 3, 10])
    expect(first.bars).toEqual([1])
    expectValid(first)
  })

  it('opening the block upwards takes the end to where it was asked, from either side', () => {
    const own = grid()
    const a = stretchBlock(own, [3, 4, 5], { fixed: 'first', to: 3.2 })
    expect(a.beats.slice(3, 6)).toEqual([1.75, 2.475, 3.2])
    const b = stretchBlock(own, [3, 4, 5], { fixed: 'last', to: 1.5 })
    expect(b.beats.slice(3, 6)).toEqual([1.5, 2.125, 2.75])
    expect(a.selection).toEqual([3, 4, 5])
    expect(a.bars).toEqual(own.bars)
    expectValid(a)
    expectValid(b)
  })

  it('does not pass the unchosen beat on its side: it stops 20 ms before', () => {
    const own = grid()
    const a = stretchBlock(own, [3, 4, 5], { fixed: 'first', to: 10 })
    expect(a.beats[5]).toBe(3.23)
    expect(a.beats[6], 'the next one is where it was').toBe(3.25)
    const b = stretchBlock(own, [3, 4, 5], { fixed: 'last', to: -10 })
    expect(b.beats[3]).toBe(1.27)
    expectValid(a)
    expectValid(b)
  })

  it('does not go past zero nor the duration', () => {
    const own = grid()
    const a = stretchBlock(own, [13, 14, 15], { fixed: 'first', to: 100 }, { duration: 8 })
    expect(a.beats[15]).toBe(8)
    expectValid(a, { duration: 8 })
    const b = stretchBlock(own, [0, 1, 2], { fixed: 'last', to: -3 })
    expect(b.beats[0]).toBe(0)
    expectValid(b)
  })

  it('shrinking never reverses the order: every interval keeps 20 ms', () => {
    const own = grid()
    const squeezed = stretchBlock(own, [3, 4, 5, 6], { fixed: 'first', to: 0 })
    expect(squeezed.beats[3]).toBe(1.75)
    expect(squeezed.beats[4] - squeezed.beats[3]).toBeCloseTo(0.02, 9)
    expect(squeezed.beats[6] - squeezed.beats[3]).toBeCloseTo(0.06, 9)
    expectValid(squeezed)
    const past = stretchBlock(own, [3, 4, 5, 6], { fixed: 'last', to: 9 })
    expect(past.beats[3]).toBe(3.19)
    expect(past.beats[6]).toBe(3.25)
    expect(past.beats[4] - past.beats[3]).toBeCloseTo(0.02, 9)
    expectValid(past)
  })

  it('squeezing to the limit keeps the proportions of the intervals', () => {
    const own = { beats: [1, 1.5, 1.6, 5], bars: [], meter: 4 }
    // el intervalo más corto (0,1 s) se queda en 20 ms: todo se encoge a una quinta parte
    const res = stretchBlock(own, [0, 1, 2], { fixed: 'first', to: 0 })
    expect(res.beats).toEqual([1, 1.1, 1.12, 5])
    const back = stretchBlock(own, [0, 1, 2], { fixed: 'last', to: 9 })
    expect(back.beats).toEqual([1.48, 1.58, 1.6, 5])
  })

  it('a selection with holes stretches the whole block between its ends', () => {
    const res = stretchBlock(grid(), [3, 5], { fixed: 'first', to: 3.2 })
    expect(res.beats.slice(3, 6)).toEqual([1.75, 2.475, 3.2])
    expect(res.selection, 'the selection stays as it was given').toEqual([3, 5])
  })

  it('with one beat or a wrong target nothing changes', () => {
    const own = grid()
    expect(stretchBlock(own, [3], { fixed: 'first', to: 4 }).beats).toEqual(own.beats)
    expect(stretchBlock(own, [3, 4], { fixed: 'middle', to: 4 }).beats).toEqual(own.beats)
    expect(stretchBlock(own, [3, 4], { fixed: 'first', to: NaN }).beats).toEqual(own.beats)
  })

  it('the beats outside the block come out as they were', () => {
    const own = grid()
    const res = stretchBlock(own, [4, 5, 6], { fixed: 'first', to: 3.5 })
    own.beats.forEach((t, i) => {
      if (i < 4 || i > 6) expect(res.beats[i]).toBe(t)
    })
  })
})

describe('spreading the beats evenly', () => {
  it('puts equal distances between the first and the last of the block', () => {
    const own = frozen({ beats: [0.5, 0.6, 1.7, 2, 3], bars: [0], meter: 4 })
    const res = spreadEven(own, frozen([0, 1, 2, 3]))
    expect(res.beats).toEqual([0.5, 1, 1.5, 2, 3])
    expect(res.bars).toEqual([0])
    expect(res.selection).toEqual([0, 1, 2, 3])
    // dos extremos bastan: lo de en medio entra en el bloque
    expect(spreadEven(own, [0, 3]).beats).toEqual([0.5, 1, 1.5, 2, 3])
    expectValid(res)
  })

  it('leaves alone a block with no room for 20 ms between beats, or with one beat', () => {
    const tight = { beats: [1, 1.01, 1.02, 1.03, 2], bars: [], meter: 4 }
    expect(spreadEven(tight, [0, 1, 2, 3]).beats).toEqual(tight.beats)
    expect(spreadEven(grid(), [4]).beats).toEqual(grid().beats)
  })
})

describe('typing a tempo for a stretch (setTempoSpan)', () => {
  it('a 76 bpm stretch in the middle of a 120 bpm song: the count changes and the outside is as it was', () => {
    const own = frozen(song())
    const res = setTempoSpan(own, [10, 30], 76)
    expectValid(res)
    // el 10 (índice 20) es el primero de dentro: de ahí, un pulso cada 0,789 s
    const period = 60 / 76
    const at = res.selection
    expect(at[0]).toBe(20)
    expect(res.beats[20]).toBe(10)
    expect(res.beats[21]).toBeCloseTo(10 + period, 3)
    expect(tempoOf(res.beats, at)).toBeCloseTo(76, 1)
    // el último (29,737) quedaría a 0,263 s del 30, menos de 0,4 periodos: se descarta
    expect(res.added).toBe(25)
    expect(res.removed).toBe(40)
    expect(res.beats).toHaveLength(80 - 40 + 25)
    expect(res.beats.at(at.at(-1) + 1)).toBe(30)
    // fuera, lo mismo
    expect(res.beats.slice(0, 20)).toEqual(own.beats.slice(0, 20))
    expect(res.beats.slice(45)).toEqual(own.beats.slice(60))
  })

  it('the «1» of the stretch fall every meter generated beats, the closest one to the old «1» first', () => {
    const res = setTempoSpan(song(), [10, 30], 76)
    const inside = res.bars.filter((b) => res.selection.includes(b))
    expect(inside).toEqual([20, 24, 28, 32, 36, 40, 44])
    // el primero viejo estaba en 10 y el generado más cerca está en 10
    expect(res.beats[inside[0]]).toBe(10)
    // fuera: los de antes igual, los de después corridos lo que cambió el número de pulsos
    const own = song()
    expect(res.bars.filter((b) => b < 20)).toEqual(own.bars.filter((b) => b < 20))
    expect(res.bars.filter((b) => b > 44)).toEqual(
      own.bars.filter((b) => b >= 60).map((b) => b - 15)
    )
  })

  it('the «1» goes to the generated beat closest to the old «1», not to the first one', () => {
    // pulsos cada 0,5 con el «1» en el 5 (2,5 s): con el tramo [1, 4) a 90 bpm
    // (0,667 s) y la fase desde el 1, el más cercano a 2,5 es el de 2,333
    const own = { beats: song().beats, bars: [5, 9], meter: 4 }
    const res = setTempoSpan(own, [1, 4], 90)
    const ones = res.bars.filter((b) => res.selection.includes(b))
    expect(ones.map((b) => res.beats[b])).toEqual([2.333])
  })

  it('the «1» pattern goes backward from the closest generated beat too', () => {
    // el «1» viejo está en 9,5; con la fase desde el 1, 90 bpm: el generado más
    // cercano es el 13.º (9,667), y de ahí cada 4 hacia atrás: 1.º, 5.º, 9.º...
    const own = { beats: song().beats, bars: [19], meter: 4 }
    const res = setTempoSpan(own, [1, 20], 90)
    const first = res.selection[0]
    const ones = res.bars.filter((b) => res.selection.includes(b)).map((b) => b - first)
    expect(ones.slice(0, 5)).toEqual([1, 5, 9, 13, 17])
    expect(res.beats[first + 13]).toBeCloseTo(9.667, 3)
  })

  it('without a «1» in the stretch, the bar of the beat before it goes on', () => {
    // «1» en el 0 y el 8 (antes) y el 24 (después); el tramo [6,12) no tiene ninguno
    const own = { beats: song().beats, bars: [0, 8, 24], meter: 4 }
    const res = setTempoSpan(own, [6.5, 12], 120)
    // antes del tramo está el pulso 12 (6,0): lleva 4 desde el último «1» (el 8)
    expect(res.selection[0]).toBe(13)
    const ones = res.bars.filter((b) => res.selection.includes(b))
    // el primero generado es el quinto desde el «1»: el «1» cae tres después
    expect(ones).toEqual([16, 20])
    expectValid(res)
    // sin pulso anterior ni «1» dentro, el primero generado es el «1»
    const lonely = setTempoSpan({ beats: [1, 1.5, 3, 3.5], bars: [], meter: 4 }, [0, 2], 120)
    expect(lonely.bars).toEqual([0])
  })

  it('a meter of zero (no accent) puts no «1» in the stretch', () => {
    const own = { beats: song().beats, bars: [], meter: 0 }
    const res = setTempoSpan(own, [10, 30], 76)
    expect(res.bars).toEqual([])
    expect(res.meter).toBe(0)
  })

  it('the phase comes from the anchor when given, and from the first old beat when not', () => {
    const own = song()
    const inferred = setTempoSpan(own, [10.2, 20], 100)
    // el primero que había dentro es el 10,5
    expect(inferred.beats[inferred.selection[0]]).toBe(10.5)
    const anchored = setTempoSpan(own, [10, 20], 100, { anchor: 10.2 })
    expect(anchored.beats[anchored.selection[0]]).toBe(10.2)
    expect(anchored.beats[anchored.selection[1]]).toBe(10.8)
    // un ancla fuera del tramo: la cuenta sigue desde allí (3,1 + 12 × 0,6 = 10,3)
    const far = setTempoSpan(own, [10, 20], 100, { anchor: 3.1 })
    expect(far.beats[far.selection[0]]).toBe(10.3)
    // el peine admite quedar 5 ms antes del principio del tramo
    const edge = setTempoSpan(own, [10, 20], 100, { anchor: 9.996 })
    expect(edge.beats[edge.selection[0]]).toBe(9.996)
    const late = setTempoSpan(own, [10, 20], 100, { anchor: 9.99 })
    expect(late.beats[late.selection[0]]).toBe(10.59)
    // sin ancla ni pulsos, desde el principio del tramo
    const hole = setTempoSpan({ beats: [0, 1, 20, 21], bars: [], meter: 4 }, [5, 8], 60)
    expect(hole.beats.slice(2, 5)).toEqual([5, 6, 7])
    expect(hole.removed).toBe(0)
  })

  it('goes forward and backward from an anchor in the middle of the stretch', () => {
    const res = setTempoSpan(song(), [10, 20], 100, { anchor: 15.25 })
    const times = res.selection.map((i) => res.beats[i])
    expect(times[0]).toBe(10.45)
    expect(times).toContain(15.25)
    expect(times.at(-1)).toBe(19.45)
    expect(res.beats[res.selection.at(-1) + 1]).toBe(20)
  })

  it('the bpm is held between 20 and 300', () => {
    const slow = setTempoSpan(song(), [10, 30], 5)
    expect(tempoOf(slow.beats, slow.selection)).toBeCloseTo(20, 1)
    const fast = setTempoSpan(song(), [10, 30], 900)
    expect(tempoOf(fast.beats, fast.selection)).toBeCloseTo(300, 0)
    expectValid(fast)
  })

  it('an empty, reversed or invalid stretch leaves the song as it was', () => {
    const own = song()
    for (const span of [
      [20, 20],
      [20, 10],
      [NaN, 5]
    ]) {
      const res = setTempoSpan(own, span, 100)
      expect(res).toMatchObject({
        beats: own.beats,
        bars: own.bars,
        selection: [],
        added: 0,
        removed: 0
      })
    }
    expect(setTempoSpan(own, [10, 20], NaN).beats).toEqual(own.beats)
  })

  it('a stretch at the very start or the very end', () => {
    const own = grid()
    const start = setTempoSpan(own, [0, 3], 60)
    expect(start.beats.slice(0, 3)).toEqual([0.25, 1.25, 2.25])
    expect(start.removed).toBe(6)
    expect(start.selection).toEqual([0, 1, 2])
    expect(start.bars[0]).toBe(0)
    expectValid(start)
    const end = setTempoSpan(song(), [38, 100], 60, { duration: 40 })
    expect(end.beats.at(-1)).toBeLessThanOrEqual(40)
    expect(end.beats.at(-1)).toBe(40)
    expectValid(end, { duration: 40 })
    // sin duración, todo el tramo
    expect(setTempoSpan(song(), [38, 60], 60).beats.at(-1)).toBe(59)
    // el tramo cubre toda la canción: no queda vecino ninguno
    const all = setTempoSpan(song(), [0, 40], 60)
    expect(all.beats).toHaveLength(40)
    expectValid(all)
  })

  it('drops the generated beat that would be closer than 0.4 periods to a neighbour', () => {
    const own = song()
    // 120 bpm: periodo 0,5; el pulso de antes del tramo está en 5,0
    const front = setTempoSpan(own, [5.1, 8], 120, { anchor: 5.12 })
    expect(front.beats[front.selection[0]], '5,12 queda a 120 ms del 5: fuera').toBe(5.62)
    // y por detrás: el pulso de después está en 8,0
    const back = setTempoSpan(own, [5.1, 8], 120, { anchor: 5.4 })
    const times = back.selection.map((i) => back.beats[i])
    expect(times.at(-1), '7,9 queda a 100 ms del 8: fuera').toBe(7.4)
    expectValid(front)
    expectValid(back)
    // por el borde: justo a 0,4 periodos se queda (0,2 s a 120 bpm)
    const keep = setTempoSpan(own, [5.1, 8], 120, { anchor: 5.2 })
    expect(keep.beats[keep.selection[0]]).toBe(5.2)
  })

  it('does not leave the song with fewer than two beats', () => {
    const own = { beats: [1, 2], bars: [0], meter: 4 }
    const res = setTempoSpan(own, [0.5, 2.5], 20)
    // 20 bpm: 3 s de periodo: solo cabría el 1; dos pulsos no quedan
    expect(res.beats).toEqual([1, 2])
    expect(res.added).toBe(0)
  })

  it('does not change what it receives', () => {
    const own = frozen(song())
    setTempoSpan(own, [10, 30], 76)
    setTempoSpan(own, [10, 30], 76, { anchor: 12, duration: 40 })
  })
})

describe('doubling and halving the beats', () => {
  it('doubleBeats puts a beat in the middle of every gap of the block; the last one gets none', () => {
    const own = frozen(grid())
    const res = doubleBeats(own, frozen([2, 3, 4]))
    expect(res.beats.slice(1, 8)).toEqual([0.75, 1.25, 1.5, 1.75, 2, 2.25, 2.75])
    expect(res.beats).toHaveLength(18)
    expect(res.selection, 'the old block and the new beats').toEqual([2, 3, 4, 5, 6])
    expectValid(res)
  })

  it('the «1» stay on their beats', () => {
    const own = grid()
    const res = doubleBeats(own, [2, 3, 4, 5, 6])
    expect(res.bars.map((b) => res.beats[b])).toEqual(own.bars.map((b) => own.beats[b]))
    expect(res.bars).toEqual([1, 8, 13, 17])
  })

  it('a gap with no room for a beat 20 ms from each side is skipped', () => {
    const own = { beats: [0, 0.03, 1], bars: [], meter: 4 }
    const res = doubleBeats(own, [0, 1, 2])
    expect(res.beats).toEqual([0, 0.03, 0.515, 1])
    expect(doubleBeats(grid(), [4]).beats, 'one beat has no gap').toEqual(grid().beats)
  })

  it('halveBeats removes every second beat of the block, keeping the first «1» of it', () => {
    const own = frozen(grid())
    const res = halveBeats(own, frozen([0, 1, 2, 3, 4, 5, 6, 7]))
    // el primer «1» de dentro es el 1: se quedan 1, 3, 5, 7 y se quitan 0, 2, 4, 6
    expect(res.beats).toEqual([
      0.75, 1.75, 2.75, 3.75, 4.25, 4.75, 5.25, 5.75, 6.25, 6.75, 7.25, 7.75
    ])
    // los «1» de pulsos que se quedan siguen siéndolo (el 1 y el 5); el de 9 y 13 se corren
    expect(res.bars).toEqual([0, 2, 5, 9])
    expect(res.selection).toEqual([0, 1, 2, 3])
    expectValid(res)
  })

  it('without a «1» in the block, the first beat of the block is kept', () => {
    const own = { beats: song().beats, bars: [0, 20], meter: 4 }
    const res = halveBeats(own, [3, 4, 5, 6, 7])
    // se quedan el 3, 5 y 7; se quitan el 4 y el 6
    expect(res.beats.slice(2, 7)).toEqual([1, 1.5, 2.5, 3.5, 4])
    expect(res.bars).toEqual([0, 18])
    expect(res.selection).toEqual([3, 4, 5])
  })

  it('the «1» on a removed beat is dropped', () => {
    const own = { beats: song().beats, bars: [0, 3, 6], meter: 3 }
    // el primer «1» de dentro es el 0: el 3 y el 6 caen en posiciones impares y pares
    const res = halveBeats(own, [0, 1, 2, 3, 4, 5, 6])
    expect(res.bars).toEqual([0, 3])
    expect(res.beats.slice(0, 4)).toEqual([0, 1, 2, 3])
  })

  it('never leaves fewer than two beats in the song', () => {
    const two = { beats: [1, 2], bars: [0], meter: 4 }
    expect(halveBeats(two, [0, 1]).beats).toEqual([1, 2])
    const three = { beats: [1, 2, 3], bars: [], meter: 4 }
    expect(halveBeats(three, [0, 1, 2]).beats).toEqual([1, 3])
    expect(halveBeats(grid(), [4]).beats, 'one beat is not a block').toEqual(grid().beats)
  })

  it('doubling and then halving the same block gives the song back', () => {
    const own = grid()
    const doubled = doubleBeats(own, filled([2, 8]))
    const back = halveBeats(doubled, doubled.selection)
    expect(back.beats).toEqual(own.beats)
    expect(back.bars).toEqual(own.bars)
  })
})

describe('bars every n beats', () => {
  it('puts a «1» on the first chosen beat and every n after, replacing the old ones inside', () => {
    const own = frozen(grid())
    // los «1» de dentro (5 y 9) se van; quedan el 1 y el 13
    const res = barsEvery(own, frozen([4, 5, 6, 7, 8, 9, 10, 11]), 3)
    expect(res.bars).toEqual([1, 4, 7, 10, 13])
    expect(res.selection).toEqual([4, 5, 6, 7, 8, 9, 10, 11])
    expect(res.beats).toEqual(own.beats)
    expect(res.meter).toBe(4)
    expectValid(res)
  })

  it('works from the ends of a selection with holes and does not touch the outside', () => {
    const res = barsEvery(grid(), [5, 11], 2)
    expect(res.bars).toEqual([1, 5, 7, 9, 11, 13])
    expect(barsEvery(grid(), [0, 4], 4).bars).toEqual([0, 4, 5, 9, 13])
  })

  it('n only goes from 2 to 8; with setMeter the song meter follows', () => {
    const own = grid()
    expect(barsEvery(own, [4, 5], 1).bars).toEqual(own.bars)
    expect(barsEvery(own, [4, 5], 9).bars).toEqual(own.bars)
    expect(barsEvery(own, [4, 5], NaN).bars).toEqual(own.bars)
    expect(barsEvery(own, [], 3).bars).toEqual(own.bars)
    expect(barsEvery(own, [4, 5, 6], 6, { setMeter: true }).meter).toBe(6)
    expect(barsEvery(own, [4, 5, 6], 6).meter).toBe(4)
  })
})

describe('removing and adding beats', () => {
  it('removeBeats takes them out, re-indexes the «1» and empties the selection', () => {
    const own = frozen(grid())
    const res = removeBeats(own, frozen([2, 3, 5]))
    expect(res.beats).toHaveLength(13)
    expect(res.beats.slice(0, 4)).toEqual([0.25, 0.75, 2.25, 3.25])
    // el 5 (un «1») se quita; el 9 pasa a 6 y el 13 a 10; el 1 se queda
    expect(res.bars).toEqual([1, 6, 10])
    expect(res.selection).toEqual([])
    expectValid(res)
  })

  it('keeps at least two beats: otherwise it does nothing', () => {
    const three = { beats: [1, 2, 3], bars: [1], meter: 4 }
    const refused = removeBeats(three, [0, 1])
    expect(refused.beats).toEqual([1, 2, 3])
    expect(refused.bars).toEqual([1])
    expect(removeBeats(three, [1]).beats).toEqual([1, 3])
    expect(removeBeats(three, []).beats).toEqual([1, 2, 3])
    expect(removeBeats(three, [7]).beats, 'an index that does not exist').toEqual([1, 2, 3])
  })

  it('addBeatAt inserts in order and moves the «1» after it one place, like the screen does', () => {
    const own = frozen(grid())
    const res = addBeatAt(own, 1.5)
    expect(res.beats.slice(1, 5)).toEqual([0.75, 1.25, 1.5, 1.75])
    expect(res.selection).toEqual([3])
    expect(res.bars).toEqual([1, 6, 10, 14])
    expect(res.refused).toBeUndefined()
    expectValid(res)
    // al principio, todos los «1» corren
    const first = addBeatAt(own, 0)
    expect(first.beats[0]).toBe(0)
    expect(first.selection).toEqual([0])
    expect(first.bars).toEqual([2, 6, 10, 14])
    // justo antes de un «1»: ese «1» corre con el pulso (el 1 pasa a ser el 2)
    const before = addBeatAt(own, 0.5)
    expect(before.selection).toEqual([1])
    expect(before.bars).toEqual([2, 6, 10, 14])
    expect(addBeatAt(own, 9).beats.at(-1)).toBe(9)
    expect(addBeatAt(own, 9).bars).toEqual(own.bars)
  })

  it('refuses a beat closer than 30 ms to another, or out of range', () => {
    const own = grid()
    for (const t of [1.27, 1.221, 1.25, 1.2791]) {
      const res = addBeatAt(own, t)
      expect(res.refused, String(t)).toBe(true)
      expect(res.beats).toEqual(own.beats)
      expect(res.bars).toEqual(own.bars)
      expect(res.selection).toEqual([])
    }
    expect(addBeatAt(own, 1.28).refused, '30 ms is allowed').toBeUndefined()
    expect(addBeatAt(own, -0.1).refused).toBe(true)
    expect(addBeatAt(own, NaN).refused).toBe(true)
    expect(addBeatAt(own, 9, { duration: 8 }).refused).toBe(true)
  })
})

describe('reverting a stretch to the automatic click', () => {
  it('puts the beats and the «1» of the automatic grid back in the span', () => {
    const auto = grid()
    const edited = setTempoSpan(auto, [2, 5], 76)
    expect(edited.beats).not.toEqual(auto.beats)
    const res = revertToAuto(frozen(edited), frozen(auto), [2, 5])
    expect(res.beats).toEqual(auto.beats)
    expect(res.bars).toEqual(auto.bars)
    // los pulsos que vienen de `auto`: los de 2,25 a 4,75
    expect(res.selection).toEqual([4, 5, 6, 7, 8, 9])
    expectValid(res)
  })

  it('only the span changes; the rest of the manual click stays', () => {
    const auto = grid()
    const manual = moveBlock(moveBlock(auto, [2], 0.1), [10], -0.1)
    const res = revertToAuto(manual, auto, [0, 2])
    expect(res.beats[2]).toBe(1.25)
    expect(res.beats[10], 'the edit outside is kept').toBe(5.15)
  })

  it('the auto beat that would sit closer than 20 ms to a neighbour outside is dropped', () => {
    const own = { beats: [0, 1, 1.99, 3, 4], bars: [], meter: 4 }
    const auto = { beats: [0, 1, 2, 3, 4], bars: [0], meter: 4 }
    // el 2,0 de auto quedaría a 10 ms del 1,99 de fuera
    const front = revertToAuto(own, auto, [2, 3.5])
    expect(front.beats).toEqual([0, 1, 1.99, 3, 4])
    expect(front.selection).toEqual([3])
    const manual = { beats: [0, 1, 2, 3.01, 4], bars: [], meter: 4 }
    const back = revertToAuto(manual, auto, [0.5, 3])
    expect(back.beats).toEqual([0, 1, 2, 3.01, 4])
    expect(back.selection).toEqual([1, 2])
    expectValid(front)
    expectValid(back)
  })

  it('with no beats of the automatic click inside the span, the manual ones there go', () => {
    const own = song(10)
    const auto = { beats: [0, 1, 2, 3, 4, 5], bars: [], meter: 4 }
    const res = revertToAuto(own, auto, [3.1, 3.9])
    expect(res.beats).toEqual(own.beats.filter((t) => t < 3.1 || t > 3.9))
    expect(res.selection).toEqual([])
    // y un tramo al revés no hace nada
    expect(revertToAuto(own, auto, [4, 3]).beats).toEqual(own.beats)
  })
})

describe('edits in sequence never break the click', () => {
  // un generador de números con semilla, para que la prueba sea siempre la misma
  const seeded = (seed) => () => {
    seed = (seed + 0x6d2b79f5) | 0
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }

  it('300 random edits keep every invariant and never touch their inputs', () => {
    const rand = seeded(7)
    const auto = frozen(song(80))
    const duration = 40
    let own = song(80)
    const pick = (list) => list[Math.floor(rand() * list.length)]
    for (let step = 0; step < 300; step++) {
      const n = own.beats.length
      const a = Math.floor(rand() * n)
      const b = Math.min(n - 1, a + Math.floor(rand() * 8))
      const sel = selectRange(a, b)
      const t0 = rand() * duration
      const t1 = t0 + rand() * 12
      const op = pick([
        'move',
        'stretch',
        'spread',
        'tempo',
        'double',
        'halve',
        'bars',
        'remove',
        'add',
        'revert'
      ])
      const input = frozen({ beats: [...own.beats], bars: [...own.bars], meter: own.meter })
      const frozenSel = frozen([...sel])
      let res
      if (op === 'move') res = moveBlock(input, frozenSel, (rand() - 0.5) * 2, { duration })
      else if (op === 'stretch') {
        res = stretchBlock(
          input,
          frozenSel,
          { fixed: pick(['first', 'last']), to: rand() * duration },
          { duration }
        )
      } else if (op === 'spread') res = spreadEven(input, frozenSel)
      else if (op === 'tempo') res = setTempoSpan(input, [t0, t1], 30 + rand() * 200, { duration })
      else if (op === 'double') res = n < 160 ? doubleBeats(input, frozenSel) : input
      else if (op === 'halve') res = halveBeats(input, frozenSel)
      else if (op === 'bars') res = barsEvery(input, frozenSel, 2 + Math.floor(rand() * 7))
      else if (op === 'remove') res = n > 20 ? removeBeats(input, frozenSel) : input
      else if (op === 'add') res = addBeatAt(input, rand() * duration, { duration })
      else res = revertToAuto(input, auto, [t0, t1])
      expectValid({ selection: [], ...res }, { duration })
      own = { beats: res.beats, bars: res.bars, meter: res.meter }
    }
    expect(own.beats.length).toBeGreaterThanOrEqual(2)
  })
})
