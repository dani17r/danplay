import { createApp } from 'vue'
import './styles/index.css'

// La misma pagina sirve para dos ventanas: la app y la ventanita de la
// bandeja. Se distinguen por la direccion, que es lo unico que se le puede
// pasar a una ventana nueva de Tauri.
const isMini = new URLSearchParams(location.search).has('mini')
document.body.classList.toggle('is-mini', isMini)

const load = isMini ? import('./MiniPlayer.vue') : import('./App.vue')
load.then(m => createApp(m.default).mount('#app'))
