// Markdown → HTML para las burbujas del asistente.
//
// El modelo contesta en markdown (negritas, listas, títulos, tablas) y la
// burbuja lo pintaba tal cual, con los asteriscos a la vista. Esto lo
// convierte a HTML sin traer ninguna biblioteca: es un subconjunto pequeño y
// suficiente para lo que devuelve un asistente de música.
//
// Seguridad: TODO el texto pasa por `escape` antes de que se marque nada.
// Por el chat entra texto escrito por terceros —títulos de YouTube, letras,
// resúmenes de páginas— y no puede colar ni una etiqueta. Los enlaces no se
// convierten en <a>: la app no tiene con qué abrirlos fuera, y un enlace
// dentro del WebView se llevaría la interfaz entera a otra página. Se
// enseñan como texto, con la dirección al lado para poder copiarla.

const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }

/**
 * Texto plano → texto seguro dentro de HTML.
 * @param {string} s
 */
export function escape(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ESCAPES[c])
}

// ------------------------------------------------------------------ inline

/**
 * Negritas, cursivas, código, tachado y enlaces dentro de una línea ya
 * escapada. Los tramos de código se apartan primero para que lo que haya
 * dentro (asteriscos, guiones bajos) no se interprete.
 * @param {string} escaped  texto ya pasado por `escape`
 */
export function inline(escaped) {
  const codes = []
  let s = escaped.replace(/`([^`\n]+)`/g, (_, code) => {
    codes.push(`<code>${code}</code>`)
    return `\uE000${codes.length - 1}\uE001`
  })

  // [texto](https://…): el texto, y la dirección al lado si es distinta
  s = s.replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g, (_, text, url) => {
    const shown = `<span class="md-link">${text}</span>`
    return text.trim() === url ? shown : `${shown} <span class="md-url">${url}</span>`
  })

  s = s.replace(/\*\*(?=\S)([\s\S]*?\S)\*\*/g, '<strong>$1</strong>')
  s = s.replace(/__(?=\S)([\s\S]*?\S)__/g, '<strong>$1</strong>')
  s = s.replace(/~~(?=\S)([\s\S]*?\S)~~/g, '<del>$1</del>')
  // cursiva con * en cualquier sitio; con _ solo entre espacios o al borde,
  // para no romper nombres_con_guiones
  s = s.replace(/(^|[^*\w])\*(?=\S)([^*\n]*?\S)\*(?!\w)/g, '$1<em>$2</em>')
  s = s.replace(/(^|[\s(])_(?=\S)([^_\n]*?\S)_(?=$|[\s.,;:!?)])/g, '$1<em>$2</em>')

  return s.replace(/\uE000(\d+)\uE001/g, (_, i) => codes[Number(i)])
}

// ------------------------------------------------------------------ bloques

const RE_HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/
const RE_RULE = /^\s*([-*_])(\s*\1){2,}\s*$/
const RE_QUOTE = /^\s*>\s?(.*)$/
const RE_UL = /^(\s*)([-*+•])\s+(.*)$/
const RE_OL = /^(\s*)(\d{1,3})[.)]\s+(.*)$/
const RE_FENCE = /^\s*```/
const RE_TABLE_SEP = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/

const hasPipe = (line) => /\|/.test(line)

/** Las celdas de una fila de tabla, sin las barras de los extremos. */
function cells(line) {
  let s = line.trim()
  if (s.startsWith('|')) s = s.slice(1)
  if (s.endsWith('|')) s = s.slice(0, -1)
  return s.split('|').map((c) => c.trim())
}

/**
 * Una lista con sus niveles. Cada elemento lleva la sangría con la que se
 * escribió; una sangría mayor que la del anterior abre una sublista.
 * @param {{indent:number, ordered:boolean, text:string, start:number}[]} items
 */
