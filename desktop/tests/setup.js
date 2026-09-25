// Lo que jsdom no trae y la interfaz usa. Sin esto, cada prueba que monta la
// barra de estudio (la onda se pinta en un <canvas>) o el reproductor del
// navegador (un <audio>) llenaba la salida de «Not implemented: …», y lo que
// de verdad fallaba quedaba enterrado entre cientos de líneas.

// Un contexto 2D que no pinta nada: la onda se dibuja igual (en el vacío) y
// el código que la calcula se ejecuta de verdad.
const noop = () => {}
const context2d = {
  setTransform: noop,
  clearRect: noop,
  fillRect: noop,
  beginPath: noop,
  moveTo: noop,
  lineTo: noop,
  stroke: noop,
  fill: noop,
  save: noop,
  restore: noop,
  globalAlpha: 1,
  fillStyle: '',
  strokeStyle: ''
}
Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', {
  configurable: true,
  value(kind) {
    return kind === '2d' ? { ...context2d, canvas: this } : null
  }
})

// <audio>: sin decodificador, reproducir y parar no hacen nada (el
// reproductor web cuenta su estado por los eventos, que las pruebas lanzan).
Object.defineProperty(HTMLMediaElement.prototype, 'play', {
  configurable: true,
  value: () => Promise.resolve()
})
Object.defineProperty(HTMLMediaElement.prototype, 'pause', { configurable: true, value: noop })
Object.defineProperty(HTMLMediaElement.prototype, 'load', { configurable: true, value: noop })

// Cada prueba desmonta lo que montó. Muchas montaban la app entera y la
// dejaban puesta: la siguiente seguía teniendo detrás a todas las anteriores
// escuchando el estado compartido (lo que suena, las preferencias), y cada
// tick del reproductor repintaba decenas de apps. Las pruebas del final del
// archivo tardaban segundos cada una y se llegaban a pisar entre ellas.
import { afterEach } from 'vitest'
import { enableAutoUnmount, VueWrapper } from '@vue/test-utils'

// desmontar dos veces (la prueba ya lo hizo) no es un fallo: se ignora
const unmount = VueWrapper.prototype.unmount
VueWrapper.prototype.unmount = function () {
  if (this.__danplayUnmounted) return
  this.__danplayUnmounted = true
  return unmount.call(this)
}
enableAutoUnmount(afterEach)
afterEach(() => {
  // lo que se teletransportó al <body> (diálogos, el fantasma del arrastre)
  // y los elementos sueltos que alguna prueba dejó a mano
  document.body.innerHTML = ''
  document.body.className = ''
})

// La ventana principal lleva el enrutador de sus páginas (src/router.js, que
// instala main.js). Cada app que se monta en una prueba recibe uno nuevo:
// compartido, la página de una prueba se colaba en la siguiente.
import { config } from '@vue/test-utils'
import { createAppRouter } from '../src/router.js'
config.global.plugins = [{ install: (app) => app.use(createAppRouter()) }]
