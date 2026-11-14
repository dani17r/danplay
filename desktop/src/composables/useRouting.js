// @ts-check
// Por dónde sale cada cosa: la canción, cada pista separada, el clic y las
// voces guía. Como una secuencia en directo: el clic y las guías por un lado
// (a los cascos del baterista) y la música por otro (a la mesa), o cada cosa
// por una salida de una interfaz de varios canales.
//
// Es de quien toca, no de cada canción: se guarda en el navegador, un reparto
// por cada salida de audio. Había uno para todas, y lo de la interfaz de la
// iglesia se aplicaba a los cascos de casa: con la música por 3-4 en una
// salida de dos canales, no sonaba, y solo lo decía el cajón de «Salidas».
//
// Manda la salida que SUENA. Si la elegida no está y suena por la del
// sistema, el reparto es el de la del sistema; una salida sin reparto guardado
// suena «todo junto»; y si a la que suena le faltan canales para el suyo,
// suena todo junto mientras tanto y se avisa donde esté quien toca. El
// reparto guardado no se toca: vuelve solo cuando vuelve la interfaz.
//
// El motor recibe los destinos («buses») y a cuál va cada cosa; las pistas
// llevan el suyo al mandarlas (`busOfStem`).
import { ref, computed, watch, effectScope } from 'vue'
import { usePlayback } from './usePlayback.js'
import { notify } from './useNotices.js'

const KEY = 'danplay.outputs'
/**
 * Lo que tarda en saberse cómo quedó la salida tras pedir otra (u otros
 * canales): Rust la vuelve a abrir y lo cuenta en el estado siguiente (hasta
 * medio segundo después, en pausa). Mientras, el estado que hay es todavía el
 * de la de antes.
 */
const SETTLE_MS = 1500

/**
 * Los destinos, por nombre: 'main' (1-2, estéreo), 'L' y 'R' (un lado, en
 * mono), 'p3' (3-4), 'p5'… (pares de canales), 'm1', 'm2'… (un canal, en
 * mono), 'x2-3' (dos canales cualesquiera, el primero a la izquierda) o
 * 'off' (no suena).
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
 * 0; null si no suena. Con `mono`, un par lleva lo mismo por los dos (lo que
 * suena junta sus dos lados): para una mesa o una sala en mono.
 * @param {Dest} dest
 * @param {boolean} [mono]
 * @returns {{left: number, right: number, mono?: boolean}|null}
 */
export function targetOf(dest, mono = false) {
  if (dest === 'off') return null
  if (dest === 'L') return { left: 0, right: 0 }
  if (dest === 'R') return { left: 1, right: 1 }
  const single = /^m(\d+)$/.exec(dest || '')
  if (single) {
    const c = Number(single[1]) - 1
    return { left: c, right: c }
  }
  const any = /^x(\d+)-(\d+)$/.exec(dest || '')
  if (any) {
    const a = Number(any[1]) - 1
    const b = Number(any[2]) - 1
    return mono ? { left: a, right: b, mono: true } : { left: a, right: b }
  }
  const pair = /^p(\d+)$/.exec(dest || '')
  const c = pair ? Number(pair[1]) - 1 : 0
  return mono ? { left: c, right: c + 1, mono: true } : { left: c, right: c + 1 }
}

/** Si el destino es un solo canal (y entonces va en mono, se elija o no). @param {Dest} dest */
export const isSingle = (dest) => dest === 'L' || dest === 'R' || /^m\d+$/.test(dest || '')

/**
 * Los canales de un destino, contando desde 1 (lo que se enciende en los
 * botones L y R, o 1, 2, 3… de cada cosa). Ninguno: no suena.
 * @param {Dest} dest
 * @returns {number[]}
 */
export function channelsOf(dest) {
  const t = targetOf(dest)
  if (!t) return []
  return t.left === t.right ? [t.left + 1] : [t.left + 1, t.right + 1]
}

/**
 * Y al revés: el destino de esos canales (uno: en mono por él; dos: el
 * primero a la izquierda y el segundo a la derecha; ninguno: no suena).
 * @param {number[]} list
 * @returns {Dest}
 */
