// El código del núcleo (el de Rust: la API, la biblioteca y el análisis),
// entero, para las pruebas que comparan nombres con él (rutas, campos,
// calidades).
//
// Se lee todo y no un archivo concreto: el núcleo reparte sus rutas y su
// lógica en módulos y los reorganiza, y lo que estas pruebas quieren saber es
// si un nombre existe en el núcleo, no en qué archivo vive. Sin sus pruebas:
// que un nombre salga en una prueba no quiere decir que el núcleo lo conozca.
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..')
export const CORE_DIRS = ['server/src', 'library/src', 'analysis/src'].map((d) => join(ROOT, d))

/** Lo que es de las pruebas del núcleo, no del núcleo. */
const TESTS = new Set(['tests', 'api_tests', 'tests.rs', 'testkit.rs', 'testing.rs'])

/** @param {string} dir @param {string[]} out */
function walk(dir, out) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (TESTS.has(e.name) || e.name.startsWith('.')) continue
    const p = join(dir, e.name)
    if (e.isDirectory()) walk(p, out)
    else if (e.name.endsWith('.rs')) out.push(p)
  }
  return out
}

let cached = null

/** Todo el Rust del núcleo, concatenado. */
export function coreSource() {
  if (cached == null) {
    cached = CORE_DIRS.flatMap((d) => walk(d, []))
      .sort()
      .map((p) => readFileSync(p, 'utf8'))
      .join('\n')
  }
  return cached
}
