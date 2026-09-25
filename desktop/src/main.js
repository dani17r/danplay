import { createApp } from 'vue'
import './styles/index.css'

// La misma pagina sirve para tres ventanas: la app, la ventanita de la
// bandeja y la proyeccion. Se distinguen por la direccion, que es lo unico
// que se le puede pasar a una ventana nueva de Tauri.
const params = new URLSearchParams(location.search)
const isMini = params.has('mini')
const isProjection = params.has('projection')
document.body.classList.toggle('is-mini', isMini)
document.body.classList.toggle('is-projection', isProjection)

// La ventana principal lleva además el enrutador de sus páginas (router.js).
async function start() {
  if (isMini) return createApp((await import('./MiniPlayer.vue')).default).mount('#app')
  if (isProjection) return createApp((await import('./Projection.vue')).default).mount('#app')
  const [{ default: App }, { createAppRouter }] = await Promise.all([
    import('./App.vue'),
    import('./router.js')
  ])
  createApp(App).use(createAppRouter()).mount('#app')
}
start()
