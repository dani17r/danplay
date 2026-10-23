// Lo que levanta la prueba de rendimiento antes de abrir WebKit: lo mismo que
// las de extremo a extremo (e2e/server.mjs: el núcleo en un HOME de mentira,
// la música de prueba) y, encima, una biblioteca grande y la interfaz
// COMPILADA servida con `vite preview`, que es la que corre dentro de la app.
// Con Vite en modo desarrollo cada módulo va por separado y sin minimizar: lo
// que se mediría no sería lo que carga la app.
//
// - e2e/server.mjs arranca el núcleo, da de alta la música pequeña y la
//   analiza; cuando su Vite contesta, todo eso está listo. Ese Vite no se usa.
// - Aquí se genera la carpeta «Rendimiento» (DANPLAY_PERF_SONGS canciones de
//   un segundo; 2000 si no se dice), se da de alta y se analiza.
// - `vite preview` sirve la carpeta compilada (DANPLAY_PERF_DIST, o dist/)
//   con el proxy de /api al núcleo (el de vite.config.js).
//
// Al acabar, Playwright lo para con SIGTERM: se para todo y e2e/server.mjs
// borra el temporal.
import { spawn, execFileSync } from 'node:child_process'
import { mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { perfSong, PERF_FOLDER } from './songs.js'

const DESKTOP = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const ROOT = process.env.DANPLAY_E2E_ROOT
const CORE = `http://127.0.0.1:${process.env.DANPLAY_E2E_CORE_PORT}`
const DEV = `http://127.0.0.1:${process.env.DANPLAY_E2E_VITE_PORT}`
const PORT = Number(process.env.DANPLAY_PERF_PORT)
const COUNT = Number(process.env.DANPLAY_PERF_SONGS) || 2000
const DIST = resolve(DESKTOP, process.env.DANPLAY_PERF_DIST || 'dist')

const children = []
let stopping = false

function log(...args) {
  console.log('[perf]', ...args)
}

function stop(code = 0) {
  if (stopping) return
  stopping = true
  for (const c of children) {
    try {
      c.kill('SIGTERM')
    } catch {
      /* ya había terminado */
    }
  }
  // e2e/server.mjs espera un momento al núcleo antes de borrar el temporal
  setTimeout(() => process.exit(code), 2500)
}
for (const s of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(s, () => stop(0))

/** La biblioteca grande, por tandas: una sola llamada a ffmpeg con miles de salidas no cabe. */
function makeMany(dir, count) {
  mkdirSync(dir, { recursive: true })
  for (let from = 0; from < count; from += 400) {
    const args = [
      '-y',
      '-loglevel',
      'error',
      '-f',
      'lavfi',
      '-i',
      'sine=frequency=440:sample_rate=22050'
    ]
    for (let i = from; i < Math.min(count, from + 400); i++) {
      const [artist, title] = perfSong(i)
      args.push(
        ...['-map', '0', '-t', '1', '-codec:a', 'libmp3lame', '-b:a', '32k', '-id3v2_version', '3'],
        ...['-metadata', `artist=${artist}`, '-metadata', `title=${title}`],
        join(dir, `${artist} - ${title}.mp3`)
      )
    }
    execFileSync('ffmpeg', args, { stdio: 'inherit', timeout: 180_000 })
  }
}

/** Una petición al núcleo, con la cabecera que exige en modo desarrollo. */
async function core(method, path, body) {
  const r = await fetch(CORE + path, {
    method,
    headers: { 'X-DanPlay': '1', ...(body ? { 'Content-Type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined
  })
  const text = await r.text()
  if (!r.ok) throw new Error(`${method} ${path}: ${r.status} ${text}`)
  return text ? JSON.parse(text) : null
}

async function waitFor(what, fn, timeout = 240_000) {
  const until = Date.now() + timeout
  let last = null
  while (Date.now() < until) {
    try {
      const v = await fn()
      if (v) return v
    } catch (e) {
      last = e
    }
    await new Promise((r) => setTimeout(r, 300))
  }
  throw new Error(`no llega ${what}${last ? ': ' + last.message : ''}`)
}

async function main() {
  if (!ROOT || !PORT) throw new Error('falta el entorno de e2e/perf/playwright.config.js')
  const base = spawn(process.execPath, [join(DESKTOP, 'e2e', 'server.mjs')], {
    cwd: DESKTOP,
    env: process.env,
    stdio: 'inherit'
  })
  children.push(base)
  base.on('exit', (code) => {
    if (!stopping) {
      console.error(`[perf] e2e/server.mjs se ha parado (código ${code})`)
      stop(1)
    }
  })
  const folder = join(ROOT, PERF_FOLDER)
  makeMany(folder, COUNT)
  // su Vite arranca cuando el núcleo ya tiene la música pequeña analizada
  await waitFor('el núcleo', async () => (await fetch(DEV + '/')).ok)
  await core('POST', '/api/folders', { path: folder, label: '', force: false })
  const started = await core('POST', '/api/scan')
  await waitFor('el análisis', async () => {
    const job = started?.job?.active === false ? started.job : await core('GET', '/api/jobs/scan')
    if (job.error) throw new Error(job.error)
    return !job.active
  })
  const { stats } = await core('GET', '/api/status')
  log(`biblioteca lista: ${stats.total} canciones; sirvo ${DIST}`)

  const preview = spawn(
    process.execPath,
    [
      join(DESKTOP, 'node_modules', 'vite', 'bin', 'vite.js'),
      'preview',
      '--host',
      '127.0.0.1',
      '--port',
      String(PORT),
      '--strictPort',
      '--outDir',
      DIST
    ],
    { cwd: DESKTOP, env: { ...process.env, DANPLAY_API: CORE }, stdio: 'inherit' }
  )
  children.push(preview)
  preview.on('exit', (code) => {
    if (!stopping) {
      console.error(`[perf] vite preview se ha parado (código ${code})`)
      stop(1)
    }
  })
}

main().catch((e) => {
  console.error('[perf]', e.message)
  stop(1)
})
