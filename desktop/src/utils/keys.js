// Teclado en las listas de canciones.
//
// Filas, fichas y azulejos son enfocables (tabindex="0") para que se pueda
// recorrer la lista sin ratón: Enter o espacio reproducen, y las flechas
// pasan a la vecina. Las cuatro vistas comparten esto para comportarse igual.

/**
 * @param {KeyboardEvent} e
 * @param {{ play: () => any, select?: () => any }} actions
 * @returns {boolean} si la tecla se ha consumido
 */
export function onSongKey(e, actions) {
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    e.stopPropagation()
    actions.play()
    return true
  }
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    const el = e.currentTarget
    if (!(el instanceof HTMLElement)) return false
    const dir = e.key === 'ArrowDown' ? 'nextElementSibling' : 'previousElementSibling'
    let sibling = el[dir]
    // los separadores del virtualizado no se enfocan: se saltan
    while (sibling && !(sibling instanceof HTMLElement && sibling.hasAttribute('tabindex'))) {
      sibling = sibling[dir]
    }
    if (sibling) {
      e.preventDefault()
      e.stopPropagation()
      sibling.focus()
      actions.select?.()
      return true
    }
  }
  return false
}
