// @ts-check
// Teclado en las listas de canciones.
//
// La lista entera es UNA parada del tabulador: la fila con el foco (o la
// elegida) es la única con tabindex="0" y las flechas pasan a la vecina. Las
// cuatro vistas comparten esto para comportarse igual; aquí solo se decide
// qué quiere decir cada tecla, sin tocar el DOM, para poder probarlo suelto.

/**
 * @typedef {{ kind: 'play' } | { kind: 'toggle' } | { kind: 'select' } | { kind: 'menu' }
 *   | { kind: 'move', by: number } | { kind: 'move', to: 'first' | 'last' }} SongKey
 *   play: poner esta; toggle: pausar o seguir lo que suena; select: añadirla o
 *   quitarla de la selección; menu: su menú; move: ir a otra.
 */

/**
 * Qué quiere decir una tecla sobre una canción de la lista.
 * @param {KeyboardEvent} e
 * @param {{ grid?: boolean, columns?: number, page?: number }} [layout]
 *   grid: van en cuadrícula (izquierda y derecha también mueven);
 *   columns: cuántas caben por línea; page: cuántas líneas salta RePág/AvPág.
 * @returns {SongKey | null}
 */
export function songKey(e, layout = {}) {
  const { grid = false, columns = 1, page = 10 } = layout
  const line = grid ? Math.max(1, columns) : 1
  if (e.altKey) return null
  switch (e.key) {
    case 'Enter':
      return { kind: 'play' }
    case ' ':
      // espacio es pausa en toda la app; con Ctrl, elegir esta sin soltar las demás
      return e.ctrlKey || e.metaKey ? { kind: 'select' } : { kind: 'toggle' }
    case 'ContextMenu':
      return { kind: 'menu' }
    case 'F10':
      return e.shiftKey ? { kind: 'menu' } : null
    case 'ArrowDown':
      return { kind: 'move', by: line }
    case 'ArrowUp':
      return { kind: 'move', by: -line }
    case 'ArrowRight':
      return grid ? { kind: 'move', by: 1 } : null
    case 'ArrowLeft':
      return grid ? { kind: 'move', by: -1 } : null
    case 'PageDown':
      return { kind: 'move', by: page * line }
    case 'PageUp':
      return { kind: 'move', by: -page * line }
    case 'Home':
      return { kind: 'move', to: 'first' }
    case 'End':
      return { kind: 'move', to: 'last' }
    default:
      return null
  }
}

/**
 * A qué posición lleva un movimiento, sin salirse de la lista.
 * @param {{ by?: number, to?: 'first'|'last' }} move
 * @param {number} from   posición de partida
 * @param {number} length cuántas hay
 */
export function targetIndex(move, from, length) {
  if (!length) return -1
  if (move.to === 'first') return 0
  if (move.to === 'last') return length - 1
  return Math.max(0, Math.min(length - 1, from + (move.by || 0)))
}
