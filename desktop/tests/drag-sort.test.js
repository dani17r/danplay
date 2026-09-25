// Ordenar arrastrando dentro de una lista larga.
//
// Con el puntero quieto en el borde del panel no llega ningun `pointermove`,
// asi que el desplazamiento automatico va por cuadros de animacion. Aqui se
// controlan a mano: se comprueba que cerca del borde la lista baja (o sube),
// que en medio no se mueve, y que al soltar se para del todo.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { startDrag, cancelDrag, onDrop, useDragSong } from '../src/composables/useDragSong.js'

const PANEL = { top: 100, height: 400 }

let panel, filas, frames, rafOriginal, cafOriginal, csOriginal

function pointer (el, type, x, y) {
  el.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y }))
}

/** Ejecuta los cuadros de animacion pendientes, uno a uno. */
function cuadro (n = 1) {
  for (let i = 0; i < n; i++) {
    const pendientes = frames.splice(0)
    for (const f of pendientes) f(performance.now())
  }
}

beforeEach(() => {
  frames = []
  rafOriginal = window.requestAnimationFrame
  cafOriginal = window.cancelAnimationFrame
  csOriginal = window.getComputedStyle
  window.requestAnimationFrame = (f) => { frames.push(f); return frames.length }
  window.cancelAnimationFrame = () => { frames.length = 0 }
  // jsdom no maqueta: el panel dice que se desplaza y donde esta
  window.getComputedStyle = (el) => ({ overflowY: el === panel ? 'auto' : 'visible' })

  panel = document.createElement('div')
  panel.setAttribute('data-sort-list', '')
  panel.getBoundingClientRect = () =>
    ({ top: PANEL.top, height: PANEL.height, bottom: PANEL.top + PANEL.height, left: 0, width: 500, right: 500 })
  // scrollTop de verdad: jsdom lo deja en 0 fijo si no hay alto
  let st = 0
  Object.defineProperty(panel, 'scrollTop', {
    get: () => st, set: (v) => { st = Math.max(0, Math.min(2000, v)) }, configurable: true
  })
  filas = [1, 2, 3].map((id) => {
    const tr = document.createElement('div')
    tr.setAttribute('data-drop', 'sort:' + id)
    tr.getBoundingClientRect = () =>
      ({ top: 100 + id * 30, height: 30, bottom: 130 + id * 30, left: 0, width: 500, right: 500 })
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

describe('desplazamiento automatico al ordenar', () => {
  it('cerca del borde de abajo la lista baja sola, cuadro a cuadro', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: filas[0] })
    pointer(filas[2], 'pointermove', 10, PANEL.top + PANEL.height - 10)
    expect(frames.length, 'deberia haber un cuadro programado').toBe(1)
    cuadro()
    const tras1 = panel.scrollTop
    expect(tras1).toBeGreaterThan(0)
    cuadro()
    expect(panel.scrollTop).toBeGreaterThan(tras1)
  })

  it('cerca del borde de arriba sube', () => {
    panel.scrollTop = 300
    startDrag({ id: 3 }, { clientX: 10, clientY: 400, button: 0, target: filas[2] })
    pointer(filas[0], 'pointermove', 10, PANEL.top + 5)
    cuadro(3)
    expect(panel.scrollTop).toBeLessThan(300)
  })

  it('en medio del panel no se mueve, aunque pasen cuadros', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: filas[0] })
    pointer(filas[1], 'pointermove', 10, PANEL.top + PANEL.height / 2)
    cuadro(5)
    expect(panel.scrollTop).toBe(0)
  })

  it('cuanto mas al borde, mas rapido', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: filas[0] })
    pointer(filas[2], 'pointermove', 10, PANEL.top + PANEL.height - 40)
    cuadro()
    const suave = panel.scrollTop
    panel.scrollTop = 0
    pointer(filas[2], 'pointermove', 10, PANEL.top + PANEL.height - 2)
    cuadro()
    expect(panel.scrollTop).toBeGreaterThan(suave)
  })

  it('al soltar, o al cancelar, no queda ningun cuadro dando vueltas', () => {
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: filas[0] })
    pointer(filas[2], 'pointermove', 10, PANEL.top + PANEL.height - 10)
    cuadro()
    expect(frames.length).toBe(1)
    cancelDrag()
    expect(frames.length).toBe(0)
    cuadro()
    expect(frames.length).toBe(0)
  })

  it('fuera de una lista ordenable no se programa nada', () => {
    const suelto = document.createElement('div')
    suelto.setAttribute('data-drop', 'favorites')
    document.body.appendChild(suelto)
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: filas[0] })
    pointer(suelto, 'pointermove', 10, 900)
    expect(frames.length).toBe(0)
    suelto.remove()
  })

  it('la mitad del destino decide antes o despues, y se entrega al soltar', () => {
    const recibido = []
    onDrop((target, song, extra) => recibido.push([target, song.id, extra.after]))
    const { drag } = useDragSong()
    startDrag({ id: 1 }, { clientX: 10, clientY: 140, button: 0, target: filas[0] })
    // la tercera fila va de 190 a 220: 195 es su mitad de arriba
    pointer(filas[2], 'pointermove', 10, 195)
    expect(drag.over).toBe('sort:3')
    expect(drag.after).toBe(false)
    pointer(filas[2], 'pointermove', 10, 215)
    expect(drag.after).toBe(true)
    pointer(filas[2], 'pointerup', 10, 215)
    expect(recibido).toEqual([['sort:3', 1, true]])
    expect(drag.song).toBe(null)
  })
})
