// Ordenar arrastrando dentro de una lista larga.
//
// Con el puntero quieto en el borde del panel no llega ningun `pointermove`,
// asi que el desplazamiento automatico va por cuadros de animacion. Aqui se
// controlan a mano: se comprueba que cerca del borde la lista baja (o sube),
// que en medio no se mueve, y que al soltar se para del todo.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { startDrag, cancelDrag, onDrop, useDragSong } from '../src/composables/useDragSong.js'

const PANEL = { top: 100, height: 400 }

let panel, rows, frames, rafOriginal, cafOriginal, csOriginal

function pointer(el, type, x, y) {
  el.dispatchEvent(
    new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y })
  )
}

/** Ejecuta los cuadros de animacion pendientes, uno a uno. */
function frame(n = 1) {
  for (let i = 0; i < n; i++) {
    const pending = frames.splice(0)
    for (const f of pending) f(performance.now())
  }
}

beforeEach(() => {
  frames = []
  rafOriginal = window.requestAnimationFrame
  cafOriginal = window.cancelAnimationFrame
  csOriginal = window.getComputedStyle
  window.requestAnimationFrame = (f) => {
    frames.push(f)
    return frames.length
  }
  window.cancelAnimationFrame = () => {
    frames.length = 0
  }
  // jsdom no maqueta: el panel dice que se desplaza y donde esta
  window.getComputedStyle = (el) => ({ overflowY: el === panel ? 'auto' : 'visible' })

  panel = document.createElement('div')
  panel.setAttribute('data-sort-list', '')
  panel.getBoundingClientRect = () => ({
    top: PANEL.top,
    height: PANEL.height,
    bottom: PANEL.top + PANEL.height,
    left: 0,
    width: 500,
    right: 500
  })
  // scrollTop de verdad: jsdom lo deja en 0 fijo si no hay alto
  let st = 0
  Object.defineProperty(panel, 'scrollTop', {
    get: () => st,
    set: (v) => {
      st = Math.max(0, Math.min(2000, v))
    },
    configurable: true
  })
  rows = [1, 2, 3].map((id) => {
    const tr = document.createElement('div')
    tr.setAttribute('data-drop', 'sort:' + id)
    tr.getBoundingClientRect = () => ({
      top: 100 + id * 30,
      height: 30,
      bottom: 130 + id * 30,
      left: 0,
      width: 500,
      right: 500
    })
    panel.appendChild(tr)
    return tr
  })
  document.body.appendChild(panel)
  onDrop(() => {})
})

afterEach(() => {
  cancelDrag()
  panel.remove()
  window.requestAnimationFrame = rafOriginal
  window.cancelAnimationFrame = cafOriginal
  window.getComputedStyle = csOriginal
})

