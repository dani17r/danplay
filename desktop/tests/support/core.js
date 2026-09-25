// El código del núcleo Python, entero, para las pruebas que comparan nombres
// con él (rutas, campos, calidades).
//
// Se lee todo el paquete y no un archivo concreto: el núcleo reparte sus
// rutas y su lógica en módulos y los reorganiza (las miniaturas salieron de
// api.py a thumbnails.py), y lo que estas pruebas quieren saber es si un
// nombre existe en el núcleo, no en qué archivo vive.
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

export const CORE_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'danplay')

/** @param {string} dir @param {string[]} out */
function walk(dir, out) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.name === '__pycache__' || e.name.startsWith('.')) continue
    const p = join(dir, e.name)
    if (e.isDirectory()) walk(p, out)
    else if (e.name.endsWith('.py')) out.push(p)
  }
  return out
}

let cached = null

/** Todo el Python del núcleo, concatenado. */
export function coreSource() {
  if (cached == null) {
    cached = walk(CORE_DIR, [])
      .sort()
      .map((p) => readFileSync(p, 'utf8'))
      .join('\n')
  }
  return cached
}
