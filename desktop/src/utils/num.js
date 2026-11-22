// @ts-check
// Los redondeos y topes de números que usa medio estudio. Estaban copiados
// en siete sitios (y dos `round3` distintos: ver abajo).

/** `v` entre `lo` y `hi`. @param {number} v @param {number} lo @param {number} hi */
export const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

/** A la centésima (segundos de un tramo, semitonos). @param {number} v */
export const round2 = (v) => Math.round(v * 100) / 100

/**
 * Al milésimo (los pulsos y los bordes de los tramos van al milisegundo).
 * El `+ 0` convierte −0 en 0: un pulso corrido a la izquierda y redondeado
 * daba −0, que en JSON es 0 pero `Object.is` (y las pruebas) lo distinguen.
 * Era lo que hacían las copias del ajuste de pulsos; en las demás no cambia
 * nada (nunca les llega un −0 que importe).
 * @param {number} t
 */
export const round3 = (t) => Math.round(t * 1000) / 1000 + 0

/** Un volumen (0..1,5) para leerlo: «80 %». @param {unknown} v */
export const pctText = (v) => Math.round((Number(v) || 0) * 100) + ' %'
