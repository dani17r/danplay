import { describe, it, expect } from 'vitest'
import { allCss } from './support/css.js'
import { readFileSync, readdirSync } from 'node:fs'
import {
  CATALOG,
  FIELDS,
  applyTheme,
  SIZES,
  applySize,
  savedSize,
  applyDensity
} from '../src/themes.js'

const css = allCss()

describe('colors come from the theme', () => {
  it('there are no fixed colors outside the variables', () => {
    const bad = []
    for (const [n, line] of css.split('\n').entries()) {
      const l = line.trim()
      // se permiten: definicion de variables, blancos/negros puros y transparencias
      if (l.startsWith('/*') || l.startsWith('*')) continue
      // se quitan las definiciones de variables: ahi el hex es legitimo
      const withoutVars = l.replace(/--[\w-]+\s*:\s*#[0-9a-fA-F]{3,8}/g, '')
      for (const m of withoutVars.matchAll(/#([0-9a-fA-F]{3,8})\b/g)) {
        const hex = m[1].toLowerCase()
        const onlyZeros = /^0+[0-9a-f]{0,2}$/.test(hex) // negro con alfa
        const onlyOnes = /^(fff|ffffff)([0-9a-f]{2})?$/.test(hex) // blanco con alfa
        if (!onlyZeros && !onlyOnes) bad.push(`style.css:${n + 1}  ${l.slice(0, 70)}`)
      }
    }
    expect(bad, 'usa var(--...) o color-mix():\n' + bad.join('\n')).toEqual([])
  })

  it('.vue components have no fixed colors either', () => {
    const views = [
      ...readdirSync('src/components')
        .filter((f) => f.endsWith('.vue'))
        .map((f) => 'src/components/' + f),
      ...readdirSync('src/components/ui').map((f) => 'src/components/ui/' + f),
      'src/App.vue'
    ]
    const bad = []
    for (const f of views) {
      for (const [n, line] of readFileSync(f, 'utf8').split('\n').entries()) {
        for (const m of line.matchAll(/#([0-9a-fA-F]{3,8})\b/g)) {
          const hex = m[1].toLowerCase()
          if (!/^0+[0-9a-f]{0,2}$/.test(hex) && !/^(fff|ffffff)([0-9a-f]{2})?$/.test(hex)) {
            bad.push(`${f}:${n + 1}  ${line.trim().slice(0, 60)}`)
          }
        }
      }
    }
    expect(bad, 'usa var(--...):\n' + bad.join('\n')).toEqual([])
  })
})

describe('theme catalog', () => {
  it('all of them define the 12 colors', () => {
    for (const [k, t] of Object.entries(CATALOG)) {
      for (const c of FIELDS) {
        expect(t.v[c.k], `${k} no define ${c.k}`).toMatch(/^#[0-9a-fA-F]{6}$/)
      }
    }
  })

  it('each theme declares whether it is light or dark', () => {
    for (const [k, t] of Object.entries(CATALOG)) {
      expect(['light', 'dark'], k).toContain(t.kind)
    }
  })

  it('there are light and dark themes', () => {
    const kinds = Object.values(CATALOG).map((t) => t.kind)
    expect(kinds).toContain('light')
    expect(kinds).toContain('dark')
  })

  it('in light ones the text is dark and the background light', () => {
    const lightness = (hex) => {
      const n = parseInt(hex.slice(1), 16)
      return (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255
    }
    for (const [k, t] of Object.entries(CATALOG)) {
      const lText = lightness(t.v.text),
        lBg = lightness(t.v.bg)
      if (t.kind === 'light') {
        expect(lBg, `${k}: el fondo deberia ser claro`).toBeGreaterThan(0.7)
        expect(lText, `${k}: el texto deberia ser oscuro`).toBeLessThan(0.35)
      } else {
        expect(lBg, `${k}: el fondo deberia ser oscuro`).toBeLessThan(0.3)
        expect(lText, `${k}: el texto deberia ser claro`).toBeGreaterThan(0.6)
      }
      // contraste suficiente entre texto y fondo
      expect(Math.abs(lText - lBg), `${k}: texto y fondo se confunden`).toBeGreaterThan(0.45)
    }
  })

  it('the accent contrasts with the panel in all of them', () => {
    const lightness = (hex) => {
      const n = parseInt(hex.slice(1), 16)
      return (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255
    }
    for (const [k, t] of Object.entries(CATALOG)) {
      expect(
        Math.abs(lightness(t.v.accent) - lightness(t.v.panel)),
        `${k}: el acento no se ve`
      ).toBeGreaterThan(0.2)
    }
  })

  it('applying a theme writes the variables on :root', () => {
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
describe('app size', () => {
  it('there are four sizes and medium is the neutral scale', () => {
    expect(Object.keys(SIZES)).toEqual(['small', 'medium', 'large', 'xlarge'])
    expect(SIZES.medium.scale).toBe(1)
  })

  it('they go from smallest to largest without repeats', () => {
    const scales = Object.values(SIZES).map((s) => s.scale)
    expect(scales).toEqual([...scales].sort((a, b) => a - b))
    expect(new Set(scales).size).toBe(scales.length)
  })

  it('applying a size writes the scale on :root', () => {
    applySize('xlarge')
    expect(document.documentElement.style.getPropertyValue('--scale')).toBe('1.3')
    applySize('small')
    expect(document.documentElement.style.getPropertyValue('--scale')).toBe('0.9')
  })

  it('an unknown size falls back to medium', () => {
    applySize('gigante')
    expect(document.documentElement.style.getPropertyValue('--scale')).toBe('1')
  })

  it('is remembered between sessions', () => {
    applySize('large')
    expect(savedSize()).toBe('large')
  })

  it('density leaves the base raw so it can be multiplied', () => {
    applyDensity('compact')
    const r = document.documentElement.style
    expect(r.getPropertyValue('--table-font-base')).toBe('12px')
    // el tamaño no debe pisarla
    applySize('xlarge')
    expect(r.getPropertyValue('--table-font-base')).toBe('12px')
  })
})

describe('preferences do not depend on who requested them first', () => {
  it('if the first component using them closes, changing the theme still applies', async () => {
    // Sus `watch` vivian en el ambito de ese componente: al cerrarlo (la onda
    // del modo estudio, por ejemplo) se paraban y el tema ya no se guardaba.
    const { mount } = await import('@vue/test-utils')
    const { defineComponent, h, nextTick } = await import('vue')
    const { usePreferences, resetPreferences } =
      await import('../src/composables/usePreferences.js')
    resetPreferences()
    localStorage.clear()
    const First = defineComponent({
      setup: () => {
        usePreferences()
        return () => h('div')
      }
    })
    mount(First).unmount()
    usePreferences().theme.value = 'ocean'
    usePreferences().layout.value = 'grid'
    await nextTick()
    expect(document.documentElement.dataset.theme).toBe('ocean')
    expect(localStorage.getItem('danplay.theme')).toBe('ocean')
    expect(localStorage.getItem('danplay.layout')).toBe('grid')
    resetPreferences()
    localStorage.clear()
  })
})
