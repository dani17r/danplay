// Arrastrar la aguja de una barra de tiempo.
//
// La barra solo entendía el clic: para ir a un punto había que acertar con el
// minuto exacto. Aquí la aguja se coge y se lleva. Mientras se arrastra, la
// barra y la hora que se ve siguen al puntero; al soltar se pide el salto UNA
// vez. No se salta en cada movimiento: cada salto es una búsqueda en el
// decodificador, y a ráfagas se oye a trompicones.
//
// Con eventos de puntero y no con el arrastre nativo, por lo mismo que
// useDragSong: dentro del WebView de escritorio se comporta distinto según el
// sistema. Los movimientos se escuchan en la ventana, así que se puede
// arrastrar por fuera de la barra sin que se suelte.
import { reactive, onUnmounted } from 'vue'

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

/**
 * @param {{ duration: import('vue').Ref<number>, seek: (seconds: number) => any }} options
 *   duration: lo que dura lo que suena, en segundos
 *   seek: a quién pedir el salto al soltar
 */
export function useScrub({ duration, seek }) {
  const scrub = reactive({
    active: false, // ¿se está arrastrando la aguja ahora mismo?
    value: 0 // el segundo bajo el puntero, mientras se arrastra
  })
  let bar = null

  /** El segundo que cae bajo el puntero, dentro de la barra. */
  function at(e) {
    const r = bar.getBoundingClientRect()
    const fraction = r.width > 0 ? clamp((e.clientX - r.left) / r.width, 0, 1) : 0
    return fraction * (duration.value || 0)
  }
  function listen(on) {
    const f = on ? window.addEventListener : window.removeEventListener
    f.call(window, 'pointermove', move)
    f.call(window, 'pointerup', release)
    f.call(window, 'pointercancel', cancel)
  }
  /** Va en el `pointerdown` de la barra. Un clic sin mover también sirve: suelta ahí. */
  function start(e) {
    if (e.button) return // solo el botón principal
    if (!duration.value) return // sin canción no hay a dónde ir
    bar = e.currentTarget
    scrub.active = true
    scrub.value = at(e)
    e.preventDefault() // que no empiece a seleccionar texto por el camino
    listen(true)
  }
  function move(e) {
    if (scrub.active) scrub.value = at(e)
  }
  function release(e) {
    if (!scrub.active) return
    const seconds = at(e)
    cancel()
    seek(seconds)
  }
  /** Deja la aguja donde estaba sin pedir nada. */
  function cancel() {
    scrub.active = false
    bar = null
    listen(false)
  }
  onUnmounted(cancel)

  return { scrub, start, cancel }
}