export function destOfChannels(list) {
  const cs = [...new Set(list)]
    .filter((c) => c >= 1)
    .sort((a, b) => a - b)
    .slice(0, 2)
  if (!cs.length) return 'off'
  if (cs.length === 1) return cs[0] === 1 ? 'L' : cs[0] === 2 ? 'R' : `m${cs[0]}`
  const [a, b] = cs
  if (a === 1 && b === 2) return 'main'
  if (b === a + 1 && a % 2 === 1) return `p${a}`
  return `x${a}-${b}`
}

/**
 * Por qué lado va en un archivo estéreo (al exportar «con los canales»): los
 * dos, la izquierda o la derecha (de una interfaz de más salidas, las
 * impares a la izquierda y las pares a la derecha); null si no suena.
 * @param {Dest} dest
 * @returns {'both'|'L'|'R'|null}
 */
export function sideOf(dest) {
  const cs = channelsOf(dest)
  if (!cs.length) return null
  if (cs.length === 2 && cs[0] % 2 !== cs[1] % 2) return 'both'
  return cs[0] % 2 === 1 ? 'L' : 'R'
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
 * @typedef {Object} Route  el reparto de una salida
 * @property {string} preset
 * @property {{song: Dest, click: Dest, cues: Dest, stems: Record<string, Dest>}} custom
 * @property {{song: boolean, click: boolean, cues: boolean, stems: Record<string, boolean>}} mono
 *   lo que va en mono por dos canales (vale con cualquier reparto)
 */
/**
 * @typedef {Route & {device: string|null}} Prefs
 *   la salida elegida (su id; null: la del sistema) y el reparto de la que suena
 */
/**
 * @typedef {Object} Saved  lo guardado
 * @property {string|null} device  la salida elegida
 * @property {Record<string, Route>} byDevice  el reparto de cada salida, por su id ('': la del sistema)
 */

/** «Todo junto»: lo de una salida que no tiene reparto guardado. @returns {Route} */
function defaultRoute() {
  return {
    preset: 'stereo',
    custom: { song: 'main', click: 'main', cues: 'main', stems: {} },
    mono: { song: false, click: false, cues: false, stems: {} }
  }
}

/** @returns {Prefs} */
function defaults() {
  return { device: null, ...defaultRoute() }
}

/** Un reparto guardado, con lo que le falte. @param {any} raw @returns {Route} */
function cleanRoute(raw) {
  const base = defaultRoute()
  return {
    preset: PRESETS.some((p) => p.v === raw?.preset) ? raw.preset : 'stereo',
    custom: {
      ...base.custom,
      ...(raw?.custom && typeof raw.custom === 'object' ? raw.custom : {}),
      stems: { ...(raw?.custom?.stems || {}) }
    },
    mono: {
      song: raw?.mono?.song === true,
      click: raw?.mono?.click === true,
      cues: raw?.mono?.cues === true,
      stems: Object.fromEntries(
        Object.entries(raw?.mono?.stems || {}).filter(([, v]) => typeof v === 'boolean')
      )
    }
  }
}

/** @returns {Saved} */
function read() {
  /** @type {Saved} */
  const out = { device: null, byDevice: {} }
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) || 'null')
    if (!raw || typeof raw !== 'object') return out
    out.device = typeof raw.device === 'string' && raw.device ? raw.device : null
    if (raw.byDevice && typeof raw.byDevice === 'object') {
      for (const [id, route] of Object.entries(raw.byDevice)) {
        if (route && typeof route === 'object') out.byDevice[id] = cleanRoute(route)
      }
    } else if ('preset' in raw || 'custom' in raw || 'mono' in raw) {
      // Hasta la 1.41 había un solo reparto, para todas las salidas: se queda
      // con la que estaba elegida al hacerlo (ninguna: la del sistema).
      out.byDevice[out.device || ''] = cleanRoute(raw)
    }
  } catch {
    /* algo que no se entiende: como recién instalado */
  }
  return out
}

const saved = ref(read())
/**
 * Las pistas de la canción que suena y los carriles de guías, con su papel
 * (las pone el estudio). Una sola para todos: el panel de «Salidas» tiene su
 * propio `useRouting()`, y con una lista suya (vacía) mandaba al motor un
 * reparto sin los buses de las pistas.
 */
