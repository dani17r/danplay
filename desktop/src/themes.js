// Paletas. Cada una redefine las variables CSS sobre :root.
// Ademas del catalogo, el usuario puede crear los suyos (se guardan en el navegador).

export const FIELDS = [
  { k: 'bg',      n: 'Fondo',            d: 'el lienzo de la ventana' },
  { k: 'panel',   n: 'Paneles',          d: 'barras lateral, superior e inferior' },
  { k: 'panel2',  n: 'Paneles suaves',   d: 'filas al pasar el raton, campos' },
  { k: 'border',  n: 'Bordes',           d: 'separadores finos' },
  { k: 'border2', n: 'Bordes marcados',  d: 'contornos de campos y botones' },
  { k: 'text',    n: 'Texto',            d: 'el texto principal' },
  { k: 'muted',   n: 'Texto secundario', d: 'artista, album, notas' },
  { k: 'muted2',  n: 'Texto apagado',    d: 'cabeceras y datos menores' },
  { k: 'accent',  n: 'Acento',           d: 'lo que suena, resaltados, enlaces' },
  { k: 'accent2', n: 'Acento fuerte',    d: 'botones principales' },
  { k: 'amber',   n: 'Estrellas',        d: 'valoraciones' },
  { k: 'red',     n: 'Alertas',          d: 'favoritos y errores' }
]

export const CATALOG = {
  night: { name: 'Noche', kind: 'dark', v: {
    bg:'#0c0d10', panel:'#131519', panel2:'#181b21', border:'#22262e', border2:'#2c313b',
    text:'#dfe3ea', muted:'#8b93a3', muted2:'#5d6472', accent:'#4ade80', accent2:'#22c55e',
    amber:'#fbbf24', red:'#f87171' } },
  midnight: { name: 'Medianoche', kind: 'dark', v: {
    bg:'#080b14', panel:'#0e1424', panel2:'#141c30', border:'#1e2a44', border2:'#27365a',
    text:'#dbe4f5', muted:'#8195b8', muted2:'#566885', accent:'#60a5fa', accent2:'#3b82f6',
    amber:'#fbbf24', red:'#fb7185' } },
  studio: { name: 'Estudio', kind: 'dark', v: {
    bg:'#12100e', panel:'#1a1714', panel2:'#211d19', border:'#2e2822', border2:'#3b332b',
    text:'#ece5da', muted:'#a3968a', muted2:'#6f6459', accent:'#fbbf24', accent2:'#f59e0b',
    amber:'#fcd34d', red:'#f87171' } },
  carbon: { name: 'Carbon', kind: 'dark', v: {
    bg:'#101012', panel:'#171719', panel2:'#1e1e21', border:'#28282c', border2:'#34343a',
    text:'#e4e4e7', muted:'#9a9aa2', muted2:'#6b6b74', accent:'#a78bfa', accent2:'#8b5cf6',
    amber:'#fbbf24', red:'#f87171' } },
  forest: { name: 'Bosque', kind: 'dark', v: {
    bg:'#0b1210', panel:'#111a17', panel2:'#16221e', border:'#1f2f28', border2:'#2a4036',
    text:'#dcebe4', muted:'#87a89a', muted2:'#5c7770', accent:'#34d399', accent2:'#10b981',
    amber:'#fcd34d', red:'#f87171' } },
  wine: { name: 'Vino', kind: 'dark', v: {
    bg:'#140b0f', panel:'#1d1116', panel2:'#26171d', border:'#361f28', border2:'#472a35',
    text:'#f0dfe5', muted:'#b58f9c', muted2:'#80616d', accent:'#fb7185', accent2:'#e11d48',
    amber:'#fbbf24', red:'#fda4af' } },
  ocean: { name: 'Oceano', kind: 'dark', v: {
    bg:'#071417', panel:'#0c1d22', panel2:'#11272e', border:'#183640', border2:'#204755',
    text:'#d7eaf0', muted:'#7fa5b3', muted2:'#557785', accent:'#22d3ee', accent2:'#06b6d4',
    amber:'#fbbf24', red:'#f87171' } },
  contrast: { name: 'Alto contraste', kind: 'dark', v: {
    bg:'#000000', panel:'#0a0a0a', panel2:'#141414', border:'#3a3a3a', border2:'#555555',
    text:'#ffffff', muted:'#c8c8c8', muted2:'#9a9a9a', accent:'#00ff88', accent2:'#00e07a',
    amber:'#ffd000', red:'#ff5f5f' } },
  light: { name: 'Claro', kind: 'light', v: {
    bg:'#f7f8fa', panel:'#ffffff', panel2:'#f0f2f5', border:'#e2e5ea', border2:'#d3d8e0',
    text:'#1a1d23', muted:'#5b6472', muted2:'#8b93a3', accent:'#16a34a', accent2:'#15803d',
    amber:'#d97706', red:'#dc2626' } },
  paper: { name: 'Papel', kind: 'light', v: {
    bg:'#f6f2ea', panel:'#fffdf8', panel2:'#efe9dd', border:'#e0d8c8', border2:'#cfc4ae',
    text:'#2c2620', muted:'#6b6153', muted2:'#948873', accent:'#b45309', accent2:'#92400e',
    amber:'#ca8a04', red:'#b91c1c' } },
  snow: { name: 'Nieve', kind: 'light', v: {
    bg:'#f4f7fb', panel:'#ffffff', panel2:'#e9eff7', border:'#dbe4ef', border2:'#c4d2e3',
    text:'#16202e', muted:'#54637a', muted2:'#8494aa', accent:'#2563eb', accent2:'#1d4ed8',
    amber:'#d97706', red:'#dc2626' } }
}

