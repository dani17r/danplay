import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'jsdom',
    include: ['tests/**/*.test.js'],
    globals: true,
    // jsdom se llevaba el 85 % del tiempo: se creaba de nuevo en cada archivo.
    // Con `vmThreads` se crea una vez por hilo y cada archivo sigue aislado.
    pool: 'vmThreads',
    setupFiles: ['tests/setup.js'],
    coverage: {
      provider: 'v8',
      // lo de la interfaz; las pruebas de extremo a extremo (e2e/) van aparte
      include: ['src/**/*.{js,vue}'],
      // los iconos son datos generados (`npm run icons`); main.js solo monta
      exclude: ['src/icons.js', 'src/main.js'],
      reporter: ['text-summary', 'text', 'html', 'json-summary'],
      // No bajar de aquí. Se sube cuando se sube la cobertura, nunca al revés.
      thresholds: { lines: 84, statements: 82, branches: 74, functions: 78 }
    }
  }
})
