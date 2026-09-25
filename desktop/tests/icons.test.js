import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { ICONS } from '../src/icons.js'

const files = [
  ...readdirSync('src/components').filter(f => f.endsWith('.vue')).map(f => 'src/components/' + f),
  'src/App.vue'
]

describe('iconos', () => {
  it('todos los que usan los componentes existen', () => {
    const faltan = []
    for (const f of files) {
      const txt = readFileSync(f, 'utf8')
      for (const m of txt.matchAll(/<Icon[^>]*?(?<![:\w])n="([a-zA-Z0-9]+)"/g)) {
        if (!ICONS[m[1]]) faltan.push(`${f}: ${m[1]}`)
      }
      // los dinamicos, solo las dos ramas del ternario: :n="cond ? 'a' : 'b'"
      for (const m of txt.matchAll(/:n="[^"]*\?\s*'([a-zA-Z0-9]+)'\s*:\s*'([a-zA-Z0-9]+)'\s*"/g)) {
        for (const name of [m[1], m[2]]) {
          if (!ICONS[name]) faltan.push(`${f}: ${name}`)
        }
      }
    }
    expect(faltan, 'iconos inexistentes: ' + faltan.join(', ')).toEqual([])
  })

  it('cada icono trae svg valido', () => {
    for (const [name, i] of Object.entries(ICONS)) {
      expect(['o', 's'], name).toContain(i.e)
      expect(i.d, name).toMatch(/^<path/)
      expect(i.d, name).toContain('d="')
    }
  })

  it('no queda ningun emoji suelto en los controles', () => {
    const emojis = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}]/u
    const sospechosos = []
    for (const f of files) {
      for (const [n, linea] of readFileSync(f, 'utf8').split('\n').entries()) {
        if (emojis.test(linea) && /<button|class="icon"|class="lupa"/.test(linea)) {
          sospechosos.push(`${f}:${n + 1}`)
        }
      }
    }
    expect(sospechosos, 'emojis en controles: ' + sospechosos.join(', ')).toEqual([])
  })
})
