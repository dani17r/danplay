// Diálogos propios: `ask()` devuelve una promesa con lo que respondió el usuario.
//
// Los `prompt()`, `confirm()` y `alert()` del navegador ignoran el tema, se
// ven de otra época y además bloquean el WebView. Este es un singleton: solo
// puede haber un diálogo abierto, y lo pinta App con <ModalDialog>.
import { ref } from 'vue'

/**
 * @typedef {Object} DialogOptions
 * @property {'confirm'|'prompt'|'alert'} [kind]
 * @property {string} [title]
 * @property {string} [message]
 * @property {string} [detail]
 * @property {string} [value]
 * @property {string} [placeholder]
 * @property {string} [okLabel]
 * @property {string} [cancelLabel]
 * @property {boolean} [danger]
 */

const dialog = ref(/** @type {DialogOptions & {open: boolean}} */ ({ open: false }))
let resolver = null

/**
 * Abre un diálogo y espera la respuesta: `true` (o el texto escrito en un
 * prompt) al aceptar, `null` al cancelar.
 * @param {DialogOptions} options
 * @returns {Promise<any>}
 */
export function ask(options) {
  // si había otro abierto, se cierra como cancelado: nunca dos a la vez
  resolver?.(null)
  return new Promise((resolve) => {
    resolver = resolve
    dialog.value = { kind: 'confirm', ...options, open: true }
  })
}

/**
 * Un aviso con un solo botón. Sustituye al `alert()` nativo.
 * @param {string} message
 * @param {Partial<DialogOptions>} [options]
 */
export function tell(message, options = {}) {
  return ask({ kind: 'alert', title: 'DanPlay', message, okLabel: 'Entendido', ...options })
}

function settle(value) {
  dialog.value = { open: false }
  const r = resolver
  resolver = null
  r?.(value)
}
export const dialogOk = (value) => settle(value === undefined ? true : value)
export const dialogCancel = () => settle(null)

export function useDialog() {
  return { dialog, ask, tell, dialogOk, dialogCancel }
}
