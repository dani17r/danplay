// Regenera src/icons.js desde heroicons (MIT).
//
// El archivo generado decía «no editar a mano», pero el programa que lo
// generaba no existía, así que no había forma de cumplirlo. Esto lo arregla:
// `scripts/icons.map.json` dice qué icono de heroicons es cada nombre nuestro
// y este programa vuelve a construir el archivo.
//
//   node scripts/icons.mjs            regenera src/icons.js
//   node scripts/icons.mjs --check    solo dice si está al día (para CI)
//   node scripts/icons.mjs --discover reconstruye el mapa comparando el
//                                     icons.js actual con heroicons
import { readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const ROOT = join(HERE, '..')
const HEROICONS = join(ROOT, 'node_modules', 'heroicons', '24')
const MAP_FILE = join(HERE, 'icons.map.json')
const OUT = join(ROOT, 'src', 'icons.js')

const STYLES = { o: 'outline', s: 'solid' }

/** El contenido de un svg de heroicons, sin la etiqueta <svg> de fuera. */
function inner(style, file) {
  const svg = readFileSync(join(HEROICONS, STYLES[style], file), 'utf8')
  return svg
    .replace(/^[\s\S]*?<svg[^>]*>/, '')
    .replace(/<\/svg>[\s\S]*$/, '')
    .replace(/\s+/g, ' ')
    .trim()
}

function build(map) {
  const icons = {}
  for (const [name, { style, file }] of Object.entries(map)) {
    icons[name] = { e: style, d: inner(style, file) }
  }
  return (
    '// Generado desde heroicons (MIT) - https://heroicons.com\n' +
    '// No editar a mano: `npm run icons`.\n' +
    `export const ICONS = ${JSON.stringify(icons)}\n`
  )
}

/** Reconstruye el mapa comparando lo que ya hay con los svg de heroicons. */
function discover() {
  const current = readFileSync(OUT, 'utf8')
  const json = current.slice(current.indexOf('{'), current.lastIndexOf('}') + 1)
  const icons = JSON.parse(json)

  const index = new Map()
  for (const [style, dir] of Object.entries(STYLES)) {
    for (const file of readdirSync(join(HEROICONS, dir))) {
      if (file.endsWith('.svg')) index.set(`${style}:${inner(style, file)}`, { style, file })
    }
  }

  const map = {}
  const missing = []
  for (const [name, icon] of Object.entries(icons)) {
    const key = `${icon.e}:${icon.d.replace(/\s+/g, ' ').trim()}`
    const found = index.get(key)
    if (found) map[name] = found
    else missing.push(name)
  }
  writeFileSync(MAP_FILE, JSON.stringify(map, null, 2) + '\n')
  console.log(`mapa con ${Object.keys(map).length} iconos`)
  if (missing.length) console.warn('sin equivalente en heroicons:', missing.join(', '))
}

const mode = process.argv[2]
if (mode === '--discover') {
  discover()
} else {
  if (!existsSync(MAP_FILE)) {
    console.error('falta scripts/icons.map.json: ejecuta `node scripts/icons.mjs --discover`')
    process.exit(1)
  }
  const map = JSON.parse(readFileSync(MAP_FILE, 'utf8'))
  const generated = build(map)
  if (mode === '--check') {
    const current = existsSync(OUT) ? readFileSync(OUT, 'utf8') : ''
    if (current.trim() !== generated.trim()) {
      console.error('src/icons.js no coincide con el mapa: ejecuta `npm run icons`')
      process.exit(1)
    }
    console.log('los iconos están al día')
  } else {
    writeFileSync(OUT, generated)
    console.log(`src/icons.js regenerado con ${Object.keys(map).length} iconos`)
  }
}
