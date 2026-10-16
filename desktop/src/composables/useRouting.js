// @ts-check
// Por dónde sale cada cosa: la canción, cada pista separada, el clic y las
// voces guía. Como una secuencia en directo: el clic y las guías por un lado
// (a los cascos del baterista) y la música por otro (a la mesa), o cada cosa
// por una salida de una interfaz de varios canales.
//
// Es de quien toca, no de cada canción: se guarda en el navegador, y por
// salida de audio (lo de la interfaz de la iglesia no vale para los cascos
// de casa). El motor recibe los destinos («buses») y a cuál va cada cosa;
// las pistas llevan el suyo al mandarlas (`busOfStem`).
import { ref, computed } from 'vue'
import { usePlayback } from './usePlayback.js'

const KEY = 'danplay.salidas'

/**
 * Los destinos, por nombre: 'main' (1-2, estéreo), 'L' y 'R' (un lado, en
 * mono), 'p3' (3-4), 'p5'… (pares de canales) y 'm1', 'm2'… (un canal, en
 * mono), o 'off' (no suena).
 * @typedef {string} Dest
 */

/**
 * Lo que se puede elegir con `channels` canales: lo de dos (estéreo, un
 * lado, el otro) y, con más, cada par y cada canal.
 * @param {number} channels
 * @returns {Array<{v: Dest, n: string}>}
 */
export function destinations(channels) {
  const out = [
    { v: 'main', n: 'Los dos lados (1-2)' },
    { v: 'L', n: 'Solo izquierda (1)' },
    { v: 'R', n: 'Solo derecha (2)' }
  ]
  for (let c = 3; c + 1 <= channels; c += 2) out.push({ v: `p${c}`, n: `Salidas ${c}-${c + 1}` })
  for (let c = 3; c <= channels; c++) out.push({ v: `m${c}`, n: `Salida ${c} (mono)` })
  out.push({ v: 'off', n: 'No suena' })
  return out
}

/**
 * El destino como lo entiende el motor: dos canales (o uno, en mono), desde
 * 0; null si no suena.
 * @param {Dest} dest
 * @returns {{left: number, right: number}|null}
 */
export function targetOf(dest) {
  if (dest === 'off') return null
  if (dest === 'L') return { left: 0, right: 0 }
  if (dest === 'R') return { left: 1, right: 1 }
  const pair = /^p(\d+)$/.exec(dest || '')
  if (pair) {
    const c = Number(pair[1]) - 1
    return { left: c, right: c + 1 }
  }
  const mono = /^m(\d+)$/.exec(dest || '')
  if (mono) {
    const c = Number(mono[1]) - 1
    return { left: c, right: c }
  }
  return { left: 0, right: 1 }
}

/** Los canales que hacen falta para un destino. @param {Dest} dest */
function channelsFor(dest) {
  const t = targetOf(dest)
  return t ? Math.max(t.left, t.right) + 1 : 0
}

/**
 * Las maneras de siempre de repartirlo.
 * @type {Array<{v: string, n: string, note: string, channels: number}>}
 */
export const PRESETS = [
  {
    v: 'stereo',
    n: 'Todo junto',
    note: 'La música, el clic y las guías, en estéreo por los dos lados',
    channels: 2
  },
  {
    v: 'click-left',
    n: 'Clic y guías a la izquierda, música a la derecha',
    note: 'Con un cable en Y o una mesa: el lado izquierdo a los cascos, el derecho a la sala (en mono)',
    channels: 2
  },
  {
    v: 'click-12',
    n: 'Clic y guías por 1-2, música por 3-4',
    note: 'Con una interfaz de 4 salidas o más: 1-2 a los cascos, 3-4 a la mesa',
    channels: 4
  },
  {
    v: 'custom',
    n: 'A medida',
    note: 'Cada pista, el clic y las guías por donde digas',
    channels: 2
  }
]

/**
 * @typedef {Object} Prefs
 * @property {string|null} device   el id de la salida (null: la del sistema)
 * @property {string} preset
 * @property {{song: Dest, click: Dest, cues: Dest, stems: Record<string, Dest>}} custom
 */

/** @returns {Prefs} */
function defaults() {
  return {
    device: null,
    preset: 'stereo',
    custom: { song: 'main', click: 'main', cues: 'main', stems: {} }
  }
}

/** @returns {Prefs} */
function read() {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) || 'null')
    if (!raw || typeof raw !== 'object') return defaults()
    const base = defaults()
    return {
      device: typeof raw.device === 'string' ? raw.device : null,
      preset: PRESETS.some((p) => p.v === raw.preset) ? raw.preset : 'stereo',
      custom: {
        ...base.custom,
        ...(raw.custom && typeof raw.custom === 'object' ? raw.custom : {}),
        stems: { ...(raw.custom?.stems || {}) }
      }
    }
  } catch {
    return defaults()
  }
}

