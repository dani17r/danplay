// La app del celular: la interfaz de la PC con su propio armazón
// (MobileApp.vue), que monta los mismos componentes (la lista, el reproductor,
// el estudio, la ficha) y deja fuera lo que es de la biblioteca de la PC.
// lo primero: lo guardado con nombres de antes pasa a los de ahora
import '../utils/legacyStorage.js'
import { createApp } from 'vue'
import '../styles/index.css'
// lo propio del teléfono, después de lo de la PC para poder retocarlo
import '../styles/mobile.css'
import MobileApp from './MobileApp.vue'

createApp(MobileApp).mount('#app')
