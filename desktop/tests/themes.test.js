import { describe, it, expect } from 'vitest'
import { allCss } from './support/css.js'
import { readFileSync, readdirSync } from 'node:fs'
import { CATALOG, FIELDS, applyTheme, SIZES, applySize, savedSize, applyDensity } from '../src/themes.js'

const css = allCss()

describe('los colores salen del tema', () => {
  it('no hay colores fijos fuera de las variables', () => {
    const malas = []
    for (const [n, linea] of css.split('\n').entries()) {
      const l = linea.trim()
      // se permiten: definicion de variables, blancos/negros puros y transparencias
      if (l.startsWith('/*') || l.startsWith('*')) continue
      // se quitan las definiciones de variables: ahi el hex es legitimo
      const sinVars = l.replace(/--[\w-]+\s*:\s*#[0-9a-fA-F]{3,8}/g, '')
      for (const m of sinVars.matchAll(/#([0-9a-fA-F]{3,8})\b/g)) {
        const hex = m[1].toLowerCase()
        const soloCeros = /^0+[0-9a-f]{0,2}$/.test(hex)      // negro con alfa
        const soloUnos = /^(fff|ffffff)([0-9a-f]{2})?$/.test(hex)  // blanco con alfa
        if (!soloCeros && !soloUnos) malas.push(`style.css:${n + 1}  ${l.slice(0, 70)}`)
      }
    }
    expect(malas, 'usa var(--...) o color-mix():\n' + malas.join('\n')).toEqual([])
  })

  it('los componentes .vue tampoco llevan colores fijos', () => {
    const vistas = [
      ...readdirSync('src/components').filter(f => f.endsWith('.vue')).map(f => 'src/components/' + f),
      ...readdirSync('src/components/ui').map(f => 'src/components/ui/' + f),
      'src/App.vue'
    ]
    const malas = []
    for (const f of vistas) {
      for (const [n, linea] of readFileSync(f, 'utf8').split('\n').entries()) {
        for (const m of linea.matchAll(/#([0-9a-fA-F]{3,8})\b/g)) {
          const hex = m[1].toLowerCase()
          if (!/^0+[0-9a-f]{0,2}$/.test(hex) && !/^(fff|ffffff)([0-9a-f]{2})?$/.test(hex)) {
            malas.push(`${f}:${n + 1}  ${linea.trim().slice(0, 60)}`)
          }
        }
      }
    }
    expect(malas, 'usa var(--...):\n' + malas.join('\n')).toEqual([])
  })
})

describe('catalogo de temas', () => {
  it('todos definen los 12 colores', () => {
    for (const [k, t] of Object.entries(CATALOG)) {
      for (const c of FIELDS) {
        expect(t.v[c.k], `${k} no define ${c.k}`).toMatch(/^#[0-9a-fA-F]{6}$/)
      }
    }
  })

  it('cada tema declara si es claro u oscuro', () => {
    for (const [k, t] of Object.entries(CATALOG)) {
      expect(['light', 'dark'], k).toContain(t.kind)
    }
  })

  it('hay temas claros y oscuros', () => {
    const tipos = Object.values(CATALOG).map(t => t.kind)
    expect(tipos).toContain('light')
    expect(tipos).toContain('dark')
  })

  it('en los claros el texto es oscuro y el fondo claro', () => {
    const luz = (hex) => {
      const n = parseInt(hex.slice(1), 16)
      return (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255
    }
    for (const [k, t] of Object.entries(CATALOG)) {
      const lTexto = luz(t.v.text), lFondo = luz(t.v.bg)
      if (t.kind === 'light') {
        expect(lFondo, `${k}: el fondo deberia ser claro`).toBeGreaterThan(0.7)
        expect(lTexto, `${k}: el texto deberia ser oscuro`).toBeLessThan(0.35)
      } else {
        expect(lFondo, `${k}: el fondo deberia ser oscuro`).toBeLessThan(0.3)
        expect(lTexto, `${k}: el texto deberia ser claro`).toBeGreaterThan(0.6)
      }
      // contraste suficiente entre texto y fondo
      expect(Math.abs(lTexto - lFondo), `${k}: texto y fondo se confunden`).toBeGreaterThan(0.45)
    }
  })

  it('el acento contrasta con el panel en todos', () => {
    const luz = (hex) => {
      const n = parseInt(hex.slice(1), 16)
      return (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255
    }
    for (const [k, t] of Object.entries(CATALOG)) {
      expect(Math.abs(luz(t.v.accent) - luz(t.v.panel)), `${k}: el acento no se ve`).toBeGreaterThan(0.2)
    }
  })

  it('aplicar un tema escribe las variables en :root', () => {
    applyTheme('light')
    const r = document.documentElement
    expect(r.dataset.kind).toBe('light')
    expect(r.style.getPropertyValue('--bg')).toBe(CATALOG.light.v.bg)
    applyTheme('night')
    expect(r.dataset.kind).toBe('dark')
  })
})

// El tamaño multiplica letra y proporciones a la vez. Es independiente de la
// densidad: la densidad decide cuanto respira una fila, el tamaño cuanto se
// ve todo. Se combinan porque el css multiplica la base por la escala.
describe('tamaño de la app', () => {
  it('hay cuatro tamaños y el mediano es la escala neutra', () => {
    expect(Object.keys(SIZES)).toEqual(['small', 'medium', 'large', 'xlarge'])
    expect(SIZES.medium.scale).toBe(1)
  })

  it('van de menor a mayor sin repetirse', () => {
    const escalas = Object.values(SIZES).map(s => s.scale)
    expect(escalas).toEqual([...escalas].sort((a, b) => a - b))
    expect(new Set(escalas).size).toBe(escalas.length)
  })

  it('aplicar un tamaño escribe la escala en :root', () => {
    applySize('xlarge')
    expect(document.documentElement.style.getPropertyValue('--scale')).toBe('1.3')
    applySize('small')
    expect(document.documentElement.style.getPropertyValue('--scale')).toBe('0.9')
  })

  it('un tamaño que no existe cae en el mediano', () => {
    applySize('gigante')
    expect(document.documentElement.style.getPropertyValue('--scale')).toBe('1')
  })

  it('se recuerda entre sesiones', () => {
    applySize('large')
    expect(savedSize()).toBe('large')
  })

  it('la densidad deja la base en crudo, para poder multiplicarla', () => {
    applyDensity('compact')
    const r = document.documentElement.style
    expect(r.getPropertyValue('--table-font-base')).toBe('12px')
    // el tamaño no debe pisarla
    applySize('xlarge')
    expect(r.getPropertyValue('--table-font-base')).toBe('12px')
  })
})
