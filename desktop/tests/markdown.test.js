import { describe, it, expect } from 'vitest'
import { renderMarkdown, escape, inline } from '../src/utils/markdown.js'

// El asistente contesta en markdown y la burbuja lo pintaba tal cual, con los
// asteriscos a la vista. Esto es lo minimo que tiene que entender, y lo que
// NUNCA puede dejar pasar: HTML de fuera.

describe('markdown del asistente: seguridad', () => {
  it('escapa cualquier HTML que venga en el texto', () => {
    const html = renderMarkdown('hola <script>alert(1)</script> <b>x</b>')
    expect(html).not.toContain('<script>')
    expect(html).not.toContain('<b>')
    expect(html).toContain('&lt;script&gt;')
  })

  it('no convierte los enlaces en <a>: la app no tiene con que abrirlos', () => {
    const html = renderMarkdown('mira [este video](https://youtu.be/abc?x=1&y=2)')
    expect(html).not.toContain('<a ')
    expect(html).toContain('class="md-link">este video</span>')
    // la direccion se ve al lado, escapada, para poder copiarla
    expect(html).toContain('https://youtu.be/abc?x=1&amp;y=2')
  })

  it('un enlace cuyo texto es la propia direccion no la repite', () => {
    const html = renderMarkdown('[https://youtu.be/abc](https://youtu.be/abc)')
    expect(html.match(/youtu\.be\/abc/g)).toHaveLength(1)
  })

  it('lo que va dentro de `codigo` no se interpreta', () => {
    expect(inline(escape('usa `**no_negrita**` aqui'))).toBe('usa <code>**no_negrita**</code> aqui')
  })
})

describe('markdown del asistente: lo que entiende', () => {
  it('negritas, cursivas y tachado', () => {
    expect(inline('**fuerte** y *suave* y ~~fuera~~')).toBe(
      '<strong>fuerte</strong> y <em>suave</em> y <del>fuera</del>'
    )
    expect(inline('__fuerte__ y _suave_')).toBe('<strong>fuerte</strong> y <em>suave</em>')
  })

  it('no rompe nombres con guiones bajos ni asteriscos sueltos', () => {
    expect(inline('snake_case_name y a*b*c')).toBe('snake_case_name y a*b*c')
  })

  it('titulos', () => {
    expect(renderMarkdown('### ¿Confirmas?')).toBe('<h3>¿Confirmas?</h3>')
    expect(renderMarkdown('# Grande')).toBe('<h1>Grande</h1>')
    expect(renderMarkdown('#sinespacio')).toBe('<p>#sinespacio</p>')
  })

  it('listas numeradas y con guion, con negritas dentro', () => {
    const html = renderMarkdown('1. **"Mi Gozo" – Barak**\n2. **Shekinah**')
    expect(html).toBe(
      '<ol><li><strong>&quot;Mi Gozo&quot; – Barak</strong></li><li><strong>Shekinah</strong></li></ol>'
    )
    expect(renderMarkdown('- uno\n- dos')).toBe('<ul><li>uno</li><li>dos</li></ul>')
    expect(renderMarkdown('• uno\n• dos')).toBe('<ul><li>uno</li><li>dos</li></ul>')
  })

  it('listas anidadas por sangria', () => {
    expect(renderMarkdown('- uno\n  - dentro\n- dos')).toBe(
      '<ul><li>uno<ul><li>dentro</li></ul></li><li>dos</li></ul>'
    )
  })

  it('una lista que no empieza en 1 conserva el numero', () => {
    expect(renderMarkdown('3. tres\n4. cuatro')).toBe(
      '<ol start="3"><li>tres</li><li>cuatro</li></ol>'
    )
  })

  it('parrafos: linea en blanco separa, salto simple es <br>', () => {
    expect(renderMarkdown('a\nb\n\nc')).toBe('<p>a<br>b</p><p>c</p>')
  })

  it('tablas', () => {
    const html = renderMarkdown('| Tema | Artista |\n|---|---|\n| Mi Gozo | Barak |')
    expect(html).toContain('<table>')
    expect(html).toContain('<th>Tema</th><th>Artista</th>')
    expect(html).toContain('<td>Mi Gozo</td><td>Barak</td>')
  })

  it('citas, reglas y bloques de codigo', () => {
    expect(renderMarkdown('> una cita')).toBe('<blockquote><p>una cita</p></blockquote>')
    expect(renderMarkdown('arriba\n\n---\n\nabajo')).toBe('<p>arriba</p><hr><p>abajo</p>')
    expect(renderMarkdown('```\n<x> & y\n```')).toBe('<pre><code>&lt;x&gt; &amp; y</code></pre>')
  })

  it('el texto de la captura sale entero y sin asteriscos', () => {
    const t =
      'Ya te pregunté si confirmas la descarga de:\n\n' +
      '1. **"I Want Jesus (Live)" – Bethel Music**\n' +
      '2. **"Ruja o Leão" – Carol Braga**\n\n' +
      'Y me dijiste **sí**.\n' +
      'Pero detecta que ya tienes una versión de *Ruja o Leão* (aunque no es la misma).\n\n' +
      '### ¿Confirmas de nuevo?\n' +
      '👉 **Sí** = bajo ambas.\n' +
      '👉 **No** = espero instrucciones.'
    const html = renderMarkdown(t)
    expect(html).not.toContain('*')
    expect(html).toContain('<h3>¿Confirmas de nuevo?</h3>')
    expect(html).toContain('<em>Ruja o Leão</em>')
    expect(html).toContain('👉 <strong>Sí</strong> = bajo ambas.<br>👉 <strong>No</strong>')
  })

  it('con nada, nada', () => {
    expect(renderMarkdown('')).toBe('')
    expect(renderMarkdown(null)).toBe('')
  })
})

