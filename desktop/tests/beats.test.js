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
  clickTimes,
  gridToOwn,
  ownToGrid,
  ownTouched,
  touchedByEdit,
  mergeWindows,
  beatChanges,
  cleanOwn,
  windowsText
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
    // con la fase del 2 y del 6 que sacó el análisis, el «1» cae donde la oyó
    const sure = { ...grid(), phase2: 0, phase6: 5 }
    expect(withMeter(sure, 6)).toMatchObject({ meter: 6, first_downbeat: 5 })
    expect(withMeter(sure, 2)).toMatchObject({ meter: 2, first_downbeat: 0 })
    // doblada o a la mitad, esas fases siguen la cuenta (y sin ellas no salen)
    expect(doubled(sure)).toMatchObject({ phase2: 0, phase6: 10 })
    expect(halved(doubled(sure))).toMatchObject({ phase2: 0, phase6: 5 })
    expect(Object.keys(doubled(grid()))).not.toContain('phase6')
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

// Lo que movió él a mano: el clic manual guarda las zonas de la canción que
// tocó (en segundos, no en pulsos) para que un re-análisis las respete y
// rehaga lo demás.
describe('the windows moved by hand', () => {
  /** 60 pulsos a 120 bpm desde 0,5 s, el «1» cada cuatro. */
  const regular = (n = 60) => {
    const beats = Array.from({ length: n }, (_, i) => 0.5 + i * 0.5)
    return {
      bpm: 120,
      meter: 4,
      beats,
      first_downbeat: 0,
      phase3: 0,
      phase4: 0,
      confidence: 1,
      bars: Array.from({ length: Math.ceil(n / 4) }, (_, i) => i * 4)
    }
  }
  /** Sin acentos: los pulsos añadidos o quitados no mueven ningún «1» de sitio. */
  const plain = (n = 60) => ({ ...regular(n), meter: 0, bars: [] })
  const edit = (g, f) => {
    const out = { ...g, beats: [...g.beats], bars: [...g.bars] }
    f(out)
    return out
  }

  it('moving one beat marks it with one beat of margin on each side', () => {
    const before = regular()
    const after = edit(before, (g) => (g.beats[10] += 0.1))
    // el pulso 10 estaba en 5,5 y queda en 5,6; los de al lado, en 5 y en 6
    expect(touchedByEdit(before, after)).toEqual([[5, 6]])
  })

  it('a beat added or removed marks the place where it was', () => {
    const before = plain()
    const added = edit(before, (g) => g.beats.splice(22, 0, 11.25))
    expect(touchedByEdit(before, added)).toEqual([[11, 11.5]])
    const removed = edit(before, (g) => g.beats.splice(30, 1))
    // sin el de 15,5 quedan los de 15 y 16 con el hueco en medio
    expect(touchedByEdit(before, removed)).toEqual([[15, 16]])
  })

  it('nothing moved, nothing marked: what was marked stays as it was', () => {
    const before = regular()
    expect(
      touchedByEdit(
        before,
        edit(before, () => {})
      )
    ).toEqual([])
    expect(
      touchedByEdit(
        before,
        edit(before, () => {}),
        [[3, 4]]
      )
    ).toEqual([[3, 4]])
  })

  it('edits close together are one window, far apart they are two', () => {
    const before = regular()
    const near = edit(before, (g) => {
      g.beats[10] += 0.1
      g.beats[13] += 0.1
    })
    expect(touchedByEdit(before, near)).toEqual([[5, 7.5]])
    const far = edit(before, (g) => {
      g.beats[10] += 0.1
      g.beats[40] += 0.1
    })
    expect(touchedByEdit(before, far)).toEqual([
      [5, 6],
      [20, 21]
    ])
  })

  it('dragging a beat with Shift (all the ones behind move) marks up to the last one', () => {
    const before = regular()
    const after = edit(before, (g) => {
      for (let k = 40; k < g.beats.length; k++) g.beats[k] += 0.05
    })
    expect(touchedByEdit(before, after)).toEqual([[20, 30.05]])
  })

  it('the windows are times: beats added before them do not move them', () => {
    const first = edit(plain(), (g) => (g.beats[40] += 0.1))
    const w1 = touchedByEdit(plain(), first)
    expect(w1).toEqual([[20, 21]])
    // después se añade un pulso mucho antes: los índices de todos los de
    // detrás cambian, la zona de arriba no
    const second = edit(first, (g) => g.beats.splice(5, 0, 2.75))
    expect(touchedByEdit(first, second, w1)).toEqual([
      [2.5, 3],
      [20, 21]
    ])
    // y quitar uno antes de ella tampoco
    const third = edit(second, (g) => g.beats.splice(1, 1))
    const w3 = touchedByEdit(second, third, touchedByEdit(first, second, w1))
    expect(w3.at(-1)).toEqual([20, 21])
    expect(w3.length).toBe(3)
  })

  it('an edit inside a window of before grows it instead of adding another', () => {
    const before = regular()
    const first = edit(before, (g) => (g.beats[10] += 0.1))
    const w1 = touchedByEdit(before, first)
    const second = edit(first, (g) => (g.beats[11] += 0.1))
    expect(touchedByEdit(first, second, w1)).toEqual([[5, 6.5]])
  })

  it('making a beat the «1» marks it, although no beat moved', () => {
    const before = regular()
    const after = edit(
      before,
      (g) =>
        (g.bars = g.bars
          .filter((b) => b !== 12)
          .concat(13)
          .sort((a, b) => a - b))
    )
    const w = touchedByEdit(before, after)
    expect(w.length).toBeGreaterThan(0)
    // los pulsos 12 y 13 están en 6,5 y 7
    for (const t of [6.5, 7]) expect(w.some(([a, b]) => a <= t && t <= b)).toBe(true)
  })

  it('what is saved as own: the grid with its windows, or without them (before they existed)', () => {
    const g = regular(8)
    const withWindows = gridToOwn(g, [
      [2, 1],
      [1.0004, 2.5]
    ])
    expect(withWindows.touched, 'las malas se descartan y se limpian').toEqual([[1, 2.5]])
    expect(withWindows.beats.length).toBe(8)
    expect(withWindows.bars).toEqual([0, 4])
    // sin ellas, igual que antes: no se inventa un `touched`
    expect('touched' in gridToOwn(g)).toBe(false)
    // y vacías sí se guardan: «no moví nada»
    expect(gridToOwn(g, []).touched).toEqual([])
  })

  it('an own click from before has no windows: it still plays, and the core works them out', () => {
    const old = { beats: [1, 1.5, 2, 2.5, 3], bars: [0], meter: 4 }
    expect(ownToGrid(old).beats).toEqual(old.beats)
    // `null`: no se sabe cuáles (no es «ninguna»: vacías sí se guardan)
    expect(ownTouched(old)).toBe(null)
    expect(ownTouched({ ...old, touched: [[2, 2.5]] })).toEqual([[2, 2.5]])
    expect(ownTouched({ ...old, touched: [] })).toEqual([])
    expect(ownTouched(null)).toBe(null)
    // el que llega con zonas, las lleva a la rejilla sin ellas
    expect(ownToGrid({ ...old, touched: [[2, 2.5]] }).beats).toEqual(old.beats)
  })

  it('windows: only real pairs, in order, overlapping ones joined', () => {
    expect(
      mergeWindows([[5, 6], [1, 2], [1.5, 3], ['x', 2], [4, 4], null, [9, 8], [-1, 2]])
    ).toEqual([
      [1, 3],
      [5, 6]
    ])
    expect(
      mergeWindows(
        [
          [1, 2],
          [2.4, 3]
        ],
        0.5
      )
    ).toEqual([[1, 3]])
    expect(mergeWindows('nada')).toEqual([])
  })

  it('changed beats: a moved one counts once', () => {
    const a = [1, 2, 3, 4]
    expect(beatChanges(a, [1, 2, 3.2, 4]).changed).toBe(1)
    expect(beatChanges(a, [1, 2, 3, 4, 5]).changed).toBe(1)
    expect(beatChanges(a, [1, 2, 4]).changed).toBe(1)
    expect(beatChanges(a, [1.0001, 2, 3, 4]).changed, 'menos de un milisegundo no es moverlo').toBe(
      0
    )
  })

  it('what comes back from the core is cleaned, with the windows it brings or the ones sent', () => {
    const got = { beats: [1, 1.5, 2, 2.5], bars: [0], meter: 4, touched: [[1.5, 2]] }
    expect(cleanOwn(got, [[0, 9]])).toEqual({ ...got })
    expect(cleanOwn({ ...got, touched: undefined }, [[1, 2]]).touched).toEqual([[1, 2]])
    expect(cleanOwn({ beats: [1], bars: [] })).toBe(null)
  })

  it('windows to read: up to three, and how many more', () => {
    expect(windowsText([[65, 80]])).toBe('1:05–1:20')
    expect(
      windowsText([
        [65, 80],
        [130, 150],
        [200, 210],
        [300, 310],
        [320, 330]
      ])
    ).toBe('1:05–1:20, 2:10–2:30, 3:20–3:30 y 2 más')
  })
})
