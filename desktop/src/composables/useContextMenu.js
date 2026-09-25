// @ts-check
// El menú contextual. Uno solo para toda la app: se abre donde se pulsó con
// las opciones que cada sitio decida.
import { ref } from 'vue'

/**
 * @typedef {Object} MenuItem
 * @property {string} [label]
 * @property {string} [icon]
 * @property {string} [note]
 * @property {boolean} [danger]
 * @property {boolean} [disabled]
 * @property {boolean} [separator]
 * @property {boolean} [empty]
 * @property {() => any} [action]
 * @property {MenuItem[]} [children]
 */

const menu = ref({
  open: false,
  x: 0,
  y: 0,
  items: /** @type {MenuItem[]} */ ([]),
  title: '',
  keyboard: false
})

/**
 * @param {MouseEvent|{clientX:number, clientY:number}} ev
 *   el clic (o, desde el teclado, dónde abrirlo)
 * @param {MenuItem[]} items
 * @param {string} [title]
 */
export function openMenu(ev, items, title = '') {
  // Desde el teclado llega un sitio y no un evento de ratón (Mayús+F10 en una
  // fila), o un clic sin ratón (Enter sobre un botón: `detail` 0). Así el
  // menú sabe si al cerrarse tiene que devolver el foco.
  const keyboard = !(ev instanceof MouseEvent) || (ev.detail === 0 && ev.type === 'click')
  menu.value = { open: true, x: ev.clientX, y: ev.clientY, items, title, keyboard }
}
export function closeMenu() {
  if (menu.value.open) menu.value = { ...menu.value, open: false }
}

export function useContextMenu() {
  return { menu, openMenu, closeMenu }
}
