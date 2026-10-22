// Lo que WebKit (el motor de la app en Linux) no pinta y Chrome sí.
//
// Las pruebas de extremo a extremo van en Chrome, y Chrome pinta cosas que
// WebKitGTK no: la sombra de una fila de tabla (`<tr>`) es una. La raya de
// dónde cae una canción arrastrada y la marca de la fila elegida estaban en
// la sombra del <tr> y en la app de verdad no salían. Esto lo vigila en el CSS.
import { describe, it, expect } from 'vitest'
import { allCss } from './support/css.js'

/** Las reglas como [selectores, cuerpo]. */
function rules(css) {
  return [...css.matchAll(/([^{}@]+)\{([^{}]*)\}/g)].map((m) => [m[1].trim(), m[2]])
}

describe('what WebKit does not paint', () => {
  it('no shadow goes on a table row: it goes on its cells', () => {
    const bad = []
    for (const [selectors, body] of rules(allCss())) {
      if (!/(^|;)box-shadow:(?!none)/.test(body)) continue
      for (const sel of selectors.split(',')) {
        // el último elemento del selector es un <tr> (con o sin clases)
        const last =
          sel
            .trim()
            .split(/[\s>+~]+/)
            .pop() || ''
        if (/^tr(?=$|[.:[])/.test(last)) bad.push(sel.trim())
      }
    }
    expect(bad).toEqual([])
  })

  it('the selected row and the drop line show on the first cell, together', () => {
    const css = allCss()
    expect(css).toMatch(
      /tbody tr\.selected\s*>\s*td:first-child\{box-shadow:\s*inset 2px 0 0 var\(--accent\)\}/
    )
    expect(css).toMatch(
      /tbody tr\.selected\.drop-before\s*>\s*td:first-child\{box-shadow:\s*inset 2px/
    )
    expect(css).toMatch(/tr\.drop-after\s*>\s*td,\.row\.drop-after\{box-shadow:\s*inset 0 -2px 0/)
  })
})
