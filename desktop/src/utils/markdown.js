// @ts-check
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
//
// Y tiempo lineal: por aquí pasa texto de fuera, y una expresión regular que
// retrocede se puede quedar pensando segundos (o colgar la ventana) con una
// línea preparada a propósito. Ver `pairDelimiter`, `links` y los topes de
// anidamiento.

/** @type {Record<string, string>} */
const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }

/**
 * Texto plano → texto seguro dentro de HTML.
 * @param {unknown} s
 */
export function escape(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ESCAPES[c])
}

/** ¿Es un blanco (o el borde de la línea)? @param {string|undefined} c */
const blank = (c) => c === undefined || /\s/.test(c)

// ------------------------------------------------------------------ inline

/**
 * Empareja un delimitador doble (`**`, `__`, `~~`) en una sola pasada.
 *
 * Con la expresión perezosa de antes (`\*\*(?=\S)([\s\S]*?\S)\*\*`), cada
 * apertura sin cierre recorría el resto de la línea: 80 KB de «**a » sin
 * cerrar tardaban casi dos segundos. Aquí se recorren las apariciones una
 * vez y se decide igual que ella: abre la primera que va seguida de algo que
 * no es un blanco, y la cierra la siguiente que va precedida de algo que no
 * es un blanco y deja al menos un carácter dentro.
 * @param {string} s
 * @param {string} delim
 * @param {string} tag
 */
function pairDelimiter(s, delim, tag) {
  const n = delim.length
  let out = ''
  let copied = 0
  let open = -1
  let i = s.indexOf(delim)
  while (i !== -1) {
    if (open >= 0 && i > open + n && !blank(s[i - 1])) {
      out += s.slice(copied, open) + `<${tag}>` + s.slice(open + n, i) + `</${tag}>`
      copied = i + n
      open = -1
      i = s.indexOf(delim, i + n)
      continue
    }
    if (open < 0 && !blank(s[i + n])) {
      open = i
      i = s.indexOf(delim, i + n)
    } else {
      i = s.indexOf(delim, i + 1)
    }
  }
  return out + s.slice(copied)
}

/**
 * `[texto](https://…)`: el texto, y la dirección al lado si es distinta.
 *
 * A mano y de izquierda a derecha, por lo mismo que `pairDelimiter`: con la
 * expresión de antes, una línea llena de «[» sin cerrar hacía que cada uno
 * recorriera el resto. El texto va del primer «[» tras el último «]» hasta
 * el «]» siguiente (no puede contener otro «]»), y la dirección, de
 * `http(s)://` hasta el primer blanco o «)», que tiene que ser un «)».
 * @param {string} s  texto ya escapado
 */
function links(s) {
  if (!s.includes('](http')) return s // lo normal: ningún enlace
  const n = s.length
  // desde cada posición, dónde acaba lo que puede ser una dirección (el
  // siguiente blanco o «)»); se calcula una sola vez, de derecha a izquierda
  const stop = new Int32Array(n + 1)
  stop[n] = n
  for (let k = n - 1; k >= 0; k--) stop[k] = s[k] === ')' || /\s/.test(s[k]) ? k : stop[k + 1]
  let out = ''
  let copied = 0
  let open = -1
  for (let i = 0; i < n; i++) {
    const c = s[i]
    if (c === '[') {
      if (open < 0) open = i
      continue
    }
    if (c !== ']') continue
    const start = open
    open = -1 // un texto no puede cruzar un «]»
    if (start < 0 || start + 1 >= i || s[i + 1] !== '(') continue
    const u = i + 2
    const scheme = s.startsWith('https://', u) ? 8 : s.startsWith('http://', u) ? 7 : 0
    const end = stop[u]
    if (!scheme || end >= n || s[end] !== ')' || end - u <= scheme) continue
    const text = s.slice(start + 1, i)
    const url = s.slice(u, end)
    const shown = `<span class="md-link">${text}</span>`
    out +=
      s.slice(copied, start) +
      (text.trim() === url ? shown : `${shown} <span class="md-url">${url}</span>`)
    copied = end + 1
    i = end
  }
  return out + s.slice(copied)
}

/**
 * Negritas, cursivas, código, tachado y enlaces dentro de una línea ya
 * escapada. Los tramos de código se apartan primero para que lo que haya
 * dentro (asteriscos, guiones bajos) no se interprete.
 * @param {string} escaped  texto ya pasado por `escape`
 */
