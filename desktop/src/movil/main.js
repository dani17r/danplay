// La app del celular: la interfaz de la PC con su propio armazón
// (MovilApp.vue), que monta los mismos componentes (la lista, el reproductor,
// el estudio, la ficha) y deja fuera lo que es de la biblioteca de la PC.
import { createApp } from 'vue'
import '../styles/index.css'
// lo propio del teléfono, después de lo de la PC para poder retocarlo
import '../styles/movil.css'
import MovilApp from './MovilApp.vue'

createApp(MovilApp).mount('#app')
