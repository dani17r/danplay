import { describe, it, expect } from 'vitest'
import { renderMarkdown, escape, inline } from '../src/utils/markdown.js'

// El asistente contesta en markdown y la burbuja lo pintaba tal cual, con los
// asteriscos a la vista. Esto es lo minimo que tiene que entender, y lo que
// NUNCA puede dejar pasar: HTML de fuera.

describe('assistant markdown: safety', () => {
  it('escapes any HTML in the text', () => {
    const html = renderMarkdown('hola <script>alert(1)</script> <b>x</b>')
    expect(html).not.toContain('<script>')
    expect(html).not.toContain('<b>')
    expect(html).toContain('&lt;script&gt;')
  })

  it('does not turn links into <a>: the app has nothing to open them with', () => {
    const html = renderMarkdown('mira [este video](https://youtu.be/abc?x=1&y=2)')
    expect(html).not.toContain('<a ')
    expect(html).toContain('class="md-link">este video</span>')
    // la direccion se ve al lado, escapada, para poder copiarla
    expect(html).toContain('https://youtu.be/abc?x=1&amp;y=2')
  })

  it('a link whose text is the URL itself does not repeat it', () => {
    const html = renderMarkdown('[https://youtu.be/abc](https://youtu.be/abc)')
    expect(html.match(/youtu\.be\/abc/g)).toHaveLength(1)
  })

  it('what is inside `code` is not interpreted', () => {
    expect(inline(escape('usa `**no_negrita**` aqui'))).toBe('usa <code>**no_negrita**</code> aqui')
  })
})

