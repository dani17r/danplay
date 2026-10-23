import { defineConfig } from 'vite-plus'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'jsdom',
    include: ['tests/**/*.test.js'],
    globals: true,
    // jsdom se llevaba el 85 % del tiempo: se creaba de nuevo en cada archivo.
    // Con `vmThreads` se crea una vez por hilo y cada archivo sigue aislado.
    // (happy-dom no sale a cuenta: con las que más tardan, que son las que
    // marcan el total, iba un 10 % más lento, y seis pruebas fallaban.)
    pool: 'vmThreads',
    // Un hilo por núcleo de verdad, no por hilo del procesador: lo que marca
    // el total son los archivos más largos (study.test.js, app.test.js), y
    // con todos los hilos a la vez cada uno iba más lento. Con la mitad
    // tarda menos (19 → 17 s en un Ryzen de 8 núcleos), gasta un 30 % menos
    // de CPU y 1 GB menos de memoria.
    maxWorkers: '50%',
    // lo ya transformado (los .vue compilados...) se guarda entre ejecuciones,
    // en node_modules/.vitest-cache: la siguiente se ahorra ese trabajo
    fsModuleCache: true,
    setupFiles: ['tests/setup.js'],
    coverage: {
      provider: 'v8',
      // lo de la interfaz; las pruebas de extremo a extremo (e2e/) van aparte
      include: ['src/**/*.{js,vue}'],
      // los iconos son datos generados (`vp run icons`); main.js solo monta
      exclude: ['src/icons.js', 'src/main.js'],
      reporter: ['text-summary', 'text', 'html', 'json-summary'],
      // No bajar de aquí. Se sube cuando se sube la cobertura, nunca al revés.
      thresholds: { lines: 84, statements: 82, branches: 74, functions: 78 }
    }
  }
})
