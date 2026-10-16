// @ts-check
// Las novedades de cada versión, cortas y para quien usa la app (lo que se
// enseña en «Acerca de DanPlay» → Novedades). El detalle técnico va en
// CHANGELOG.md; una prueba comprueba que cada versión de allí está aquí.
//
// `date` vacía: la versión aún no se ha publicado.

/**
 * @typedef {Object} Release
 * @property {string} version
 * @property {string} date      AAAA-MM-DD, o '' si aún no se publicó
 * @property {string} summary   una línea
 * @property {string[]} items   lo nuevo, una frase cada cosa
 */

/** @type {Release[]} */
export const RELEASES = [
  {
    version: '1.28.0',
    date: '',
    summary: 'La pista y el metrónomo, listos para tocar en directo, y tus listas ya no se rompen.',
    items: [
      'Motor de audio nuevo: los tramos y el cambio a las pistas, sin cortes.',
      'El clic ya no se calla ni se salta: análisis nuevo en toda la biblioteca.',
      'Metrónomo más fuerte, con claves y charles, y tu propio clic hecho a mano.',
      '«Re-analizar…» el clic de toda la canción o solo de un tramo.',
      '«Salidas»: el clic y las guías por un lado y la música por otro, como una secuencia.',
      'Guías habladas: escritas (las dice una voz) o grabadas con tu micrófono.',
      'Separar en máxima calidad: percusión, voz y coros, guitarras, piano, órgano, teclados, cuerdas, metales y vientos.',
      '«Abrir secuencia»: las de MultiTracks y Loop Community, listas para tocar.',
      'Tus listas no se rompen: si mueves tu música o vuelves a bajar una canción, vuelve a su sitio.',
      'Lo tuyo (listas, estrellas, estudio) se guarda junto a tu música y se recupera solo.',
      'Si una canción de una lista desaparece, se queda como «No está» para buscarla o bajarla.',
      'Al borrar una canción, eliges si sale también de sus listas.',
      'Novedades, términos y condiciones y créditos, en «Acerca de DanPlay».',
      'En el celular: la música del teléfono, el volumen a mano, las salidas y las guías.'
    ]
  },
  {
    version: '1.27.0',
    date: '2026-10-14',
    summary: 'La app del celular, rehecha para los dedos.',
    items: [
      'Todo más grande y con más aire, y se llama DanPlay, como en la PC.',
      'Menú de lado con tus listas, las favoritas y los ajustes.',
      'Reproductor nuevo y modo estudio a la medida de la pantalla.',
      'Los mandos en la bandeja de Android y en la pantalla de bloqueo.'
    ]
  },
  {
    version: '1.26.0',
    date: '2026-10-14',
    summary: 'Lo que separas en la PC, ahora se estudia en el celular.',
    items: [
      'App nueva para Android con las canciones ya separadas.',
      'Cada pista con su volumen, el clic de la PC, velocidad y tono.',
      'Lo que dejaste puesto en el estudio de la PC viaja con la canción.'
    ]
  },
  {
    version: '1.25.0',
    date: '2026-10-11',
    summary: 'Clic automático, cambios sin cortes y «Transformar».',
    items: [
      'Cada canción trae su clic, y lo calculado ya no se pierde.',
      'De la canción a sus pistas sin corte, y fundido entre canciones.',
      '«Transformar» ordena tu carpeta canción por canción.',
      'El estudio a todo el alto, y movimiento suave en toda la app.'
    ]
  },
  {
    version: '1.24.1',
    date: '2026-10-11',
    summary: 'Arreglos del asistente.',
    items: ['Dos arreglos del asistente, y los paquetes de Mac también para Intel.']
  },
  {
    version: '1.24.0',
    date: '2026-10-10',
    summary: 'El asistente entiende la lista del domingo, y elegir varias es fácil.',
    items: [
      'Pega la lista de WhatsApp tal cual: el asistente la entiende.',
      'Busca primero en tu biblioteca y arma la lista con lo que hay.',
      'Botón «Seleccionar»: casillas y una barra para hacer cosas con varias.',
      'El reproductor ya no se corta cuando el disco está ocupado.'
    ]
  },
  {
    version: '1.23.0',
    date: '2026-10-07',
    summary: 'La ventana se adapta, y el reproductor tiene formas.',
    items: [
      'La ventana se adapta a cualquier tamaño.',
      'Reproductor a pantalla completa, media pantalla, columna, cuadrito o barra.'
    ]
  },
  {
    version: '1.22.0',
    date: '2026-10-07',
    summary: 'Un metrónomo más sentado.',
    items: ['Ya no se acelera ni mete golpes de más, y el «1» cae donde toca.']
  },
  {
    version: '1.21.0',
    date: '2026-10-07',
    summary: 'Acordes de verdad.',
    items: ['Los acordes y el tono salen de cifrados publicados por músicos, con su enlace.']
  },
  {
    version: '1.20.3',
    date: '2026-10-07',
    summary: 'Mantenimiento.',
    items: ['Todo al día por dentro; no cambia nada de lo que ves.']
  },
  {
    version: '1.20.2',
    date: '2026-10-03',
    summary: 'Windows sin ventanas negras.',
    items: ['Ya no se abren consolas al separar, descargar o analizar canciones.']
  },
  {
    version: '1.20.1',
    date: '2026-10-03',
    summary: 'Solo los modelos de IA que funcionan.',
    items: ['Al elegir el modelo de IA, solo salen los que tu clave puede usar.']
  },
  {
    version: '1.20.0',
    date: '2026-09-26',
    summary: 'Tus descargas, en su carpeta.',
    items: ['La primera vez que bajas algo, eliges dónde guardarlo.']
  }
]

/**
 * Compara dos versiones «1.2.3». Negativo si `a` es anterior.
 * @param {string} a @param {string} b
 */
export function compareVersions(a, b) {
  const pa = String(a || '0')
    .split('.')
    .map(Number)
  const pb = String(b || '0')
    .split('.')
    .map(Number)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0)
    if (d) return d
  }
  return 0
}

/**
 * Las versiones que trajo una actualización: las posteriores a `seen` hasta
 * `current` (incluida). Sin `seen` (primera vez), ninguna.
 * @param {string} seen @param {string} current
 */
export function newSince(seen, current) {
  if (!seen) return []
  return RELEASES.filter(
    (r) => compareVersions(r.version, seen) > 0 && compareVersions(r.version, current) <= 0
  )
}