const prefs = ref(read())
/** Las salidas del equipo (se piden al abrir el panel). */
const devices = ref(/** @type {import('../api.js').AudioDevice[]} */ ([]))
let started = false

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs.value))
  } catch {
    /* modo privado */
  }
}

/**
 * A dónde va cada cosa con lo elegido: la música (y cada pista), el clic y
 * las guías. Una pista con papel (el «Click Track» o la «Guide» de una
 * secuencia) va con el clic o con las guías, salvo que se le diga otra cosa.
 * @param {Prefs} p
 * @param {string} kind  'song' | 'click' | 'cues' | 'stem'
 * @param {string} [key]  la pista (su clave: 'drums', 'vocals'…)
 * @param {string} [role] su papel: 'click', 'guide' o ''
 * @returns {Dest}
 */
export function destOf(p, kind, key, role = '') {
  if (kind === 'stem' && role && !(p.preset === 'custom' && p.custom.stems[key || ''])) {
    return destOf(p, role === 'click' ? 'click' : 'cues')
  }
  switch (p.preset) {
    case 'click-left':
      return kind === 'click' || kind === 'cues' ? 'L' : 'R'
    case 'click-12':
      return kind === 'click' || kind === 'cues' ? 'main' : 'p3'
    case 'custom':
      if (kind === 'stem') return p.custom.stems[key || ''] || p.custom.song || 'main'
      return p.custom[kind] || 'main'
    default:
      return 'main'
  }
}

/**
 * Una pista: su clave y su papel (de una lista de claves o de pistas).
 * @param {string | {key: string, role?: string}} s
 * @returns {{key: string, role: string}}
 */
const stemOf = (s) =>
  typeof s === 'string' ? { key: s, role: '' } : { key: s.key, role: s.role || '' }

/**
 * Los buses que manda al motor, y el índice de cada destino usado.
 * @param {Prefs} p
 * @param {Array<string | {key: string, role?: string}>} stemList  las pistas de la canción que suena
 */
export function buildRouting(p, stemList = []) {
  /** @type {Dest[]} */
  const used = []
  const busOf = (/** @type {Dest} */ d) => {
    let i = used.indexOf(d)
    if (i < 0) {
      used.push(d)
      i = used.length - 1
    }
    return i
  }
  const song = busOf(destOf(p, 'song'))
  const stems = Object.fromEntries(
    stemList.map(stemOf).map((st) => [st.key, busOf(destOf(p, 'stem', st.key, st.role))])
  )
  const click = busOf(destOf(p, 'click'))
  const cues = busOf(destOf(p, 'cues'))
  const channels = Math.max(2, ...used.map(channelsFor))
  return {
    spec: { buses: used.map(targetOf), song, click, cues },
    stems,
    channels
  }
}

export function useRouting() {
  const player = usePlayback()
  /** Las pistas de la canción que suena, con su papel (las pone el estudio). */
  const stemList = ref(/** @type {Array<{key: string, role: string}>} */ ([]))
  const routing = computed(() => buildRouting(prefs.value, stemList.value))

  /** Manda al motor la salida y el enrutado. */
  async function apply() {
    const r = routing.value
    await player.setOutput({ device: prefs.value.device, channels: r.channels })
    await player.setRouting(r.spec)
  }
  if (!started) {
    started = true
    apply()
  }

  /** @param {Partial<Prefs>} patch */
  async function update(patch) {
    prefs.value = { ...prefs.value, ...patch }
    save()
    await apply()
  }
  /** @param {'song'|'click'|'cues'} kind @param {Dest} dest */
  function setCustom(kind, dest) {
    return update({ preset: 'custom', custom: { ...prefs.value.custom, [kind]: dest } })
  }
  /** @param {string} key @param {Dest} dest */
  function setStem(key, dest) {
    return update({
      preset: 'custom',
      custom: { ...prefs.value.custom, stems: { ...prefs.value.custom.stems, [key]: dest } }
    })
  }
  async function loadDevices() {
    devices.value = await player.outputDevices()
    return devices.value
  }
  /** El bus de una pista (para mandarlo con ella). @param {string} key */
  const busOfStem = (key) => routing.value.stems[key] ?? routing.value.spec.song
  /**
   * Las pistas de la canción que suena cambiaron.
   * @param {Array<string | {key: string, role?: string}>} list
   */
  function setStems(list) {
    const next = list.map(stemOf)
    const now = stemList.value
    const same =
      next.length === now.length &&
      next.every((st, i) => st.key === now[i].key && st.role === now[i].role)
    if (same) return Promise.resolve()
    stemList.value = next
    return apply()
  }
  return {
    prefs,
    devices,
    routing,
    update,
    setCustom,
    setStem,
    loadDevices,
    busOfStem,
    setStems,
    apply,
    /** la salida abierta (de Rust) */
    output: player.output
  }
}

/** Vuelve a lo de fábrica. Solo para pruebas. */
export function resetRouting() {
  prefs.value = read()
  devices.value = []
  started = false
}
