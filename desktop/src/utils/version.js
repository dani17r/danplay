// @ts-check
// Comparar versiones, sin las novedades. La app lo necesita al arrancar
// (¿se actualizó desde la última vez?) y lo traía de data/releases.js, que
// arrastraba las 300 líneas de novedades al trozo del arranque.

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