describe('assistant markdown: what it understands', () => {
  it('bold, italic and strikethrough', () => {
    expect(inline('**fuerte** y *suave* y ~~fuera~~')).toBe(
      '<strong>fuerte</strong> y <em>suave</em> y <del>fuera</del>'
    )
    expect(inline('__fuerte__ y _suave_')).toBe('<strong>fuerte</strong> y <em>suave</em>')
  })

  it('does not break names with underscores or stray asterisks', () => {
    expect(inline('snake_case_name y a*b*c')).toBe('snake_case_name y a*b*c')
  })

  it('headings', () => {
    expect(renderMarkdown('### ¿Confirmas?')).toBe('<h3>¿Confirmas?</h3>')
    expect(renderMarkdown('# Grande')).toBe('<h1>Grande</h1>')
    expect(renderMarkdown('#sinespacio')).toBe('<p>#sinespacio</p>')
  })

  it('numbered and dash lists, with bold inside', () => {
    const html = renderMarkdown('1. **"Mi Gozo" – Barak**\n2. **Shekinah**')
    expect(html).toBe(
      '<ol><li><strong>&quot;Mi Gozo&quot; – Barak</strong></li><li><strong>Shekinah</strong></li></ol>'
    )
    expect(renderMarkdown('- uno\n- dos')).toBe('<ul><li>uno</li><li>dos</li></ul>')
    expect(renderMarkdown('• uno\n• dos')).toBe('<ul><li>uno</li><li>dos</li></ul>')
  })

  it('lists nested by indentation', () => {
    expect(renderMarkdown('- uno\n  - dentro\n- dos')).toBe(
      '<ul><li>uno<ul><li>dentro</li></ul></li><li>dos</li></ul>'
    )
  })

  it('a list that does not start at 1 keeps its number', () => {
    expect(renderMarkdown('3. tres\n4. cuatro')).toBe(
      '<ol start="3"><li>tres</li><li>cuatro</li></ol>'
    )
  })

  it('paragraphs: a blank line separates, a single break is <br>', () => {
    expect(renderMarkdown('a\nb\n\nc')).toBe('<p>a<br>b</p><p>c</p>')
  })

  it('tables', () => {
    const html = renderMarkdown('| Tema | Artista |\n|---|---|\n| Mi Gozo | Barak |')
    expect(html).toContain('<table>')
    expect(html).toContain('<th>Tema</th><th>Artista</th>')
    expect(html).toContain('<td>Mi Gozo</td><td>Barak</td>')
  })

  it('quotes, rules and code blocks', () => {
    expect(renderMarkdown('> una cita')).toBe('<blockquote><p>una cita</p></blockquote>')
    expect(renderMarkdown('arriba\n\n---\n\nabajo')).toBe('<p>arriba</p><hr><p>abajo</p>')
    expect(renderMarkdown('```\n<x> & y\n```')).toBe('<pre><code>&lt;x&gt; &amp; y</code></pre>')
  })

  it('the screenshot text comes out whole and without asterisks', () => {
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

  it('nothing in, nothing out', () => {
    expect(renderMarkdown('')).toBe('')
    expect(renderMarkdown(null)).toBe('')
  })
})

// Por el chat entra texto de fuera. Nada de eso puede colgar la ventana: una
// expresion regular que retrocede tardaba casi dos segundos con 80 KB de
// negritas sin cerrar, y diez mil «>» seguidos agotaban la pila.
describe('assistant markdown: hostile text', () => {
  /** Lo ejecuta y comprueba que no tarda; el tope es holgado, antes eran segundos. */
  const quick = (fn, ms = 250) => {
    const t0 = performance.now()
    const out = fn()
    expect(performance.now() - t0, 'tarda demasiado').toBeLessThan(ms)
    return out
  }

  it('ten thousand «>» in a row do not run out of stack', () => {
    const html = quick(() => renderMarkdown('>'.repeat(10000)))
    expect(html.startsWith('<blockquote>')).toBe(true)
    // las citas se anidan hasta un tope; lo de mas alla se lee como texto
    expect(html.match(/<blockquote>/g).length).toBeLessThanOrEqual(8)
    expect(html).toContain('&gt;&gt;&gt;')
  })

  it('80 KB of unclosed bold parse in a moment', () => {
    for (const d of ['**', '__', '~~']) {
      const html = quick(() => renderMarkdown(`${d}a `.repeat(20000)))
      expect(html).not.toMatch(/<(strong|del)>/)
    }
  })

  it('and the ones that do close are still marked the same', () => {
    const t = '**uno** y **dos** '.repeat(2000)
    const html = quick(() => renderMarkdown(t))
    expect(html.match(/<strong>/g)).toHaveLength(4000)
    expect(inline('***a**')).toBe('<strong>*a</strong>')
    expect(inline('** no ** es negrita')).toBe('** no ** es negrita')
  })

  it('a line full of unclosed brackets does not get stuck either', () => {
    quick(() => renderMarkdown('['.repeat(80000)))
    const html = quick(() => renderMarkdown('[a]'.repeat(20000) + '(http://x.y)'))
    expect(html.match(/md-link/g)).toHaveLength(1)
    quick(() => renderMarkdown('[a](http://'.repeat(20000)))
  })

  it('nor does a heading with many spaces inside', () => {
    const html = quick(() => renderMarkdown('# a' + ' '.repeat(80000) + 'b'))
    expect(html.startsWith('<h1>a')).toBe(true)
    expect(html.endsWith('b</h1>')).toBe(true)
  })

  it('deeply nested lists stop at a cap', () => {
    const t = Array.from({ length: 3000 }, (_, i) => ' '.repeat(i * 2) + '- x').join('\n')
    const html = quick(() => renderMarkdown(t))
    expect(html.match(/<ul>/g).length).toBeLessThanOrEqual(10)
  })
})

describe('assistant markdown: headings', () => {
  it('the optional closing hashes are removed', () => {
    expect(renderMarkdown('## Hola ##')).toBe('<h2>Hola</h2>')
  })

  it('a heading ending in a sharp sign keeps it', () => {
    // «Tono: C#» salia como «Tono: C»
    expect(renderMarkdown('## Tono: C#')).toBe('<h2>Tono: C#</h2>')
  })
})
