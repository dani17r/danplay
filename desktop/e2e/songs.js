// Las canciones de la biblioteca de prueba: [artista, título, Hz del tono].
// Las usan el servidor (para generarlas) y las pruebas (para buscarlas).
export const SONGS = [
  ['Barak', 'Mi Gozo', 440],
  ['Barak', 'Sera Llena La Tierra', 330],
  ['New Wine', 'Shekinah', 262],
  ['New Wine', 'A Una Voz', 392],
  ['Miel San Marcos', 'Que Se Abra El Cielo', 294]
]

// Una biblioteca grande aparte (carpeta «Grande», que solo da de alta la
// prueba que la usa): muchos artistas con dos canciones cada uno, como
// «Artistas» en una biblioteca de verdad. Canciones de un segundo, para que
// generarlas y analizarlas no cueste nada.
export const BIG_COUNT = 240
/** @param {number} i @returns {[string, string]} [artista, título] */
export const bigSong = (i) => [
  `Coro ${String(Math.floor(i / 2)).padStart(3, '0')}`,
  `Tema ${String(i).padStart(3, '0')}`
]
