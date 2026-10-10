// @ts-check
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
//
// Las filas de un repertorio son destino ellas mismas (`data-drop="sort:id"`)
// para poder cambiar el orden arrastrando: ahi importa ademas por que MITAD
// del destino va el puntero, para dejar la cancion antes o despues. Una
// lista que se ordena lleva `data-sort-list` en su caja, y mientras se
// arrastra por sus bordes se desplaza sola: si no, en un repertorio largo no
// habria forma de llevar una cancion del final al principio.
//
// Varias a la vez: si la canción que se agarra es una de las elegidas (y son
// dos o más), van todas, en el orden en que se ven. Quien llama a
// `startDrag` las pasa; este composable no sabe nada de la selección. Si se
// agarra una que no está entre ellas, va sola y la selección no se toca.
import { reactive, computed, markRaw, toRaw } from 'vue'
import { nearestScroller } from './useVirtualRows.js'

// Cuanto hay que mover antes de que esto sea un arrastre y no un clic. Sin
// margen, elegir una cancion con un pulso tembloroso empezaria a arrastrarla.
const THRESHOLD = 6

// A cuantos pixeles del borde de la lista empieza a desplazarse sola, y
// cuanto se mueve por cuadro como mucho (mas rapido cuanto mas al borde).
const EDGE = 44
const SPEED = 14

/** @typedef {import('../api.js').Song} Song */

// Nada en la mano. Sin proxy: es una lista que se sustituye entera y nunca se
// toca por dentro (con Ctrl+A puede llevar miles de canciones).
const NOTHING = markRaw(/** @type {Song[]} */ ([]))

const drag = reactive({
  song: /** @type {Song|null} */ (null), // la que se agarró; null si no hay arrastre
  songs: NOTHING, // todo lo que va en la mano, en el orden en que se ve: [song] o las elegidas
  x: 0,
  y: 0, // donde esta el puntero, para pintar el fantasma
  over: null, // el `data-drop` del destino que hay debajo, si hay alguno
  after: false // ¿va el puntero por la segunda mitad del destino?
})

let handler = null // que hacer al soltar; lo pone la app
let pending = null // arrastre armado, todavia sin pasar del umbral
let scroller = null // el panel de la lista ordenable por la que se arrastra
let frame = 0 // el cuadro del desplazamiento automatico
let origin = null // la fila de la que salió el arrastre
let unswallow = null // deja de tragarse el clic de después de soltar

// Los ids de lo que va en la mano, para marcar sus filas sin recorrer la
// lista por cada una.
const carriedIds = computed(() => new Set(drag.songs.map((s) => s.id)))

/**
 * Apunta que destino hay bajo el puntero, a partir del elemento que lo
 * recibe. El fantasma no estorba: no recibe eventos.
 *
 * En una fila se mira ademas por que mitad va el puntero: la de arriba deja
 * la cancion antes, la de abajo despues. Las fichas de una cuadricula se
 * ordenan de izquierda a derecha, asi que ahi lo dicen con
 * `data-drop-axis="x"` y se mira la mitad izquierda o derecha.
 */
function setOver(el, x, y) {
  const target = el?.closest?.('[data-drop]')
  drag.over = target?.getAttribute('data-drop') || null
  if (!target) {
    drag.after = false
    return
  }
  const r = target.getBoundingClientRect()
  drag.after =
    target.getAttribute('data-drop-axis') === 'x'
      ? x > r.left + r.width / 2
      : y > r.top + r.height / 2
}

/**
 * Mientras el puntero esta cerca del borde de una lista ordenable, la lista
 * se desplaza sola. Va por cuadros y no por eventos porque, con el puntero
 * quieto en el borde, no llega ningun `pointermove` y aun asi hay que seguir
 * bajando.
 */
function autoScroll() {
  frame = 0
  if (!drag.song || !scroller) return
  const r = scroller.getBoundingClientRect()
  let dy = 0
  if (drag.y < r.top + EDGE) dy = -Math.ceil(((r.top + EDGE - drag.y) / EDGE) * SPEED)
  else if (drag.y > r.bottom - EDGE) dy = Math.ceil(((drag.y - (r.bottom - EDGE)) / EDGE) * SPEED)
  if (dy) {
    const before = scroller.scrollTop
    scroller.scrollTop = before + dy
    // Lo que hay bajo el puntero ha cambiado sin que el puntero se mueva.
    if (scroller.scrollTop !== before && typeof document.elementFromPoint === 'function') {
      setOver(document.elementFromPoint(drag.x, drag.y), drag.x, drag.y)
    }
  }
  if (typeof requestAnimationFrame === 'function') frame = requestAnimationFrame(autoScroll)
}

