// @ts-check
// Lo que se elige solo en el celular (Ajustes): cómo se pone una canción,
// lo apretada que va la lista y si la pantalla se queda encendida. El tema y
// el tamaño son los de la PC (usePreferences).
import { ref, watch } from 'vue'

/**
 * @param {string} key
 * @param {boolean} fallback
 */
function flag(key, fallback) {
  let start = fallback
  try {
    const v = localStorage.getItem(key)
    if (v != null) start = v === '1'
  } catch {
    /* sin almacenamiento */
  }
  const r = ref(start)
  watch(r, (v) => {
    try {
      localStorage.setItem(key, v ? '1' : '0')
    } catch {
      /* sin almacenamiento */
    }
  })
  return r
}

/** Un toque en la canción la pone (si no, la elige y el doble toque la pone). */
export const tapPlays = flag('danplay.movil.toque', false)
/** Filas más bajas: caben más canciones. */
export const compactList = flag('danplay.movil.compacta', false)
/** La pantalla no se apaga mientras suena (el teléfono en el atril). */
export const keepAwake = flag('danplay.movil.pantalla', true)
