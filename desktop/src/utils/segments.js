// @ts-check
// Los tramos que se repiten en el modo estudio: [a, b] en segundos de la
// canción. Uno solo es el bucle A-B de siempre; con varios, al acabar uno se
// salta al siguiente y del último al primero, saltándose lo de entre medias.
// Rust los recorre así (`player/engine.rs`), y aquí se dejan igual que allí:
// en orden y juntando los que se pisan.

/** @typedef {[number, number]} Segment */

const round2 = (v) => Math.round(v * 100) / 100

/**
 * En orden, sin los que no valen y juntando los que se pisan.
 * @param {ReadonlyArray<ReadonlyArray<number>>|null|undefined} list
 * @returns {Segment[]}
 */
export function cleanSegments(list) {
  const valid = []
  for (const s of list || []) {
    if (!Array.isArray(s) || s.length !== 2) continue
    const a = Number(s[0])
    const b = Number(s[1])
    if (Number.isFinite(a) && Number.isFinite(b) && a >= 0 && b > a)
      valid.push([round2(a), round2(b)])
  }
  valid.sort((x, y) => x[0] - y[0])
  /** @type {Segment[]} */
  const merged = []
  for (const [a, b] of valid) {
    const last = merged.at(-1)
    if (last && a <= last[1]) last[1] = Math.max(last[1], b)
    else merged.push([a, b])
  }
  return merged
}

/**
 * ¿Son los mismos tramos? A la centésima: lo que se guarda va redondeado.
 * @param {ReadonlyArray<ReadonlyArray<number>>} x
 * @param {ReadonlyArray<ReadonlyArray<number>>} y
 */
export function sameSegments(x, y) {
  if (x.length !== y.length) return false
  return x.every((s, i) => Math.abs(s[0] - y[i][0]) < 0.011 && Math.abs(s[1] - y[i][1]) < 0.011)
}

/**
 * En qué tramo cae `t`, o -1.
 * @param {ReadonlyArray<ReadonlyArray<number>>} list
 * @param {number} t
 */
export function segmentAt(list, t) {
  return list.findIndex(([a, b]) => t >= a - 0.01 && t < b)
}

/**
 * El primer tramo que aún no ha acabado en `t` (o el primero, si ya pasaron
 * todos): a donde va la canción al empezar a repetir.
 * @param {ReadonlyArray<ReadonlyArray<number>>} list
 * @param {number} t
 */
export function nextSegment(list, t) {
  const i = list.findIndex(([, b]) => b > t)
  return i < 0 ? 0 : i
}

/**
 * Cuánto suenan todos juntos, en segundos.
 * @param {ReadonlyArray<ReadonlyArray<number>>} list
 */
export function segmentsLength(list) {
  return list.reduce((sum, [a, b]) => sum + (b - a), 0)
}

/**
 * Los bordes llevados al pulso más cercano de la rejilla, sin que ninguno
 * quede más corto que `min`. Para que un tramo empiece y acabe a tiempo.
 * @param {ReadonlyArray<ReadonlyArray<number>>} list
 * @param {ReadonlyArray<number>} beats  segundos de cada pulso, en orden
 * @param {number} [min]
 * @returns {Segment[]}
 */
export function snapToBeats(list, beats, min = 0.5) {
  if (!beats?.length) return cleanSegments(list)
  const nearest = (t) => {
    let lo = 0
    let hi = beats.length - 1
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (beats[mid] < t) lo = mid + 1
      else hi = mid
    }
    const i = lo
    return i > 0 && Math.abs(beats[i - 1] - t) <= Math.abs(beats[i] - t) ? i - 1 : i
  }
  return cleanSegments(
    list.map(([a, b]) => {
      const i = nearest(a)
      let j = nearest(b)
      while (j < beats.length - 1 && beats[j] - beats[i] < min) j++
      return beats[j] - beats[i] >= min ? [beats[i], beats[j]] : [a, b]
    })
  )
}
