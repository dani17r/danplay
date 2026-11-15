import { describe, it, expect } from 'vitest'
import { BeatHistory } from '../src/utils/beatHistory.js'

/** Un clic propio de pruebas: `n` pulsos y las zonas que se tocaron. */
const own = (n, touched) => ({
  beats: Array.from({ length: n }, (_, i) => i * 0.5),
  bars: [0],
  meter: 4,
  ...(touched ? { touched } : {})
})

/** Un reloj a mano, para no esperar nunca. */
function clock(start = 1000) {
  let t = start
  const now = () => t
  now.tick = (ms) => (t += ms)
  return now
}

describe('undo and redo of the click of one song', () => {
  it('starts empty: nothing to undo or redo', () => {
    const h = new BeatHistory()
    expect(h.canUndo).toBe(false)
    expect(h.canRedo).toBe(false)
    expect(h.undoLabel).toBe('')
    expect(h.redoLabel).toBe('')
    expect(h.size).toEqual([0, 0])
    expect(h.undo(own(3))).toBeNull()
    expect(h.redo(own(3))).toBeNull()
  })

  it('undo gives back the state before the last change, and redo the state after it', () => {
    const h = new BeatHistory()
    const a = own(2)
    const b = own(3)
    const c = own(4)
    h.record('Mover', a) // a → b
    h.record('Doblar', b) // b → c
    expect(h.size).toEqual([2, 0])
    expect(h.undoLabel).toBe('Doblar')

    const first = h.undo(c)
    expect(first).toEqual({ label: 'Doblar', own: b })
    expect(h.size).toEqual([1, 1])
    expect(h.redoLabel).toBe('Doblar')
    expect(h.undoLabel).toBe('Mover')

    const second = h.undo(b)
    expect(second).toEqual({ label: 'Mover', own: a })
    expect(h.canUndo).toBe(false)
    expect(h.undo(a)).toBeNull()

    // rehacer, en el orden en que se hizo
    expect(h.redo(a)).toEqual({ label: 'Mover', own: b })
    expect(h.redo(b)).toEqual({ label: 'Doblar', own: c })
    expect(h.redo(c)).toBeNull()
    expect(h.size).toEqual([2, 0])
    // y deshacer otra vez vuelve por donde vino
    expect(h.undo(c)).toEqual({ label: 'Doblar', own: b })
  })

  it('a song with no manual click yet: undoing back to it gives null (use the automatic one)', () => {
    const h = new BeatHistory()
    h.record('Mover', null)
    const current = own(5)
    const undone = h.undo(current)
    expect(undone).toEqual({ label: 'Mover', own: null })
    // rehacer devuelve el clic que se tenía
    expect(h.redo(null)).toEqual({ label: 'Mover', own: current })
    // y rehaciendo desde «sin clic», el siguiente deshacer vuelve a null
    expect(h.undo(current)).toEqual({ label: 'Mover', own: null })
  })

  it('a new change clears what could be redone', () => {
    const h = new BeatHistory()
    h.record('Uno', own(1))
    h.record('Dos', own(2))
    h.undo(own(3))
    expect(h.canRedo).toBe(true)
    h.record('Tres', own(2))
    expect(h.canRedo).toBe(false)
    expect(h.redoLabel).toBe('')
    expect(h.redo(own(9))).toBeNull()
    expect(h.size).toEqual([2, 0])
    expect(h.undoLabel).toBe('Tres')
  })

  it('what goes in and what comes out are copies: nothing is shared', () => {
    const h = new BeatHistory()
    const before = own(3, [[1, 2]])
    h.record('Mover', before)
    // cambiar el original después no cambia lo guardado
    before.beats.push(99)
    before.bars.push(2)
    before.touched[0][1] = 50
    const current = own(4, [[0, 1]])
    const out = h.undo(current)
    expect(out.own).toEqual(own(3, [[1, 2]]))
    // y lo devuelto no es lo guardado: tocarlo no estropea el rehacer ni otro deshacer
    out.own.beats.push(77)
    out.own.touched.push([8, 9])
    current.beats.push(55)
    current.touched[0][0] = 3
    const again = h.redo(own(3, [[1, 2]]))
    expect(again.own).toEqual(own(4, [[0, 1]]))
    again.own.touched[0][1] = 42
    expect(h.undo(own(4, [[0, 1]])).own).toEqual(own(3, [[1, 2]]))
    expect(h.redo(own(3, [[1, 2]])).own.touched).toEqual([[0, 1]])
  })

  it('keeps the touched windows of every snapshot', () => {
    const h = new BeatHistory()
    const s3 = own(3, [[1, 2]])
    const s4 = own(4, [
      [1, 2],
      [5, 6]
    ])
    const s5 = own(5, [
      [1, 2],
      [5, 6],
      [8, 9]
    ])
    h.record('Mover', s3)
    h.record('Mover otro', s4)
    expect(h.undo(s5).own.touched).toEqual([
      [1, 2],
      [5, 6]
    ])
    expect(h.undo(s4).own.touched).toEqual([[1, 2]])
    expect(h.redo(s3).own.touched).toEqual([
      [1, 2],
      [5, 6]
    ])
    expect(h.redo(s4).own.touched).toHaveLength(3)
  })

  it('an own without touched stays without it', () => {
    const h = new BeatHistory()
    h.record('Mover', own(3))
    expect('touched' in h.undo(own(4)).own).toBe(false)
  })

  it('clear forgets everything', () => {
    const h = new BeatHistory()
    h.record('Uno', own(1))
    h.record('Dos', own(2))
    h.undo(own(3))
    h.clear()
    expect(h.size).toEqual([0, 0])
    expect(h.canUndo || h.canRedo).toBe(false)
    expect(h.undo(own(1))).toBeNull()
  })
})

