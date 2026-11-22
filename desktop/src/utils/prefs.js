// @ts-check
// Lo que el estudio y el clic recuerdan en el navegador (el candado, el paso
// del tono, los golpes a la vista…): en JSON, y sin romper si no hay
// almacenamiento (modo privado). Estaba copiado en tres sitios.

/** @param {string} key @param {any} fallback */
export function readPref(key, fallback) {
  try {
    const v = localStorage.getItem(key)
    return v == null ? fallback : JSON.parse(v)
  } catch {
    return fallback
  }
}

/** @param {string} key @param {any} value */
export function savePref(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* modo privado */
  }
}
