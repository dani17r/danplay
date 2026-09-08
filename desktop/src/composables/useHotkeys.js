// Atajos de teclado con ámbito.
//
// Un atajo global «espacio = pausa» es cómodo hasta que pisa un control: con
// un botón enfocado, espacio alternaba la reproducción Y pulsaba el botón; las
// flechas dentro de un desplegable movían además el volumen; y n/p/s/r/m
// actuaban con un diálogo o un menú abiertos. Aquí se decide una sola vez
// cuándo un atajo tiene sentido.
import { onMounted, onUnmounted } from 'vue'

/** Con el foco en uno de estos, los atajos no actúan: el control manda. */
export const HOTKEY_IGNORE =
  'button, [role], input, textarea, select, [contenteditable], .modal, .ctx, .select-box'

/** Mientras haya uno de estos abierto, los atajos tampoco. */
export const HOTKEY_BLOCKERS = '.modal-back, .ctx, .drawer-backdrop, [role="dialog"]'

/**
 * Nombre de la combinación de una tecla: «space», «arrowright»,
 * «shift+arrowright», «ctrl+q». Ctrl y Cmd cuentan igual.
 * @param {KeyboardEvent} e
 */
export function comboOf(e) {
  const key = e.key === ' ' ? 'space' : String(e.key || '').toLowerCase()
  const parts = []
  if (e.ctrlKey || e.metaKey) parts.push('ctrl')
  if (e.altKey) parts.push('alt')
  if (e.shiftKey) parts.push('shift')
  parts.push(key)
  return parts.join('+')
}

/**
 * ¿Hay que dejar pasar esta tecla sin tocarla?
 * @param {KeyboardEvent} e
 */
export function shouldIgnore(e) {
  if (e.defaultPrevented) return true
  const t = e.target
  if (t && typeof t.closest === 'function' && t.closest(HOTKEY_IGNORE)) return true
  if (typeof document !== 'undefined' && document.querySelector(HOTKEY_BLOCKERS)) return true
  return false
}

/**
 * @param {Record<string, (e: KeyboardEvent) => any>} bindings  combinación → acción
 * @param {{ global?: boolean, target?: Window | Document }} [options]
 *   global: actúa aunque el foco esté en un control o haya un diálogo
 *   (para cosas como Ctrl+Q).
 * @returns {{ handle: (e: KeyboardEvent) => boolean }}
 */
export function useHotkeys(bindings, options = {}) {
  const map = new Map(Object.entries(bindings).map(([k, fn]) => [k.toLowerCase(), fn]))

  function handle(e) {
    if (e.repeat && !options.allowRepeat) {
      // mantener pulsada una flecha sí debe seguir avanzando; una letra, no
      if (!/arrow/.test(String(e.key).toLowerCase())) return false
    }
    const fn = map.get(comboOf(e))
    if (!fn) return false
    if (!options.global && shouldIgnore(e)) return false
    e.preventDefault()
    e.stopPropagation()
    fn(e)
    return true
  }

  onMounted(() => (options.target || window).addEventListener('keydown', handle))
  onUnmounted(() => (options.target || window).removeEventListener('keydown', handle))
  return { handle }
}