function renderList(items) {
  let html = ''
  const stack = [] // [{indent, ordered}]
  const close = () => {
    const top = stack.pop()
    html += `</li></${top.ordered ? 'ol' : 'ul'}>`
  }
  for (const item of items) {
    while (stack.length && item.indent < stack[stack.length - 1].indent) close()
    const top = stack[stack.length - 1]
    if (!top || item.indent > top.indent) {
      const tag = item.ordered ? 'ol' : 'ul'
      const start = item.ordered && item.start > 1 ? ` start="${item.start}"` : ''
      html += `<${tag}${start}><li>${inline(escape(item.text))}`
      stack.push({ indent: item.indent, ordered: item.ordered })
    } else if (top.ordered !== item.ordered) {
      // cambia el tipo de lista al mismo nivel: se cierra y se abre otra
      close()
      const tag = item.ordered ? 'ol' : 'ul'
      html += `<${tag}><li>${inline(escape(item.text))}`
      stack.push({ indent: item.indent, ordered: item.ordered })
    } else {
      html += `</li><li>${inline(escape(item.text))}`
    }
  }
  while (stack.length) close()
  return html
}

/**
 * Markdown → HTML seguro.
 * @param {string} text
 * @returns {string}
 */
export function renderMarkdown(text) {
  const lines = String(text ?? '')
    .replace(/\r\n?/g, '\n')
    .split('\n')
  const out = []
  let i = 0
  let paragraph = []

  const flushParagraph = () => {
    if (!paragraph.length) return
    out.push(`<p>${paragraph.map((l) => inline(escape(l))).join('<br>')}</p>`)
    paragraph = []
  }

  while (i < lines.length) {
    const line = lines[i]

    if (!line.trim()) {
      flushParagraph()
      i++
      continue
    }

    if (RE_FENCE.test(line)) {
      flushParagraph()
      const body = []
      i++
      while (i < lines.length && !RE_FENCE.test(lines[i])) body.push(lines[i++])
      i++ // la valla de cierre, si la hay
      out.push(`<pre><code>${escape(body.join('\n'))}</code></pre>`)
      continue
    }

    const heading = line.match(RE_HEADING)
    if (heading) {
      flushParagraph()
      const level = heading[1].length
      out.push(`<h${level}>${inline(escape(heading[2]))}</h${level}>`)
      i++
      continue
    }

    if (RE_RULE.test(line)) {
      flushParagraph()
      out.push('<hr>')
      i++
      continue
    }

    if (RE_QUOTE.test(line)) {
      flushParagraph()
      const body = []
      while (i < lines.length && RE_QUOTE.test(lines[i])) body.push(lines[i++].match(RE_QUOTE)[1])
      out.push(`<blockquote>${renderMarkdown(body.join('\n'))}</blockquote>`)
      continue
    }

    if (RE_UL.test(line) || RE_OL.test(line)) {
      flushParagraph()
      const items = []
      while (i < lines.length) {
        const ul = lines[i].match(RE_UL)
        const ol = ul ? null : lines[i].match(RE_OL)
        if (ul) {
          items.push({ indent: ul[1].length, ordered: false, text: ul[3], start: 1 })
        } else if (ol) {
          items.push({ indent: ol[1].length, ordered: true, text: ol[3], start: Number(ol[2]) })
        } else if (lines[i].trim() && /^\s{2,}/.test(lines[i]) && items.length) {
          // continuación sangrada del elemento anterior
          items[items.length - 1].text += ' ' + lines[i].trim()
        } else {
          break
        }
        i++
      }
      out.push(renderList(items))
      continue
    }

    if (hasPipe(line) && i + 1 < lines.length && RE_TABLE_SEP.test(lines[i + 1])) {
      flushParagraph()
      const head = cells(line)
      i += 2
      const rows = []
      while (i < lines.length && hasPipe(lines[i]) && lines[i].trim()) rows.push(cells(lines[i++]))
      const th = head.map((c) => `<th>${inline(escape(c))}</th>`).join('')
      const tr = rows
        .map((r) => `<tr>${head.map((_, k) => `<td>${inline(escape(r[k] ?? ''))}</td>`).join('')}</tr>`)
        .join('')
      out.push(`<div class="md-table"><table><thead><tr>${th}</tr></thead><tbody>${tr}</tbody></table></div>`)
      continue
    }

    paragraph.push(line)
    i++
  }
  flushParagraph()
  return out.join('')
}