describe('auto-scroll while sorting', () => {
  it('near the bottom edge the list scrolls down on its own, frame by frame', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: rows[0] })
    pointer(rows[2], 'pointermove', 10, PANEL.top + PANEL.height - 10)
    expect(frames.length, 'deberia haber un cuadro programado').toBe(1)
    frame()
    const afterOne = panel.scrollTop
    expect(afterOne).toBeGreaterThan(0)
    frame()
    expect(panel.scrollTop).toBeGreaterThan(afterOne)
  })

  it('near the top edge it scrolls up', () => {
    panel.scrollTop = 300
    startDrag({ id: 3 }, { clientX: 10, clientY: 400, button: 0, target: rows[2] })
    pointer(rows[0], 'pointermove', 10, PANEL.top + 5)
    frame(3)
    expect(panel.scrollTop).toBeLessThan(300)
  })

  it('in the middle of the panel it does not move, even as frames pass', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: rows[0] })
    pointer(rows[1], 'pointermove', 10, PANEL.top + PANEL.height / 2)
    frame(5)
    expect(panel.scrollTop).toBe(0)
  })

  it('the closer to the edge, the faster', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: rows[0] })
    pointer(rows[2], 'pointermove', 10, PANEL.top + PANEL.height - 40)
    frame()
    const smooth = panel.scrollTop
    panel.scrollTop = 0
    pointer(rows[2], 'pointermove', 10, PANEL.top + PANEL.height - 2)
    frame()
    expect(panel.scrollTop).toBeGreaterThan(smooth)
  })

  it('on drop, or on cancel, no frame is left running', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: rows[0] })
    pointer(rows[2], 'pointermove', 10, PANEL.top + PANEL.height - 10)
    frame()
    expect(frames.length).toBe(1)
    cancelDrag()
    expect(frames.length).toBe(0)
    frame()
    expect(frames.length).toBe(0)
  })

  it('outside a sortable list nothing is scheduled', () => {
    const loose = document.createElement('div')
    loose.setAttribute('data-drop', 'favorites')
    document.body.appendChild(loose)
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: rows[0] })
    pointer(loose, 'pointermove', 10, 900)
    expect(frames.length).toBe(0)
    loose.remove()
  })

  it('the half of the target decides before or after, and is delivered on drop', () => {
    const received = []
    onDrop((target, song, extra) => received.push([target, song.id, extra.after]))
    const { drag } = useDragSong()
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: rows[0] })
    // la tercera fila va de 190 a 220: 195 es su mitad de arriba
    pointer(rows[2], 'pointermove', 10, 195)
    expect(drag.over).toBe('sort:3')
    expect(drag.after).toBe(false)
    pointer(rows[2], 'pointermove', 10, 215)
    expect(drag.after).toBe(true)
    pointer(rows[2], 'pointerup', 10, 215)
    expect(received).toEqual([['sort:3', 1, true]])
    expect(drag.song).toBe(null)
  })

  it('with a single one, `songs` arrives as [song]: the old behaviour still holds', () => {
    const received = []
    onDrop((target, song, extra) => received.push([target, extra.songs.map((s) => s.id)]))
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: rows[0] })
    pointer(rows[2], 'pointermove', 10, 195)
    pointer(rows[2], 'pointerup', 10, 195)
    expect(received).toEqual([['sort:3', [1]]])
  })
})

// Varias elegidas se ordenan como un bloque: el arrastre las lleva todas y
// la app decide dónde dejarlas. Aquí, lo que es del composable: que el
// bloque llega entero, en su orden, con la mitad del destino, y que el
// desplazamiento por el borde funciona igual que con una.
describe('sorting a block', () => {
  const block = () => [{ id: 1 }, { id: 2 }]

  it('is delivered whole, in order and with the target half', () => {
    const received = []
    onDrop((target, song, extra) =>
      received.push([target, song.id, extra.songs.map((s) => s.id), extra.after])
    )
    const { drag, isDragged } = useDragSong()
    const b = block()
    startDrag(b[1], { clientX: 10, clientY: 175, button: 0, target: rows[1] }, b)
    pointer(rows[2], 'pointermove', 10, 215)
    expect(drag.over).toBe('sort:3')
    expect(drag.after).toBe(true)
    // las dos van en la mano; la tercera, no
    expect([1, 2, 3].map(isDragged)).toEqual([true, true, false])
    pointer(rows[2], 'pointerup', 10, 215)
    expect(received).toEqual([['sort:3', 2, [1, 2], true]])
    expect(drag.songs).toEqual([])
  })

  it('near the edge the list also scrolls on its own with the block in hand', () => {
    const b = block()
    startDrag(b[0], { clientX: 10, clientY: 140, button: 0, target: rows[0] }, b)
    pointer(rows[2], 'pointermove', 10, PANEL.top + PANEL.height - 10)
    frame(2)
    expect(panel.scrollTop).toBeGreaterThan(0)
    cancelDrag()
    expect(frames.length).toBe(0)
  })

  it('cancelling it releases the whole block', () => {
    const { drag, isDragged } = useDragSong()
    const b = block()
    startDrag(b[0], { clientX: 10, clientY: 140, button: 0, target: rows[0] }, b)
    pointer(rows[1], 'pointermove', 10, 300)
    expect(drag.songs).toHaveLength(2)
    cancelDrag()
    expect(drag.song).toBe(null)
    expect(drag.songs).toEqual([])
    expect([1, 2].map(isDragged)).toEqual([false, false])
  })
})
