// @ts-check
/**
 * Copiar un texto al portapapeles.
 *
 * Se usa el del navegador cuando se puede; si el WebView lo niega (pasa fuera
 * de https o sin un gesto del usuario por medio), se recurre al truco de
 * siempre: un campo invisible, seleccionarlo y copiar. Devuelve si salió bien:
 * quien llama decide qué decir, y no dice «copiado» cuando no lo está.
 *
 * Para que el segundo camino funcione, la llamada debe ir pegada al clic: no
 * hagas una petición antes (el navegador deja copiar solo dentro del gesto).
 * @param {string} value
 * @returns {Promise<boolean>}
 */
export async function copyText(value) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value)
      return true
    }
  } catch {
    /* sin permiso o sin contexto seguro: se prueba lo de abajo */
  }
  try {
    const area = document.createElement('textarea')
    area.value = value
    area.setAttribute('readonly', '')
    area.style.cssText = 'position:fixed;top:-1000px;opacity:0'
    document.body.appendChild(area)
    area.select()
    const ok = document.execCommand('copy')
    area.remove()
    return ok
  } catch {
    return false
  }
}