export function inline(escaped) {
  /** @type {string[]} */
  const codes = []
  let s = escaped.replace(/`([^`\n]+)`/g, (_, code) => {
    codes.push(`<code>${code}</code>`)
    return `${codes.length - 1}`
  })

  s = links(s)
  s = pairDelimiter(s, '**', 'strong')
  s = pairDelimiter(s, '__', 'strong')
  s = pairDelimiter(s, '~~', 'del')
  // cursiva con * en cualquier sitio; con _ solo entre espacios o al borde,
  // para no romper nombres_con_guiones. Estas dos no retroceden: lo de dentro
  // no puede contener el propio delimitador, así que cada apertura mira como
  // mucho hasta el siguiente.
  s = s.replace(/(^|[^*\w])\*(?=\S)([^*\n]*?\S)\*(?!\w)/g, '$1<em>$2</em>')
  s = s.replace(/(^|[\s(])_(?=\S)([^_\n]*?\S)_(?=$|[\s.,;:!?)])/g, '$1<em>$2</em>')

  return s.replace(/(\d+)/g, (_, i) => codes[Number(i)])
}

// ------------------------------------------------------------------ bloques

// Sin `\s*#*\s*$` al final: esa cola retrocedía sobre cada blanco y era
// cuadrática con una línea de muchos espacios. El cierre se quita a mano.
const RE_HEADING = /^(#{1,6})[ \t]+(.*)$/
const RE_RULE = /^\s*([-*_])(\s*\1){2,}\s*$/
const RE_QUOTE = /^\s*>\s?(.*)$/
const RE_UL = /^(\s*)([-*+•])\s+(.*)$/
const RE_OL = /^(\s*)(\d{1,3})[.)]\s+(.*)$/
const RE_FENCE = /^\s*```/
const RE_TABLE_SEP = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/

// Hasta dónde se anidan citas y listas. Más allá no aporta nada al leer, y
// 10.000 «>» seguidos eran 10.000 llamadas anidadas: el navegador se
// quedaba sin pila y la burbuja no se pintaba.
const MAX_QUOTE_DEPTH = 8
const MAX_LIST_DEPTH = 10

/** @param {string} line */
const hasPipe = (line) => /\|/.test(line)

/**
 * El texto de un título sin su cierre opcional de «#». Solo si va separado
 * por un blanco: «## Tono: C#» no pierde el sostenido.
 * @param {string} raw
 */
function headingText(raw) {
  const t = raw.trimEnd()
  let j = t.length
  while (j > 0 && t[j - 1] === '#') j--
  if (j < t.length && (j === 0 || blank(t[j - 1]))) return t.slice(0, j).trimEnd()
  return t
}

/**
 * Las celdas de una fila de tabla, sin las barras de los extremos.
 * @param {string} line
 */
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
  /** @type {{indent: number, ordered: boolean}[]} */
  const stack = []
  const close = () => {
    const top = stack.pop()
    html += `</li></${top?.ordered ? 'ol' : 'ul'}>`
  }
  for (const item of items) {
    while (stack.length && item.indent < stack[stack.length - 1].indent) close()
    const top = stack[stack.length - 1]
    if (!top || (item.indent > top.indent && stack.length < MAX_LIST_DEPTH)) {
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
 * @param {unknown} text
 * @returns {string}
 */
export function renderMarkdown(text) {
  return render(String(text ?? ''), 0)
}

/**
 * @param {string} text
 * @param {number} depth  cuántas citas hay por fuera
 */
function render(text, depth) {
  const lines = text.replace(/\r\n?/g, '\n').split('\n')
  /** @type {string[]} */
  const out = []
  let i = 0
  /** @type {string[]} */
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
      out.push(`<h${level}>${inline(escape(headingText(heading[2])))}</h${level}>`)
      i++
      continue
    }

    if (RE_RULE.test(line)) {
      flushParagraph()
      out.push('<hr>')
      i++
      continue
    }

    if (depth < MAX_QUOTE_DEPTH && RE_QUOTE.test(line)) {
      flushParagraph()
      const body = []
      while (i < lines.length && RE_QUOTE.test(lines[i])) {
        body.push(/** @type {RegExpMatchArray} */ (lines[i++].match(RE_QUOTE))[1])
      }
      out.push(`<blockquote>${render(body.join('\n'), depth + 1)}</blockquote>`)
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
        .map(
          (r) => `<tr>${head.map((_, k) => `<td>${inline(escape(r[k] ?? ''))}</td>`).join('')}</tr>`
        )
        .join('')
      out.push(
        `<div class="md-table"><table><thead><tr>${th}</tr></thead><tbody>${tr}</tbody></table></div>`
      )
      continue
    }

    paragraph.push(line)
    i++
  }
  flushParagraph()
  return out.join('')
}
