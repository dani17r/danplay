// Arrastrar una cancion hasta donde se pueda soltar.
//
// No se usa el arrastre nativo del navegador (`draggable` + `dragstart`):
// dentro del WebView de escritorio se comporta distinto segun el sistema y
// arrastra una imagen fantasma del elemento que aqui no pinta nada. Con
// eventos de puntero mandamos nosotros y se ve igual en todas partes.
//
// Los destinos no se registran en ninguna lista: basta con que un elemento
// lleve el atributo `data-drop`. Asi la tabla de canciones no tiene que saber
// quien recibe, ni el que recibe saber quien arrastra.
import { reactive, computed } from 'vue'

// Cuanto hay que mover antes de que esto sea un arrastre y no un clic. Sin
// margen, elegir una cancion con un pulso tembloroso empezaria a arrastrarla.
const THRESHOLD = 6

const drag = reactive({
  song: null,      // la cancion que va en la mano; null si no hay arrastre
  x: 0, y: 0,      // donde esta el puntero, para pintar el fantasma
  over: null       // el `data-drop` del destino que hay debajo, si hay alguno
})

let handler = null     // que hacer al soltar; lo pone la app
let pending = null     // arrastre armado, todavia sin pasar del umbral

/** Que destino hay bajo el puntero. El fantasma no estorba: no recibe eventos. */
function targetOf (e) {
  return e.target?.closest?.('[data-drop]')?.getAttribute('data-drop') || null
}

/**
 * Mientras se arrastra no se empieza a seleccionar texto.
 *
 * El css ya lleva `user-select:none`, pero eso no basta: el WebView de
 * escritorio (WebKit) igualmente empieza a seleccionar las filas por las que
 * pasa el puntero, y ves media lista en azul mientras llevas una cancion. Se
 * corta el evento en origen, que es lo unico que se comporta igual en todos
 * los motores.
 */
function noSelect (e) { e.preventDefault() }

function listen (on) {
  const f = on ? window.addEventListener : window.removeEventListener
  f.call(window, 'pointermove', move)
  f.call(window, 'pointerup', drop)
  f.call(window, 'pointercancel', cancelDrag)
  f.call(document, 'selectstart', noSelect)
  f.call(document, 'dragstart', noSelect)   // y el arrastre nativo del texto
}

function move (e) {
  if (pending) {
    const d = Math.abs(e.clientX - pending.x) + Math.abs(e.clientY - pending.y)
    if (d < THRESHOLD) return
    drag.song = pending.song
    pending = null
    document.body.classList.add('dragging-song')
    // Por si el motor alcanzo a marcar algo en los primeros pixeles, antes
    // de que esto contara como arrastre.
    try { window.getSelection()?.removeAllRanges() } catch { /* da igual */ }
  }
  if (!drag.song) return
  drag.x = e.clientX
  drag.y = e.clientY
  drag.over = targetOf(e)
}

function drop () {
  const song = drag.song
  const target = drag.over
  cancelDrag()
  if (song && target && handler) handler(target, song)
}

/** Suelta lo que se lleve sin hacer nada con ello. */
export function cancelDrag () {
  pending = null
  drag.song = null
  drag.over = null
  if (typeof document !== 'undefined') document.body.classList.remove('dragging-song')
  listen(false)
}

/**
 * Empieza a arrastrar `song`. Va en el `pointerdown` de la fila o la ficha;
 * hasta que el puntero no se mueve de verdad no pasa nada, asi que un clic
 * normal sigue siendo un clic.
 */
export function startDrag (song, e) {
  // Con el dedo no: arrastrar una fila chocaria con desplazar la lista. En
  // tactil se sigue usando el menu de opciones de la cancion.
  if (e.pointerType === 'touch') return
  if (e.button) return                            // solo el boton principal
  // Ni desde un control: el corazon, las estrellas o el boton de reproducir
  // tienen lo suyo que hacer.
  if (e.target?.closest?.('button, .heart, .stars, input, a')) return
  cancelDrag()
  pending = { song, x: e.clientX, y: e.clientY }
  listen(true)
}

/** Que hacer cuando se suelta algo. Solo hay uno: lo pone la app. */
export function onDrop (fn) { handler = fn }

export function useDragSong () {
  const dragging = computed(() => !!drag.song)
  return {
    drag,
    dragging,
    /** ¿Esta el puntero sobre este destino ahora mismo? */
    isOver: (key) => drag.over === key,
    /** ¿Es esta la cancion que se esta arrastrando? */
    isDragged: (id) => drag.song?.id === id,
    startDrag,
    cancelDrag,
    onDrop
  }
}