/**
 * Tamaño general de la app. Multiplica letra y proporciones a la vez, para
 * quien no ve bien de cerca. Es independiente de la densidad: la densidad
 * decide cuanto respira una fila, el tamaño decide cuanto se ve todo.
 */
export const SIZES = {
  small:  { name: 'Pequeño',    scale: 0.9,  note: 'cabe mas en pantalla' },
  medium: { name: 'Mediano',    scale: 1,    note: 'el de siempre' },
  large:  { name: 'Grande',     scale: 1.14, note: 'letra mas grande' },
  xlarge: { name: 'Muy grande', scale: 1.3,  note: 'maxima comodidad' }
}

export const DENSITIES = {
  comfortable: { name: 'Comoda',   rowPad: '7px',   fontSize: '13px' },
  normal:      { name: 'Normal',   rowPad: '5px',   fontSize: '12.5px' },
  compact:     { name: 'Compacta', rowPad: '2.5px', fontSize: '12px' }
}

const CUSTOM_KEY = 'danplay.customThemes'

export function customThemes () {
  try { return JSON.parse(localStorage.getItem(CUSTOM_KEY) || '{}') } catch { return {} }
}
export function saveCustomTheme (key, theme) {
  const t = customThemes(); t[key] = theme
  localStorage.setItem(CUSTOM_KEY, JSON.stringify(t))
  return t
}
export function deleteCustomTheme (key) {
  const t = customThemes(); delete t[key]
  localStorage.setItem(CUSTOM_KEY, JSON.stringify(t))
  return t
}
/** Catalogo + los del usuario. */
export function allThemes () { return { ...CATALOG, ...customThemes() } }

export function applyTheme (key, preview = null) {
  const t = preview || allThemes()[key] || CATALOG.night
  const r = document.documentElement
  for (const [k, v] of Object.entries(t.v)) r.style.setProperty('--' + k, v)
  r.dataset.theme = key
  r.dataset.kind = t.kind || 'dark'
  if (!preview) { try { localStorage.setItem('danplay.theme', key) } catch {} }
  return t
}
export function applyDensity (key) {
  const d = DENSITIES[key] || DENSITIES.comfortable
  const r = document.documentElement
  // se guardan «en crudo»: el css los multiplica por la escala del tamaño,
  // asi densidad y tamaño se combinan en vez de pisarse
  r.style.setProperty('--row-pad-base', d.rowPad)
  r.style.setProperty('--table-font-base', d.fontSize)
  try { localStorage.setItem('danplay.density', key) } catch {}
  return d
}

export function applySize (key) {
  const s = SIZES[key] || SIZES.medium
  document.documentElement.style.setProperty('--scale', String(s.scale))
  try { localStorage.setItem('danplay.size', key) } catch {}
  return s
}

export function savedSize () {
  try {
    const k = localStorage.getItem('danplay.size') || 'medium'
    return SIZES[k] ? k : 'medium'
  } catch { return 'medium' }
}
export function savedTheme () {
  try {
    const g = localStorage.getItem('danplay.theme') || 'night'
    return allThemes()[g] ? g : 'night'
  } catch { return 'night' }
}
export function savedDensity () {
  try { return localStorage.getItem('danplay.density') || 'comfortable' } catch { return 'comfortable' }
}
export const THEMES = CATALOG   // compatibilidad
