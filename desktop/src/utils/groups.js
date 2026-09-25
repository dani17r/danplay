// @ts-check
// Cómo se agrupa una lista de canciones (por carpeta, artista, álbum,
// inicial, género o tono).
//
// Vive aparte porque lo necesitan dos sitios que tienen que estar de
// acuerdo: la lista que pinta los grupos y la app, que elige con Mayús y
// arma la cola. La app usaba el orden de la lista sin agrupar, y en
// «Artistas» ordenada por título Mayús elegía canciones que no estaban
// entre las dos pulsadas, y «siguiente» saltaba a otro grupo.

/** @typedef {import('../api.js').Song} Song */

/** @type {Record<string, (s: Song) => string>} */
const KEYS = {
  folder: (s) => s.folder || '(raíz)',
  artist: (s) => s.artist || 'Sin artista',
  album: (s) => s.album || 'Sin álbum',
  initial: (s) => (s.title || s.file || '#')[0].toUpperCase(),
  genre: (s) => s.genre || 'Sin género',
  key: (s) => s.key || 'Sin tono'
}

/**
 * El grupo de una canción.
 * @param {Song} song
 * @param {string} by
 */
export function groupKey(song, by) {
  return KEYS[by]?.(song) ?? '—'
}

/**
 * Los grupos, por orden alfabético; dentro de cada uno, las canciones en el
 * orden de la lista.
 * @param {Song[]} songs
 * @param {string} by
 * @returns {Array<[string, Song[]]>}
 */
export function groupSongs(songs, by) {
  /** @type {Map<string, Song[]>} */
  const groups = new Map()
  for (const s of songs) {
    const k = groupKey(s, by)
    const list = groups.get(k)
    if (list) list.push(s)
    else groups.set(k, [s])
  }
  return [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0], 'es'))
}

/**
 * Las canciones en el orden en que se ven agrupadas.
 * @param {Song[]} songs
 * @param {string} by
 */
export function groupedOrder(songs, by) {
  return groupSongs(songs, by).flatMap(([, list]) => list)
}
