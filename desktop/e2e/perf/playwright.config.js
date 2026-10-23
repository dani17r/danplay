// La prueba de rendimiento de la interfaz: la compilada, en WebKit (el motor
// de la app en Linux), contra el núcleo de verdad y una biblioteca grande.
// Aparte de las de extremo a extremo: no comprueba nada, mide, y tarda.
//
//   npm run perf                    (compila y mide)
//   DANPLAY_E2E_CORE=…/danplay-server DANPLAY_PERF_RUNS=7 npm run perf
//
// Hace falta el WebKit de Playwright (`npx playwright install webkit`, en la
// caché del usuario). Lo que mide y cómo, en e2e/perf/startup.perf.js; el
// resultado sale por pantalla y en test-results/perf.json (o donde diga
// DANPLAY_PERF_OUT). DANPLAY_PERF_DIST sirve otra carpeta compilada, para
// comparar dos versiones con el mismo núcleo y la misma máquina.
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

// Como en playwright.config.js: se eligen una vez y los procesos de las
// pruebas, que vuelven a leer este archivo, los heredan del entorno.
if (!process.env.DANPLAY_PERF_PORT) {
  process.env.DANPLAY_PERF_PORT = String(await freePort())
  process.env.DANPLAY_E2E_VITE_PORT = String(await freePort())
  process.env.DANPLAY_E2E_CORE_PORT = String(await freePort())
}
if (!process.env.DANPLAY_E2E_ROOT) {
  process.env.DANPLAY_E2E_ROOT = mkdtempSync(join(tmpdir(), 'danplay-perf-'))
}
const base = `http://127.0.0.1:${process.env.DANPLAY_PERF_PORT}`

export default defineConfig({
  testDir: '.',
  testMatch: '*.perf.js',
  workers: 1,
  fullyParallel: false,
  retries: 0,
  // varias pasadas, cada una con su arranque en frío
  timeout: 15 * 60_000,
  expect: { timeout: 20_000 },
  reporter: [['list']],
  outputDir: '../../test-results/perf',
  use: {
    baseURL: base,
    browserName: 'webkit',
    viewport: { width: 1400, height: 900 },
    deviceScaleFactor: 1,
    locale: 'es-ES',
    trace: 'off'
  },
  webServer: {
    command: 'node e2e/perf/server.mjs',
    cwd: '../..',
    url: base + '/',
    reuseExistingServer: false,
    // generar y analizar miles de canciones, en un disco lento, tarda
    timeout: 600_000,
    stdout: 'pipe',
    stderr: 'pipe',
    gracefulShutdown: { signal: 'SIGTERM', timeout: 15_000 }
  }
})
