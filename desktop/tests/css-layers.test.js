import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

// Cuando un selector se define dos veces y las dos definiciones tocan la MISMA
// propiedad, gana la última y la primera desaparece sin decir nada. Casi
// siempre es a propósito (una capa que afina lo de antes), pero una vez no lo
// fue: `thead th` estaba definido con `position:sticky` y ochenta líneas más
// abajo otra vez con `position:relative`, así que la cabecera de la tabla se
// iba con la lista al desplazar. Nadie lo vio en meses.
//
// Esta prueba no prohíbe los choques: los CUENTA. Los que hay son conocidos y
// están abajo con su razón. Si aparece uno nuevo, la prueba lo dice y hay que
// decidir a conciencia si es una capa o un descuido.
const DIR = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'styles')

/** Los archivos en el orden en que el navegador los aplica. */
function filesInOrder() {
  const index = readFileSync(join(DIR, 'index.css'), 'utf8')
  const listed = [...index.matchAll(/@import url\('\.\/([^']+)'\)/g)].map((m) => m[1])
  const rest = readdirSync(DIR).filter(
    (f) => f.endsWith('.css') && f !== 'index.css' && !listed.includes(f)
  )
  return [...listed, ...rest]
}

/** Reglas de primer nivel: las de dentro de un @media son otra cosa. */
function topLevelRules(text) {
  const rules = []
  let depth = 0
  let start = 0
  let bodyStart = 0
  let selector = null
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (c === '{') {
      if (depth === 0) {
        selector = text.slice(start, i).trim().split('\n').pop().trim()
        bodyStart = i + 1
      }
      depth++
    } else if (c === '}') {
      depth--
      if (depth === 0) {
        if (selector && !selector.startsWith('@')) {
          rules.push({ selector, body: text.slice(bodyStart, i).trim() })
        }
        start = i + 1
      }
    }
  }
  return rules
}

/** Las propiedades de un cuerpo de regla, sin las variables. */
function declarations(body) {
  const out = {}
  for (const part of body.split(/;(?![^(]*\))/)) {
    const at = part.indexOf(':')
    if (at < 0) continue
    const name = part.slice(0, at).trim()
    if (name && !name.startsWith('--')) out[name] = part.slice(at + 1).trim()
  }
  return out
}

/** Selectores repetidos que además se pisan una propiedad. */
function silentOverrides() {
  const seen = new Map()
  const clashes = []
  for (const file of filesInOrder()) {
    for (const { selector, body } of topLevelRules(readFileSync(join(DIR, file), 'utf8'))) {
      const now = declarations(body)
      const before = seen.get(selector)
      if (!before) {
        seen.set(selector, { file, declarations: now })
        continue
      }
      const properties = Object.keys(before.declarations).filter(
        (p) => p in now && before.declarations[p] !== now[p]
      )
      if (properties.length) {
        clashes.push(
          `${selector}  (${before.file} -> ${file}): ${properties.sort().join(', ')}`
        )
      }
    }
  }
  return clashes.sort()
}

// Los que hay hoy, cada uno con su porqué. Todos son «primero lo general,
// luego lo afinado», que es como se quiso.
const CONOCIDOS = [
  // el relleno de las filas cambia con la densidad que elija el usuario
  'td  (base.css -> list-cards.css): padding',
  // los iconos del lateral crecieron para alinearse con el texto
  '.nav-link .nav-icon  (base.css -> list-cards.css): width',
  // la cola se ensanchó al añadirle la carátula
  '.queue  (list-cards.css -> ui.css): width',
  // cada fila reserva arriba el hueco de su etiqueta («Sonaba antes»…)
  '.queue-row  (list-cards.css -> ui.css): gap, padding',
  // en pantallas estrechas el aviso ocupa lo que haya
  '.toast  (search.css -> responsive.css): max-width'
].sort()

describe('capas del css', () => {
  it('ningún selector pisa a otro sin querer', () => {
    expect(silentOverrides()).toEqual(CONOCIDOS)
  })

  it('la cabecera de la tabla se queda fija', () => {
    // el choque que sí era un fallo, y que ya no está
    const clashes = silentOverrides().filter((c) => c.startsWith('thead th'))
    expect(clashes, 'thead th vuelve a pisarse a sí mismo').toEqual([])
  })
})
