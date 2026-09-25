// @ts-check
// Tonos: transponer el nombre de una tonalidad por semitonos, y contar el
// corrimiento en tonos como lo dice un músico (medio tono, un tono y medio).
//
// Para el modo estudio: si la canción está en G y se sube un tono, se
// enseña «G → A». Solo el nombre; los acordes de la ficha los transpone el
// núcleo (`api.transpose`), que sabe de cejillas y grados.

const SHARPS = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B']
const FLATS = ['C', 'Db', 'D', 'Eb', 'E', 'F', 'Gb', 'G', 'Ab', 'A', 'Bb', 'B']
// cómo se escribe cada tono cuando no hay una preferencia clara
const SPELL_MAJOR = { 1: 'Db', 3: 'Eb', 6: 'F#', 8: 'Ab', 10: 'Bb' }
const SPELL_MINOR = { 1: 'C#', 3: 'Eb', 6: 'F#', 8: 'G#', 10: 'Bb' }

/**
 * Parte un tono en (índice cromático, ¿menor?, resto): «F#m» → [6, true, ''].
 * @param {string} key
 * @returns {[number, boolean, string] | null}
 */
export function parseKey(key) {
  // la nota, su alteración, «m»/«min»/«menor» si es menor, y lo que sobre
  // (un «/G» del bajo, un «maj7»…); una palabra suelta no es un tono
  const m = String(key || '')
    .trim()
    .match(/^([A-Ga-g])([#b♯♭]?)\s*(m(?![a-z])|min\b|menor\b|-)?(?![a-z])(.*)$/)
  if (!m) return null
  let index = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 }[m[1].toUpperCase()]
  if (m[2] === '#' || m[2] === '♯') index += 1
  if (m[2] === 'b' || m[2] === '♭') index -= 1
  return [(index + 12) % 12, !!m[3], (m[4] || '').trim()]
}

/**
 * El tono corrido `semitones` (positivo sube). Devuelve '' si no se entiende.
 *
 * Con un cuarto de tono (medio semitono) no hay nombre de tonalidad: se dice
 * de cuál se parte y cuánto se corre, «G +¼» o «F# −¼».
 * @param {string} key @param {number} semitones
 */
export function transposeKey(key, semitones) {
  const parsed = parseKey(key)
  if (!parsed) return ''
  const [index, minor, rest] = parsed
  const n = Math.round((Number(semitones) || 0) * 100) / 100
  const whole = Math.trunc(n)
  const target = (((index + whole) % 12) + 12) % 12
  const flatish = /b|♭/.test(String(key))
  const sharpish = /#|♯/.test(String(key))
  let name
  if (flatish) name = FLATS[target]
  else if (sharpish) name = SHARPS[target]
  else name = (minor ? SPELL_MINOR : SPELL_MAJOR)[target] || SHARPS[target]
  const off = n - whole
  return (
    name +
    (minor ? 'm' : '') +
    (rest ? ' ' + rest : '') +
    (Math.abs(off) >= 0.01 ? ' ' + toneLabel(off) : '')
  )
}

/** «+2» / «−3» / «0», para enseñar el corrimiento. */
export function semitoneLabel(n) {
  if (!n) return '0'
  return (n > 0 ? '+' : '−') + Math.abs(n)
}

// Las fracciones de tono que tienen su signo.
const FRACTIONS = { 25: '¼', 50: '½', 75: '¾' }

/**
 * El corrimiento en tonos, como lo cuenta un músico: medio tono es un
 * semitono y un tono son dos. «+½», «−1», «+1½», «+¼», «0».
 * @param {number} semitones
 */
export function toneLabel(semitones) {
  const hundredths = Math.round(Math.abs(Number(semitones) || 0) * 50) // tonos × 100
  if (!hundredths) return '0'
  const sign = Number(semitones) > 0 ? '+' : '−'
  const whole = Math.floor(hundredths / 100)
  const part = hundredths % 100
  if (!part) return sign + whole
  if (FRACTIONS[part]) return sign + (whole || '') + FRACTIONS[part]
  return sign + hundredths / 100
}

/** «tono» o «tonos»: hasta uno, en singular (medio tono, un tono). */
export function toneUnit(semitones) {
  return Math.abs(Number(semitones) || 0) > 2 ? 'tonos' : 'tono'
}
