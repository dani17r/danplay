import { defineConfig, lazyPlugins } from 'vite-plus'

// Las herramientas de Vue (el botón flotante con el árbol de componentes)
// solo en `vp dev` abierto en un navegador. Ni en `tauri dev` (Tauri
// arranca Vite con TAURI_ENV_PLATFORM puesto, y dentro del WebView el botón
// estorba), ni en el build, ni en las pruebas de extremo a extremo
// (DANPLAY_DEVTOOLS=0), donde el botón podría tapar lo que se pulsa.
const withDevTools = (command) =>
  command === 'serve' && !process.env.TAURI_ENV_PLATFORM && process.env.DANPLAY_DEVTOOLS !== '0'

export default defineConfig(({ command, mode }) => ({
  // Los plugins, solo cuando arranca Vite de verdad (dev, build, preview):
  // `vp fmt` también lee este archivo y no los necesita.
  plugins: lazyPlugins(async () => {
    const { default: vue } = await import('@vitejs/plugin-vue')
    if (!withDevTools(command)) return [vue()]
    const { default: vueDevTools } = await import('vite-plugin-vue-devtools')
    return [vue(), vueDevTools()]
  }),
  // `vp fmt` (Oxfmt), con las mismas opciones que teníamos con Prettier. Los
  // scripts le piden cuatro hilos (`--threads 4`): los .vue y el HTML los
  // formatea con el Prettier que lleva dentro, cada hilo arranca el suyo, y
  // con los 16 de este equipo tardaba más (2,4 s) que con 4 (1,5 s).
  fmt: {
    semi: false,
    singleQuote: true,
    printWidth: 100,
    trailingComma: 'none',
    arrowParens: 'always',
    vueIndentScriptAndStyle: false,
    sortPackageJson: false,
    ignorePatterns: [
      'dist',
      'dist-mobile',
      'src-tauri',
      'coverage',
      'playwright-report',
      'test-results',
      'bun.lock',
      // generado desde heroicons: `vp run icons`
      'src/icons.js'
    ]
  },
  clearScreen: false,
  server: {
    // las e2e levantan su propio Vite y su propio núcleo en puertos libres
    port: Number(process.env.DANPLAY_VITE_PORT) || 5273,
    strictPort: true,
    proxy: { '/api': process.env.DANPLAY_API || 'http://127.0.0.1:8730' }
  },
  // `--mode mobile`: la app del celular (mobile.html, ver src/mobile/), en su
  // propia carpeta, que es la que empaqueta mobile/tauri.conf.json
  build:
    mode === 'mobile'
      ? { target: 'esnext', outDir: 'dist-mobile', rollupOptions: { input: 'mobile.html' } }
      : { target: 'esnext', outDir: 'dist' }
}))
