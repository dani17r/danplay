// Verifica como el frontend construye las URLs de audio y portada.
// Esta prueba habria cazado el fallo de "doble clic y no suena": en Linux y
// Windows Tauri sirve los protocolos propios como http://<esquema>.localhost/,
// no como <esquema>://localhost/ (eso solo vale en macOS).
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

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

console.log('URLs de medios')

const linux = await cargarApi({ tauri: true, plataforma: 'Linux x86_64' })
prueba('en Tauri/Linux el audio usa http://danplay.localhost', () => {
  assert.equal(linux.audioUrl(7), 'http://danplay.localhost/audio/7')
})
prueba('en Tauri/Linux la portada usa http://danplay.localhost', () => {
  assert.equal(linux.coverUrl(7), 'http://danplay.localhost/cover/7')
})
prueba('la URL no usa el esquema danplay:// en Linux', () => {
  assert.ok(!linux.audioUrl(1).startsWith('danplay://'),
    'danplay://... no lo resuelve el WebView de Linux; nada suena')
})

const mac = await cargarApi({ tauri: true, plataforma: 'MacIntel' })
prueba('en Tauri/macOS el audio usa danplay://localhost', () => {
  assert.equal(mac.audioUrl(7), 'danplay://localhost/audio/7')
})

const web = await cargarApi({ tauri: false, plataforma: 'Linux x86_64' })
prueba('fuera de Tauri cae al proxy HTTP de Vite', () => {
  assert.equal(web.audioUrl(7), '/api/song/7/audio')
  assert.equal(web.coverUrl(7), '/api/song/7/cover')
})
prueba('la ruta de medios coincide con lo que espera Rust (clase/id)', () => {
  const u = new URL(linux.audioUrl(42))
  const partes = u.pathname.replace(/^\//, '').split('/')
  assert.deepEqual(partes, ['audio', '42'])
})

// Esta prueba existe porque paso de verdad: al pasar el codigo a ingles el JS
// empezo a pedir /cover/<id> y Rust seguia comparando con "portada". Las
// caratulas dejaron de verse y nada lo detecto, porque aqui solo se miraba
// la clase «audio». Ahora se leen las clases que Rust acepta de su fuente.
prueba('Rust entiende TODAS las clases de medios que pide el JS', () => {
  const rust = readFileSync(new URL('../desktop/src-tauri/src/main.rs', import.meta.url), 'utf8')
  const aceptadas = [...rust.matchAll(/class\s*==\s*"([a-z_]+)"/g)].map(m => m[1])
  // el audio es la rama por defecto, no lleva comparacion explicita
  const conocidas = new Set([...aceptadas, 'audio'])

  for (const clase of ['audio', 'cover']) {
    const url = clase === 'audio' ? linux.audioUrl(7) : linux.coverUrl(7)
    const primera = new URL(url).pathname.replace(/^\//, '').split('/')[0]
    assert.equal(primera, clase, `coverUrl/audioUrl deberia empezar por ${clase}`)
    assert.ok(conocidas.has(primera),
      `Rust no atiende la clase "${primera}"; acepta: ${[...conocidas].join(', ')}`)
  }
})

console.log(fallos ? `\n${fallos} fallo(s)` : '\ntodo correcto')
process.exit(fallos ? 1 : 0)