const stemList = ref(/** @type {Array<{key: string, role: string}>} */ ([]))
/** Las salidas del equipo (se piden al abrir el estudio o el panel). */
const devices = ref(/** @type {import('../api.js').AudioDevice[]} */ ([]))
let started = false

// --- lo que se sabe de la salida que suena
/**
 * Recién pedida otra salida ('device') u otros canales ('channels'): hasta
 * que Rust cuente la nueva, lo que diga el estado es de la de antes.
 */
const settling = ref(/** @type {''|'device'|'channels'} */ (''))
let settleTimer = /** @type {ReturnType<typeof setTimeout>|undefined} */ (undefined)
/** Los canales que se pidieron la última vez. */
const asked = ref(0)
/**
 * Si la elegida estaba o no la última vez que se abrió. En pausa un rato Rust
 * suelta la salida y el estado ya no lo dice: sin esto, el reparto iba y
 * venía con cada pausa larga.
 */
const fallbackSeen = ref(/** @type {{device: string, fallback: boolean}|null} */ (null))
/** Y con cuántos canales se abrió, pidiéndole cuántos. */
const channelsSeen = ref(
  /** @type {{device: string|null, fallback: boolean, channels: number, asked: number}|null} */ (
    null
  )
)
/** Lo último mandado como salida: si cambia, Rust la vuelve a abrir. */
let sent = /** @type {{device: string|null, channels: number}|null} */ (null)
/** Lo último mandado, entero (para no repetirlo). */
let applied = ''
/** El aviso de «faltan canales» que ya se dio. */
let said = ''
/** Lo que vigila la salida: en un ámbito propio, no en el del primer componente que lo use. */
let scope = /** @type {import('vue').EffectScope|null} */ (null)

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(saved.value))
  } catch {
    /* modo privado */
  }
}

