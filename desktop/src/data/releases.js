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
    version: '1.40.1',
    date: '2026-10-24',
    summary: 'Por dentro, más ordenada; y unos arreglos que se notan.',
    items: [
      'Al mejorar las pistas mientras suena, el clic sigue cuadrando con los golpes.',
      'Editar la ficha de una canción sin año ya no escribe «None» como año.',
      'Los avisos de lo que falla salen en castellano.',
      'Bajar de YouTube justo después de poner al día yt-dlp ya no falla.'
    ]
  },
  {
    version: '1.40.0',
    date: '2026-10-23',
    summary: 'Por dentro, nueva: más rápida, más ligera y sin programas aparte.',
    items: [
      'Abre antes: lo que se ve llega primero y lo demás se carga mientras la usas.',
      'Gasta menos memoria: un proceso menos y la ventanita y la proyección, solo al usarlas.',
      'Recorrer tus carpetas, hasta cuatro veces más rápido.',
      'Separar en pistas gasta unos 300 MB menos.',
      'yt-dlp se pone al día solo desde Descargas.',
      'Arreglado: si borrabas la base con la app abierta, ya no se queda sin poder guardar.'
    ]
  },
  {
    version: '1.30.0',
    date: '2026-10-17',
    summary: 'Como una secuencia: L y R en cada carril, la guía automática y exportar claro.',
    items: [
      'L y R en cada carril (canción, pistas, clic y guías), con Est o Mono.',
      'El clic tiene su carril debajo de la onda, con su volumen y sus canales.',
      'Guía automática: anuncia la intro, las estrofas, los coros, el puente y los solos.',
      'Cambia la voz de las guías de un carril cuando quieras.',
      'Exportar, en una ventana: la canción, la mezcla o para el celular, con los canales si quieres.',
      'Con la onda bloqueada, «ir con un clic» para saltar igual.',
      'Un recorrido con animaciones que enseña el modo estudio.',
      'Menús cortos, con lo puesto marcado en color.'
    ]
  },
  {
    version: '1.28.3',
    date: '2026-10-17',
    summary: 'El celular: el estudio a la vista, las guías para oírlas y toda tu música.',
    items: [
      'Con un tramo, «Al acabar: parar» ya se ve y avisa al pararse (parecía una pausa sola).',
      'En el teléfono, la cabecera y la barra del tramo ya no se pisan.',
      '«Salidas» en la cabecera del estudio, con estéreo o mono para cada cosa.',
      'Las guías de la PC se oyen y se mezclan en el teléfono, carril a carril.',
      'La música del teléfono, también la de la tarjeta SD y la de Telegram o WhatsApp.'
    ]
  },
  {
    version: '1.28.2',
    date: '2026-10-16',
    summary: 'El modo estudio, más a mano: mandos sobre la onda, iconos y un editor de guías.',
    items: [
      'Escuchar una guía ya funciona en Linux (antes daba un error).',
      'Sonar, pausar, detener y saltar 5 s, encima de la onda.',
      'Iconos con su aviso al dejar el ratón encima, en vez de textos largos.',
      'Importa un audio corto como guía, hasta lo que dura la canción.',
      'Recorta lo grabado, ponle fundidos, quita el ruido y los graves, iguala el volumen.',
      'Cada guía se ve con su onda y lo que dura.',
      'El imán: las guías se pegan a los pulsos del clic, aunque cambie el tempo.',
      'Pulsa una guía de la lista y te lleva a ella en su carril.'
    ]
  },
  {
    version: '1.28.1',
    date: '2026-10-16',
    summary: 'Las guías en sus carriles, exportar con o sin guías y la onda a tu gusto.',
    items: [
      'En Linux vuelven la onda y separar en pistas (fallaban con canciones nuevas).',
      'Los paquetes de Linux traen ffmpeg, fpcalc y Deno: no hay que instalar nada.',
      'Guías en carriles, hasta seis, cada uno con su volumen y su salida.',
      'Las guías se ponen, se mueven y se cambian con la canción parada.',
      'Exporta la canción o las pistas, con las guías o sin ellas.',
      'Seis estilos para la onda: barras, bloques, rayitas, suave, contorno y reflejo.',
      'Ctrl + rueda acerca, Mayús + rueda recorre, la rueda sola baja el panel.',
      'Cada cosa, en estéreo o en mono, y solo las salidas de audio que tienes.'
    ]
  },
  {
    version: '1.28.0',
    date: '2026-10-16',
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
