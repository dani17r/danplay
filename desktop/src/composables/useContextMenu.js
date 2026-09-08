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

const menu = ref({ open: false, x: 0, y: 0, items: /** @type {MenuItem[]} */ ([]), title: '' })

/**
 * @param {MouseEvent|{clientX:number, clientY:number}} ev
 * @param {MenuItem[]} items
 * @param {string} [title]
 */
export function openMenu(ev, items, title = '') {
  menu.value = { open: true, x: ev.clientX, y: ev.clientY, items, title }
}
export function closeMenu() {
  if (menu.value.open) menu.value = { ...menu.value, open: false }
}

export function useContextMenu() {
  return { menu, openMenu, closeMenu }
}
