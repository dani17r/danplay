import { createApp } from 'vue'
import './styles/index.css'

// La misma pagina sirve para dos ventanas: la app y la ventanita de la
// bandeja. Se distinguen por la direccion, que es lo unico que se le puede
// pasar a una ventana nueva de Tauri.
const params = new URLSearchParams(location.search)
const isMini = params.has('mini')
const isProjection = params.has('projection')
document.body.classList.toggle('is-mini', isMini)
document.body.classList.toggle('is-projection', isProjection)

const load = isMini ? import('./MiniPlayer.vue')
  : isProjection ? import('./Projection.vue')
  : import('./App.vue')
load.then(m => createApp(m.default).mount('#app'))
