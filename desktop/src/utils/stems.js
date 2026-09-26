// @ts-check
// Las pistas separadas de una canción en el mezclador del estudio: qué suena
// de cada una, y cómo se llama la mezcla que se guarda.

/**
 * @typedef {Object} StemInfo  Una pista tal como la cuenta el núcleo.
 * @property {string} source   drums, vocals, bass, guitar, piano, other
 * @property {string} name     Batería, Voces…
 * @property {string} path
 * @property {{peaks: number[], rms: number[]}|null} [wave]
 */
/**
 * @typedef {Object} StemSettings  Lo que se tocó de una pista.
 * @property {number} [gain]  0..2 (1 = como viene)
 * @property {number} [pan]   -1..1
 * @property {boolean} [mute]
 * @property {boolean} [solo]
 */

/**
 * Cada pista con su mezcla y si suena: callada no suena nunca; si alguna
 * está en solo, solo suenan las que lo estén (se pueden dejar varias, la
 * batería y el bajo a la vez); si no, todas.
 * @param {StemInfo[]} tracks
 * @param {Record<string, StemSettings>} settings
 */
export function stemPlan(tracks, settings) {
  const any = tracks.some((t) => settings[t.source]?.solo)
  return tracks.map((t) => {
    const s = settings[t.source] || {}
    const gain = Number.isFinite(s.gain) ? Math.min(2, Math.max(0, Number(s.gain))) : 1
    const pan = Number.isFinite(s.pan) ? Math.min(1, Math.max(-1, Number(s.pan))) : 0
    const mute = !!s.mute
    const solo = !!s.solo
    return {
      key: t.source,
      name: t.name,
      path: t.path,
      wave: t.wave || null,
      gain,
      pan,
      mute,
      solo,
      on: !mute && (!any || solo) && gain > 0
    }
  })
}

/** Sin tildes (salvo la ñ), como todos los nombres de la casa. */
function plain(text) {
  return text
    .replace(/ñ/g, '\uE000')
    .replace(/Ñ/g, '\uE001')
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/\uE000/g, 'ñ')
    .replace(/\uE001/g, 'Ñ')
}

/**
 * El nombre de la mezcla que se guarda, sin extensión: el de la canción y
 * lo que le falta («Mi Gozo (sin bateria)», «… (sin bateria ni bajo)»), o lo
 * único que lleva («… (solo voces)»). Con todas sonando, «(mezcla)».
 * @param {string} file  el archivo de la canción (o su título)
 * @param {Array<{name: string, on: boolean}>} plan
 */
export function mixFileName(file, plan) {
  const base = plain(file.replace(/\.[a-z0-9]{2,4}$/i, '')).trim() || 'Cancion'
  const off = plan.filter((t) => !t.on).map((t) => plain(t.name).toLowerCase())
  const on = plan.filter((t) => t.on).map((t) => plain(t.name).toLowerCase())
  let what = 'mezcla'
  if (off.length && off.length <= 2) what = 'sin ' + off.join(' ni ')
  else if (on.length && on.length <= 2) what = 'solo ' + on.join(' y ')
  return `${base} (${what})`.replace(/[\\/:*?"<>|]/g, '_')
}
