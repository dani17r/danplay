// La hoja de estilos, entera. Está partida por áreas (src/styles/) pero las
// pruebas la miran como un todo, que es como la ve el navegador.
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

// `new URL(...).pathname` daba una ruta absoluta desde la raíz del disco:
// vitest resuelve los módulos con URLs propias, no con rutas de archivo.
const DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'src', 'styles')

/** Todo el CSS de la aplicación concatenado en el orden en que se carga. */
export function allCss() {
  const index = readFileSync(join(DIR, 'index.css'), 'utf8')
  const order = [...index.matchAll(/@import url\('\.\/([^']+)'\)/g)].map((m) => m[1])
  const rest = readdirSync(DIR).filter((f) => f.endsWith('.css') && f !== 'index.css' && !order.includes(f))
  return [...order, ...rest].map((f) => readFileSync(join(DIR, f), 'utf8')).join('\n')
}
