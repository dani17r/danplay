import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import vueDevTools from 'vite-plugin-vue-devtools'

// Las herramientas de Vue (el botón flotante con el árbol de componentes)
// solo en `npm run dev` abierto en un navegador. Ni en `tauri dev` (Tauri
// arranca Vite con TAURI_ENV_PLATFORM puesto, y dentro del WebView el botón
// estorba), ni en el build, ni en las pruebas de extremo a extremo
// (DANPLAY_DEVTOOLS=0), donde el botón podría tapar lo que se pulsa.
const withDevTools = (command) =>
  command === 'serve' && !process.env.TAURI_ENV_PLATFORM && process.env.DANPLAY_DEVTOOLS !== '0'

export default defineConfig(({ command }) => ({
  plugins: [vue(), withDevTools(command) ? vueDevTools() : null],
  clearScreen: false,
  server: {
    // las e2e levantan su propio Vite y su propio núcleo en puertos libres
    port: Number(process.env.DANPLAY_VITE_PORT) || 5273,
    strictPort: true,
    proxy: { '/api': process.env.DANPLAY_API || 'http://127.0.0.1:8730' }
  },
  build: { target: 'esnext', outDir: 'dist' }
}))
