import { describe, it, expect } from 'vitest'
import { allCss } from './support/css.js'
import { readFileSync } from 'node:fs'

const css = allCss()
// sin comentarios: si no, se cuelan en la captura del selector
const clean = css.replace(/\/\*[\s\S]*?\*\//g, '')
const rules = [...clean.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({
  sel: m[1].trim(),
  body: m[2].replace(/\s+/g, '')
}))
const forSelector = (s) => rules.filter((r) => r.sel.split(',').some((x) => x.trim() === s))
const lastValue = (s, prop) => {
  const vals = forSelector(s).flatMap((r) =>
    [...r.body.matchAll(new RegExp(`(?:^|;)${prop}:([^;]+)`, 'g'))].map((m) => m[1])
  )
  return vals.at(-1)
}

describe('nothing should cause horizontal scrolling', () => {
  it('inline controls are not block flex', () => {
    // display:flex convierte el elemento en bloque: ocupa todo el ancho
    // y parte la maquetacion. Deben ser inline-flex.
    for (const sel of ['.btn', '.chip', '.badge', '.speed']) {
      expect(lastValue(sel, 'display'), `${sel} deberia ser inline-flex`).toBe('inline-flex')
    }
  })

  it('the body does not scroll horizontally', () => {
    expect(css).toMatch(/html,body,#app\{[^}]*overflow:hidden/)
  })

  it('the table adapts on its own, and only overflows if you widen it', () => {
    expect(lastValue('table', 'table-layout')).toBe('fixed')
    // `auto` y no `hidden`: mientras las columnas quepan no sale ninguna barra
    // —la tabla se reparte el ancho—, pero desde que las columnas se pueden
    // ensanchar a mano hay que poder llegar a las de la derecha. Con `hidden`
    // se quedaban recortadas sin salida.
    expect(lastValue('.table-wrap', 'overflow-x')).toBe('auto')
    // y sin tocar nada la tabla no se pasa: solo crece con anchos a medida
    expect(css).toMatch(/\.song-table\.sized\{[^}]*width:auto/)
  })

  it('cells truncate with an ellipsis', () => {
    const th = forSelector('th').concat(forSelector('td'))
    expect(th.some((r) => r.body.includes('text-overflow:ellipsis'))).toBe(true)
  })

  it('the main areas can shrink', () => {
    for (const sel of ['.app', '.main', '.center']) {
      expect(lastValue(sel, 'min-width'), `${sel} necesita min-width:0`).toBe('0')
    }
  })

  it('floating panels are limited to the window width', () => {
    expect(lastValue('.queue', 'max-width')).toContain('100vw')
    expect(lastValue('.toasts', 'max-width')).toContain('100vw')
  })

  it('there are breakpoints for narrow windows', () => {
    const medias = [...css.matchAll(/@media\s*\(max-width:\s*(\d+)px\)/g)].map((m) => Number(m[1]))
    expect(medias.length, 'faltan media queries').toBeGreaterThan(4)
    expect(Math.min(...medias)).toBeLessThanOrEqual(1000)
  })

  it('secondary columns hide before the important ones', () => {
    const sort = ['.col-kbps', '.col-bpm', '.col-key', '.col-stars']
    const widths = sort.map((c) => {
      const m = css.match(
        new RegExp(`@media\\s*\\(max-width:\\s*(\\d+)px\\)\\{\\s*\\${c}\\{display:none`)
      )
      return m ? Number(m[1]) : null
    })
    expect(
      widths.every((a) => a !== null),
      'todas deben tener su punto de corte'
    ).toBe(true)
    for (let i = 1; i < widths.length; i++) {
      expect(widths[i], `${sort[i]} deberia esconderse despues que ${sort[i - 1]}`).toBeLessThan(
        widths[i - 1]
      )
    }
  })
})

// La fila seleccionada llevaba class="sel", el mismo name que usaba el boton
// del desplegable. La regla global `.sel{display:flex;height:36px}` convertia la
// fila en una caja flex, se salia de la maquetacion fija de la tabla y sus
// celdas se encogian hasta quedar en una letra. Que no vuelva a pasar.
describe('row state classes do not clash with widgets', () => {
  const marked = ['src/components/SongTable.vue', 'src/components/SongGrid.vue']
    .map((f) => readFileSync(f, 'utf8'))
    .join('\n')
  const states = [
    ...new Set(
      [...marked.matchAll(/:class="\{([^}]*)\}"/g)].flatMap((m) =>
        [...m[1].matchAll(/([a-zA-Z][\w-]*)\s*:/g)].map((x) => x[1])
      )
    )
  ]

  it('the state classes are found in the markup', () => {
    expect(states).toContain('selected')
    expect(states.length).toBeGreaterThan(1)
  })

  it('none is styled as if it were a standalone box', () => {
    for (const c of states) {
      for (const prop of ['display', 'height', 'width', 'padding', 'border-radius']) {
        expect(
          lastValue('.' + c, prop),
          `.${c} es un estado de fila: una regla global no debe fijarle ${prop}`
        ).toBeUndefined()
      }
    }
  })
})
