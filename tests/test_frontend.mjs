// Verifica como el frontend construye las URLs de portada (y de audio, en el
// navegador).
// Esta prueba habria cazado el fallo de "doble clic y no suena": en Linux y
// Windows Tauri sirve los protocolos propios como http://<esquema>.localhost/,
// no como <esquema>://localhost/ (eso solo vale en macOS).
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

let fallos = 0
const prueba = (nombre, fn) => {
  try { fn(); console.log('  ok   ' + nombre) }
  catch (e) { fallos++; console.log('  FALLA ' + nombre + '\n        ' + e.message) }
}

async function cargarApi ({ tauri, plataforma }) {
  globalThis.window = tauri ? { __TAURI_INTERNALS__: { invoke: async () => '{}' } } : {}
  Object.defineProperty(globalThis, 'navigator', {
    value: { platform: plataforma, userAgent: plataforma }, configurable: true, writable: true })
  const mod = await import('../desktop/src/api.js?' + Math.random())
  return mod.api
}

/** Todo el Rust de la app: el protocolo propio ha vivido en main.rs y en protocol.rs. */
function rustDeLaApp () {
  const dir = fileURLToPath(new URL('../desktop/src-tauri/src/', import.meta.url))
  const out = []
  const walk = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, e.name)
      if (e.isDirectory()) walk(p)
      else if (e.name.endsWith('.rs')) out.push(readFileSync(p, 'utf8'))
    }
  }
  walk(dir)
  return out.join('\n')
}

console.log('URLs de medios')

const linux = await cargarApi({ tauri: true, plataforma: 'Linux x86_64' })
prueba('en Tauri/Linux la portada usa http://danplay.localhost', () => {
  assert.equal(linux.coverUrl(7), 'http://danplay.localhost/cover/7')
  assert.equal(linux.coverUrl(7, 96), 'http://danplay.localhost/cover/7?size=96')
})
prueba('la URL no usa el esquema danplay:// en Linux', () => {
  assert.ok(!linux.coverUrl(1).startsWith('danplay://'),
    'danplay://... no lo resuelve el WebView de Linux; no se ve nada')
})
prueba('dentro de la app el audio no tiene URL: lo reproduce Rust', () => {
  assert.equal(linux.audioUrl(7), null)
})

const mac = await cargarApi({ tauri: true, plataforma: 'MacIntel' })
prueba('en Tauri/macOS la portada usa danplay://localhost', () => {
  assert.equal(mac.coverUrl(7), 'danplay://localhost/cover/7')
  // y si esa forma falla, se prueba la otra
  assert.equal(mac.coverUrlAlt(7), 'http://danplay.localhost/cover/7')
})

const web = await cargarApi({ tauri: false, plataforma: 'Linux x86_64' })
prueba('fuera de Tauri cae al proxy HTTP de Vite', () => {
  assert.equal(web.audioUrl(7), '/api/song/7/audio')
  assert.equal(web.coverUrl(7), '/api/song/7/cover')
})
prueba('la ruta de medios coincide con lo que espera Rust (clase/id)', () => {
  const u = new URL(linux.coverUrl(42))
  const partes = u.pathname.replace(/^\//, '').split('/')
  assert.deepEqual(partes, ['cover', '42'])
})

// Esta prueba existe porque paso de verdad: al pasar el codigo a ingles el JS
// empezo a pedir /cover/<id> y Rust seguia comparando con "portada". Las
// caratulas dejaron de verse y nada lo detecto. Ahora se leen las clases que
// Rust acepta de su fuente, este donde este el protocolo.
prueba('Rust entiende la clase de medios que pide el JS', () => {
  const rust = rustDeLaApp()
  const conocidas = new Set([...rust.matchAll(/kind\s*[!=]=\s*"([a-z_]+)"/g)].map(m => m[1]))
  const primera = new URL(linux.coverUrl(7)).pathname.replace(/^\//, '').split('/')[0]
  assert.equal(primera, 'cover', 'coverUrl deberia empezar por cover')
  assert.ok(conocidas.has(primera),
    `Rust no atiende la clase "${primera}"; atiende: ${[...conocidas].join(', ') || 'ninguna'}`)
})

console.log(fallos ? `\n${fallos} fallo(s)` : '\ntodo correcto')
process.exit(fallos ? 1 : 0)
