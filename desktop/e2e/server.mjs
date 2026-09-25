// Lo que levantan las pruebas de extremo a extremo antes de abrir Chrome: el
// núcleo de verdad y Vite, en modo navegador.
//
// - Un HOME entero de mentira (HOME y XDG_* en un temporal): ni los ajustes,
//   ni la base, ni las claves de IA de quien las ejecuta se tocan.
//   Tampoco el .env del proyecto (DANPLAY_PROJECT_ENV=0): sin eso el núcleo
//   lo copia al temporal y arranca con las claves de IA de quien programa.
// - Una carpeta de música pequeña generada con ffmpeg (tonos de unos
//   segundos, con sus etiquetas ID3), sin nada de las dependencias de
//   desarrollo: la CI solo instala lo que el núcleo necesita para funcionar.
// - El núcleo por TCP (`danplay.cli serve --port`), como en `npm run dev`,
//   con la carpeta dada de alta por la API y ya analizada. El vigilante de
//   carpetas va encendido: una de las pruebas mueve la carpeta por fuera.
// - Vite con el proxy apuntando a ese núcleo.
//
// El temporal lo crea playwright.config.js (DANPLAY_E2E_ROOT), para que las
// pruebas sepan dónde está la música aunque una la quite o la mueva.
// Playwright arranca esto (webServer) y espera a que Vite conteste; al acabar
// lo para con SIGTERM y aquí se para todo lo demás y se borra el temporal.
import { spawn, execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, rmSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { SONGS, BIG_COUNT, bigSong } from './songs.js'

const DESKTOP = join(dirname(fileURLToPath(import.meta.url)), '..')
const ROOT = join(DESKTOP, '..')
const PYTHON = process.env.DANPLAY_PYTHON || join(ROOT, '.venv', 'bin', 'python')
const CORE_PORT = Number(process.env.DANPLAY_E2E_CORE_PORT || 8731)
const VITE_PORT = Number(process.env.DANPLAY_E2E_VITE_PORT || 5274)
const CORE = `http://127.0.0.1:${CORE_PORT}`

const children = []
let temp = null
let stopping = false

function log(...args) {
  console.log('[e2e]', ...args)
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
  // un momento para que el núcleo cierre su base antes de borrar el temporal
  setTimeout(() => {
    if (temp) rmSync(temp, { recursive: true, force: true })
    process.exit(code)
  }, 1500)
}
for (const s of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(s, () => stop(0))

/** Una canción de verdad: un tono de `seconds` segundos con sus etiquetas. */
function makeSong(dir, artist, title, hz, seconds = 20) {
  const folder = join(dir, 'Artistas', artist)
  mkdirSync(folder, { recursive: true })
  const file = join(folder, `${artist} - ${title}.mp3`)
  execFileSync(
    'ffmpeg',
    [
      '-y',
      '-loglevel',
      'error',
      '-f',
      'lavfi',
      '-i',
      `sine=frequency=${hz}:sample_rate=44100`,
      '-t',
      String(seconds),
      '-codec:a',
      'libmp3lame',
      '-b:a',
      '64k',
      '-id3v2_version',
      '3',
      '-metadata',
      `artist=${artist}`,
      '-metadata',
      `title=${title}`,
      file
    ],
    { stdio: 'inherit', timeout: 60_000 }
  )
  return file
}

/**
 * La biblioteca grande: todas de una vez, con una sola llamada a ffmpeg (un
 * tono de un segundo, codificado una vez por archivo con sus etiquetas).
 */
function makeMany(dir, count) {
  mkdirSync(dir, { recursive: true })
  const args = [
    '-y',
    '-loglevel',
    'error',
    '-f',
    'lavfi',
    '-i',
    'sine=frequency=440:sample_rate=22050'
  ]
  for (let i = 0; i < count; i++) {
    const [artist, title] = bigSong(i)
    args.push(
      '-map',
      '0',
      '-t',
      '1',
      '-codec:a',
      'libmp3lame',
      '-b:a',
      '32k',
      '-id3v2_version',
      '3',
      '-metadata',
      `artist=${artist}`,
      '-metadata',
      `title=${title}`,
      join(dir, `${artist} - ${title}.mp3`)
    )
  }
  execFileSync('ffmpeg', args, { stdio: 'inherit', timeout: 120_000 })
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

async function waitFor(what, fn, timeout = 90_000) {
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
  if (!existsSync(PYTHON)) throw new Error(`no encuentro Python en ${PYTHON} (¿falta el .venv?)`)
  temp = process.env.DANPLAY_E2E_ROOT || mkdtempSync(join(tmpdir(), 'danplay-e2e-'))
  const music = join(temp, 'Musica')
  for (const [artist, title, hz] of SONGS) makeSong(music, artist, title, hz)
  makeMany(join(temp, 'Grande'), BIG_COUNT)
  log(`música de prueba en ${music}`)

  const env = {
    ...process.env,
    HOME: join(temp, 'home'),
    XDG_CONFIG_HOME: join(temp, 'config'),
    XDG_DATA_HOME: join(temp, 'data'),
    XDG_CACHE_HOME: join(temp, 'cache'),
    DANPLAY_PROJECT_ENV: '0',
    // La biblioteca del núcleo (donde van las descargas y su carpeta
    // Entrada) aparte de la música: el núcleo vuelve a crear la Entrada
    // cada vez que se le pregunta por ella, y si la música fuera esa misma
    // carpeta, al moverla por fuera reaparecería vacía en su sitio y nunca
    // saldría «No encuentro tu música» (es de la capa Python: ver el informe).
    DANPLAY_LIBRARY: join(temp, 'Biblioteca'),
    // si este proceso muere sin avisar, el núcleo se va con él (contrato E)
    DANPLAY_PARENT_PID: String(process.pid),
    PYTHONDONTWRITEBYTECODE: '1',
    PYTHONUNBUFFERED: '1'
  }
  for (const d of [env.HOME, env.XDG_CONFIG_HOME, env.XDG_DATA_HOME, env.XDG_CACHE_HOME]) {
    mkdirSync(d, { recursive: true })
  }
  const py = spawn(
    PYTHON,
    ['-m', 'danplay.cli', 'serve', '--host', '127.0.0.1', '--port', String(CORE_PORT)],
    { cwd: ROOT, env, stdio: 'inherit' }
  )
  children.push(py)
  py.on('exit', (code) => {
    if (!stopping) {
      console.error(`[e2e] el núcleo se ha parado (código ${code})`)
      stop(1)
    }
  })
  await waitFor('el núcleo', () => core('GET', '/api/status'))

  // la carpeta, dada de alta y analizada, como si se hubiera elegido en la bienvenida
  await core('POST', '/api/folders', { path: music, label: '', force: false })
  const started = await core('POST', '/api/scan')
  await waitFor('el análisis', async () => {
    const job =
      started?.job?.active === false ? started.job : await core('GET', '/api/jobs/escaneo')
    if (job.error) throw new Error(job.error)
    return !job.active
  })
  const { stats } = await core('GET', '/api/status')
  log(`núcleo listo en ${CORE}: ${stats.total} canciones`)

  const vite = spawn(
    process.execPath,
    [join(DESKTOP, 'node_modules', 'vite', 'bin', 'vite.js'), '--host', '127.0.0.1'],
    {
      cwd: DESKTOP,
      env: {
        ...process.env,
        DANPLAY_API: CORE,
        DANPLAY_VITE_PORT: String(VITE_PORT),
        DANPLAY_DEVTOOLS: '0'
      },
      stdio: 'inherit'
    }
  )
  children.push(vite)
  vite.on('exit', (code) => {
    if (!stopping) {
      console.error(`[e2e] Vite se ha parado (código ${code})`)
      stop(1)
    }
  })
}

main().catch((e) => {
  console.error('[e2e]', e.message)
  stop(1)
})
