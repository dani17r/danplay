// @ts-check
// Los términos y condiciones de DanPlay y quién lo hizo.
//
// Se enseñan la primera vez que se abre la app («Acerca de DanPlay» →
// Términos y condiciones, y la ventana de aceptarlos). Sin aceptarlos se
// puede escuchar y ordenar la música, pero no separar en pistas, usar el
// asistente de IA ni «Transformar» (ver useTerms.js).
//
// Si se cambia el texto de forma que importe, sube TERMS_VERSION: la app los
// vuelve a pedir antes de esas funciones.

/** Quien hizo DanPlay, y cómo hablar con él. */
export const AUTHOR = {
  name: 'Daniel Rangel',
  role: 'Creador y desarrollador de DanPlay',
  telegram: '@Dani17r',
  url: 'https://t.me/Dani17r'
}

/** La versión de los términos: la fecha en que se escribieron. */
export const TERMS_VERSION = '2026-10-16'

/** Lo que pide aceptarlos, para decirlo donde haga falta. */
export const GATED = {
  separate: 'separar canciones en pistas',
  chat: 'usar el asistente de IA',
  transform: 'ordenar y renombrar con «Transformar»'
}

/**
 * @typedef {Object} TermsSection
 * @property {string} title
 * @property {string[]} body  párrafos
 */

/** @type {TermsSection[]} */
export const TERMS = [
  {
    title: 'Quién hace DanPlay',
    body: [
      `DanPlay es un reproductor creado y desarrollado por ${AUTHOR.name}. La aplicación, ` +
        'su diseño y su código son obra suya, y los derechos sobre ellos le pertenecen.'
    ]
  },
  {
    title: 'Aceptar estos términos',
    body: [
      'Estos términos se aplican a todo el uso de DanPlay. Al marcar la casilla y pulsar ' +
        '«Aceptar» confirmas que los has leído hasta el final y que estás de acuerdo con ellos.',
      'Si no los aceptas, puedes seguir usando DanPlay para escuchar y organizar tu música, ' +
        `pero no podrás ${GATED.separate}, ${GATED.chat} ni ${GATED.transform}. ` +
        'Puedes aceptarlos cuando quieras desde «Acerca de DanPlay».'
    ]
  },
  {
    title: 'La licencia',
    body: [
      'DanPlay se distribuye bajo la PolyForm Strict License 1.0.0. Puedes usarlo para fines ' +
        'personales y no comerciales, también en tu iglesia o en una organización sin ánimo ' +
        'de lucro. No puedes venderlo, redistribuirlo, modificarlo ni hacer obras derivadas.',
      'Para cualquier otro uso, incluido el comercial, escribe a su autor. Lo que vale ' +
        'legalmente es el texto completo de la licencia, que viaja con la aplicación.'
    ]
  },
  {
    title: 'Tu música y los derechos de autor',
    body: [
      'Eres responsable de tener derecho a usar la música que abres, descargas, separas o ' +
        'compartes con DanPlay.',
      'Las pistas separadas, los clics y las guías son para tu uso: ensayar, estudiar y ' +
        'tocar en directo. No las publiques ni las distribuyas sin permiso de quien tiene ' +
        'los derechos de la canción.'
    ]
  },
  {
    title: 'Descargas',
    body: [
      'DanPlay baja de YouTube lo que tú le pides, con la herramienta libre yt-dlp. Respeta ' +
        'las condiciones de cada plataforma y la ley de tu país: descarga solo lo que tengas ' +
        'derecho a guardar.'
    ]
  },
  {
    title: 'Separar en pistas, el clic y las voces',
    body: [
      'La separación en pistas, el detector del pulso y las voces de las guías son redes ' +
        'neuronales que funcionan en tu equipo, sin internet salvo para bajarlas la primera ' +
        'vez desde los repositorios de sus autores. Cada una tiene su propia licencia, que ' +
        'se respeta (ver la documentación de dependencias).',
      'Sus resultados son aproximados: una pista puede llevar restos de otros instrumentos ' +
        'y el clic puede equivocarse. Revísalos antes de tocar con ellos en directo.',
      'Separar en máxima calidad pide mucha memoria y procesador durante varios minutos ' +
        'por canción.',
      'Las voces sintéticas no se pueden usar para imitar a una persona real ni para engañar ' +
        'a nadie, como piden sus licencias.'
    ]
  },
  {
    title: 'El asistente de IA',
    body: [
      'El asistente envía tus mensajes, y los datos de tu biblioteca que necesite para ' +
        'contestar (títulos, artistas, listas), al proveedor de IA que tú elijas, con tu clave ' +
        'y bajo sus condiciones. DanPlay no controla esos servicios ni lo que hacen con los ' +
        'datos. Las opciones gratis sin clave son servicios de terceros, con sus propios ' +
        'límites.',
      'La IA puede equivocarse. Revisa lo que propone antes de confirmarlo: nada se borra ni ' +
        'se mueve sin que tú lo aceptes.'
    ]
  },
  {
    title: 'Transformar: ordenar y renombrar en masa',
    body: [
      '«Transformar» mueve y renombra archivos de tus carpetas. Revisa cada cambio antes de ' +
        'aplicarlo y ten una copia de seguridad de tu música. Puedes deshacer el último, pero ' +
        'DanPlay no responde de archivos perdidos o mal nombrados.'
    ]
  },
  {
    title: 'Privacidad',
    body: [
      'DanPlay funciona en tu equipo. No tiene cuentas, no manda estadísticas y no sigue lo ' +
        'que haces.',
      'Solo sale a internet lo que tú pides: descargas, letras, carátulas y acordes, la ' +
        'identificación de canciones, la IA que elijas, el envío por Telegram y la descarga ' +
        'de los modelos y de yt-dlp.',
      'Tus listas, estrellas y ajustes se guardan en tu equipo y, para que no se pierdan si ' +
        'mueves la música, en una carpeta «.danplay» dentro de tu carpeta de música.'
    ]
  },
  {
    title: 'Sin garantía',
    body: [
      'DanPlay se ofrece tal cual, sin garantías de ningún tipo. En la medida en que la ley ' +
        'lo permita, su autor no responde de daños o pérdidas (de datos, archivos, tiempo o ' +
        'dinero) que vengan de usarlo o de no poder usarlo.'
    ]
  },
  {
    title: 'Cambios en los términos',
    body: [
      'Si estos términos cambian, DanPlay te los enseñará otra vez y tendrás que aceptarlos ' +
        'antes de volver a usar las funciones que dependen de ellos.'
    ]
  },
  {
    title: 'Contacto y colaboración',
    body: [
      `¿Dudas, ideas o quieres colaborar con DanPlay? Escribe a ${AUTHOR.name} por Telegram: ` +
        `${AUTHOR.telegram} (${AUTHOR.url}).`
    ]
  }
]
