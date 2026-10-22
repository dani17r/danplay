// @ts-check
// El permiso de leer la música del teléfono (Android: «música y audio»). Lo
// lleva Kotlin (MusicPermission.kt) por `window.DanPlayMusic`; la respuesta
// de pedirlo llega por `window.__danplayMusicPermission`. Fuera de Android (la
// app de la PC en modo celular, las pruebas) no hace falta: siempre sí.

import { api } from '../api.js'

/** @returns {any} */
const bridge = () => /** @type {any} */ (window).DanPlayMusic

/**
 * Las canciones del teléfono según Android (su índice de música, con la
 * tarjeta SD y las carpetas de las apps, sin los tonos ni las alarmas), o
 * null fuera de Android o si no lo sabe dar (entonces se recorren carpetas).
 * @returns {Array<{path: string}> | null}
 */
export function phoneMedia() {
  const b = bridge()
  if (!b?.songs) return null
  try {
    const list = JSON.parse(b.songs())
    return Array.isArray(list) ? list : null
  } catch {
    return null
  }
}

/** Se lo cuenta a la app antes de mirar las carpetas o la música. */
export async function syncPhoneMedia() {
  const list = phoneMedia()
  if (list) await api.phoneMedia(list).catch(() => null)
}

/** Poner al día la música del teléfono (con lo que cuenta Android). */
export async function scanPhone() {
  await syncPhoneMedia()
  return api.phoneScan()
}

/** Si ya se puede leer la música del teléfono. */
export function hasMusicPermission() {
  const b = bridge()
  if (!b?.granted) return true
  try {
    return !!b.granted()
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
  if (!b?.request) return Promise.resolve(true)
  if (hasMusicPermission()) return Promise.resolve(true)
  return new Promise((resolve) => {
    const w = /** @type {any} */ (window)
    const done = (/** @type {boolean} */ ok) => {
      clearTimeout(timer)
      w.__danplayMusicPermission = null
      resolve(!!ok)
    }
    // si Android no contesta (se cerró el cuadro de otra forma), se mira solo
    const timer = setTimeout(() => done(hasMusicPermission()), 60000)
    w.__danplayMusicPermission = done
    try {
      b.request()
    } catch {
      done(false)
    }
  })
}
