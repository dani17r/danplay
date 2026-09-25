// La hoja de estilos, entera. Está partida por áreas (src/styles/) pero las
// pruebas la miran como un todo, que es como la ve el navegador.
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

// `new URL(...).pathname` daba una ruta absoluta desde la raíz del disco:
// vitest resuelve los módulos con URLs propias, no con rutas de archivo.
const DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'src', 'styles')

/**
 * El CSS sin el formato: cada regla en su línea, sin espacios alrededor de
 * llaves, dos puntos y puntos y coma (`.chip{cursor:pointer}`). Prettier lo
 * deja con una declaración por línea, que es como se lee mejor; las pruebas
 * buscan reglas y propiedades, y así no dependen de cómo esté sangrado.
 * @param {string} css
 */
export function compactCss(css) {
  return css
    .replace(/\r/g, '')
    .replace(/,\s*\n\s*/g, ',') // selectores partidos en varias líneas
    .replace(/\s*\{\s*/g, '{')
    .replace(/;\s*/g, ';')
    .replace(/;?\s*\}/g, '}')
    .replace(/([\w-]+):[ \t]+/g, '$1:') // «position: sticky» → «position:sticky»
    .replace(/\n{2,}/g, '\n')
}

/** Todo el CSS de la aplicación concatenado en el orden en que se carga. */
export function allCss() {
  const index = readFileSync(join(DIR, 'index.css'), 'utf8')
  const order = [...index.matchAll(/@import url\('\.\/([^']+)'\)/g)].map((m) => m[1])
  const rest = readdirSync(DIR).filter(
    (f) => f.endsWith('.css') && f !== 'index.css' && !order.includes(f)
  )
  return compactCss([...order, ...rest].map((f) => readFileSync(join(DIR, f), 'utf8')).join('\n'))
}