/** @param {'device'|'channels'} kind */
function settle(kind) {
  settling.value = kind === 'device' || settling.value === 'device' ? 'device' : 'channels'
  clearTimeout(settleTimer)
  settleTimer = setTimeout(() => {
    settling.value = ''
  }, SETTLE_MS)
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
 * Si eso va en mono (juntando sus dos lados). Una pista con papel va como el
 * clic o las guías, salvo que se le diga; una sin decir, como la canción.
 * @param {Prefs} p
 * @param {string} kind  'song' | 'click' | 'cues' | 'stem'
 * @param {string} [key]
 * @param {string} [role]
 * @returns {boolean}
 */
export function monoOf(p, kind, key, role = '') {
  const m = p.mono || defaults().mono
  if (kind === 'stem') {
    const own = m.stems?.[key || '']
    if (typeof own === 'boolean') return own
    return monoOf(p, role === 'click' ? 'click' : role === 'guide' ? 'cues' : 'song')
  }
  return !!m[/** @type {'song'|'click'|'cues'} */ (kind)]
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
  // un bus por destino y forma (el mismo par en estéreo y en mono son dos)
  /** @type {Array<{dest: Dest, mono: boolean}>} */
  const used = []
  const busOf = (/** @type {Dest} */ dest, /** @type {boolean} */ mono) => {
    const joined = mono && !isSingle(dest) && dest !== 'off'
    let i = used.findIndex((u) => u.dest === dest && u.mono === joined)
    if (i < 0) {
      used.push({ dest, mono: joined })
      i = used.length - 1
    }
    return i
  }
  const of = (/** @type {string} */ kind, key = '', role = '') =>
    busOf(destOf(p, kind, key, role), monoOf(p, kind, key, role))
  const song = of('song')
  const stems = Object.fromEntries(
    stemList.map(stemOf).map((st) => [st.key, of('stem', st.key, st.role)])
  )
  const click = of('click')
  const cues = of('cues')
  const channels = Math.max(2, ...used.map((u) => channelsFor(u.dest)))
  return {
    spec: { buses: used.map((u) => targetOf(u.dest, u.mono)), song, click, cues },
    stems,
    channels
  }
}

/**
 * Los canales de una salida del equipo por su id ('': la del sistema), si se
 * han mirado y está libre.
 * @param {string} key
 */
function listed(key) {
  const d = devices.value.find((x) => (key ? x.id === key : x.default))
  return d && !d.busy && d.channels > 0 ? d.channels : null
}

/** @param {number} n */
const channelsText = (n) => `${n} ${n === 1 ? 'canal' : 'canales'}`

/**
 * Lo que se sabe de la salida que suena y lo que sale de ello: qué reparto
 * le toca, si le faltan canales y lo que se manda al motor. Uno para todos
 * (el estudio, el cajón de «Salidas» y cada carril miran lo mismo).
 */
function createSounding() {
  const player = usePlayback()
  /** La salida abierta, si dice algo (recién pedida aún no tiene canales). */
  const status = computed(() => {
    const o = player.output.value
    return o && !o.error && o.channels > 0 ? o : null
  })
  /** La elegida no está (o la usa otro programa) y suena por la del sistema. */
  const fellBack = computed(() => {
    const id = saved.value.device
    if (!id) return false
    const o = status.value
    if (o && settling.value !== 'device') return !!o.fallback
    const seen = fallbackSeen.value
    if (seen && seen.device === id) return seen.fallback
    // sin haberla abierto aún: lo que digan las salidas del equipo, si se han
    // mirado (una lista vacía no dice nada: puede ser que no se pudo mirar)
    return devices.value.length > 0 && !devices.value.some((d) => d.id === id && !d.busy)
  })
  /** De qué salida es el reparto: de la que suena. */
  const soundingKey = computed(() => (fellBack.value ? '' : saved.value.device || ''))
  /** La salida elegida y el reparto de la que suena. */
  const prefs = computed(
    /** @returns {Prefs} */ () => ({
      device: saved.value.device,
      ...(saved.value.byDevice[soundingKey.value] || defaultRoute())
    })
  )
  /** El reparto guardado, como lo querría el motor. */
  const wanted = computed(() => buildRouting(prefs.value, stemList.value))
  /**
   * Los canales de la salida que suena, o null si no se sabe todavía. Lo dice
   * la salida abierta; cerrada o reabriéndose, lo que se vio la última vez
   * (si vale para lo que se pide ahora) o lo que admite según el equipo.
   */
  const available = computed(() => {
    const o = status.value
    if (o && !settling.value) return o.channels
    const need = wanted.value.channels
    const seen = channelsSeen.value
    if (
      seen &&
      seen.device === saved.value.device &&
      seen.fallback === fellBack.value &&
      // le sobran para lo de ahora, o se quedó corta: son todos los que tiene
      (seen.channels >= need || seen.channels < seen.asked)
    ) {
      return seen.channels
    }
    return listed(soundingKey.value)
  })
  /**
   * A la salida le faltan canales para su reparto. (Una de un solo canal no
   * cuenta contra «todo junto»: ya no hay nada más junto que eso.)
   */
  const short = computed(
    () => available.value != null && Math.max(2, available.value) < wanted.value.channels
  )
  /** Lo que se manda al motor: el reparto guardado o, si faltan canales, todo junto. */
  const routing = computed(() =>
    short.value
      ? buildRouting({ device: prefs.value.device, ...defaultRoute() }, stemList.value)
      : wanted.value
  )
  const sendKey = computed(() =>
    JSON.stringify([saved.value.device, wanted.value.channels, routing.value.spec])
  )

  /** Manda al motor la salida y el enrutado. */
  async function apply() {
    const device = saved.value.device
    // otra salida: lo que cuenta el estado (si la elegida está, sus canales)
    // es todavía de la de antes
    if (!sent || sent.device !== device) settle('device')
    const channels = wanted.value.channels
    if (sent && sent.device === device && sent.channels !== channels) settle('channels')
    sent = { device, channels }
    asked.value = channels
    applied = sendKey.value
    // Se piden siempre los canales del reparto guardado, aunque suene todo
    // junto porque faltan: la salida no se reabre al ir y venir, y si resulta
    // que los tiene, el estado lo dice.
    await player.setOutput({ device, channels })
    // (el de ahora, no el de antes de esperar: si cambió mientras tanto, la
    // última orden lleva lo último)
    await player.setRouting(routing.value.spec)
  }

  async function loadDevices() {
    devices.value = await player.outputDevices()
    // Con la salida cerrada, lo recién mirado vale más que lo que se vio al
    // abrirla la última vez: si la interfaz se enchufó después, ya está.
    if (!status.value) fallbackSeen.value = null
    return devices.value
  }

  /** Lo que vigila la salida: lo que cambia sin que nadie toque nada. */
  function watchOutput() {
    scope = effectScope(true)
    scope.run(() => {
      // la salida que suena resultó ser otra, o le faltan (o ya tiene) los
      // canales: lo que toca ahora, al motor
      watch(sendKey, (key) => {
        if (key !== applied) apply()
      })
      // lo visto, para cuando Rust suelte la salida (tras un rato en pausa)
      watch(
        [
          () => (status.value ? `${status.value.fallback ? 1 : 0}|${status.value.channels}` : ''),
          settling,
          asked,
          () => saved.value.device
        ],
        () => {
          const o = status.value
          if (!o || settling.value === 'device') return
          const device = saved.value.device
          if (device) fallbackSeen.value = { device, fallback: !!o.fallback }
          if (settling.value) return
          channelsSeen.value = {
            device,
            fallback: fellBack.value,
            channels: o.channels,
            asked: asked.value
          }
        },
        { immediate: true }
      )
      // faltan canales: se dice donde esté quien toca, una vez por cada caso
      watch(
        () =>
          short.value
            ? `${soundingKey.value}|${available.value}|${wanted.value.channels}`
            : available.value == null
              ? '?'
              : '',
        (key) => {
          // sin saberlo aún: ni se dice ni se da por arreglado
          if (key === '?' || key === said) return
          said = key
          if (!key) return
          notify(
            `La salida tiene ${channelsText(available.value || 0)} y el reparto de «Salidas» pide ` +
              `${wanted.value.channels}: suena todo junto hasta que enchufes la interfaz o lo cambies ` +
              'en «Salidas» (en el modo estudio).',
            'info',
            10
          )
        },
        { immediate: true }
      )
    })
  }

  return {
    player,
    fellBack,
    soundingKey,
    prefs,
    wanted,
    available,
    short,
    routing,
    apply,
    loadDevices,
    watchOutput
  }
}
/** @type {ReturnType<typeof createSounding>|null} */
let shared = null

export function useRouting() {
  const sounding = (shared ||= createSounding())
  const { player, prefs, routing, wanted, available, short, fellBack, soundingKey } = sounding
  const { apply, loadDevices } = sounding

  if (!started) {
    started = true
    apply()
    sounding.watchOutput()
    // Con una salida elegida, si está o no se sabe ya, sin esperar a abrirla
    // con el primer play: el reparto es el de la que va a sonar desde la
    // primera nota, y la salida no se reabre a mitad de canción.
    if (saved.value.device) loadDevices().catch(() => {})
  }

  /**
   * Cambia la salida elegida (`device`) o el reparto de la que suena.
   * @param {Partial<Prefs>} patch
   */
  async function update(patch) {
    const { device, ...route } = patch
    if ('device' in patch && (device || null) !== saved.value.device) {
      saved.value = { ...saved.value, device: device || null }
      settle('device')
    }
    if (Object.keys(route).length) {
      const key = soundingKey.value
      const now = saved.value.byDevice[key] || defaultRoute()
      saved.value = {
        ...saved.value,
        byDevice: { ...saved.value.byDevice, [key]: { ...now, ...route } }
      }
    }
    save()
    await apply()
  }
  /**
   * Lo de ahora, a medida: cada cosa donde suena con el reparto elegido. Al
   * cambiar una sola desde «Clic y guías a la izquierda» (por ejemplo), las
   * demás se quedan donde estaban en vez de volver todas a los dos lados.
   */
  function asCustom() {
    const p = prefs.value
    if (p.preset === 'custom') return p.custom
    return {
      song: destOf(p, 'song'),
      click: destOf(p, 'click'),
      cues: destOf(p, 'cues'),
      stems: Object.fromEntries(
        stemList.value.map((st) => [st.key, destOf(p, 'stem', st.key, st.role)])
      )
    }
  }
  /** @param {'song'|'click'|'cues'} kind @param {Dest} dest */
  function setCustom(kind, dest) {
    return update({ preset: 'custom', custom: { ...asCustom(), [kind]: dest } })
  }
  /** @param {string} key @param {Dest} dest */
  function setStem(key, dest) {
    const custom = asCustom()
    return update({
      preset: 'custom',
      custom: { ...custom, stems: { ...custom.stems, [key]: dest } }
    })
  }
  /**
   * Por dónde suena una cosa y si en mono: la canción, el clic, las guías o
   * una pista (`key`, con su papel).
   * @param {'song'|'click'|'cues'|'stem'} kind @param {string} [key] @param {string} [role]
   * @returns {{dest: Dest, mono: boolean}}
   */
  function routeOf(kind, key = '', role = '') {
    const roleOf = role || stemList.value.find((st) => st.key === key)?.role || ''
    return {
      dest: destOf(prefs.value, kind, key, roleOf),
      mono: monoOf(prefs.value, kind, key, roleOf)
    }
  }
  /**
   * Mandar una cosa por esos canales (desde los botones L y R de su carril).
   * @param {'song'|'click'|'cues'|'stem'} kind @param {string} key @param {Dest} dest
   */
  function setRoute(kind, key, dest) {
    return kind === 'stem' ? setStem(key, dest) : setCustom(kind, dest)
  }
  /**
   * Cómo va en un archivo estéreo al exportar «con los canales»: el lado y
   * si en mono; null si no suena.
   * @param {'song'|'click'|'cues'|'stem'} kind @param {string} [key] @param {string} [role]
   * @returns {{side: 'both'|'L'|'R', mono: boolean}|null}
   */
  function exportRoute(kind, key = '', role = '') {
    const r = routeOf(kind, key, role)
    const side = sideOf(r.dest)
    return side ? { side, mono: side === 'both' ? r.mono : true } : null
  }
  /** Si algo no suena «por los dos lados, en estéreo» (y exportar con los canales cambia algo). */
  const routed = computed(() => {
    const things = [
      routeOf('song'),
      routeOf('click'),
      routeOf('cues'),
      ...stemList.value.map((st) => routeOf('stem', st.key, st.role))
    ]
    return things.some((t) => t.dest !== 'main' || t.mono)
  })
  /** Los canales de la salida que suena (2 si no se sabe). */
  const deviceChannels = computed(() => Math.max(2, Math.min(16, listed(soundingKey.value) || 2)))
  /**
   * En mono o en estéreo (no cambia el reparto: vale con cualquiera).
   * @param {'song'|'click'|'cues'|'stem'} kind @param {boolean} on @param {string} [key]
   */
  function setMono(kind, on, key = '') {
    const m = prefs.value.mono || defaults().mono
    return update({
      mono: kind === 'stem' ? { ...m, stems: { ...m.stems, [key]: on } } : { ...m, [kind]: on }
    })
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
    setMono,
    routeOf,
    setRoute,
    exportRoute,
    routed,
    deviceChannels,
    loadDevices,
    busOfStem,
    setStems,
    apply,
    /** a la salida que suena le faltan canales para su reparto: suena todo junto */
    short,
    /** los canales de la salida que suena (null: aún no se sabe) */
    available,
    /** los canales que pide el reparto guardado */
    needs: computed(() => wanted.value.channels),
    /** la elegida no está y suena por la del sistema, con el reparto de esa */
    fellBack,
    /** la salida abierta (de Rust) */
    output: player.output
  }
}

/** Vuelve a lo de fábrica. Solo para pruebas. */
export function resetRouting() {
  scope?.stop()
  scope = null
  clearTimeout(settleTimer)
  settling.value = ''
  asked.value = 0
  fallbackSeen.value = null
  channelsSeen.value = null
  sent = null
  applied = ''
  said = ''
  saved.value = read()
  stemList.value = []
  devices.value = []
  started = false
}