/** Que lista ordenable hay bajo el puntero, si hay alguna, y arranca o para el desplazamiento. */
function trackScroller(el) {
  const list = el?.closest?.('[data-sort-list]') || null
  const next = list ? nearestScroller(list) : null
  if (next === scroller) return
  scroller = next
  if (frame) {
    cancelAnimationFrame(frame)
    frame = 0
  }
  if (scroller && typeof requestAnimationFrame === 'function')
    frame = requestAnimationFrame(autoScroll)
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
function noSelect(e) {
  e.preventDefault()
}

function listen(on) {
  const f = on ? window.addEventListener : window.removeEventListener
  f.call(window, 'pointermove', move)
  f.call(window, 'pointerup', drop)
  f.call(window, 'pointercancel', cancelDrag)
  f.call(document, 'selectstart', noSelect)
  f.call(document, 'dragstart', noSelect) // y el arrastre nativo del texto
}

function move(e) {
  if (pending) {
    const d = Math.abs(e.clientX - pending.x) + Math.abs(e.clientY - pending.y)
    if (d < THRESHOLD) return
    drag.song = pending.song
    drag.songs = pending.songs
    origin = pending.from
    pending = null
    document.body.classList.add('dragging-song')
    // Por si el motor alcanzo a marcar algo en los primeros pixeles, antes
    // de que esto contara como arrastre.
    try {
      window.getSelection()?.removeAllRanges()
    } catch {
      /* da igual */
    }
  }
  if (!drag.song) return
  drag.x = e.clientX
  drag.y = e.clientY
  setOver(e.target, e.clientX, e.clientY)
  trackScroller(e.target)
}

function drop() {
  // la misma canción que va en `songs`, no su proxy: así se puede buscar en ella
  const song = drag.song && toRaw(drag.song)
  const songs = drag.songs
  const target = drag.over
  const after = drag.after
  const from = origin
  cancelDrag()
  if (!song) return // solo era un clic: que siga siendo un clic
  swallowClick(from)
  if (target && handler) handler(target, song, { after, songs: songs.length ? songs : [song] })
}

/**
 * Al soltar sobre la misma fila de la que salió el arrastre, el navegador
 * lanza además un clic en ella. No lo es: elegiría esa fila y soltaría las
 * demás elegidas (o, en el modo «Seleccionar», la quitaría de ellas). Ese
 * clic llega en el mismo instante que el `pointerup`, así que se escucha
 * solo hasta la siguiente vuelta del bucle de eventos, y solo sobre esa fila.
 * @param {Element|null} from
 */
function swallowClick(from) {
  if (!from || typeof window === 'undefined') return
  /** @param {Event} ev */
  const stop = (ev) => {
    if (!(ev.target instanceof Node) || !from.contains(ev.target)) return
    ev.preventDefault()
    ev.stopPropagation()
  }
  window.addEventListener('click', stop, true)
  const timer = setTimeout(() => unswallow?.(), 0)
  unswallow = () => {
    clearTimeout(timer)
    window.removeEventListener('click', stop, true)
    unswallow = null
  }
}

/** Suelta lo que se lleve sin hacer nada con ello. */
export function cancelDrag() {
  pending = null
  drag.song = null
  drag.songs = NOTHING
  drag.over = null
  drag.after = false
  origin = null
  unswallow?.()
  scroller = null
  if (frame) {
    cancelAnimationFrame(frame)
    frame = 0
  }
  if (typeof document !== 'undefined') document.body.classList.remove('dragging-song')
  listen(false)
}

/**
 * Lo que va en la mano: el grupo entero si la agarrada es una de él y son dos
 * o más; si no, ella sola.
 * @param {Song} song
 * @param {Song[]|null|undefined} songs
 * @returns {Song[]}
 */
function carried(song, songs) {
  if (!Array.isArray(songs) || songs.length < 2) return markRaw([song])
  if (!songs.some((s) => s?.id === song.id)) return markRaw([song])
  return markRaw(songs.slice())
}

/**
 * Empieza a arrastrar `song`. Va en el `pointerdown` de la fila o la ficha;
 * hasta que el puntero no se mueve de verdad no pasa nada, asi que un clic
 * normal sigue siendo un clic.
 *
 * `songs`: las elegidas, en el orden en que se ven, si `song` es una de ellas.
 * Con dos o más (y `song` dentro) el arrastre las lleva todas; si no, solo a
 * `song`. Lo pasa quien llama (useSongList): la selección no es cosa de aquí.
 * @param {Song} song
 * @param {any} e  el `pointerdown` (o algo con su forma)
 * @param {Song[]|null} [songs]
 */
export function startDrag(song, e, songs = null) {
  // Con el dedo no: arrastrar una fila chocaria con desplazar la lista. En
  // tactil se sigue usando el menu de opciones de la cancion.
  if (e.pointerType === 'touch') return
  if (e.button) return // solo el boton principal
  // Ni desde un control: el corazon, las estrellas o el boton de reproducir
  // tienen lo suyo que hacer.
  if (e.target?.closest?.('button, .heart, .stars, input, a')) return
  cancelDrag()
  pending = {
    song,
    songs: carried(song, songs),
    from: e.currentTarget instanceof Element ? e.currentTarget : e.target || null,
    x: e.clientX,
    y: e.clientY
  }
  listen(true)
}

/**
 * Qué hacer cuando se suelta algo. Solo hay uno: lo pone la app.
 * Recibe el destino, la canción agarrada y `{ after, songs }`: `after`, si se
 * soltó en la segunda mitad del destino (debajo, o a la derecha en una
 * cuadrícula); `songs`, todo lo que iba en la mano en el orden en que se ve
 * (siempre al menos `[song]`).
 * @param {(target: string, song: Song, extra: { after: boolean, songs: Song[] }) => any} fn
 */
export function onDrop(fn) {
  handler = fn
}

export function useDragSong() {
  const dragging = computed(() => !!drag.song)
  return {
    drag,
    dragging,
    /** ¿Esta el puntero sobre este destino ahora mismo? */
    isOver: (key) => drag.over === key,
    /** Sobre este destino y en su primera mitad: lo que se suelte va ANTES. */
    isBefore: (key) => drag.over === key && !drag.after,
    /** Sobre este destino y en su segunda mitad: lo que se suelte va DESPUES. */
    isAfter: (key) => drag.over === key && drag.after,
    /** ¿Va esta canción en la mano (sola o con las demás elegidas)? */
    isDragged: (/** @type {number} */ id) => carriedIds.value.has(id),
    startDrag,
    cancelDrag,
    onDrop
  }
}
