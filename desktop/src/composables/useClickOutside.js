// Cierra un panel al pulsar fuera de el o al pulsar Escape.
// Lo usan todos los menus flotantes para que se comporten igual.
import { onMounted, onUnmounted, unref } from 'vue'

export function onClickOutside (elementos, close) {
  const lista = () => (Array.isArray(elementos) ? elementos : [elementos])
    .map(e => unref(e))
    .filter(Boolean)
    .map(e => e.$el ?? e)

  function fuera (ev) {
    if (!lista().some(el => el.contains?.(ev.target))) close()
  }
  function escape (ev) { if (ev.key === 'Escape') close() }

  onMounted(() => {
    document.addEventListener('mousedown', fuera)
    document.addEventListener('keydown', escape)
  })
  onUnmounted(() => {
    document.removeEventListener('mousedown', fuera)
    document.removeEventListener('keydown', escape)
  })
}
