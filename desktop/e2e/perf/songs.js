// La biblioteca grande de la prueba de rendimiento: miles de canciones de un
// segundo, de cuatro en cuatro por artista. Los nombres no se parecen a los
// de e2e/songs.js, para que buscar «Pista 1234» dé una sola.
export const PERF_FOLDER = 'Rendimiento'

/** @param {number} i @returns {[string, string]} [artista, título] */
export const perfSong = (i) => [
  `Banda ${String(Math.floor(i / 4)).padStart(3, '0')}`,
  `Pista ${String(i).padStart(4, '0')}`
]