describe('holding a key down is one undo step (merging)', () => {
  it('the same label and key, within the window, make one step with the oldest snapshot', () => {
    const now = clock()
    const h = new BeatHistory({ now })
    // cinco pulsaciones seguidas de la flecha: se parte de 1 y se llega a 6
    for (let n = 1; n <= 5; n++) {
      h.record('Mover con flechas', own(n), 'nudge')
      now.tick(30)
    }
    expect(h.size).toEqual([1, 0])
    expect(h.undo(own(6))).toEqual({ label: 'Mover con flechas', own: own(1) })
    expect(h.redo(own(1)).own).toEqual(own(6))
  })

  it('the window slides: each press extends it', () => {
    const now = clock()
    const h = new BeatHistory({ now })
    for (let n = 1; n <= 10; n++) {
      h.record('Mover', own(n), 'nudge')
      now.tick(400) // cada una a menos de 600 ms de la anterior, pero todas juntas, 4 s
    }
    expect(h.size[0]).toBe(1)
  })

  it('a pause longer than mergeMs starts another step', () => {
    const now = clock()
    const h = new BeatHistory({ now })
    h.record('Mover', own(1), 'nudge')
    now.tick(601)
    h.record('Mover', own(2), 'nudge')
    expect(h.size[0]).toBe(2)
    // con otro mergeMs
    const wide = new BeatHistory({ now, mergeMs: 5000 })
    wide.record('Mover', own(1), 'nudge')
    now.tick(3000)
    wide.record('Mover', own(2), 'nudge')
    expect(wide.size[0]).toBe(1)
  })

  it('a different label or key does not merge', () => {
    const now = clock()
    const h = new BeatHistory({ now })
    h.record('Mover', own(1), 'nudge')
    h.record('Estirar', own(2), 'nudge')
    h.record('Estirar', own(3), 'otro')
    expect(h.size[0]).toBe(3)
  })

  it('without a key it never merges: two drags in a row are two steps', () => {
    const now = clock()
    const h = new BeatHistory({ now })
    h.record('Mover pulsos', own(1))
    h.record('Mover pulsos', own(2))
    expect(h.size[0]).toBe(2)
  })

  it('an undo in between breaks the chain', () => {
    const now = clock()
    const h = new BeatHistory({ now })
    h.record('Mover', own(1), 'nudge')
    h.record('Mover', own(2), 'nudge')
    h.undo(own(3))
    h.record('Mover', own(1), 'nudge')
    expect(h.size).toEqual([1, 0])
    // (el paso de antes se deshizo del todo: este es uno nuevo)
    const out = h.undo(own(2))
    expect(out.own).toEqual(own(1))
    expect(h.canUndo).toBe(false)
    // con otro paso de antes en la pila, lo nuevo no se pega a él
    const f = new BeatHistory({ now })
    f.record('Otra cosa', own(1))
    f.record('Mover', own(2), 'nudge')
    f.undo(own(3))
    f.record('Mover', own(2), 'nudge')
    expect(f.size).toEqual([2, 0])
    // tras deshacer y rehacer tampoco se junta con lo de antes
    const g = new BeatHistory({ now })
    g.record('Mover', own(1), 'nudge')
    g.undo(own(2))
    g.redo(own(1))
    g.record('Mover', own(2), 'nudge')
    expect(g.size[0]).toBe(2)
  })

  it('merging still clears redo', () => {
    const now = clock()
    const h = new BeatHistory({ now })
    h.record('Mover', own(1), 'nudge')
    h.record('Otra cosa', own(2))
    h.undo(own(3))
    expect(h.canRedo).toBe(true)
    h.record('Mover', own(2), 'nudge')
    expect(h.canRedo).toBe(false)
  })

  it('works with the real clock too', () => {
    const h = new BeatHistory()
    h.record('Mover', own(1), 'nudge')
    h.record('Mover', own(2), 'nudge')
    expect(h.size[0]).toBe(1)
  })
})

describe('the size of the history', () => {
  it('over the limit, the oldest steps are dropped', () => {
    const h = new BeatHistory({ limit: 3 })
    for (let n = 1; n <= 5; n++) h.record(`Paso ${n}`, own(n))
    expect(h.size).toEqual([3, 0])
    expect(h.undoLabel).toBe('Paso 5')
    expect(h.undo(own(9)).label).toBe('Paso 5')
    expect(h.undo(own(5)).label).toBe('Paso 4')
    expect(h.undo(own(4))).toEqual({ label: 'Paso 3', own: own(3) })
    expect(h.undo(own(3)), 'steps 1 and 2 are gone').toBeNull()
  })

  it('the default is 200 steps', () => {
    const h = new BeatHistory()
    for (let n = 0; n < 250; n++) h.record(`Paso ${n}`, own(2))
    expect(h.size[0]).toBe(200)
    expect(h.undoLabel).toBe('Paso 249')
  })

  it('the limit holds after undoing and redoing too', () => {
    const h = new BeatHistory({ limit: 2 })
    h.record('A', own(1))
    h.record('B', own(2))
    h.undo(own(3))
    h.undo(own(2))
    h.redo(own(1))
    h.redo(own(2))
    h.record('C', own(3))
    h.record('D', own(4))
    h.record('E', own(5))
    expect(h.size[0]).toBe(2)
  })
})
