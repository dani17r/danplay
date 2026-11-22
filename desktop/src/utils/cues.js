// @ts-check
// Lo que comparten la barra del estudio y la línea de tiempo sobre las guías
// y los marcadores.

/** Hasta cuántos carriles de guías (como el núcleo). */
export const MAX_CUE_LANES = 6

/**
 * Los tramos de un marcador: sus partes si son varias, su tramo (`t`..`end`)
 * si lo es, o nada si es un instante.
 *
 * Había dos: la barra pedía más de una parte y la línea de tiempo se
 * conformaba con una. La buena es la de la barra: un marcador de un solo
 * tramo se guarda sin `parts` (`setMarkerParts`, `payload`), así que una
 * parte suelta solo puede venir de datos viejos o escritos a mano, y ahí
 * mandan `t` y `end`, que son los que se ven y se mueven.
 * @param {{ t: number, end?: number, parts?: number[][] }} m
 */
export const markerParts = (m) =>
  m.parts && m.parts.length > 1
    ? m.parts
    : /** @type {number} */ (m.end) > m.t
      ? [[m.t, /** @type {number} */ (m.end)]]
      : []
