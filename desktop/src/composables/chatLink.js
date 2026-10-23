// @ts-check
// Lo poco del asistente que la app necesita desde el arranque, sin cargarlo:
// qué hace la app por él (`connectChat`) y si espera tu respuesta (la marca
// de «Asistente» en la barra lateral). El asistente de verdad (useChat.js,
// con sus tarjetas y su markdown) llega con su página, que es la única forma
// de empezar una conversación; mientras no se abre, no hay nada que esperar.
import { ref } from 'vue'

/** @typedef {import('../api.js').ChatContext} ChatContext */

/**
 * Lo que la app hace por el chat. Por defecto nada: sin app conectada (las
 * pruebas del componente suelto) el chat funciona igual. `go` abre otra
 * página de la app (la de Ajustes, desde la franja del proveedor).
 * @returns {{ reload: () => any, action: (a: any) => any, context: () => (ChatContext|null), go: (view: {kind: string}) => any }}
 */
export const NO_HOOKS = () => ({
  reload: () => {},
  action: () => {},
  context: () => null,
  go: () => {}
})

export const hooks = NO_HOOKS()

/**
 * La app se conecta al chat: qué hacer con sus órdenes y qué tiene la persona
 * delante. `context` se llama al mandar cada mensaje, no antes: así no depende
 * de lo que cambia cuatro veces por segundo mientras suena algo.
 * @param {Partial<typeof hooks>} h
 * @returns {() => void} cómo desconectarse
 */
export function connectChat(h) {
  Object.assign(hooks, h)
  return () => Object.assign(hooks, NO_HOOKS())
}

/** Hay una tarjeta esperando y la página del asistente no se ve (lo pone useChat.js). */
export const chatAttention = ref(false)
