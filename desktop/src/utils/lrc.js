// Letras con tiempos (LRC): lo que da LRCLIB y lo que algunos mp3 llevan en
// su USLT. Cada línea va precedida de una o varias marcas `[mm:ss.xx]`.
//
// Se parsea aquí y no en el núcleo porque es cosa de pintar: la ficha
// enseña la línea que suena y deja saltar a una pulsándola.

const STAMP = /\[(\d{1,2}):(\d{2})(?:[.:](\d{1,3}))?\]/g

/**
 * @typedef {Object} LrcLine
 * @property {number} t     segundos desde el principio
 * @property {string} text
 */

/**
 * Las líneas con su tiempo, ordenadas. `null` si el texto no es LRC (menos
 * de dos marcas: una sola suele ser un resto, no una letra sincronizada).
 * @param {string} text
 * @returns {LrcLine[]|null}
 */
export function parseLrc(text) {
  if (!text || typeof text !== 'string') return null
  const lines = []
  for (const raw of text.split(/\r?\n/)) {
    const stamps = []
    let m
    let end = 0
    STAMP.lastIndex = 0
    while ((m = STAMP.exec(raw))) {
      const ms = m[3] ? Number((m[3] + '00').slice(0, 3)) : 0
      stamps.push(Number(m[1]) * 60 + Number(m[2]) + ms / 1000)
      end = STAMP.lastIndex
    }
    if (!stamps.length) continue
    const t = raw.slice(end).trim()
    for (const s of stamps) lines.push({ t: s, text: t })
  }
  if (lines.length < 2) return null
  lines.sort((a, b) => a.t - b.t)
  return lines
}

/**
 * La letra sin marcas de tiempo, para leerla o copiarla.
 * @param {string} text
 */
export function stripLrc(text) {
  if (!text) return ''
  return text
    .split(/\r?\n/)
    // fuera las marcas de tiempo y las de cabecera ([ar:…], [ti:…], [offset:…])
    .filter((l) => !/^\s*\[[a-z]+:[^\]]*\]\s*$/i.test(l))
    .map((l) => l.replace(/\[\d{1,2}:\d{2}(?:[.:]\d{1,3})?\]/g, '').trim())
    .filter((l, i, all) => l || (i > 0 && all[i - 1]))
    .join('\n')
    .trim()
}

/**
 * Qué línea suena en ese segundo: la última cuyo tiempo ya pasó. -1 antes
 * de la primera.
 * @param {LrcLine[]} lines
 * @param {number} position
 */
export function currentLine(lines, position) {
  let i = -1
  for (let k = 0; k < lines.length; k++) {
    if (lines[k].t <= position + 0.05) i = k
    else break
  }
  return i
}
