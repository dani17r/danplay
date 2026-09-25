// @ts-check
// Preferencias de presentación: tema, densidad, tamaño, vista, agrupación y
// tamaño de ficha. Se recuerdan en el navegador y se aplican al arrancar.
import { ref, watch, effectScope } from 'vue'
import {
  applyTheme,
  applyDensity,
  applySize,
  savedTheme,
  savedDensity,
  savedSize
} from '../themes.js'

// Cuatro formas de ver lo mismo, de más apretada a más visual:
//   rows   una línea por canción, a lo largo
//   cards  fichas pequeñas con su portada
//   grid   cuadrícula de carátulas
//   table  tabla con columnas ordenables y ajustables
export const VIEWS = ['rows', 'cards', 'grid', 'table']
export const VIEW_NAMES = {
  rows: 'Lista fina',
  cards: 'Fichas',
  grid: 'Cuadrícula',
  table: 'Tabla'
}
export const VIEW_ICONS = {
  rows: 'viewCompact',
  cards: 'viewList',
  grid: 'viewGrid',
  table: 'queue'
}
// El nombre viejo «list»/«compact» se traduce solo, para no perder la
// preferencia de quien ya tenía una elegida.
export const LEGACY_VIEWS = { list: 'table', compact: 'rows' }

export const GROUPINGS = [
  { v: '', n: 'Sin agrupar' },
  { v: 'folder', n: 'Por carpeta' },
  { v: 'artist', n: 'Por artista' },
  { v: 'album', n: 'Por álbum' },
  { v: 'initial', n: 'Por inicial' },
  { v: 'genre', n: 'Por género' },
  { v: 'key', n: 'Por tono' }
]

function read(key, fallback = '') {
  try {
    return localStorage.getItem(key) ?? fallback
  } catch {
    return fallback
  }
}
function write(key, value) {
  try {
    localStorage.setItem(key, String(value))
  } catch {
    /* modo privado */
  }
}

/**
 * Qué vista hay guardada. Se comprueba en vez de fiarse: la disposición
 * movible que hubo antes escribía en esta misma clave, y quien la usara
 * tiene ahí un objeto que no es ningún formato de vista.
 */
export function savedLayout() {
  const v = read('danplay.layout', 'table')
  return VIEWS.includes(v) ? v : LEGACY_VIEWS[v] || 'table'
}

let instance = null
let scope = null

/**
 * Las preferencias, una sola vez para toda la ventana.
 *
 * Sus `watch` viven en un ámbito propio (`effectScope(true)`), no en el del
 * primer componente que las pide: si ese era, por ejemplo, la línea de tiempo
 * del modo estudio, al cerrarla se paraban los `watch` y cambiar el tema ya
 * no se guardaba ni se aplicaba.
 */
export function usePreferences() {
  if (instance) return instance
  scope = effectScope(true)
  instance = scope.run(createPreferences)
  return instance
}

function createPreferences() {
  const theme = ref(savedTheme())
  const density = ref(savedDensity())
  const appSize = ref(savedSize())
  const layout = ref(savedLayout())
  const groupBy = ref(read('danplay.groupBy'))
  const cardSize = ref(Number(read('danplay.cardSize', '164')) || 164)
  const showDetails = ref(read('danplay.showDetails', '1') !== '0')

  watch(theme, (v) => applyTheme(v))
  watch(density, (v) => applyDensity(v))
  watch(appSize, (v) => applySize(v))
  watch(layout, (v) => write('danplay.layout', v))
  watch(groupBy, (v) => write('danplay.groupBy', v))
  watch(cardSize, (v) => write('danplay.cardSize', v))
  watch(showDetails, (v) => write('danplay.showDetails', v ? '1' : '0'))

  /** Pinta lo guardado. Se llama una vez, al montar la app. */
  function applyAll() {
    applyTheme(theme.value)
    applyDensity(density.value)
    applySize(appSize.value)
  }

  return { theme, density, appSize, layout, groupBy, cardSize, showDetails, applyAll }
}

/** Olvida la instancia (y para sus `watch`). Para pruebas. */
export function resetPreferences() {
  scope?.stop()
  scope = null
  instance = null
}
