// @ts-check
// La forma de la ventana: normal, pantalla completa, media pantalla, columna,
// cuadrito, barra... (`shape.rs`). La ventana es siempre la misma y cambia de
// diseño según su tamaño (`useViewport`): la columna es el de móvil, y el
// cuadrito y la barra son solo el reproductor. Aquí solo se pide la forma y
// se recuerda cuál es, para marcarla en el menú.
import { ref, readonly } from 'vue'
import { app, inTauri } from '../api.js'
import { notify } from './useNotices.js'
import { errorMessage } from '../api.js'

/** Las formas, en el orden del menú. `only`: las que no hay fuera de la app. */
export const SHAPES = [
  { v: 'normal', n: 'Normal', icon: 'shapeNormal', note: 'como estaba' },
  { v: 'completa', n: 'Pantalla completa', icon: 'fullscreen', note: 'F11' },
  { v: 'maximizada', n: 'Maximizada', icon: 'maximize', only: true },
  { v: 'izquierda', n: 'Media pantalla, izquierda', icon: 'half', only: true },
  { v: 'derecha', n: 'Media pantalla, derecha', icon: 'half', only: true },
  { v: 'columna', n: 'Columna', icon: 'column', note: 'la lista, estrecha', only: true },
  { v: 'cuadrito', n: 'Cuadrito', icon: 'square', note: 'siempre encima', only: true },
  { v: 'barra', n: 'Barra', icon: 'bar', note: 'siempre encima', only: true }
]

const shape = ref('normal')
const onTop = ref(false)
let asked = false

/** Pregunta la forma una vez (al abrir, la ventana puede venir de antes). */
async function load() {
  if (asked) return
  asked = true
  try {
    const [s, top] = await app.windowShape()
    shape.value = s
    onTop.value = top
  } catch {
    // sin Rust (pruebas, navegador viejo): se queda normal
  }
}

/** @param {string} v */
async function setShape(v) {
  try {
    shape.value = await app.setWindowShape(v)
  } catch (e) {
    notify(errorMessage(e), 'info')
  }
}

async function toggleOnTop() {
  try {
    onTop.value = await app.setWindowOnTop(!onTop.value)
  } catch (e) {
    notify(errorMessage(e), 'info')
  }
}

/** Pantalla completa o como estaba (F11). */
function toggleFullscreen() {
  return setShape(shape.value === 'completa' ? 'normal' : 'completa')
}

/**
 * Las opciones del menú de formas, para `ContextMenu`: la de ahora marcada,
 * y «siempre encima» al final.
 */
function menuItems() {
  /** @type {import('./useContextMenu.js').MenuItem[]} */
  const items = SHAPES.filter((s) => inTauri || !s.only).map((s) => ({
    label: s.n,
    icon: shape.value === s.v ? 'check' : s.icon,
    note: s.note,
    action: () => setShape(s.v)
  }))
  if (inTauri) {
    items.push({ separator: true })
    items.push({
      label: onTop.value ? 'Quitar «siempre encima»' : 'Siempre encima',
      icon: 'pin',
      note: 'de las demás ventanas',
      action: toggleOnTop
    })
  }
  return items
}

export function useWindowShape() {
  load()
  return {
    shape: readonly(shape),
    onTop: readonly(onTop),
    setShape,
    toggleOnTop,
    toggleFullscreen,
    menuItems
  }
}
