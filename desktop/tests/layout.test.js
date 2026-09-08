import { describe, it, expect } from 'vitest'
import { allCss } from './support/css.js'
import { readFileSync } from 'node:fs'

const css = allCss()
// sin comentarios: si no, se cuelan en la captura del selector
const limpio = css.replace(/\/\*[\s\S]*?\*\//g, '')
const reglas = [...limpio.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(m => ({ sel: m[1].trim(), body: m[2].replace(/\s+/g, '') }))
const paraSelector = (s) => reglas.filter(r => r.sel.split(',').some(x => x.trim() === s))
const ultimoValor = (s, prop) => {
  const vals = paraSelector(s).flatMap(r =>
    [...r.body.matchAll(new RegExp(`(?:^|;)${prop}:([^;]+)`, 'g'))].map(m => m[1]))
  return vals.at(-1)
}

describe('nada debe provocar desplazamiento horizontal', () => {
  it('los controles en linea no son flex de bloque', () => {
    // display:flex convierte el elemento en bloque: ocupa todo el ancho
    // y parte la maquetacion. Deben ser inline-flex.
    for (const sel of ['.btn', '.chip', '.badge', '.speed']) {
      expect(ultimoValor(sel, 'display'), `${sel} deberia ser inline-flex`).toBe('inline-flex')
    }
  })

  it('el cuerpo no desplaza en horizontal', () => {
    expect(css).toMatch(/html,body,#app\{[^}]*overflow:hidden/)
  })

  it('la tabla se adapta sola, y solo desborda si tu la ensanchas', () => {
    expect(ultimoValor('table', 'table-layout')).toBe('fixed')
    // `auto` y no `hidden`: mientras las columnas quepan no sale ninguna barra
    // —la tabla se reparte el ancho—, pero desde que las columnas se pueden
    // ensanchar a mano hay que poder llegar a las de la derecha. Con `hidden`
    // se quedaban recortadas sin salida.
    expect(ultimoValor('.table-wrap', 'overflow-x')).toBe('auto')
    // y sin tocar nada la tabla no se pasa: solo crece con anchos a medida
    expect(css).toMatch(/\.song-table\.medida\{[^}]*width:auto/)
  })

  it('las celdas recortan con puntos suspensivos', () => {
    const th = paraSelector('th').concat(paraSelector('td'))
    expect(th.some(r => r.body.includes('text-overflow:ellipsis'))).toBe(true)
  })

  it('las zonas principales pueden encogerse', () => {
    for (const sel of ['.app', '.main', '.center']) {
      expect(ultimoValor(sel, 'min-width'), `${sel} necesita min-width:0`).toBe('0')
    }
  })

  it('los paneles flotantes se limitan al ancho de la ventana', () => {
    expect(ultimoValor('.queue', 'max-width')).toContain('100vw')
    expect(ultimoValor('.toasts', 'max-width')).toContain('100vw')
  })

  it('hay puntos de ruptura para ventanas estrechas', () => {
    const medias = [...css.matchAll(/@media\s*\(max-width:\s*(\d+)px\)/g)].map(m => Number(m[1]))
    expect(medias.length, 'faltan media queries').toBeGreaterThan(4)
    expect(Math.min(...medias)).toBeLessThanOrEqual(1000)
  })

  it('las columnas secundarias se esconden antes que las importantes', () => {
    const sort = ['.col-kbps', '.col-bpm', '.col-key', '.col-stars']
    const anchos = sort.map(c => {
      const m = css.match(new RegExp(`@media\\s*\\(max-width:\\s*(\\d+)px\\)\\{\\s*\\${c}\\{display:none`))
      return m ? Number(m[1]) : null
    })
    expect(anchos.every(a => a !== null), 'todas deben tener su punto de corte').toBe(true)
    for (let i = 1; i < anchos.length; i++) {
      expect(anchos[i], `${sort[i]} deberia esconderse despues que ${sort[i - 1]}`)
        .toBeLessThan(anchos[i - 1])
    }
  })
})

// La fila seleccionada llevaba class="sel", el mismo name que usaba el boton
// del desplegable. La regla global `.sel{display:flex;height:36px}` convertia la
// fila en una caja flex, se salia de la maquetacion fija de la tabla y sus
// celdas se encogian hasta quedar en una letra. Que no vuelva a pasar.
describe('las clases de estado de las filas no chocan con los widgets', () => {
  const marcado = ['src/components/SongTable.vue', 'src/components/SongGrid.vue']
    .map(f => readFileSync(f, 'utf8')).join('\n')
  const estados = [...new Set(
    [...marcado.matchAll(/:class="\{([^}]*)\}"/g)]
      .flatMap(m => [...m[1].matchAll(/([a-zA-Z][\w-]*)\s*:/g)].map(x => x[1])))]

  it('se encuentran las clases de estado del marcado', () => {
    expect(estados).toContain('selected')
    expect(estados.length).toBeGreaterThan(1)
  })

  it('ninguna se estiliza como si fuera una caja suelta', () => {
    for (const c of estados) {
      for (const prop of ['display', 'height', 'width', 'padding', 'border-radius']) {
        expect(ultimoValor('.' + c, prop),
          `.${c} es un estado de fila: una regla global no debe fijarle ${prop}`)
          .toBeUndefined()
      }
    }
  })
})
