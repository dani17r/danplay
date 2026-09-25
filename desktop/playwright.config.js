// Pruebas de extremo a extremo: la interfaz de verdad en Chrome, contra el
// núcleo de verdad (en modo navegador: sin Rust). Ver e2e/server.mjs.
//
//   npm run e2e
//
// Usa el Chrome del sistema (`channel: 'chrome'`): no hace falta descargar
// ningún navegador. El informe queda en playwright-report/ (la CI lo sube si
// algo falla) y las trazas de lo que falló, en test-results/.
import { defineConfig } from '@playwright/test'
import { mkdtempSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/** Un puerto libre, preguntándoselo al sistema. */
function freePort() {
  return new Promise((resolve, reject) => {
    const s = createServer()
    s.unref()
    s.on('error', reject)
    s.listen(0, '127.0.0.1', () => {
      const { port } = /** @type {import('node:net').AddressInfo} */ (s.address())
      s.close(() => resolve(port))
    })
  })
}

// Los puertos y el temporal (el HOME de mentira y la música de prueba) se
// eligen una vez: los procesos de las pruebas vuelven a leer este archivo y
// heredan el entorno, así que usan los mismos. El temporal lo borra
// e2e/server.mjs al terminar.
if (!process.env.DANPLAY_E2E_VITE_PORT) {
  process.env.DANPLAY_E2E_VITE_PORT = String(await freePort())
  process.env.DANPLAY_E2E_CORE_PORT = String(await freePort())
}
if (!process.env.DANPLAY_E2E_ROOT) {
  process.env.DANPLAY_E2E_ROOT = mkdtempSync(join(tmpdir(), 'danplay-e2e-'))
}
const base = `http://127.0.0.1:${process.env.DANPLAY_E2E_VITE_PORT}`

export default defineConfig({
  testDir: 'e2e',
  // un solo núcleo con una sola biblioteca: las pruebas van de una en una
  workers: 1,
  fullyParallel: false,
  retries: process.env.CI ? 1 : 0,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: [['list'], ['html', { outputFolder: 'playwright-report', open: 'never' }]],
  outputDir: 'test-results',
  use: {
    baseURL: base,
    channel: 'chrome',
    viewport: { width: 1400, height: 900 },
    locale: 'es-ES',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    // el reproductor del navegador suena con un <audio>
    launchOptions: { args: ['--autoplay-policy=no-user-gesture-required'] }
  },
  webServer: {
    command: 'node e2e/server.mjs',
    url: base + '/',
    reuseExistingServer: false,
    // generar la biblioteca, levantar el núcleo y analizar: en un disco lento
    // tarda
    timeout: 240_000,
    stdout: 'pipe',
    stderr: 'pipe',
    gracefulShutdown: { signal: 'SIGTERM', timeout: 10_000 }
  }
})
