// @ts-check
// El permiso de leer la música del teléfono (Android: «música y audio»). Lo
// lleva Kotlin (PermisoMusica.kt) por `window.DanPlayMusica`; la respuesta
// de pedirlo llega por `window.__danplayPermisoMusica`. Fuera de Android (la
// app de la PC en modo celular, las pruebas) no hace falta: siempre sí.

/** @returns {any} */
const bridge = () => /** @type {any} */ (window).DanPlayMusica

/** Si ya se puede leer la música del teléfono. */
export function hasMusicPermission() {
  const b = bridge()
  if (!b?.tiene) return true
  try {
    return !!b.tiene()
  } catch {
    return false
  }
}

/**
 * Lo pide (Android enseña su cuadro) y espera la respuesta.
 * @returns {Promise<boolean>}
 */
export function askMusicPermission() {
  const b = bridge()
  if (!b?.pedir) return Promise.resolve(true)
  if (hasMusicPermission()) return Promise.resolve(true)
  return new Promise((resolve) => {
    const w = /** @type {any} */ (window)
    const done = (/** @type {boolean} */ ok) => {
      clearTimeout(timer)
      w.__danplayPermisoMusica = null
      resolve(!!ok)
    }
    // si Android no contesta (se cerró el cuadro de otra forma), se mira solo
    const timer = setTimeout(() => done(hasMusicPermission()), 60000)
    w.__danplayPermisoMusica = done
    try {
      b.pedir()
    } catch {
      done(false)
    }
  })
}