// Por el chat entra texto de fuera. Nada de eso puede colgar la ventana: una
// expresion regular que retrocede tardaba casi dos segundos con 80 KB de
// negritas sin cerrar, y diez mil «>» seguidos agotaban la pila.
describe('markdown del asistente: texto hostil', () => {
  /** Lo ejecuta y comprueba que no tarda; el tope es holgado, antes eran segundos. */
  const rapido = (fn, ms = 250) => {
    const t0 = performance.now()
    const out = fn()
    expect(performance.now() - t0, 'tarda demasiado').toBeLessThan(ms)
    return out
  }

  it('diez mil «>» seguidos no se quedan sin pila', () => {
    const html = rapido(() => renderMarkdown('>'.repeat(10000)))
    expect(html.startsWith('<blockquote>')).toBe(true)
    // las citas se anidan hasta un tope; lo de mas alla se lee como texto
    expect(html.match(/<blockquote>/g).length).toBeLessThanOrEqual(8)
    expect(html).toContain('&gt;&gt;&gt;')
  })

  it('80 KB de negritas sin cerrar se leen en un momento', () => {
    for (const d of ['**', '__', '~~']) {
      const html = rapido(() => renderMarkdown(`${d}a `.repeat(20000)))
      expect(html).not.toMatch(/<(strong|del)>/)
    }
  })

  it('y las que si cierran se siguen marcando igual', () => {
    const t = '**uno** y **dos** '.repeat(2000)
    const html = rapido(() => renderMarkdown(t))
    expect(html.match(/<strong>/g)).toHaveLength(4000)
    expect(inline('***a**')).toBe('<strong>*a</strong>')
    expect(inline('** no ** es negrita')).toBe('** no ** es negrita')
  })

  it('una linea llena de corchetes sin cerrar tampoco se atasca', () => {
    rapido(() => renderMarkdown('['.repeat(80000)))
    const html = rapido(() => renderMarkdown('[a]'.repeat(20000) + '(http://x.y)'))
    expect(html.match(/md-link/g)).toHaveLength(1)
    rapido(() => renderMarkdown('[a](http://'.repeat(20000)))
  })

  it('ni un titulo con muchos espacios dentro', () => {
    const html = rapido(() => renderMarkdown('# a' + ' '.repeat(80000) + 'b'))
    expect(html.startsWith('<h1>a')).toBe(true)
    expect(html.endsWith('b</h1>')).toBe(true)
  })

  it('las listas muy anidadas se quedan en un tope', () => {
    const t = Array.from({ length: 3000 }, (_, i) => ' '.repeat(i * 2) + '- x').join('\n')
    const html = rapido(() => renderMarkdown(t))
    expect(html.match(/<ul>/g).length).toBeLessThanOrEqual(10)
  })
})

describe('markdown del asistente: titulos', () => {
  it('el cierre opcional de almohadillas se quita', () => {
    expect(renderMarkdown('## Hola ##')).toBe('<h2>Hola</h2>')
  })

  it('un titulo que acaba en sostenido no lo pierde', () => {
    // «Tono: C#» salia como «Tono: C»
    expect(renderMarkdown('## Tono: C#')).toBe('<h2>Tono: C#</h2>')
  })
})
