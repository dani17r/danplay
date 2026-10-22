// @ts-check
// Cierra un panel al pulsar fuera de el o al pulsar Escape.
// Lo usan todos los menus flotantes para que se comporten igual.
import { onMounted, onUnmounted, unref } from 'vue'

export function onClickOutside(elements, close) {
  const all = () =>
    (Array.isArray(elements) ? elements : [elements])
      .map((e) => unref(e))
      .filter(Boolean)
      .map((e) => e.$el ?? e)

  function onOutside(ev) {
    if (!all().some((el) => el.contains?.(ev.target))) close()
  }
  function escape(ev) {
    if (ev.key === 'Escape') close()
  }

  onMounted(() => {
    document.addEventListener('mousedown', onOutside)
    document.addEventListener('keydown', escape)
  })
  onUnmounted(() => {
    document.removeEventListener('mousedown', onOutside)
    document.removeEventListener('keydown', escape)
  })
}
